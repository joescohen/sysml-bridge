//! Direct against implied. One rule, used by the trace chain and the tables:
//!
//! A link from A to B is *implied* for A' and B' when A' reaches A and B' reaches B through composition (the element is a
//! part of the other: its owner chain) or through generalization and typing (the element is typed by, or specializes, the
//! other), and no link drawn between A' and B' themselves exists. A link that is drawn between the two elements is *direct*.
//! The public feature this follows is the "implied relations" option of requirement matrices: a cell may show a link that
//! nobody drew.

use super::line_of;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};

/// The rule in words, stated in every reply that carries `basis`.
pub const RULE: &str = "A link from A to B is implied for A' and B' when A' reaches A and B' reaches B through composition (part of: the owner chain) or through generalization and typing (typed by, specializes), and no link joins A' and B' themselves. A link drawn between the two elements is direct.";

/// Most steps one walk up takes, and most elements it visits (the walk keeps a visited set).
const MAX_STEPS: usize = 8;
pub(crate) const MAX_VISITED: usize = 400;

/// One step from an element to what it reaches.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub to: ElementId,
    pub how: &'static str,
}

fn next(m: &Model, x: ElementId) -> Vec<Step> {
    let e = &m.elements[x];
    let mut out = Vec::new();
    if let Some(o) = e.owner {
        out.push(Step { to: o, how: "part of" });
    }
    for r in &e.refs {
        let how = match r.role {
            RefRole::Type => "typed by",
            RefRole::Specializes => "specializes",
            _ => continue,
        };
        if let Some(t) = r.resolved() {
            out.push(Step { to: t, how });
        }
    }
    out
}

/// The steps that lead from `from` to `target`, nearest first, or `None` when `from` does not reach it (and when they are the
/// same element: that is a direct link, not an implied one).
pub fn up_path(m: &Model, from: ElementId, target: ElementId) -> Option<Vec<Step>> {
    if from == target {
        return None;
    }
    let mut seen: HashSet<ElementId> = HashSet::from([from]);
    let mut q: VecDeque<(ElementId, Vec<Step>)> = VecDeque::from([(from, Vec::new())]);
    while let Some((x, path)) = q.pop_front() {
        if path.len() >= MAX_STEPS {
            continue;
        }
        for s in next(m, x) {
            if !seen.insert(s.to) || seen.len() > MAX_VISITED {
                continue;
            }
            let mut p = path.clone();
            p.push(s);
            if s.to == target {
                return Some(p);
            }
            q.push_back((s.to, p));
        }
    }
    None
}

/// The path as the reply shows it: each element walked, how, and where it is written.
pub fn through_json(m: &Model, from: ElementId, steps: &[Step]) -> Value {
    let mut at = from;
    let mut out = Vec::new();
    for s in steps {
        let e = &m.elements[s.to];
        out.push(json!({ "from": m.elements[at].qualified_name, "step": s.how, "to": e.qualified_name, "file": m.files[e.file].path, "line": line_of(m, e) }));
        at = s.to;
    }
    json!(out)
}

/// `{basis: "direct"}`.
pub fn direct() -> Value {
    json!({ "basis": "direct" })
}

/// The basis of a link end: the element a link names (`named`) against the element the reply is about (`about`). Direct when
/// they are the same. Implied through the path when one reaches the other: `about` reaches `named` (the link is drawn on the
/// whole or on the type, the reply is about a part or an instance), or `named` reaches `about` (the link is drawn on a part or
/// an instance and the reply shows it on the whole or on the type). Implied with no path when the link was reached some other
/// way (a feature chain), which the reply says.
pub fn basis_of(m: &Model, about: ElementId, named: ElementId) -> Value {
    if about == named {
        return direct();
    }
    if let Some(p) = up_path(m, about, named) {
        return json!({ "basis": "implied", "through": through_json(m, about, &p) });
    }
    if let Some(p) = up_path(m, named, about) {
        return json!({ "basis": "implied", "through": through_json(m, named, &p) });
    }
    json!({ "basis": "implied", "through": [], "through_note": "reached by a feature chain or a port, not by a part or a type" })
}

/// The context of a link end: a usage nested in a definition or in another usage stands for one place in the design (the front
/// wheel of a car, not every wheel), so a link on it holds for that place only. `{path, within}`, or `None` for an element that is
/// not a usage with such an owner (a definition, a package member).
pub fn context_of(m: &Model, id: ElementId) -> Option<Value> {
    let e = &m.elements[id];
    if e.is_def || e.name.is_none() {
        return None;
    }
    let o = &m.elements[e.owner?];
    if matches!(o.kind.as_str(), "package" | "library package") {
        return None;
    }
    Some(json!({ "path": e.qualified_name, "within": o.qualified_name, "says": "holds for this usage only, not for every use of its type" }))
}

/// `basis` with the `context` of the end it describes.
pub fn with_context(m: &Model, basis: &Value, end: Option<ElementId>) -> Value {
    let mut b = basis.clone();
    if let (Some(c), Some(o)) = (end.and_then(|id| context_of(m, id)), b.as_object_mut()) {
        o.insert("context".into(), c);
    }
    b
}

/// Copy `basis` and `through` onto an object.
pub fn put(target: &mut Value, basis: &Value) {
    if let (Some(t), Some(b)) = (target.as_object_mut(), basis.as_object()) {
        for (k, v) in b {
            t.insert(k.clone(), v.clone());
        }
    }
}

/// True when A' and B' are joined by a link drawn between exactly those two elements (then the link is direct for them).
pub fn is_direct_pair(a_end: ElementId, b_end: ElementId, a: ElementId, b: ElementId) -> bool {
    a_end == a && b_end == b
}

/// The implied pairs of one link: every `(A', B')` where A' reaches the link's source and B' reaches its target, found from
/// the side of one element. `candidates` are the elements to try for the other side. Used by the trace chain to label a hop.
pub fn implied_for(m: &Model, a: ElementId, b: ElementId, a_prime: ElementId, b_prime: ElementId) -> Option<(Vec<Step>, Vec<Step>)> {
    let ia = if a_prime == a { Some(Vec::new()) } else { up_path(m, a_prime, a) };
    let ib = if b_prime == b { Some(Vec::new()) } else { up_path(m, b_prime, b) };
    match (ia, ib) {
        (Some(x), Some(y)) if !(x.is_empty() && y.is_empty()) => Some((x, y)),
        _ => None,
    }
}


/// Every element `from` reaches (the same walk as `up_path`, once, with a visited set), nearest first.
pub fn reach_set(m: &Model, from: ElementId) -> Vec<ElementId> {
    let mut seen: HashSet<ElementId> = HashSet::from([from]);
    let mut order = Vec::new();
    let mut q: VecDeque<(ElementId, usize)> = VecDeque::from([(from, 0)]);
    while let Some((x, d)) = q.pop_front() {
        if d >= MAX_STEPS {
            continue;
        }
        for s in next(m, x) {
            if seen.insert(s.to) && seen.len() <= MAX_VISITED {
                order.push(s.to);
                q.push_back((s.to, d + 1));
            }
        }
    }
    order
}
