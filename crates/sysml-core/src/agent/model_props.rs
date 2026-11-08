//! Derived properties that a team declared in the model (an imported customization): read as data from the
//! `SysMLv1::derivedPropertySpecification` metadata, listed with `source: "model"`, and evaluated when they are a simple chain of
//! hops over satisfy, verify, refine, derive, allocate or trace links. Any other is listed with the reason it is not evaluated.

use super::line_of;
use super::trace_props::{Index, Prop, Side};
use crate::model::*;
use serde_json::{json, Value};

/// Most declarations read, most hops of one chain, and the steps one call may spend evaluating all of them. A chain of more
/// hops is listed, not evaluated; a call that runs out of steps lists the rest as not evaluated, with the reason.
const MAX_DECLARED: usize = 500;
pub const MAX_HOPS: usize = 8;
pub const STEP_BUDGET: usize = 20_000;
/// Longest name or applies-to text kept from a declaration.
const MAX_FIELD: usize = 120;

pub struct Decl {
    pub name: String,
    pub applies_to: String,
    pub hops: Vec<(&'static str, bool)>,
    /// `Some` when the property is not evaluated: the reason.
    pub reason: Option<String>,
    pub at: ElementId,
}

fn key_of(c: &Element) -> Option<String> {
    c.refs.iter().find(|r| r.role == RefRole::Assign).map(|r| r.text.clone()).or_else(|| c.name.clone()).filter(|k| !k.is_empty()).map(|k| k.trim_matches('\'').to_string())
}

fn text_of(v: &str) -> String {
    let t = v.trim();
    t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t).to_string()
}

/// A text a declaration gives, kept at most `MAX_FIELD` characters.
fn kept(v: &str) -> String {
    v.chars().take(MAX_FIELD).collect()
}

fn kind_static(k: &str) -> Option<&'static str> {
    Some(match k {
        "satisfy" => "satisfy",
        "verify" => "verify",
        "refine" => "refine",
        "derive" => "derive",
        "allocate" => "allocate",
        "trace" => "trace",
        _ => return None,
    })
}

pub fn declared(m: &Model) -> Vec<Decl> {
    let mut out = Vec::new();
    for e in m.elements.iter().filter(|e| e.kind == "metadata" && !e.is_def) {
        if !e.refs.iter().any(|r| r.role == RefRole::Type && r.text.ends_with("derivedPropertySpecification")) {
            continue;
        }
        if out.len() >= MAX_DECLARED {
            break;
        }
        let (mut name, mut applies, mut hops, mut eval) = (String::new(), String::new(), String::new(), String::new());
        for c in e.children.iter().map(|c| &m.elements[*c]) {
            let (Some(k), Some(v)) = (key_of(c), c.value.as_deref()) else { continue };
            match k.as_str() {
                "derivedName" => name = kept(&text_of(v)),
                "appliesTo" => applies = kept(&text_of(v)),
                "hops" => hops = text_of(v),
                "evaluation" => eval = text_of(v),
                _ => {}
            }
        }
        let mut chain: Vec<(&'static str, bool)> = Vec::new();
        // What the model says about the evaluation is a claim of the file: it stands quoted, after our own words.
        let mut reason = if eval == "evaluated" {
            None
        } else if eval.is_empty() {
            Some("declared in the model, not evaluated: no evaluation was recorded".to_string())
        } else {
            let said = eval.strip_prefix("declared in the model, not evaluated:").unwrap_or(&eval).trim();
            Some(format!("declared in the model, not evaluated; the model gives this reason: {}", super::text::quote_clip(said)))
        };
        if reason.is_none() && hops.split(", ").filter(|h| !h.is_empty()).nth(MAX_HOPS).is_some() {
            reason = Some(format!("declared in the model, not evaluated: more than {MAX_HOPS} hops"));
        }
        if reason.is_none() {
            for h in hops.split(", ").filter(|h| !h.is_empty()).take(MAX_HOPS) {
                let (k, d) = h.split_once(':').unwrap_or((h, "direct"));
                match kind_static(k) {
                    Some(k) => chain.push((k, d == "opposite")),
                    None => reason = Some(format!("declared in the model, not evaluated: the hop {} is not one the tools read", super::text::quote_clip(h))),
                }
            }
            if chain.is_empty() && reason.is_none() {
                reason = Some("declared in the model, not evaluated: no hop".to_string());
            }
        }
        out.push(Decl { name, applies_to: applies, hops: chain, reason, at: e.id });
    }
    out
}

/// The row of `list_traceability_properties` for one declaration.
pub fn table_row(m: &Model, d: &Decl) -> Value {
    let e = &m.elements[d.at];
    let mut v = json!({
        "name": super::text::model_name(&d.name),
        "source": "model",
        "applies_to": d.applies_to,
        "evaluated": d.reason.is_none(),
        "hops": d.hops.iter().map(|(k, o)| format!("{k}:{}", if *o { "opposite" } else { "direct" })).collect::<Vec<_>>(),
        "declared_at": { "file": m.files[e.file].path, "line": line_of(m, e) },
    });
    if let Some(r) = &d.reason {
        v["reason"] = json!(r);
    }
    v
}

fn applies(m: &Model, d: &Decl, id: ElementId) -> bool {
    let e = &m.elements[id];
    match d.applies_to.to_ascii_lowercase().as_str() {
        "" => true,
        "requirement" => e.kind == "requirement",
        "block" | "class" => e.kind == "part",
        _ => true,
    }
}

/// The model-declared properties of one element: each evaluated one with its items (the elements reached through the hops), each other one with its reason.
pub fn of(m: &Model, ix: &Index, id: ElementId) -> Vec<Value> {
    of_with(m, ix, id, STEP_BUDGET)
}

/// `of` with the step budget of the call given (the tests use a small one).
pub fn of_with(m: &Model, ix: &Index, id: ElementId, budget: usize) -> Vec<Value> {
    let mut steps = 0usize;
    declared(m)
        .iter()
        .filter(|d| applies(m, d, id))
        .map(|d| {
            let mut row = table_row(m, d);
            if d.reason.is_none() && steps >= budget {
                row["evaluated"] = json!(false);
                row["reason"] = json!(format!("declared in the model, not evaluated: the step budget of one call ({budget} steps) was used up before this property"));
            } else if d.reason.is_none() {
                let mut cur: Vec<(ElementId, ElementId, &'static str)> = vec![(id, id, "")];
                for (kind, opposite) in &d.hops {
                    let p = Prop { name: "", all_name: "", group: "", applies_to: "any element", kind, side: if *opposite { Side::FromTarget } else { Side::FromSource }, rule: "" };
                    let mut next = Vec::new();
                    let mut seen = std::collections::BTreeSet::new();
                    for (x, _, _) in &cur {
                        steps += 1;
                        if steps > budget {
                            break;
                        }
                        for h in ix.hop(m, &p, *x) {
                            if seen.insert(h.0) && next.len() < 200 {
                                next.push(h);
                            }
                        }
                    }
                    cur = next;
                }
                if steps > budget {
                    row["evaluated"] = json!(false);
                    row["reason"] = json!(format!("declared in the model, not evaluated: the step budget of one call ({budget} steps) ran out while evaluating it"));
                } else {
                    let items: Vec<Value> = cur.iter().map(|(o, v, k)| ix.entry_of(m, *o, *v, k)).collect();
                    row["count"] = json!(items.len());
                    row["items"] = json!(items);
                }
            }
            row
        })
        .collect()
}
