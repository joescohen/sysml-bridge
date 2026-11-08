//! `delete_relationship {kind, source, target, of?, owner?, all?}`: delete a relationship statement by what it relates,
//! so no caller needs the positional `Owner::$n` ref of an unnamed statement. The statement is found among the
//! statements the model reads (satisfy, verify, allocate, derive, refine, trace, dependency, connect, flow, succession
//! flow, succession, transition, bind) and the `perform` usages of a part; the delete itself is `delete_element`.

use super::*;
use crate::agent::graph;

const KINDS: &str = "satisfy, verify, allocate, derive, refine, trace, dependency, connect, flow, succession flow, succession, transition, bind or perform";

/// The statement kind the graph reads for a kind the caller may write.
fn graph_kind(k: &str) -> Option<(&'static str, Option<bool>)> {
    // The bool says whether the flow must (Some(true)) or must not (Some(false)) be a succession flow.
    Some(match k.trim().to_ascii_lowercase().replace(['_', '-'], " ").as_str() {
        "satisfy" => ("satisfy", None),
        "verify" => ("verify", None),
        "allocate" | "allocation" => ("allocate", None),
        "derive" => ("derive", None),
        "refine" => ("refine", None),
        "trace" => ("trace", None),
        "dependency" => ("dependency", None),
        "connect" | "connection" | "interface" => ("connect", None),
        "flow" | "item flow" => ("flow", Some(false)),
        "succession flow" | "successionflow" => ("flow", Some(true)),
        "object flow" => ("flow", None),
        "succession" | "control flow" => ("succession", None),
        "transition" => ("transition", None),
        "bind" | "binding" => ("bind", None),
        "perform" => ("perform", None),
        _ => return None,
    })
}

fn norm_path(s: &str) -> String {
    s.trim().replace('.', "::").split("::").map(|g| g.trim().trim_matches('\'')).collect::<Vec<_>>().join("::")
}

/// How well an end matches what the caller gave: 2 by its text (as written, or below the statement's owner), 1 by the
/// element it resolves to, 0 not at all.
fn end_score(model: &Model, via: &Element, end: &graph::End, given: &str, located: Option<ElementId>) -> u8 {
    let g = norm_path(given);
    let text = norm_path(&end.text);
    let owner_qn = via.owner.map(|o| norm_path(&model.elements[o].qualified_name)).unwrap_or_default();
    if g == text || (!owner_qn.is_empty() && g == format!("{owner_qn}::{text}")) {
        return 2;
    }
    // Written below an ancestor of the statement: `Pkg::Act::a::x` for `a.x` declared in `Pkg::Act::$3`.
    let mut o = via.owner;
    while let Some(x) = o {
        if g == format!("{}::{text}", norm_path(&model.elements[x].qualified_name)) {
            return 2;
        }
        o = model.elements[x].owner;
    }
    match (located, end.id) {
        (Some(a), Some(b)) if a == b => 1,
        _ => 0,
    }
}

fn item_of(model: &Model, e: &Element) -> Option<ElementId> {
    e.refs.iter().find(|r| r.role == RefRole::Type).and_then(|r| r.resolved()).map(|t| graph::unalias(model, t))
}

/// Every statement that matches, best matches only, as (element, score).
fn matches(model: &Model, a: &Value) -> Res<Vec<ElementId>> {
    let given_kind = req_as(a, "kind", &format!("the kind of relationship: {KINDS}"))?;
    let (kind, succ) = graph_kind(given_kind).ok_or_else(|| Fail::from(format!("unknown relationship kind '{given_kind}'. Fix: pass one of {KINDS}.")))?;
    let source = req_as(a, "source", "one end as create_relationship took it (for satisfy: the requirement's satisfier; for a flow: the out pin)")?;
    let target = req_as(a, "target", "the other end as create_relationship took it")?;
    let at = |s: &str| locate_ref(model, s).ok().map(|l| graph::unalias(model, l.el.id));
    let (ls, lt) = (at(source), at(target));
    let owner = match text_arg(a, "owner") {
        Some(o) => Some(locate(model, o)?.id),
        None => None,
    };
    let item = match text_arg(a, "of").or_else(|| text_arg(a, "item")) {
        Some(i) => Some(graph::unalias(model, locate(model, i)?.id)),
        None => None,
    };
    let mut found: Vec<(ElementId, u8)> = Vec::new();
    if kind == "perform" {
        for e in model.elements.iter().filter(|e| e.kind.starts_with("perform") && !e.is_def) {
            let Some(o) = e.owner else { continue };
            let did = e.refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::References | RefRole::Target)).filter_map(|r| r.resolved()).any(|t| Some(graph::unalias(model, t)) == lt);
            if Some(o) == ls && did && owner.map(|w| w == o).unwrap_or(true) {
                found.push((e.id, 2));
            }
        }
    } else {
        for s in graph::statements(model).into_iter().filter(|s| s.kind == kind) {
            let via = &model.elements[s.via];
            if let Some(want) = succ {
                if via.has_prefix("succession") != want {
                    continue;
                }
            }
            if owner.map(|w| via.owner != Some(w) && via.id != w).unwrap_or(false) {
                continue;
            }
            if item.is_some() && item_of(model, via) != item {
                continue;
            }
            let (a1, b1) = (end_score(model, via, &s.source, source, ls), end_score(model, via, &s.target, target, lt));
            if a1 > 0 && b1 > 0 && !found.iter().any(|(x, _)| *x == s.via) {
                found.push((s.via, a1 + b1));
            }
        }
    }
    let best = found.iter().map(|(_, s)| *s).max().unwrap_or(0);
    Ok(found.into_iter().filter(|(_, s)| *s == best).map(|(e, _)| e).collect())
}

pub(super) fn delete_relationship(work: &mut Vec<FileIn>, a: &Value, events: &mut Vec<IdEvent>) -> Res<Value> {
    let all = flag(a, "all");
    let mut deleted: Vec<Value> = Vec::new();
    loop {
        let model = model_of(work);
        let hits = matches(&model, a)?;
        if hits.is_empty() {
            if !deleted.is_empty() {
                break;
            }
            return Err(Fail::with(
                format!(
                    "no {} from {} to {} in the model. Fix: pass the ends as create_relationship took them (the qualified names, or the ends as the statement writes them, such as plan.path); `show` or `trace` on an end lists its relationships.",
                    req(a, "kind")?,
                    crate::agent::text::quote_clip(req(a, "source")?),
                    crate::agent::text::quote_clip(req(a, "target")?)
                ),
                json!({ "rule": "relationship-not-found" }),
            ));
        }
        if hits.len() > 1 && !all {
            let list: Vec<String> = hits.iter().map(|h| model.elements[*h].qualified_name.clone()).collect();
            return Err(Fail::with(
                format!("{} relationships match: {}. Fix: pass owner or of (the item) to pick one, or all: true to delete every one.", hits.len(), list.join(", ")),
                json!({ "rule": "relationship-ambiguous", "candidates": list }),
            ));
        }
        // One at a time, the last in the text first, so the positional names of the others hold.
        let h = *hits.iter().max_by_key(|h| (model.elements[**h].file, model.elements[**h].span.start)).unwrap();
        let qn = model.elements[h].qualified_name.clone();
        let text = model.files.get(model.elements[h].file).map(|f| f.text[model.elements[h].span.start..model.elements[h].span.end].to_string()).unwrap_or_default();
        delete_element(work, &json!({ "ref": qn }), events)?;
        deleted.push(json!({ "ref": qn, "text": text }));
        if !all || deleted.len() >= MAX_LIST {
            break;
        }
    }
    Ok(json!({ "deleted": deleted, "ref": deleted[0]["ref"] }))
}
