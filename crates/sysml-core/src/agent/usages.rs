//! Used By and Depends On: for one element (and, if asked, everything it owns), every place the model uses it or that it uses,
//! each with the reason in a fixed list of plain words, the file and line, counts by reason, and the level of detail
//! (elements, classifiers or packages). Read from the relationship statements of the model (`graph::statements_cut`), so the same
//! facts stand in the same words wherever they are shown. Nothing is stored; no judgement is written.

use super::graph;
use super::line_of;
use super::{Fail, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// Most rows in a reply; the counts are always the full ones.
pub const MAX_ROWS: usize = 1000;

/// The fixed list of reasons: every `used_as` of a reply is one of these (the words never carry a raw metaclass name).
pub const REASONS: &[&str] = &[
    "type of part", "type of port", "type of attribute", "type of item", "type of other usage", "typed by",
    "specialized by", "specializes", "redefined by", "redefines", "referenced by", "references",
    "satisfied by", "satisfies", "verified by", "verifies", "refined by", "refines", "derived by", "derives from",
    "allocated to it", "allocated to", "traced from", "traces to", "depended on by", "depends on",
    "end of connector", "end of binding", "end of flow", "in succession with", "end of transition", "linked",
    "mentioned in text", "mentions in text",
];

pub struct Opts {
    pub direction: String,
    pub recursive: bool,
    pub ignore_inner: bool,
    pub level: String,
    pub offset: usize,
    pub limit: usize,
    /// The references in text of the model (see `element_links::mentions`): a use of the element they name, owned by the text's element.
    pub mentions: Vec<(ElementId, ElementId)>,
    /// The scan of the texts hit a bound: the mentions listed are not all of them.
    pub mentions_cut: bool,
}

fn words(kind: &str, row: &Element) -> (&'static str, &'static str) {
    match kind {
        "typing" => (
            match row.kind.as_str() {
                "part" | "ref" => "type of part",
                "port" => "type of port",
                "attribute" => "type of attribute",
                "item" => "type of item",
                _ => "type of other usage",
            },
            "typed by",
        ),
        "specialization" => ("specialized by", "specializes"),
        "redefinition" => ("redefined by", "redefines"),
        "reference" => ("referenced by", "references"),
        "satisfy" => ("satisfied by", "satisfies"),
        "verify" => ("verified by", "verifies"),
        "refine" => ("refined by", "refines"),
        "derive" => ("derived by", "derives from"),
        "allocate" => ("allocated to it", "allocated to"),
        "trace" => ("traced from", "traces to"),
        "dependency" => ("depended on by", "depends on"),
        "connect" => ("end of connector", "end of connector"),
        "bind" => ("end of binding", "end of binding"),
        "flow" | "itemflow" => ("end of flow", "end of flow"),
        "succession" => ("in succession with", "in succession with"),
        "transition" => ("end of transition", "end of transition"),
        _ => ("linked", "linked"),
    }
}

fn symmetric(kind: &str) -> bool {
    matches!(kind, "connect" | "bind" | "flow" | "itemflow" | "succession" | "transition")
}

fn descendants(m: &Model, id: ElementId, out: &mut HashSet<ElementId>) {
    let mut stack = vec![id];
    while let Some(x) = stack.pop() {
        if out.insert(x) {
            stack.extend(m.elements[x].children.iter().copied());
        }
    }
}

/// The element a row stands for at a level: itself, its nearest classifier (a definition) or its nearest package.
fn at_level(m: &Model, id: ElementId, level: &str) -> ElementId {
    if level == "elements" {
        return id;
    }
    let mut at = Some(id);
    while let Some(x) = at {
        let e = &m.elements[x];
        let hit = match level {
            "classifiers" => e.is_def || e.kind == "package",
            _ => e.kind == "package" || e.kind == "library package",
        };
        if hit {
            return x;
        }
        at = e.owner;
    }
    id
}

pub fn usages(m: &Model, name: &str, o: &Opts) -> Res<Value> {
    let subject_el = m.find(name).ok_or_else(|| Fail::from(format!("{} is not an element of the model.", super::text::quote_clip(name))))?;
    let used_by = match o.direction.as_str() {
        "used_by" | "" => true,
        "depends_on" => false,
        other => return Err(format!("direction is used_by or depends_on, not {}.", super::text::quote_clip(other)).into()),
    };
    if !matches!(o.level.as_str(), "elements" | "classifiers" | "packages") {
        return Err(format!("level is elements, classifiers or packages, not {}.", super::text::quote_clip(&o.level)).into());
    }
    let mut subject: HashSet<ElementId> = HashSet::new();
    if o.recursive {
        descendants(m, subject_el.id, &mut subject);
    } else {
        subject.insert(subject_el.id);
    }
    let (edges, cuts) = graph::edges_cut(m);
    // (row element, reason, link statement)
    let mut hits: Vec<(ElementId, &'static str, ElementId)> = Vec::new();
    let mut dropped_inner = 0usize;
    for (s, a, b) in &edges {
        let (a, b) = (graph::unalias(m, *a), graph::unalias(m, *b));
        let mut push = |row: ElementId, reason: &'static str| {
            if o.ignore_inner && subject.contains(&row) {
                dropped_inner += 1;
            } else {
                hits.push((row, reason, s.via));
            }
        };
        if used_by {
            if subject.contains(&b) {
                push(a, words(s.kind, &m.elements[a]).0);
            }
            if symmetric(s.kind) && subject.contains(&a) {
                push(b, words(s.kind, &m.elements[b]).0);
            }
        } else {
            if subject.contains(&a) {
                push(b, words(s.kind, &m.elements[a]).1);
            }
            if symmetric(s.kind) && subject.contains(&b) {
                push(a, words(s.kind, &m.elements[b]).1);
            }
        }
    }
    // A reference in text is a use of the element it names, and the element that owns the text depends on it.
    for (owner, target) in &o.mentions {
        let (row, hit, reason) = if used_by { (*owner, subject.contains(target), "mentioned in text") } else { (*target, subject.contains(owner), "mentions in text") };
        if hit {
            if o.ignore_inner && subject.contains(&row) {
                dropped_inner += 1;
            } else {
                hits.push((row, reason, *owner));
            }
        }
    }
    let total = hits.len();
    let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();
    let mut elements: HashSet<ElementId> = HashSet::new();
    for (row, reason, _) in &hits {
        *by_reason.entry(reason).or_default() += 1;
        elements.insert(*row);
    }
    // Rows: one per (level element, reason); the count says how many usages it stands for.
    let mut grouped: BTreeMap<(String, &str), (ElementId, usize, ElementId)> = BTreeMap::new();
    for (row, reason, via) in &hits {
        let at = at_level(m, *row, &o.level);
        let e = grouped.entry((m.elements[at].qualified_name.clone(), reason)).or_insert((at, 0, *via));
        e.1 += 1;
    }
    let limit = if o.limit == 0 { MAX_ROWS } else { o.limit.min(MAX_ROWS) };
    let rows: Vec<Value> = grouped
        .iter()
        .skip(o.offset)
        .take(limit)
        .map(|((_, reason), (id, n, via))| {
            let e = &m.elements[*id];
            let v = &m.elements[*via];
            let mut r = super::brief(m, e);
            r["used_as"] = json!(reason);
            r["count"] = json!(n);
            r["via"] = json!({ "ref": v.qualified_name, "file": m.files[v.file].path, "line": line_of(m, v) });
            r
        })
        .collect();
    let mut out = json!({
        "element": super::brief(m, subject_el),
        "direction": if used_by { "used_by" } else { "depends_on" },
        "options": { "recursive": o.recursive, "ignore_inner": o.ignore_inner, "level": o.level },
        "total": total,
        "in_elements": elements.len(),
        "dropped_as_inner": dropped_inner,
        "by_reason": by_reason,
        "reasons": REASONS,
        "rows": rows,
        "row_total": grouped.len(),
        "offset": o.offset,
        "truncated": grouped.len() > o.offset + rows.len(),
        "limit": limit,
    });
    // A bound on the texts read means "mentioned in text" is a lower bound: the absence of a row is not an answer.
    out["mentions_incomplete"] = json!(o.mentions_cut);
    if o.mentions_cut {
        out["truncated"] = json!(true);
        out["mentions_bound"] = json!("the references in text were read up to a bound (50000 in the model, 100 per text); the rows \"mentioned in text\" may be missing some");
    }
    graph::mark_cut(m, &mut out, &cuts);
    Ok(out)
}
