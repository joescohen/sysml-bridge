//! Suspect links, the rule results a person set aside, the validation results, usages and Quick Find: the read half of
//! what the server's `suspect.rs` held. They run on a [`Ctx`], so the in-memory build answers them too. What a person writes (a
//! clear, an ignore) stays in the server.

use super::suspect::{self, Opts, Track, ALL_FIELDS};
use super::text::{model_name, quote_claim, quote_clip, stored_token};
use super::tools::{err, model_of, out, Ctx, ToolErr, ToolResult};
use serde_json::{json, Value};

/// The words for the way a decision arrived, only when the route is known.
pub fn way_says(route: &str, authenticated: bool) -> String {
    let who = if authenticated { "reviewer authenticated" } else { "reviewer not authenticated" };
    match route {
        "http" | "viewer" | "viewer_page" => format!("through the HTTP route, {who}"),
        "command" => format!("through the command, {who}"),
        _ => who.to_string(),
    }
}

pub fn opts_of(a: &Value) -> Result<Opts, ToolErr> {
    let mut o = Opts::default();
    if let Some(t) = a.get("tracked_end").filter(|v| !v.is_null()) {
        o.track = t.as_str().and_then(Track::parse).ok_or_else(|| err("argument 'tracked_end' is \"requirement\" (the default), \"other\" or \"both\"."))?;
    }
    if let Some(f) = a.get("fields").filter(|v| !v.is_null()) {
        let list = f.as_array().ok_or_else(|| err(format!("argument 'fields' is a list of {}.", ALL_FIELDS.join(", "))))?;
        let mut fields: Vec<&'static str> = Vec::new();
        for x in list {
            match x.as_str().and_then(|s| ALL_FIELDS.iter().find(|k| **k == s)) {
                Some(k) if !fields.contains(k) => fields.push(k),
                Some(_) => {}
                None => return Err(err(format!("argument 'fields' holds {}; the fields are {}.", quote_clip(x.as_str().unwrap_or("?")), ALL_FIELDS.join(", ")))),
            }
        }
        if fields.is_empty() {
            return Err(err(format!("argument 'fields' is empty; name at least one of {}.", ALL_FIELDS.join(", "))));
        }
        o.fields = fields;
    }
    Ok(o)
}
/// The text of a review, quoted.
pub fn review_says(r: &Value) -> String {
    let note = r["note"].as_str().filter(|n| !n.is_empty()).map(|n| format!(", note {}", quote_clip(n))).unwrap_or_default();
    let how = way_says(if r["route"] == "http" { "http" } else { "command" }, r["authenticated"] == true);
    format!("cleared by {} ({how}) at {}{note}", quote_claim(r["by"].as_str().unwrap_or("")), stored_token(&r["time"]))
}
/// The listing for `since` (and `until`): the options, the data, and the two bar lines, as `list_suspect_links` and a clear read it.
pub fn listing(ctx: &dyn Ctx, a: &Value) -> Result<(Opts, Value, String, String), ToolErr> {
    let mut opts = opts_of(a)?;
    if a.get("since").filter(|v| !v.is_null()).is_none() {
        return Err(err("missing argument 'since': a baseline name, a proposal id (\"P-001\": the links that proposal makes suspect), a project revision or #N."));
    }
    let p = ctx.suspect_pair(a)?;
    opts.since = p.bar_before.clone();
    let (bm, am) = (model_of(&p.before), model_of(&p.after));
    let store = ctx.reviews();
    let mut data = suspect::list(&bm, &am, &opts, &store);
    data["versions"] = p.versions;
    if ctx.offline() {
        data["clears"] = json!("none stored: this copy keeps no review of a link, so every link whose tracked end changed is listed as suspect and none is shown as cleared");
    }
    Ok((opts, data, p.bar_before, p.bar_after))
}
/// `list_suspect_links {since, until?, tracked_end?, fields?}`.
pub fn list_suspect_links(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let (opts, data, bar_before, bar_after) = listing(ctx, a)?;
    // The words of a person in the review records, quoted in the reply text and kept as data.
    let mut text = format!(
        "{} -> {}: {} suspect link(s) (tracked end: {}): {} to clear, {} cleared.\\n",
        bar_before,
        bar_after,
        data["total"],
        opts.track.word(),
        data["suspect"],
        data["cleared"]
    )
    .replace("\\n", "\n");
    for l in data["links"].as_array().into_iter().flatten().take(60) {
        let fields: Vec<&str> = l["changed"].as_array().into_iter().flatten().filter_map(|c| c["field"].as_str()).collect();
        let state = match l["review"].as_object() {
            Some(_) => review_says(&l["review"]),
            None => "not cleared".to_string(),
        };
        text.push_str(&format!(
            "- {} link at line {}: {} -> {}; changed: {}; {state}\n",
            stored_token(&l["link"]["kind"]),
            stored_token(&l["link"]["line"]),
            model_name(l["client"]["ref"].as_str().unwrap_or("")),
            model_name(l["supplier"]["ref"].as_str().unwrap_or("")),
            fields.join(", ")
        ));
    }
    out(text, data)
}
/// The fingerprint of one rule result: SHA-256 over a versioned, length-prefixed form of its gate, rule, element and message.
pub fn result_fingerprint(f: &Value) -> String {
    let t = |k: &str| f[k].as_str().unwrap_or("").as_bytes().to_vec();
    let parts: Vec<(&str, Vec<u8>)> = vec![("gate", t("gate")), ("rule", t("rule")), ("element", t("element")), ("message", t("message"))];
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    suspect::sha256_hex(&suspect::frame("sysml-bridge-ignore-v1", &refs))
}
/// How many earlier states of the write log an ignore is checked against.
pub const IGNORE_SPAN_STATES: usize = 24;
/// The stable id the id file gives the element with this qualified name, if any.
pub fn stable_id_of(ids: &std::collections::BTreeMap<String, String>, element: &str) -> Option<String> {
    ids.iter().find(|(_, q)| q.as_str() == element).map(|(id, _)| id.clone())
}
/// Each finding of a reply gets its `fingerprint`, and `ignored` with the record when a person set that exact result aside.
/// A record applies only to the element it was made for (its stable id, when it has one) and only while the result has stayed
/// present in every state of the write log since the record was made: a result that was gone and returns is listed again,
/// with `ignore_lapsed` saying why. Where the log no longer leads back to the record (a save outside the tools, or more than
/// 24 writes), the record does not apply, and that is said.
pub fn mark_ignored(ctx: &dyn Ctx, v: &mut Value) {
    let records = ctx.rule_ignores();
    let Some(list) = v.get_mut("findings").and_then(Value::as_array_mut) else { return };
    let ids = ctx.ids();
    let files = ctx.files();
    let now = super::project_revision(&files);
    // The states and what each one gives, computed once and only when a record of an earlier revision matches.
    let mut past: Option<Vec<(String, std::collections::BTreeSet<String>)>> = None;
    for f in list.iter_mut() {
        let fp = result_fingerprint(f);
        f["fingerprint"] = json!(fp);
        f["ignored"] = json!(false);
        let Some(r) = records.iter().rev().find(|r| r["fingerprint"].as_str() == Some(fp.as_str())) else { continue };
        let element = f["element"].as_str().unwrap_or("");
        let here = stable_id_of(&ids, element);
        if r["element_id"].as_str().map(str::to_string) != here {
            f["ignore_lapsed"] = json!("the element is not the one the result was set aside for (its stable id differs)");
            continue;
        }
        let from = r["revision"].as_str().unwrap_or("");
        let held = if from == now {
            Ok(())
        } else {
            let states = past.get_or_insert_with(|| {
                ctx.past_states(&files, IGNORE_SPAN_STATES)
                    .into_iter()
                    .map(|(rev, fs)| {
                        let set = match super::rules::check_all(&model_of(&fs), None, None) {
                            Ok(all) => all["findings"].as_array().into_iter().flatten().map(result_fingerprint).collect(),
                            Err(_) => Default::default(),
                        };
                        (rev, set)
                    })
                    .collect()
            });
            match states.iter().position(|(rev, _)| rev == from) {
                None => Err("the write log no longer leads back to the moment it was set aside, so it cannot be shown that the result stayed present".to_string()),
                Some(i) if states[..=i].iter().all(|(_, set)| set.contains(&fp)) => Ok(()),
                Some(_) => Err("the result was gone for a while after it was set aside and has come back".to_string()),
            }
        };
        match held {
            Ok(()) => {
                f["ignored"] = json!(true);
                f["ignored_by"] = json!({ "by": r["by"], "note": r["note"], "time": r["time"], "route": r["route"], "authenticated": r["authenticated"].as_bool().unwrap_or(false) });
            }
            Err(why) => f["ignore_lapsed"] = json!(why),
        }
    }
}
/// `get_validation_results`: the rule results for marking, grouped by rule and rolled up by owner; `scope` validates one package.
pub fn get_validation_results(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let strs = |k: &str| -> Option<Vec<String>> { a.get(k).and_then(Value::as_array).map(|v| v.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()) };
    let files = ctx.files();
    let m = model_of(&files);
    let (gates, require) = (strs("gates"), strs("require"));
    // Same key as check_all with no coverage owner, so the two calls share one check.
    let options = json!({ "gates": gates, "require": require, "coverage_owner": null });
    let mut all = super::tools::check_all_cached(&files, &options, || super::rules::check_all(&m, gates.as_deref(), require.as_deref())).map_err(|e| err(e.message))?;
    mark_ignored(ctx, &mut all);
    let o = super::results::Opts { scope: a.get("scope").and_then(Value::as_str).map(str::to_string), min_severity: a.get("min_severity").and_then(Value::as_str).map(str::to_string), offset: a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize, limit: a.get("limit").and_then(Value::as_u64).unwrap_or(0) as usize };
    let mut v = super::results::results(&m, &all, &o).map_err(|e| err(e.message))?;
    v["revision"] = json!(super::project_revision(&files));
    let s = &v["summary"];
    let text = format!("{} errors, {} warnings, {} infos ({} ignored) in {} rule(s){}.", s["errors"], s["warnings"], s["infos"], s["ignored"], v["by_rule"].as_array().map(Vec::len).unwrap_or(0), match v["scope"].as_str() { Some(sc) => format!(" under {}", stored_token(&json!(sc))), None => String::new() });
    out(text, v)
}
/// `get_usages`: Used By and Depends On, with the reason of each row.
pub fn get_usages(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let name = a.get("element").and_then(Value::as_str).ok_or_else(|| err("missing argument 'element': the qualified name of the element."))?;
    let files = ctx.files();
    let m = model_of(&files);
    let is_pkg = m.find(name).map(|e| e.kind == "package").unwrap_or(false);
    let flag = |k: &str, d: bool| a.get(k).and_then(Value::as_bool).unwrap_or(d);
    let mentioned = super::element_links::mentions(&m, &ctx.ids());
    let o = super::usages::Opts {
        direction: a.get("direction").and_then(Value::as_str).unwrap_or("used_by").to_string(),
        recursive: flag("recursive", is_pkg),
        ignore_inner: flag("ignore_inner", true),
        level: a.get("level").and_then(Value::as_str).unwrap_or("elements").to_string(),
        offset: a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
        limit: a.get("limit").and_then(Value::as_u64).unwrap_or(0) as usize,
        mentions: mentioned.0.into_iter().filter_map(|x| x.target.map(|t| (x.owner, t))).collect(),
        mentions_cut: mentioned.1,
    };
    let v = super::usages::usages(&m, name, &o).map_err(|e| err(e.message))?;
    let text = format!("{} usage(s) in {} element(s){}.", v["total"], v["in_elements"], if v["truncated"] == true { "; the rows are cut, the counts are full" } else { "" });
    out(text, v)
}
/// `find`: Quick Find, ranked, camel case and wildcards, first 100 and `more`.
pub fn find(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let text = a.get("text").and_then(Value::as_str).ok_or_else(|| err("missing argument 'text': what to find."))?;
    // The matcher lists everything for empty text; the tool keeps its contract that blank text is a mistake ('*' lists all).
    if text.trim().is_empty() {
        return Err(err("text is empty. Type part of a name, an id, initials, or a pattern with * and ? ('*' lists everything)."));
    }
    for option in ["match_anywhere", "use_camel_case"] {
        if a.get(option).is_some_and(|v| !v.is_boolean()) {
            return Err(err(format!("argument '{option}' must be a boolean.")));
        }
    }
    let o = super::find::Opts { match_anywhere: a.get("match_anywhere").and_then(Value::as_bool).unwrap_or(false), use_camel_case: a.get("use_camel_case").and_then(Value::as_bool).unwrap_or(true), text: text.to_string(), scope: a.get("scope").and_then(Value::as_str).unwrap_or("any").to_string(), offset: a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize, limit: a.get("limit").and_then(Value::as_u64).unwrap_or(0) as usize };
    let m = model_of(&ctx.files());
    let v = super::find::find(&m, &o).map_err(|e| err(e.message))?;
    let t = if v["total"] == 0 { format!("Nothing matches {}.", quote_clip(text)) } else { format!("{} of {}{}", v["shown"], v["total"], if v["more"] == true { " (more)" } else { "" }) };
    out(t, v)
}
