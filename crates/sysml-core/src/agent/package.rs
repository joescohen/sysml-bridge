//! The change package: the record a program files for one change, between a baseline and a later state.
//!
//! Nothing here is a second implementation. The element changes are the change report (`change::compare_models`), the
//! gate totals are `rules::check_all` and the budget is `budget::delta`. This module orders them as a reader files them
//! and writes them as one Markdown document.

use super::text::{self, has_number_segment, md_table, md_text, quote_claim, visible, Names};
use super::{change, rules};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The stages a change is grouped by, in the order an engineer works; the last one holds what belongs to none.
pub const STAGES: [(&str, &str); 6] = [
    ("requirements", "Requirements"),
    ("functions", "Functions"),
    ("components", "Components"),
    ("interfaces", "Interfaces and signals"),
    ("verification", "Verification"),
    ("other", "Other"),
];

/// Most rows of one list in the Markdown document; the structured reply holds every row.
const DOC_ROWS: usize = 400;

/// A list of notes (gaps, twins), each with `says_shown`: its sentence as a document shows it, with the names as `nm`
/// writes them. `says` keeps the qualified names.
fn shown_says(list: &Value, nm: &Names) -> Value {
    Value::Array(
        list.as_array()
            .into_iter()
            .flatten()
            .map(|x| {
                let mut x = x.clone();
                x["says_shown"] = json!(nm.rewrite(x["says"].as_str().unwrap_or("")));
                x
            })
            .collect(),
    )
}

/// The sentence of a note as a document shows it.
fn said(x: &Value) -> &str {
    x["says_shown"].as_str().filter(|t| !t.is_empty()).or(x["says"].as_str()).unwrap_or("")
}

/// The stage an element belongs to: by what it is, else by the first owner that says.
fn stage_of(m: &Model, e: &Element) -> usize {
    let mut at = Some(e);
    while let Some(x) = at {
        let k = x.kind.as_str();
        let owner = x.owner.map(|o| &m.elements[o]);
        let in_action = owner.map(|o| o.kind == "action").unwrap_or(false);
        let s = if k.starts_with("verify") || k == "verification" {
            Some(4)
        } else if k.starts_with("satisfy") {
            Some(1)
        } else if k.starts_with("perform") || k == "allocation" {
            Some(2)
        } else if k == "requirement" || k == "concern" {
            Some(0)
        } else if k == "dependency" {
            match x.prefixes.iter().find_map(|p| p.strip_prefix('#')).and_then(super::graph::tag_edge_kind) {
                Some("itemflow") => Some(3),
                Some("verify") => Some(4),
                Some("satisfy") => Some(1),
                Some(_) => Some(0),
                None => None,
            }
        } else if matches!(k, "flow" | "message") {
            Some(if in_action { 1 } else { 3 })
        } else if matches!(k, "connection" | "interface" | "port" | "binding") {
            Some(3)
        } else if matches!(k, "item" | "enum") && x.is_def || (k == "attribute" && x.is_def) {
            Some(3)
        } else if matches!(k, "action" | "state" | "succession" | "transition" | "calc") {
            Some(1)
        } else if k == "part" {
            Some(2)
        } else {
            None
        };
        if let Some(s) = s {
            return s;
        }
        at = owner;
    }
    5
}

fn fact_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(l) => l.iter().map(fact_text).collect::<Vec<_>>().join(", "),
        Value::Object(o) if o.contains_key("text") => fact_text(&o["text"]),
        other => other.to_string(),
    }
}

fn fact_name(k: &str) -> &str {
    match k {
        "doc" => "text",
        "short_name" => "id",
        "of" => "item",
        "prefixes" => "modifiers",
        other => other,
    }
}

struct Row {
    change: &'static str,
    stage: usize,
    kind: String,
    reference: String,
    label: String,
    /// The name a document shows: the name with its owner, not the whole path.
    shown: String,
    before: String,
    after: String,
    facts: Vec<Value>,
    contains: Vec<String>,
}

/// The requirement a reader sees: `R-4 swapTime`.
fn requirement_name(nm: &Names, r: &Element) -> String {
    match &r.short_name {
        Some(id) => format!("{id} {}", nm.show(r)),
        None => nm.show(r),
    }
}

fn gate_totals(v: &Value) -> Value {
    json!({ "passed": v["passed"], "errors": v["errors"], "warnings": v["warnings"], "infos": v["infos"], "gates": v["gates"], "by_rule": v["by_rule"] })
}

/// The gate totals of a model, as a baseline records them and a change package compares them.
pub fn gates_of(m: &Model) -> Value {
    rules::check_all(m, None, None).map(|v| gate_totals(&v)).unwrap_or(Value::Null)
}

/// Findings in `a` that `b` does not have, counted by rule, with the first of each.
fn findings_only_in(a: &Value, b: &Value) -> Vec<Value> {
    let key = |f: &Value| format!("{}\u{1}{}\u{1}{}\u{1}{}", f["gate"].as_str().unwrap_or(""), f["rule"].as_str().unwrap_or(""), f["element"].as_str().unwrap_or(""), f["message"].as_str().unwrap_or(""));
    let mut have: HashMap<String, usize> = HashMap::new();
    for f in b["findings"].as_array().into_iter().flatten() {
        *have.entry(key(f)).or_default() += 1;
    }
    let mut by_rule: BTreeMap<(String, String, String), Vec<Value>> = BTreeMap::new();
    for f in a["findings"].as_array().into_iter().flatten() {
        match have.get_mut(&key(f)) {
            Some(n) if *n > 0 => *n -= 1,
            _ => by_rule.entry((f["gate"].as_str().unwrap_or("").into(), f["rule"].as_str().unwrap_or("").into(), f["severity"].as_str().unwrap_or("").into())).or_default().push(json!({ "element": f["element"], "message": f["message"], "file": f["file"], "line": f["line"] })),
        }
    }
    by_rule.into_iter().map(|((gate, rule, severity), l)| json!({ "gate": gate, "rule": rule, "severity": severity, "count": l.len(), "findings": l.into_iter().take(20).collect::<Vec<_>>() })).collect()
}

/// `get_change_package`: everything that changed between the model `bm` (the baseline) and the model `am`, as the
/// record of one change. `meta` holds what the caller knows and the models do not: `title`, `reason`, `author`,
/// `prepared`, `from`, `to`, `writes`.
pub fn change_package(bm: &Model, am: &Model, ids: &BTreeMap<String, String>, meta: &Value) -> Value {
    let report = change::compare_models(bm, am, ids, ids, true);
    let (nb, na) = (Names::new(bm), Names::new(am));
    // The names a document shows: a name with its owner. The structured rows keep the full path in `ref` and `label`.
    let (sb, sa) = (Names::brief(bm), Names::brief(am));
    let shown_of = |m: &Model, nm: &Names, e: &Element| -> String {
        match (&e.short_name, e.kind == "requirement" && !e.is_def) {
            (Some(id), true) => format!("{id} {}", e.display_name()),
            _ => {
                let _ = m;
                nm.show(e)
            }
        }
    };

    // ---- the rows of the change report, each with a name a reader knows and the stage it belongs to
    let mut rows: Vec<Row> = Vec::new();
    // What the change touched, as elements of the baseline: the element itself, and the owner of a member that was
    // added, changed or removed. The text says how.
    let mut touched: HashMap<ElementId, Vec<String>> = HashMap::new();
    let mut touch = |id: ElementId, how: String| {
        let l = touched.entry(id).or_default();
        if !l.contains(&how) {
            l.push(how);
        }
    };
    // The element of the baseline that an element of the later state was: by its previous name, else by its name.
    let mut was_named: HashMap<String, String> = HashMap::new();
    for g in ["elements", "connections", "interfaces", "flows", "traceability"] {
        for r in report[g]["changed"].as_array().into_iter().flatten() {
            if let (Some(now), Some(old)) = (r["ref"].as_str(), r["previous_ref"].as_str()) {
                was_named.insert(now.to_string(), old.to_string());
            }
        }
    }
    let before_of = |qn: &str| -> Option<&Element> {
        if has_number_segment(qn) {
            return None;
        }
        bm.find(was_named.get(qn).map(String::as_str).unwrap_or(qn))
    };
    // The first owner that has a name of its own: the element a reader would say was changed.
    let named_owner = |m: &'_ Model, e: &Element| -> Option<ElementId> {
        let mut o = e.owner;
        while let Some(x) = o {
            let oe = &m.elements[x];
            if oe.kind != "objective" && oe.name.is_some() && !has_number_segment(&oe.qualified_name) {
                return Some(x);
            }
            o = oe.owner;
        }
        None
    };
    // An element that came or went whole: a statement inside it is listed with it, not as a row of its own.
    let whole = |list: &str| -> Vec<String> { report["elements"][list].as_array().into_iter().flatten().filter_map(|r| r["ref"].as_str()).map(|q| format!("{q}::")).collect() };
    let (whole_added, whole_removed) = (whole("added"), whole("removed"));
    for g in ["elements", "connections", "interfaces", "flows", "traceability"] {
        for (list, change) in [("added", "added"), ("removed", "removed"), ("changed", "changed"), ("re_ended", "changed")] {
            for r in report[g][list].as_array().into_iter().flatten() {
                let qn = r["ref"].as_str().unwrap_or("");
                if (change == "added" && whole_added.iter().any(|w| qn.starts_with(w.as_str()))) || (change == "removed" && whole_removed.iter().any(|w| qn.starts_with(w.as_str()))) {
                    continue;
                }
                let (m, nm, snm) = if change == "removed" { (bm, &nb, &sb) } else { (am, &na, &sa) };
                let Some(e) = m.find(qn) else { continue };
                if e.kind == "package" && change == "changed" && r["changed"].as_array().map(|l| l.is_empty()).unwrap_or(true) {
                    continue;
                }
                let label = nm.show(e);
                let mut facts = Vec::new();
                let (mut before, mut after) = (Vec::new(), Vec::new());
                for (k, v) in r["facts"].as_object().into_iter().flatten() {
                    let (b, a) = (fact_text(&v["before"]), fact_text(&v["after"]));
                    let (b, a) = (nb.rel(&b), na.rel(&a));
                    facts.push(json!({ "fact": fact_name(k), "before": b, "after": a }));
                    before.push(format!("{}: {}", fact_name(k), if b.is_empty() { "(none)" } else { &b }));
                    after.push(format!("{}: {}", fact_name(k), if a.is_empty() { "(none)" } else { &a }));
                }
                let mut contains: Vec<String> = Vec::new();
                if change != "changed" {
                    let inside = m.descendants(e.id);
                    contains = inside.iter().map(|d| &m.elements[*d]).filter(|d| d.kind != "doc" && d.kind != "objective").take(12).map(|d| nm.show(d)).collect();
                    let value = e.value.as_ref().map(|v| format!("value: {v}"));
                    let ty = e.refs_of(RefRole::Type).next().filter(|_| !matches!(e.kind.as_str(), "flow" | "message")).map(|t| format!("type: {}{}", if t.conjugated { "~" } else { "" }, nm.rel(&t.text)));
                    let doc = super::text::doc_of(m, e).map(|d| format!("text: {d}"));
                    let joins = match (e.refs_of(RefRole::Source).next(), e.refs_of(RefRole::Target).next()) {
                        (Some(a), Some(b)) if matches!(e.kind.as_str(), "connection" | "interface" | "binding" | "flow" | "message" | "allocation") => Some(format!("joins: {} to {}", nm.rel(&a.text), nm.rel(&b.text))),
                        _ => None,
                    };
                    let says: Vec<String> = [joins, ty, value, doc].into_iter().flatten().chain((!contains.is_empty()).then(|| format!("holds: {}", contains.join("; ")))).collect();
                    if change == "added" {
                        after = says;
                    } else {
                        before = says;
                    }
                }
                // What it touches in the baseline.
                let what = format!("{change} {label}");
                let facts_text = facts.iter().map(|f| f["fact"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ");
                match change {
                    "changed" => {
                        if let Some(b) = before_of(qn) {
                            touch(b.id, format!("its {facts_text} changed"));
                        }
                        if let Some(o) = named_owner(am, e).and_then(|o| before_of(&am.elements[o].qualified_name)) {
                            touch(o.id, what);
                        }
                    }
                    "removed" => {
                        touch(e.id, "removed".into());
                        if let Some(o) = named_owner(bm, e) {
                            touch(o, what);
                        }
                    }
                    _ => {
                        if let Some(o) = named_owner(am, e).and_then(|o| before_of(&am.elements[o].qualified_name)) {
                            touch(o.id, what);
                        }
                    }
                }
                rows.push(Row { change, stage: stage_of(m, e), kind: e.keyword(), reference: qn.to_string(), label, shown: shown_of(m, snm, e), before: before.join("; "), after: after.join("; "), facts, contains });
            }
        }
    }

    // ---- the requirement-level summary: text and values, before and after
    let mut requirements = Vec::new();
    let is_req = |e: &&Element| e.kind == "requirement" && !e.is_def;
    let mut matched: BTreeSet<ElementId> = BTreeSet::new();
    let mut changed_reqs: Vec<ElementId> = Vec::new();
    for r in am.elements.iter().filter(is_req) {
        let old = before_of(&r.qualified_name).filter(|o| o.kind == "requirement");
        let (text_a, vals_a) = (super::text::doc_of(am, r).unwrap_or_default(), super::text::values(am, r));
        match old {
            None => requirements.push(json!({ "change": "added", "ref": r.qualified_name, "id": r.short_name, "name": requirement_name(&na, r), "text": { "before": Value::Null, "after": text_a }, "values": vals_a.iter().map(|(n, v, _)| json!({ "name": n, "before": Value::Null, "after": v, "change": "added" })).collect::<Vec<_>>() })),
            Some(o) => {
                matched.insert(o.id);
                let (text_b, vals_b) = (super::text::doc_of(bm, o).unwrap_or_default(), super::text::values(bm, o));
                let mut vals = Vec::new();
                for (n, v, _) in &vals_a {
                    match vals_b.iter().find(|x| &x.0 == n) {
                        Some((_, old_v, _)) if old_v == v => vals.push(json!({ "name": n, "before": old_v, "after": v, "change": "unchanged" })),
                        Some((_, old_v, _)) => vals.push(json!({ "name": n, "before": old_v, "after": v, "change": "changed" })),
                        None => vals.push(json!({ "name": n, "before": Value::Null, "after": v, "change": "added" })),
                    }
                }
                for (n, v, _) in vals_b.iter().filter(|x| !vals_a.iter().any(|y| y.0 == x.0)) {
                    vals.push(json!({ "name": n, "before": v, "after": Value::Null, "change": "removed" }));
                }
                let id_changed = o.short_name != r.short_name || o.name != r.name;
                if text_a != text_b || id_changed || vals.iter().any(|v| v["change"] != "unchanged") {
                    changed_reqs.push(o.id);
                    requirements.push(json!({ "change": "changed", "ref": r.qualified_name, "id": r.short_name, "name": requirement_name(&na, r), "previous_name": if id_changed { json!(requirement_name(&nb, o)) } else { Value::Null }, "text": { "before": text_b, "after": text_a, "changed": text_a != text_b }, "values": vals }));
                }
            }
        }
    }
    for o in bm.elements.iter().filter(is_req).filter(|o| !matched.contains(&o.id)) {
        requirements.push(json!({ "change": "removed", "ref": o.qualified_name, "id": o.short_name, "name": requirement_name(&nb, o), "text": { "before": super::text::doc_of(bm, o).unwrap_or_default(), "after": Value::Null }, "values": super::text::values(bm, o).iter().map(|(n, v, _)| json!({ "name": n, "before": v, "after": Value::Null, "change": "removed" })).collect::<Vec<_>>() }));
    }

    // ---- the gates and the budget, before and after
    let (gb, ga) = (rules::check_all(bm, None, None).unwrap_or(Value::Null), rules::check_all(am, None, None).unwrap_or(Value::Null));
    // A finding names elements by qualified name; a reader gets the names of the state it was found in.
    let readable = |mut l: Vec<Value>, nm: &Names| -> Vec<Value> {
        for f in l.iter_mut().flat_map(|r| r["findings"].as_array_mut().into_iter().flatten()) {
            f["says"] = json!(format!("{}: {}", nm.rewrite(f["element"].as_str().unwrap_or("the model")), nm.rewrite(f["message"].as_str().unwrap_or(""))));
        }
        l
    };
    let gates = json!({ "before": gate_totals(&gb), "after": gate_totals(&ga), "new": readable(findings_only_in(&ga, &gb), &na), "cleared": readable(findings_only_in(&gb, &ga), &nb) });
    let budget = super::budget::delta(bm, am);

    let mut by_stage = Vec::new();
    for (i, (id, title)) in STAGES.iter().enumerate() {
        let of = |change: &str| -> Vec<Value> {
            rows.iter()
                .filter(|r| r.stage == i && r.change == change)
                .map(|r| json!({ "change": r.change, "ref": r.reference, "kind": r.kind, "label": r.label, "shown": r.shown, "before": r.before, "after": r.after, "facts": r.facts, "contains": r.contains }))
                .collect()
        };
        let (added, changed, removed) = (of("added"), of("changed"), of("removed"));
        by_stage.push(json!({ "id": id, "title": title, "counts": { "added": added.len(), "changed": changed.len(), "removed": removed.len() }, "added": added, "changed": changed, "removed": removed }));
    }
    let count = |c: &str| rows.iter().filter(|r| r.change == c).count();
    // A second element of the same name as one this change touched: the reader is told that it exists and was left.
    let mut twin_refs: Vec<String> = rows.iter().filter(|r| r.change != "removed").map(|r| r.reference.clone()).collect();
    twin_refs.extend(touched.keys().map(|i| bm.elements[*i].qualified_name.clone()));
    twin_refs.sort();
    twin_refs.dedup();
    let twins = shown_says(&json!(text::twin_notes(am, &twin_refs, "not changed")), &na);
    let members = json!({ "added": report["summary"]["elements"]["added"], "changed": report["summary"]["elements"]["changed"], "removed": report["summary"]["elements"]["removed"], "counted_as": "every element, each member of an added element counted (change_report_summary.elements)" });
    let counts_says = {
        let (a, c, r) = (count("added"), count("changed"), count("removed"));
        let more = members["added"].as_u64().filter(|m| *m != a as u64).map(|m| format!(" (the members of an added element are not counted; {m} with them)")).unwrap_or_default();
        format!("{a} element(s) added{more}, {c} changed, {r} removed")
    };
    let mut out = json!({
        "title": meta["title"],
        "reason": meta["reason"],
        "author": meta["author"],
        "prepared": meta["prepared"],
        "from": meta["from"],
        "to": meta["to"],
        "writes": meta["writes"],
        "changed": report["changed"],
        "requirements": requirements,
        "changes": { "counts": { "added": count("added"), "changed": count("changed"), "removed": count("removed") }, "counts_detail": { "counted_as": "elements at the top of an addition: an added element and the members it holds are one", "with_members": members, "says": counts_says }, "stages": by_stage },
        "twins": twins,
        "gates": gates,
        "budget": budget,
        "change_report_summary": report["summary"],
    });
    if report["truncated"] == true {
        out["truncated_lists"] = report["truncated_lists"].clone();
    }
    if let Some(n) = report.get("not_fully_compared") {
        out["not_fully_compared"] = n.clone();
    }
    out["markdown"] = json!(markdown(&out));
    out
}

fn s<'a>(v: &'a Value) -> &'a str {
    v.as_str().unwrap_or("")
}

fn cut_rows(rows: &mut Vec<Vec<String>>, doc: &mut String) {
    if rows.len() > DOC_ROWS {
        let more = rows.len() - DOC_ROWS;
        rows.truncate(DOC_ROWS);
        doc.push_str(&format!("The first {DOC_ROWS} rows are shown; {more} more are in the structured reply.\n\n"));
    }
}

/// The package as one Markdown document that stands alone: headings, tables, before and after side by side. A list
/// with nothing in it is a sentence, not an empty table.
pub fn markdown(p: &Value) -> String {
    let mut d = String::new();
    let title = if s(&p["title"]).is_empty() { "Change package" } else { s(&p["title"]) };
    d.push_str(&format!("# {}\n\n", md_text(&visible(title))));
    let state = |v: &Value| {
        let mut parts = Vec::new();
        if let Some(b) = v["baseline"].as_str() {
            parts.push(format!("baseline {b}"));
        } else if !s(&v["is"]).is_empty() {
            parts.push(if s(&v["is"]) == "current" { "the current state".to_string() } else { format!("the state {}", s(&v["is"])) });
        }
        if !s(&v["note"]).is_empty() {
            parts.push(quote_claim(s(&v["note"])));
        }
        parts.push(format!("revision {}", s(&v["revision"])));
        if !s(&v["time"]).is_empty() {
            parts.push(format!("{} {}", if v["baseline"].is_string() { "frozen" } else { "last write" }, s(&v["time"])));
        }
        if let Some(n) = v["element_count"].as_u64() {
            parts.push(format!("{n} elements"));
        }
        parts.join(", ")
    };
    let mut head = vec![vec!["Reason".to_string(), if s(&p["reason"]).is_empty() { "not stated".into() } else { if s(&p["reason"]).contains(['\n', '\r']) { "the proposer's words, quoted below the table".into() } else { visible(s(&p["reason"])) } }], vec!["Author (signed as)".into(), if s(&p["author"]).is_empty() { "not stated".into() } else { quote_claim(s(&p["author"])) }], vec!["From".into(), state(&p["from"])], vec!["To".into(), state(&p["to"])]];
    let w = &p["writes"];
    if let Some(n) = w["count"].as_u64() {
        head.push(vec!["Writes in this change".into(), if n == 0 { "none recorded in the write log".to_string() } else { format!("{n} (write #{} to #{}, {} to {}), signed as {}", w["first"], w["last"], s(&w["first_time"]), s(&w["last_time"]), w["authors"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(quote_claim).collect::<Vec<_>>().join(", ")).unwrap_or_default()) }]);
    }
    if !s(&p["prepared"]).is_empty() {
        head.push(vec!["Prepared".into(), s(&p["prepared"]).to_string()]);
    }
    let c = &p["changes"]["counts"];
    head.push(vec!["Size".into(), format!("{} requirement(s) changed; {}", p["requirements"].as_array().map_or(0, Vec::len), p["changes"]["counts_detail"]["says"].as_str().map(str::to_string).unwrap_or_else(|| format!("{} element(s) added, {} changed, {} removed", c["added"], c["changed"], c["removed"])))]);
    d.push_str(&md_table(&["Change record", "Value"], &head));
    d.push('\n');
    if s(&p["reason"]).contains(['\n', '\r']) {
        d.push_str(&super::text::md_quote("The reason, as the proposer wrote it", s(&p["reason"])));
    }
    if p["changed"] == false {
        d.push_str("The two states are the same model: nothing changed.\n\n");
    }

    // 1
    d.push_str("## 1. Requirements changed\n\n");
    let reqs = p["requirements"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if reqs.is_empty() {
        d.push_str("No requirement was added, removed or changed.\n\n");
    } else {
        let mut rows = Vec::new();
        for r in reqs {
            let name = s(&r["name"]).to_string();
            let change = s(&r["change"]);
            if r["text"]["changed"] != false {
                rows.push(vec![name.clone(), change.to_string(), "text".into(), s(&r["text"]["before"]).to_string(), s(&r["text"]["after"]).to_string()]);
            }
            for v in r["values"].as_array().into_iter().flatten().filter(|v| v["change"] != "unchanged") {
                rows.push(vec![name.clone(), format!("value {}", s(&v["change"])), s(&v["name"]).to_string(), v["before"].as_str().map(|x| format!("{} = {x}", s(&v["name"]))).unwrap_or_default(), v["after"].as_str().map(|x| format!("{} = {x}", s(&v["name"]))).unwrap_or_default()]);
            }
            if !r["previous_name"].is_null() {
                rows.push(vec![name.clone(), change.to_string(), "id or name".into(), s(&r["previous_name"]).to_string(), name.clone()]);
            }
        }
        cut_rows(&mut rows, &mut d);
        d.push_str(&md_table(&["Requirement", "Change", "What", "Before", "After"], &rows));
        d.push('\n');
    }

    // 2
    d.push_str("## 2. What changed, by stage\n\n");
    let mut any = false;
    for st in p["changes"]["stages"].as_array().into_iter().flatten() {
        let n = &st["counts"];
        let total = n["added"].as_u64().unwrap_or(0) + n["changed"].as_u64().unwrap_or(0) + n["removed"].as_u64().unwrap_or(0);
        if total == 0 {
            continue;
        }
        any = true;
        d.push_str(&format!("### {} ({} added, {} changed, {} removed)\n\n", md_text(s(&st["title"])), n["added"], n["changed"], n["removed"]));
        let mut rows = Vec::new();
        for list in ["changed", "added", "removed"] {
            for r in st[list].as_array().into_iter().flatten() {
                let name = if s(&r["shown"]).is_empty() { s(&r["label"]) } else { s(&r["shown"]) };
                rows.push(vec![list.to_string(), name.to_string(), s(&r["kind"]).to_string(), s(&r["before"]).to_string(), s(&r["after"]).to_string()]);
            }
        }
        cut_rows(&mut rows, &mut d);
        d.push_str(&md_table(&["Change", "Element", "Kind", "Before", "After"], &rows));
        d.push('\n');
    }
    let quiet: Vec<&str> = p["changes"]["stages"].as_array().into_iter().flatten().filter(|st| ["added", "changed", "removed"].iter().all(|k| st["counts"][k] == 0) && st["id"] != "other").map(|st| s(&st["title"])).collect();
    if !any {
        d.push_str("No element was added, removed or changed.\n\n");
    } else if !quiet.is_empty() {
        d.push_str(&format!("Nothing changed in: {}.\n\n", quiet.join(", ")));
    }

    // Twins: what a reader could miss.
    let twins = p["twins"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if !twins.is_empty() {
        d.push_str("Same name in another place (an element this change touched has a twin):\n\n");
        for t in twins {
            d.push_str(&format!("- {}\n", md_text(said(t))));
        }
        d.push('\n');
    }
    // 3
    d.push_str("## 3. Checks before and after\n\n");
    let g = &p["gates"];
    if g["before"].is_object() && g["after"].is_object() {
        let line = |v: &Value| format!("{}: {} errors, {} warnings, {} infos", if v["passed"] == true { "passed" } else { "FAILED" }, v["errors"], v["warnings"], v["infos"]);
        let mut rows = vec![vec!["All gates".to_string(), line(&g["before"]), line(&g["after"])]];
        let names: BTreeSet<&String> = g["before"]["gates"].as_object().into_iter().flatten().chain(g["after"]["gates"].as_object().into_iter().flatten()).map(|(k, _)| k).collect();
        for n in names {
            let cell = |v: &Value| if v["gates"][n.as_str()].is_object() { line(&v["gates"][n.as_str()]) } else { "not run".to_string() };
            rows.push(vec![n.clone(), cell(&g["before"]), cell(&g["after"])]);
        }
        d.push_str(&md_table(&["Gate", "Before", "After"], &rows));
        d.push('\n');
        let reg = rules::registry();
        let standing: Vec<String> = g["after"]["by_rule"].as_array().into_iter().flatten().map(|r| {
            let meaning = reg.iter().find(|i| i.id == s(&r["rule"])).map(|i| format!(" (the rule: {})", i.meaning.trim_end_matches('.'))).unwrap_or_default();
            format!("{}/{} ({}) x{}{}", md_text(s(&r["gate"])), md_text(s(&r["rule"])), md_text(s(&r["severity"])), r["count"], md_text(&meaning))
        }).collect();
        if !standing.is_empty() {
            d.push_str(&format!("Findings that stand in the model after the change, by rule: {}.\n\n", standing.join("; ")));
        }
        for (key, title, none) in [("new", "New findings", "No new finding: the change added no error, warning or info."), ("cleared", "Cleared findings", "No finding was cleared.")] {
            let l = g[key].as_array().map(Vec::as_slice).unwrap_or(&[]);
            if l.is_empty() {
                d.push_str(&format!("{none}\n\n"));
                continue;
            }
            d.push_str(&format!("{title}, by rule:\n\n"));
            let rows: Vec<Vec<String>> = l.iter().map(|r| vec![format!("{}/{}", s(&r["gate"]), s(&r["rule"])), s(&r["severity"]).to_string(), r["count"].to_string(), r["findings"].as_array().and_then(|f| f.first()).map(|f| s(&f["says"]).to_string()).unwrap_or_default()]).collect();
            d.push_str(&md_table(&["Rule", "Severity", "Count", "First finding"], &rows));
            d.push('\n');
        }
    } else {
        d.push_str("The gates could not be run on both states.\n\n");
    }

    d.push_str(&budget_markdown(&p["budget"]));
    d
}

/// Section 4 of the record: the budget before and after. A model that states no amount and no limit has no section.
fn budget_markdown(b: &Value) -> String {
    if b["relevant"] != true {
        return String::new();
    }
    let q = s(&b["quantity"]);
    let mut d = format!("\n## 4. Budget: {q}\n\n");
    let limit = |v: &Value| match v["limit"].as_object() {
        Some(_) => format!("{} ({})", s(&v["limit"]["limit_text"]), s(&v["limit"]["label"])),
        None => "no limit stated".to_string(),
    };
    let margin = |v: &Value| if v["limit"].is_object() { s(&v["limit"]["margin_text"]).to_string() } else { String::new() };
    let verdict = |v: &Value| match s(&v["verdict"]) {
        "pass" => "PASS".to_string(),
        "fail" => "FAIL".to_string(),
        other => other.to_string(),
    };
    let (x, y) = (&b["before"], &b["after"]);
    let rows = vec![
        vec!["Root used".to_string(), s(&x["root"]).to_string(), s(&y["root"]).to_string()],
        vec![format!("Total {q}"), s(&x["total_text"]).to_string(), s(&y["total_text"]).to_string()],
        vec!["Limit".to_string(), limit(x), limit(y)],
        vec!["Margin".to_string(), margin(x), margin(y)],
        vec!["Verdict".to_string(), verdict(x), verdict(y)],
        vec![format!("Parts that state no {q}"), x["not_stated"].to_string(), y["not_stated"].to_string()],
    ];
    d.push_str(&md_table(&["Budget", "Before", "After"], &rows));
    d.push('\n');
    let parts = b["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if parts.is_empty() {
        d.push_str(&format!("No part changed its {q}.\n"));
    } else {
        d.push_str(&format!("Parts whose {q} changed:\n\n"));
        let mut rows: Vec<Vec<String>> = parts.iter().map(|r| vec![s(&r["change"]).to_string(), s(&r["label"]).to_string(), s(&r["before"]).to_string(), s(&r["after"]).to_string()]).collect();
        cut_rows(&mut rows, &mut d);
        d.push_str(&md_table(&["Change", "Part", "Before", "After"], &rows));
    }
    d
}
