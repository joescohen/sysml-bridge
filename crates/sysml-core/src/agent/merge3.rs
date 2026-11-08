//! A three-way comparison: the changes two revisions (`ours` and `theirs`) made to a common base, and which of them
//! conflict. Elements are paired with the base as the change report pairs them (`change::pair`), so a rename or a move
//! is one change, not a removal and an addition.
//!
//! The kinds of conflict follow the merge of Cameo / MagicDraw 2026x (its conflict, equivalence and dependency rules),
//! mapped to SysML v2 text:
//! - `both-changed`: both sides changed the same fact of one element (its name, owner, type, value, doc, ...) to
//!   different values. The same new value on both sides is an equivalent change, not a conflict.
//! - `removed-and-changed`: one side removed an element; the other changed it or something inside it, or added an
//!   element inside it. Removed on both sides is equivalent.
//! - `reference-to-removed`: one side added or re-pointed a reference (type, specialization, redefinition, end) to an
//!   element the other side removed.
//! - `specialization-cycle`: the specializations each side added are fine alone and form a cycle together.
//! - `both-added`: both sides added an element of the same qualified name with different text. The same text is
//!   equivalent.
//!
//! The comparison reads the three models. It writes nothing and does not merge text.

use super::change;
use crate::model::{Element, ElementId, Model, RefRole};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Rows of a list in the reply. The counts stay exact.
pub const MAX_ROWS: usize = 500;

const REF_FACTS: &[&str] = &["type", "of", "specializes", "redefines", "references", "source", "target"];
const REF_ROLES: &[RefRole] = &[RefRole::Type, RefRole::Specializes, RefRole::Redefines, RefRole::References, RefRole::Source, RefRole::Target];

/// What one side did to the base.
struct SideChanges<'a> {
    name: &'static str,
    model: &'a Model,
    /// Element of this side to the base element.
    pair_a: HashMap<ElementId, ElementId>,
    removed: HashSet<ElementId>,
    changed: HashMap<ElementId, Map<String, Value>>,
    added: Vec<ElementId>,
}

impl<'a> SideChanges<'a> {
    fn new(name: &'static str, base: &'a Model, side: &'a Model, ids: &'a BTreeMap<String, String>) -> Self {
        let p = change::pair(base, side, ids, ids, true);
        let (mut removed, mut changed) = (HashSet::new(), HashMap::new());
        for e in base.elements.iter().filter(|e| !change::skipped(e)) {
            match p.pair_b.get(&e.id) {
                None => {
                    removed.insert(e.id);
                }
                Some(x) => {
                    let d = change::element_diff(base, side, &p.pair_b, e, &side.elements[*x]);
                    if !d.is_empty() {
                        changed.insert(e.id, d);
                    }
                }
            }
        }
        let added = side.elements.iter().filter(|e| !change::skipped(e) && !p.pair_a.contains_key(&e.id)).map(|e| e.id).collect();
        SideChanges { name, model: side, pair_a: p.pair_a, removed, changed, added }
    }

    /// The base element an element of this side stands in, itself or its nearest paired owner.
    fn base_anchor(&self, mut x: ElementId) -> Option<ElementId> {
        loop {
            if let Some(b) = self.pair_a.get(&x) {
                return Some(*b);
            }
            x = self.model.elements[x].owner?;
        }
    }

    fn counts(&self) -> Value {
        json!({ "added": self.added.len(), "changed": self.changed.len(), "removed": self.removed.len() })
    }
}

fn within(base: &Model, e: ElementId, top: ElementId) -> bool {
    let mut cur = Some(e);
    while let Some(c) = cur {
        if c == top {
            return true;
        }
        cur = base.elements[c].owner;
    }
    false
}

fn text_of<'m>(m: &'m Model, e: &Element) -> &'m str {
    m.files[e.file].text.get(e.span.start..e.span.end).unwrap_or("").trim()
}

/// Specialization edges between base elements that one side states.
fn spec_edges(base_of: &dyn Fn(ElementId) -> Option<ElementId>, m: &Model) -> HashSet<(ElementId, ElementId)> {
    let mut out = HashSet::new();
    for e in &m.elements {
        let Some(a) = base_of(e.id) else { continue };
        for r in e.refs_of(RefRole::Specializes) {
            if let Some(b) = r.resolved().and_then(base_of) {
                out.insert((a, b));
            }
        }
    }
    out
}

/// The strongly connected parts of a graph that hold a cycle (more than one node, or a node that specializes itself).
fn cycles(edges: &HashSet<(ElementId, ElementId)>) -> Vec<Vec<ElementId>> {
    let mut adj: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for (a, b) in edges {
        adj.entry(*a).or_default().push(*b);
        adj.entry(*b).or_default();
    }
    let mut nodes: Vec<ElementId> = adj.keys().copied().collect();
    nodes.sort_unstable();
    // Tarjan's algorithm, without recursion.
    let (mut index, mut low, mut on, mut stack, mut out) = (HashMap::new(), HashMap::new(), HashSet::new(), Vec::new(), Vec::new());
    let mut next = 0usize;
    for &start in &nodes {
        if index.contains_key(&start) {
            continue;
        }
        let mut work: Vec<(ElementId, usize)> = vec![(start, 0)];
        index.insert(start, next);
        low.insert(start, next);
        next += 1;
        stack.push(start);
        on.insert(start);
        while let Some(&(v, i)) = work.last() {
            let succ = &adj[&v];
            if i < succ.len() {
                let w = succ[i];
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                if !index.contains_key(&w) {
                    index.insert(w, next);
                    low.insert(w, next);
                    next += 1;
                    stack.push(w);
                    on.insert(w);
                    work.push((w, 0));
                } else if on.contains(&w) {
                    let l = low[&v].min(index[&w]);
                    low.insert(v, l);
                }
            } else {
                work.pop();
                if let Some(&(p, _)) = work.last() {
                    let l = low[&p].min(low[&v]);
                    low.insert(p, l);
                }
                if low[&v] == index[&v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on.remove(&w);
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    if comp.len() > 1 || edges.contains(&(v, v)) {
                        comp.sort_unstable();
                        out.push(comp);
                    }
                }
            }
        }
    }
    out
}

/// The three-way comparison of `ours` and `theirs` against `base`.
pub fn merge_check(base: &Model, ours: &Model, theirs: &Model) -> Value {
    let ids = BTreeMap::new();
    let o = SideChanges::new("ours", base, ours, &ids);
    let t = SideChanges::new("theirs", base, theirs, &ids);
    let qn = |b: ElementId| base.elements[b].qualified_name.clone();
    let mut conflicts: Vec<Value> = Vec::new();
    let mut equivalent: Vec<Value> = Vec::new();

    // Both changed one element.
    let mut both: Vec<&ElementId> = o.changed.keys().filter(|b| t.changed.contains_key(*b)).collect();
    both.sort_unstable();
    for b in both {
        let (fo, ft) = (&o.changed[b], &t.changed[b]);
        for (field, vo) in fo {
            let Some(vt) = ft.get(field) else { continue };
            if vo["after"] == vt["after"] {
                equivalent.push(json!({ "kind": "same-change", "element": qn(*b), "field": field, "value": vo["after"] }));
            } else {
                conflicts.push(json!({
                    "kind": "both-changed", "element": qn(*b), "field": field, "base": vo["before"], "ours": vo["after"], "theirs": vt["after"],
                    "why": format!("both sides changed the {field} of this element, to different values"),
                }));
            }
        }
    }

    // Removed on one side, changed (or added to) on the other.
    for (gone, other) in [(&o, &t), (&t, &o)] {
        let mut tops: Vec<ElementId> = gone.removed.iter().copied().filter(|b| base.elements[*b].owner.map(|p| !gone.removed.contains(&p)).unwrap_or(true)).collect();
        tops.sort_unstable();
        for top in tops {
            if gone.name == "theirs" && o.removed.contains(&top) {
                continue; // reported once below, as equivalent
            }
            if other.removed.contains(&top) {
                equivalent.push(json!({ "kind": "removed-on-both", "element": qn(top) }));
                continue;
            }
            let mut touched: Vec<String> = other.changed.keys().filter(|b| within(base, **b, top) && !other.removed.contains(*b)).map(|b| qn(*b)).collect();
            for x in &other.added {
                let owner = other.model.elements[*x].owner;
                if owner.and_then(|p| other.base_anchor(p)).is_some_and(|a| within(base, a, top)) && owner.map(|p| other.pair_a.contains_key(&p)).unwrap_or(false) {
                    touched.push(other.model.elements[*x].qualified_name.clone());
                }
            }
            if !touched.is_empty() {
                touched.sort();
                let n = touched.len();
                touched.truncate(20);
                conflicts.push(json!({
                    "kind": "removed-and-changed", "element": qn(top), "removed_by": gone.name, "changed_by": other.name, "changed": touched, "changed_count": n,
                    "why": format!("{} removed this element; {} changed it or added to it", gone.name, other.name),
                }));
            }
        }
    }

    // A new or re-pointed reference to an element the other side removed.
    for (s, other) in [(&o, &t), (&t, &o)] {
        let mut rows: Vec<(String, String, String)> = Vec::new();
        for e in &s.model.elements {
            let fresh = !s.pair_a.contains_key(&e.id);
            let repointed = s.pair_a.get(&e.id).and_then(|b| s.changed.get(b)).is_some_and(|d| d.keys().any(|k| REF_FACTS.contains(&k.as_str())));
            if !fresh && !repointed {
                continue;
            }
            for r in e.refs.iter().filter(|r| REF_ROLES.contains(&r.role)) {
                let Some(target) = r.resolved().and_then(|x| s.pair_a.get(&x).copied()) else { continue };
                if other.removed.contains(&target) {
                    rows.push((e.qualified_name.clone(), format!("{:?}", r.role).to_lowercase(), qn(target)));
                }
            }
        }
        rows.sort();
        rows.dedup();
        for (el, role, target) in rows {
            conflicts.push(json!({
                "kind": "reference-to-removed", "element": el, "side": s.name, "role": role, "references": target, "removed_by": other.name,
                "why": format!("{} points to an element {} removed", s.name, other.name),
            }));
        }
    }

    // Specializations that form a cycle only together.
    let base_of_o = |x: ElementId| o.pair_a.get(&x).copied();
    let base_of_t = |x: ElementId| t.pair_a.get(&x).copied();
    let (eo, et) = (spec_edges(&base_of_o, ours), spec_edges(&base_of_t, theirs));
    let union: HashSet<(ElementId, ElementId)> = eo.union(&et).copied().collect();
    let (co, ct): (Vec<Vec<ElementId>>, Vec<Vec<ElementId>>) = (cycles(&eo), cycles(&et));
    let alone = |comp: &Vec<ElementId>, side: &Vec<Vec<ElementId>>| side.iter().any(|c| comp.iter().all(|n| c.contains(n)) || c.iter().all(|n| comp.contains(n)));
    for comp in cycles(&union) {
        if !alone(&comp, &co) && !alone(&comp, &ct) {
            conflicts.push(json!({
                "kind": "specialization-cycle", "elements": comp.iter().map(|b| qn(*b)).collect::<Vec<_>>(),
                "why": "the specializations of the two sides together form a cycle",
            }));
        }
    }

    // Both added an element of one name.
    let top_added = |s: &SideChanges| -> HashMap<String, ElementId> {
        s.added
            .iter()
            .filter(|x| {
                let e = &s.model.elements[**x];
                e.name.is_some() && e.owner.map(|p| s.pair_a.contains_key(&p)).unwrap_or(true)
            })
            .map(|x| (s.model.elements[*x].qualified_name.clone(), *x))
            .collect()
    };
    let (ao, at) = (top_added(&o), top_added(&t));
    let mut names: Vec<&String> = ao.keys().filter(|k| at.contains_key(*k)).collect();
    names.sort();
    for n in names {
        let (xo, xt) = (&ours.elements[ao[n]], &theirs.elements[at[n]]);
        if text_of(ours, xo) == text_of(theirs, xt) {
            equivalent.push(json!({ "kind": "added-on-both", "element": n }));
        } else {
            conflicts.push(json!({ "kind": "both-added", "element": n, "why": "both sides added an element of this name, with different text" }));
        }
    }

    let (nc, ne) = (conflicts.len(), equivalent.len());
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for c in &conflicts {
        *by_kind.entry(c["kind"].as_str().unwrap_or("").to_string()).or_default() += 1;
    }
    conflicts.truncate(MAX_ROWS);
    equivalent.truncate(MAX_ROWS);
    json!({
        "clean": nc == 0,
        "ours": o.counts(),
        "theirs": t.counts(),
        "conflict_count": nc,
        "conflicts_by_kind": by_kind,
        "conflicts": conflicts,
        "equivalent_count": ne,
        "equivalent": equivalent,
        "listed": { "conflicts": nc.min(MAX_ROWS), "equivalent": ne.min(MAX_ROWS) },
    })
}
