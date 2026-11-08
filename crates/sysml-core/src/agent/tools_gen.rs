//! Relation maps, coverage history and the next free id: the read half of what the server's `gen.rs` holds. They run on
//! a [`Ctx`].

use super::coverage_history as cov;
use super::numbering;
use super::relmap;
use super::text::{quote_claim, quote_clip, stored_token};
use super::tools::{err, out, model_of, Ctx, ToolErr, ToolResult};
use serde_json::{json, Value};
pub fn strs(a: &Value, k: &str) -> Vec<String> {
    match a.get(k) {
        Some(Value::Array(x)) => x.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}
pub fn text_of(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::to_string)
}
pub fn num(a: &Value, k: &str) -> usize {
    a.get(k).and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX)).unwrap_or(0)
}
/// Most characters in one text argument, most entries in a list argument not named below, most nodes read when the arguments are checked.
pub const MAX_ARG_LEN: usize = 500;
pub const MAX_ARG_LIST: usize = 50;
pub const MAX_ARG_NODES: usize = 2000;
pub fn too_long(k: &str, v: &Value, cap: usize, seen: &mut usize) -> Result<(), ToolErr> {
    *seen += 1;
    if *seen > MAX_ARG_NODES {
        return Err(err(format!("argument {k} holds more than {MAX_ARG_NODES} values; send fewer.")));
    }
    match v {
        Value::String(s) if s.len() > cap => Err(err(format!("argument {k} is {} characters; at most {cap} are accepted.", s.len()))),
        Value::Array(x) => x.iter().try_for_each(|i| too_long(k, i, cap, seen)),
        Value::Object(o) => o.values().try_for_each(|i| too_long(k, i, cap, seen)),
        _ => Ok(()),
    }
}
/// The size of the arguments, tested at the edge before the model is read or built. `lists` names the largest array of an argument
/// (any other array holds at most 50 entries); `long` names the text arguments that may exceed 500 characters (up to 4000).
pub fn edge(a: &Value, lists: &[(&str, usize)], long: &[&str]) -> Result<(), ToolErr> {
    let Value::Object(o) = a else { return Ok(()) };
    let mut seen = 0usize;
    for (k, v) in o {
        if let Value::Array(x) = v {
            let max = lists.iter().find(|(n, _)| n == k).map(|(_, m)| *m).unwrap_or(MAX_ARG_LIST);
            if x.len() > max {
                return Err(err(format!("argument {k} has {} entries; at most {max} are accepted. Send fewer.", x.len())));
            }
        }
        too_long(k, v, if long.contains(&k.as_str()) { 4000 } else { MAX_ARG_LEN }, &mut seen)?;
    }
    Ok(())
}
/// `get_relation_map`: the nodes and edges reached from one context by the chosen relations; with no context, the definitions kept from the import and the presets.
pub fn get_relation_map(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    edge(a, &[("criteria", relmap::MAX_CRITERIA)], &[])?;
    let defs: Vec<relmap::Def> = ctx.all_files().into_iter().filter(|f| f.path.ends_with(".diagrams.json")).flat_map(|f| relmap::definitions(&f.path, &f.text)).collect();
    let mut context = a.get("context").and_then(Value::as_str).map(str::to_string);
    let mut preset = text_of(a, "preset");
    let mut criteria = Vec::new();
    for c in a.get("criteria").and_then(Value::as_array).into_iter().flatten() {
        let kind = c["kind"].as_str().ok_or_else(|| err("each criterion needs a 'kind', for example {\"kind\": \"satisfy\", \"direction\": \"to_source\"}."))?;
        criteria.push(relmap::Criterion { kind: kind.to_string(), direction: c["direction"].as_str().unwrap_or("both").to_string() });
    }
    let mut depth = num(a, "depth");
    let mut note = None;
    if let Some(want) = a.get("map").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        let d = defs.iter().find(|d| d.key == want).or_else(|| defs.iter().find(|d| d.title == want)).ok_or_else(|| err(format!("argument map: {} is not a relation map kept from the import. Call get_relation_map with no arguments to list them; pass the key or the title.", quote_clip(want))))?;
        if context.is_none() {
            context = Some(d.context.clone());
        }
        let mapped: Vec<&str> = d.criteria.iter().filter_map(|c| relmap::criterion_of(c)).collect();
        let unmapped: Vec<&String> = d.criteria.iter().filter(|c| relmap::criterion_of(c).is_none()).collect();
        if criteria.is_empty() && preset.is_none() {
            if mapped.is_empty() {
                return Err(err(format!("the map {} names no criterion this reader walks; pass criteria in the call.", quote_clip(&d.title))));
            }
            for k in mapped {
                if !criteria.iter().any(|c: &relmap::Criterion| c.kind == k) {
                    criteria.push(relmap::Criterion { kind: k.to_string(), direction: "both".into() });
                }
            }
        }
        if depth == 0 {
            depth = d.depth;
        }
        if !unmapped.is_empty() {
            note = Some(format!("{} criterion name(s) of the map are not walked: {}", unmapped.len(), unmapped.iter().map(|c| quote_clip(c)).collect::<Vec<_>>().join(", ")));
        }
    }
    let Some(context) = context.filter(|c| !c.trim().is_empty()) else {
        let rows: Vec<Value> = defs.iter().map(|d| json!({ "key": d.key, "title": d.title, "context": d.context, "criteria": d.criteria, "depth": d.depth, "walked": d.criteria.iter().filter_map(|c| relmap::criterion_of(c)).collect::<Vec<_>>() })).collect();
        let text = format!("{} relation map(s) kept from the import, {} preset(s). Pass context (for example Package::Element) to draw from an element, or map (a key or title above) to draw one.", rows.len(), relmap::PRESETS.len());
        return out(text, json!({ "maps": rows, "presets": relmap::PRESETS, "context_required": false, "limits": { "maps": relmap::MAX_DEFS } }));
    };
    let o = relmap::Opts {
        context,
        preset: preset.take(),
        criteria,
        types: strs(a, "types"),
        scope: strs(a, "scope"),
        depth,
        all_depth: a.get("depth").and_then(Value::as_str) == Some("all"),
        repeat: a.get("single_node").and_then(Value::as_bool) == Some(false),
        implied: a.get("implied").and_then(Value::as_bool) == Some(true),
    };
    let m = model_of(&ctx.files());
    let mut v = relmap::relation_map(&m, &o).map_err(|e| err(e.message))?;
    if let Some(n) = note {
        v["bounds"].as_array_mut().map(|b| b.push(json!(n)));
    }
    let s = &v["summary"];
    let text = format!(
        "Relation map of {}: {} element(s) in {} level(s), {} edge(s); {} requirement(s) without a satisfier, {} without a verifier{}.",
        quote_clip(v["context"]["ref"].as_str().unwrap_or("")),
        s["elements"],
        s["levels"],
        s["edges"],
        s["no_satisfier"],
        s["no_verifier"],
        if v["truncated"] == true { "; cut at the node bound" } else { "" }
    );
    out(text, v)
}
/// `get_coverage_history`: the four coverage measures for each baseline and for the workspace now.
pub fn get_coverage_history(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    coverage_history_capped(ctx, a, cov::MAX_CALL_STEPS)
}
/// The same with the step budget of the whole call given (the tests use a small one).
pub fn coverage_history_capped(ctx: &dyn Ctx, a: &Value, cap: usize) -> ToolResult {
    edge(a, &[("measures", cov::MEASURES.len()), ("baselines", cov::MAX_BASELINES)], &[])?;
    let o = cov::Opts { scope: text_of(a, "scope"), covering_scope: strs(a, "covering_scope"), atomic_only: a.get("atomic_only").and_then(Value::as_bool).unwrap_or(false) };
    let want = strs(a, "baselines");
    let with_now = a.get("include_now").and_then(Value::as_bool).unwrap_or(true);
    let measures: Vec<String> = strs(a, "measures");
    for m in &measures {
        if !cov::MEASURES.iter().any(|(n, _)| n == m) {
            return Err(err(format!("measure is one of {}, not {}.", cov::MEASURES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "), quote_clip(m))));
        }
    }
    let _guard = ctx.hold();
    let current = ctx.files();
    let mut names: Vec<String> = if want.is_empty() { ctx.baseline_names() } else { want };
    let mut bounds: Vec<String> = Vec::new();
    if names.len() > cov::MAX_BASELINES {
        bounds.push(format!("baselines: the {} most recent of {} are evaluated; name the ones you want with baselines", cov::MAX_BASELINES, names.len()));
        names = names.split_off(names.len() - cov::MAX_BASELINES);
    }
    if ctx.offline() {
        bounds.push("baselines: this copy holds none (they are kept by the full application); only the Now row is computed".to_string());
    }
    let mut rows: Vec<Value> = Vec::new();
    // One budget for the whole call: baselines x measures x rules, each state costing its parse, a pass over its statements, and the reach.
    let mut used: usize = 0;
    let mut stopped: Option<String> = None;
    let mut eval = |label: String, files: &[crate::api::FileIn], info: Value, now: bool| -> Result<bool, ToolErr> {
        let parse = files.iter().map(|f| f.text.len() / 64 + 1).sum::<usize>();
        if used.saturating_add(parse) > cap {
            return Ok(false);
        }
        used += parse;
        let m = model_of(files);
        let mut r = cov::measure(&m, &o).map_err(|e| err(e.message))?;
        used = used.saturating_add(r["steps"].as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).unwrap_or(0));
        if !measures.is_empty() {
            if let Some(mm) = r["measures"].as_object_mut() {
                mm.retain(|k, _| measures.contains(k));
            }
        }
        r["name"] = json!(label);
        r["is_now"] = json!(now);
        r["revision"] = info["revision"].clone();
        r["time"] = info["time"].clone();
        rows.push(r);
        Ok(true)
    };
    for n in &names {
        let (files, info) = ctx.state_named(&current, n)?;
        if !eval(n.clone(), &files, info, false)? {
            stopped = Some(n.clone());
            break;
        }
    }
    if with_now && stopped.is_none() {
        let rev = crate::agent::project_revision(&current);
        if !eval("Now".into(), &current, json!({ "revision": rev, "time": null }), true)? {
            stopped = Some("Now".into());
        }
    }
    if let Some(at) = stopped {
        bounds.push(format!("steps: the call stopped before {} at its limit of {} steps for all baselines together; ask for fewer baselines or a smaller scope", quote_clip(&at), cap));
    }
    // A bound inside one state (statements cut, implied reach cut) makes its percentages lower bounds: say so for the whole reply.
    for r in &rows {
        let name = quote_clip(r["name"].as_str().unwrap_or(""));
        if r.get("pairs_truncated").is_some() {
            bounds.push(format!("statements: in {name} some statements were read only in part; the covered counts and percentages are lower bounds (see not_checked)"));
        }
        if r["implied_reach_cut"].as_u64().unwrap_or(0) > 0 {
            bounds.push(format!("reach: in {name} {} requirement(s) hit the implied reach bound; their implied coverage may be understated", r["implied_reach_cut"]));
        }
    }
    // The change from the row before, per measure and rule, in percentage points.
    for i in 1..rows.len() {
        let mut delta = serde_json::Map::new();
        if let Some(mm) = rows[i]["measures"].as_object() {
            for (k, v) in mm {
                for rule in ["requirement_rule", "grouping_rule"] {
                    let (now, before) = (v[rule]["percent"].as_f64(), rows[i - 1]["measures"][k][rule]["percent"].as_f64());
                    if let (Some(n), Some(b)) = (now, before) {
                        delta.entry(k.clone()).or_insert_with(|| json!({}))[rule] = json!((((n - b) * 10.0).round()) / 10.0);
                    }
                }
            }
        }
        rows[i]["change"] = Value::Object(delta);
    }
    let text = rows
        .iter()
        .map(|r| {
            let sat = &r["measures"]["satisfaction"]["requirement_rule"]["percent"];
            format!("{}: {} requirement(s), satisfaction {}%{}", quote_claim(r["name"].as_str().unwrap_or("")), r["requirements"], if sat.is_null() { "n/a".to_string() } else { sat.to_string() }, if r["is_now"] == true { "" } else { "" })
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = if rows.is_empty() { "No baseline recorded. create_baseline {name} freezes the current state.".to_string() } else { text };
    let _ = stored_token;
    out(text, json!({ "rows": rows, "scope": o.scope, "covering_scope": o.covering_scope, "atomic_only": o.atomic_only, "owner_rules": { "requirement_rule": "Treat Owner As Requirement: covered by a link of its own", "grouping_rule": "Treat Owner As Grouping Element: also covered when an owning requirement is covered by its own link, or all the requirements it owns are covered" }, "truncated": !bounds.is_empty(), "steps": used, "bounds": bounds, "limits": { "baselines": cov::MAX_BASELINES, "steps": cap, "cost_per_baseline": "its text (1 step per 64 bytes) + its statements + 4 x its requirements + at most 400 for each counted requirement without a direct link" } }))
}
/// `get_next_id`: the next free requirement id under a scope, read-only.
pub fn get_next_id(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    edge(a, &[], &[])?;
    let scope = a.get("scope").and_then(Value::as_str).ok_or_else(|| err("missing argument 'scope': the qualified name of a package or requirement, for example Package::Section."))?;
    let (prefix, separator) = (text_of(a, "prefix"), text_of(a, "separator"));
    let m = model_of(&ctx.files());
    let v = numbering::next_id(&m, scope, prefix.as_deref(), separator.as_deref()).map_err(|e| err(e.message))?;
    out(format!("Next free id under {}: {}. Nothing is reserved.", quote_clip(v["scope"].as_str().unwrap_or("")), quote_claim(v["next"].as_str().unwrap_or(""))), v)
}
