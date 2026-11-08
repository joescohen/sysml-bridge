//! Validation results for marking: the rule results of the model (or of one package), grouped by rule with the plain sentence of
//! the rule, placed on the elements, and rolled up by owner up the tree (worst severity and counts for every package and owner
//! that holds a result). Nothing is stored here: the findings come from `rules::check_all` and the ignore records that a person
//! made are marked on them by the caller (`ignored`). Bounds are stated in the reply.

use super::line_of;
use super::{rules, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

/// Most findings placed on the tree in one call; the rest are counted (`unplaced_by_bound`).
pub const MAX_PLACED: usize = 200_000;
/// Most roll-up rows in a reply.
pub const MAX_ROLLUP: usize = 5000;
/// Most result rows in a reply.
pub const MAX_RESULTS: usize = 1000;

pub struct Opts {
    pub scope: Option<String>,
    pub min_severity: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

fn rank(s: &str) -> u8 {
    match s {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    }
}

#[derive(Default, Clone, Copy)]
struct Counts {
    errors: usize,
    warnings: usize,
    infos: usize,
    ignored: usize,
    own: usize,
}

impl Counts {
    fn add(&mut self, sev: &str, ignored: bool) {
        if ignored {
            self.ignored += 1;
        } else {
            match sev {
                "error" => self.errors += 1,
                "warning" => self.warnings += 1,
                _ => self.infos += 1,
            }
        }
    }
    fn worst(&self) -> Option<&'static str> {
        if self.errors > 0 {
            Some("error")
        } else if self.warnings > 0 {
            Some("warning")
        } else if self.infos > 0 {
            Some("info")
        } else {
            None
        }
    }
}

/// `all` is a `check_all` reply with `fingerprint` and `ignored` on its findings.
pub fn results(model: &Model, all: &Value, o: &Opts) -> Res<Value> {
    let scope = match &o.scope {
        Some(s) => Some(model.find(s).ok_or_else(|| format!("scope {} is not an element of the model. Give the qualified name of a package or element.", super::text::quote_clip(s)))?.qualified_name.clone()),
        None => None,
    };
    let min = match o.min_severity.as_deref() {
        None | Some("") => 2,
        Some(s @ ("error" | "warning" | "info")) => rank(s),
        Some(s) => return Err(format!("min_severity is error, warning or info, not {}.", super::text::quote_clip(s)).into()),
    };
    let sentences: HashMap<String, String> = rules::list_rules(None, None)?["rules"].as_array().into_iter().flatten().filter_map(|r| Some((r["id"].as_str()?.to_string(), r["meaning"].as_str()?.to_string()))).collect();
    let in_scope = |f: &Value| match (&scope, f["element"].as_str()) {
        (None, _) => true,
        (Some(s), Some(e)) => e == s || e.starts_with(&format!("{s}::")),
        (Some(_), None) => false,
    };
    let all_f = all["findings"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let kept: Vec<&Value> = all_f.iter().filter(|f| in_scope(f) && rank(f["severity"].as_str().unwrap_or("info")) <= min).collect();
    let mut total = Counts::default();
    let mut by_rule: BTreeMap<(String, String), (String, usize, usize, Vec<String>)> = BTreeMap::new();
    let mut tree: HashMap<ElementId, Counts> = HashMap::new();
    let (mut unplaced, mut beyond) = (0usize, 0usize);
    for (i, f) in kept.iter().enumerate() {
        let sev = f["severity"].as_str().unwrap_or("info");
        let ign = f["ignored"] == true;
        total.add(sev, ign);
        let row = by_rule.entry((f["gate"].as_str().unwrap_or("").to_string(), f["rule"].as_str().unwrap_or("").to_string())).or_insert((sev.to_string(), 0, 0, Vec::new()));
        if ign {
            row.2 += 1;
        } else {
            row.1 += 1;
            if row.3.len() < 5 {
                if let Some(e) = f["element"].as_str() {
                    row.3.push(e.to_string());
                }
            }
        }
        if i >= MAX_PLACED {
            beyond += 1;
            continue;
        }
        match f["element"].as_str().and_then(|e| model.find(e)) {
            None => unplaced += 1,
            Some(e) => {
                tree.entry(e.id).or_default().add(sev, ign);
                tree.get_mut(&e.id).unwrap().own += 1;
                let mut at = e.owner;
                let mut steps = 0;
                while let Some(o) = at {
                    tree.entry(o).or_default().add(sev, ign);
                    at = model.elements[o].owner;
                    steps += 1;
                    if steps > 10_000 {
                        break;
                    }
                }
            }
        }
    }
    let mut rules_out: Vec<Value> = by_rule
        .into_iter()
        .map(|((gate, rule), (sev, n, ign, els))| {
            json!({ "gate": gate, "rule": rule, "abbreviation": rule.to_ascii_uppercase(), "severity": sev, "sentence": sentences.get(&rule), "count": n, "ignored": ign, "examples": els })
        })
        .collect();
    rules_out.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()).then(a["rule"].as_str().cmp(&b["rule"].as_str())));
    let mut roll: Vec<(String, ElementId, Counts)> = tree.into_iter().map(|(id, c)| (model.elements[id].qualified_name.clone(), id, c)).collect();
    roll.sort_by(|a, b| a.0.cmp(&b.0));
    let roll_total = roll.len();
    let rollup: Vec<Value> = roll
        .iter()
        .take(MAX_ROLLUP)
        .map(|(q, id, c)| {
            let e = &model.elements[*id];
            json!({ "ref": q, "kind": e.keyword(), "worst": c.worst(), "errors": c.errors, "warnings": c.warnings, "infos": c.infos, "ignored": c.ignored, "own": c.own > 0, "file": model.files[e.file].path, "line": line_of(model, e) })
        })
        .collect();
    let limit = if o.limit == 0 { 200 } else { o.limit.min(MAX_RESULTS) };
    let page: Vec<Value> = kept
        .iter()
        .skip(o.offset)
        .take(limit)
        .map(|f| {
            let rule = f["rule"].as_str().unwrap_or("");
            json!({
                "element": f["element"], "severity": f["severity"], "rule": rule, "abbreviation": rule.to_ascii_uppercase(), "message": f["message"], "sentence": sentences.get(rule),
                "gate": f["gate"], "file": f["file"], "line": f["line"], "fix": f["fix"], "fingerprint": f["fingerprint"], "ignored": f["ignored"],
            })
        })
        .collect();
    let mut bounds: Vec<String> = Vec::new();
    if beyond > 0 {
        bounds.push(format!("placing on the tree: {beyond} result(s) past the limit of {MAX_PLACED} are in the counts and the rule groups but not in the roll-up"));
    }
    if roll_total > MAX_ROLLUP {
        bounds.push(format!("roll-up: {roll_total} owners hold results, the first {MAX_ROLLUP} (by path) are listed"));
    }
    Ok(json!({
        "scope": scope,
        "min_severity": o.min_severity,
        "summary": { "errors": total.errors, "warnings": total.warnings, "infos": total.infos, "ignored": total.ignored, "results": kept.len(), "passed": total.errors == 0,
                     "note": "ignored results are listed and counted in `ignored`, and left out of errors, warnings and infos" },
        "by_rule": rules_out,
        "rollup": rollup,
        "rollup_total": roll_total,
        "unplaced": unplaced,
        "results": page,
        "offset": o.offset,
        "limit": limit,
        "truncated": kept.len() > o.offset + page.len(),
        "bounds": bounds,
        "gates_not_run": all["gates"].as_object().map(|g| g.iter().filter(|(_, v)| v["ran"] == false).map(|(k, _)| k.clone()).collect::<Vec<_>>()).unwrap_or_default(),
    }))
}
