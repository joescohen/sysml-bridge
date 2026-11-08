//! Traceability properties in the words of the public feature "Traceability" of requirements tools: for one element, the
//! one-hop properties (Satisfied By, Verified By, Refined By, Traced To, Derived, Derived From, Master, Satisfies, Verifies,
//! Refines, Traced From, Allocated To, Allocated From, and Cameo's Realizing Element and Specifying Element) and, for each, the "All ..." property, the closure of the one-hop one.
//!
//! Nothing here is stored. Every list is read from the relationship statements of the model (`graph::statements`), so the same
//! words stand for the same links wherever they are shown. The tool states facts with a file and a line and writes no judgement.

use super::graph::{self, Cut};
use super::implied;
use super::line_of;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Most entries one list shows; the count is always the full one.
pub const MAX_LIST: usize = 200;

/// Which end of the link the element stands at, and which end the list holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The element is the target (supplier) of the link; the list holds the sources (clients).
    FromTarget,
    /// The element is the source (client); the list holds the targets (suppliers).
    FromSource,
}

/// One property.
pub struct Prop {
    pub name: &'static str,
    pub all_name: &'static str,
    /// The group the public feature shows it in: Specification, Realization or Other.
    pub group: &'static str,
    /// `requirement` for a property of requirements; `any element` otherwise.
    pub applies_to: &'static str,
    /// The relationship kind read (`graph::Stmt::kind`); `copy` is a kind this reader does not find in a model.
    pub kind: &'static str,
    pub side: Side,
    pub rule: &'static str,
}

/// The pseudo-kind of the Realizing and Specifying pair: any of `ABSTRACTION_KINDS`.
pub const ABSTRACTION: &str = "abstraction";
/// The relationship kinds read as abstractions, in the order lists show them.
pub const ABSTRACTION_KINDS: &[&str] = &["satisfy", "verify", "refine", "trace", "derive", "allocate"];

/// The one table. Grouping follows the "Traceability" property group of requirements tools (Specification, Realization, Other);
/// the order is the order a reader goes down. Satisfy, verify, derive and copy are kinds of trace in that feature.
pub const PROPS: &[Prop] = &[
    Prop { name: "Derived", all_name: "All Derived", group: "Specification", applies_to: "requirement", kind: "derive", side: Side::FromTarget, rule: "the requirements that are the client of a derive link whose supplier is this requirement" },
    Prop { name: "Derived From", all_name: "All Derived From", group: "Specification", applies_to: "requirement", kind: "derive", side: Side::FromSource, rule: "the requirements that are the supplier of a derive link whose client is this requirement" },
    Prop { name: "Master", all_name: "All Master", group: "Specification", applies_to: "requirement", kind: "copy", side: Side::FromSource, rule: "the supplier of the copy link in which this requirement is the client (this reader finds no copy link in a model, so the list is empty)" },
    Prop { name: "Satisfied By", all_name: "All Satisfied By", group: "Realization", applies_to: "requirement", kind: "satisfy", side: Side::FromTarget, rule: "the elements that are the client of a satisfy link whose supplier is this requirement; a trace link from a design element to the requirement is listed too, marked via.kind trace" },
    Prop { name: "Verified By", all_name: "All Verified By", group: "Realization", applies_to: "requirement", kind: "verify", side: Side::FromTarget, rule: "the elements that are the client of a verify link whose supplier is this requirement" },
    Prop { name: "Refined By", all_name: "All Refined By", group: "Realization", applies_to: "requirement", kind: "refine", side: Side::FromTarget, rule: "the elements that are the client of a refine link whose supplier is this requirement" },
    Prop { name: "Traced To", all_name: "All Traced To", group: "Other", applies_to: "requirement", kind: "trace", side: Side::FromTarget, rule: "the elements that are the client of a trace link whose supplier is this requirement" },
    Prop { name: "Satisfies", all_name: "All Satisfies", group: "Other", applies_to: "any element", kind: "satisfy", side: Side::FromSource, rule: "the suppliers of the satisfy links whose client is this element; a trace link from this element to a requirement is listed too, marked via.kind trace" },
    Prop { name: "Verifies", all_name: "All Verifies", group: "Other", applies_to: "any element", kind: "verify", side: Side::FromSource, rule: "the suppliers of the verify links whose client is this element" },
    Prop { name: "Refines", all_name: "All Refines", group: "Other", applies_to: "any element", kind: "refine", side: Side::FromSource, rule: "the suppliers of the refine links whose client is this element" },
    Prop { name: "Traced From", all_name: "All Traced From", group: "Other", applies_to: "any element", kind: "trace", side: Side::FromSource, rule: "the suppliers of the trace links whose client is this element" },
    Prop { name: "Allocated To", all_name: "All Allocated To", group: "Other", applies_to: "any element", kind: "allocate", side: Side::FromSource, rule: "the elements this element is allocated to: the supplier of an allocate link whose client is this element" },
    Prop { name: "Allocated From", all_name: "All Allocated From", group: "Other", applies_to: "any element", kind: "allocate", side: Side::FromTarget, rule: "the elements allocated to this element: the client of an allocate link whose supplier is this element" },
    // Cameo's UML-level pair, listed in its requirement Specification window: every abstraction-based link at once (satisfy,
    // verify, refine, trace, derive and allocate all extend Abstraction there), so the "All ..." form crosses kinds.
    Prop { name: "Realizing Element", all_name: "All Realizing Elements", group: "Other", applies_to: "any element", kind: ABSTRACTION, side: Side::FromTarget, rule: "the clients of every satisfy, verify, refine, trace, derive or allocate link whose supplier is this element (Cameo: Abstraction, target to source); the All form follows links of any of these kinds" },
    Prop { name: "Specifying Element", all_name: "All Specifying Elements", group: "Other", applies_to: "any element", kind: ABSTRACTION, side: Side::FromSource, rule: "the suppliers of every satisfy, verify, refine, trace, derive or allocate link whose client is this element (Cameo: Abstraction, source to target); the All form follows links of any of these kinds" },
];

/// The closure rule, stated in every reply.
pub const CLOSURE_RULE: &str = "An 'All ...' property is the closure of the one-hop property: the property is read again for each element it returns, until nothing new comes up (each element once, the element itself left out).";

/// The links of a model, indexed by kind and end.
pub struct Index {
    /// (kind, source) -> [(target, link)]
    by_src: HashMap<(&'static str, ElementId), Vec<(ElementId, ElementId)>>,
    /// (kind, target) -> [(source, link)]
    by_tgt: HashMap<(&'static str, ElementId), Vec<(ElementId, ElementId)>>,
    pub cuts: Vec<Cut>,
    /// The kind each allocation statement is (`perform`, `calls`, `owns`), for an index with allocations; `allocate` when absent.
    alloc_via: HashMap<ElementId, &'static str>,
}

pub fn index(model: &Model) -> Index {
    let (stmts, cuts) = graph::statements_cut(model);
    let mut ix = Index { by_src: HashMap::new(), by_tgt: HashMap::new(), cuts, alloc_via: HashMap::new() };
    for s in &stmts {
        let (Some(a), Some(b)) = (s.source.id, s.target.id) else { continue };
        if a == b {
            continue;
        }
        ix.by_src.entry((s.kind, a)).or_default().push((b, s.via));
        ix.by_tgt.entry((s.kind, b)).or_default().push((a, s.via));
    }
    ix
}

/// `index`, with Allocated To and Allocated From read from the allocations every view reads (`allocation::allocations`):
/// a `perform`, a call operation action and an action a part owns count, as an `allocate` does, and an entry's `via.kind`
/// says which (`allocate`, `perform`, `calls`, `owns`). A perform written in a part definition is allocated to each usage
/// typed by it, and the definition lists it in its Allocated From. `get_traceability` reads this one.
pub fn index_with_allocations(model: &Model) -> Index {
    let mut ix = index(model);
    ix.by_src.retain(|(k, _), _| *k != "allocate");
    ix.by_tgt.retain(|(k, _), _| *k != "allocate");
    for a in super::allocation::allocations(model).into_iter().filter(|a| a.is_direct() && a.function != a.part) {
        ix.by_src.entry(("allocate", a.function)).or_default().push((a.part, a.statement));
        ix.by_tgt.entry(("allocate", a.part)).or_default().push((a.function, a.statement));
        if let Some(d) = a.defined_on {
            let l = ix.by_tgt.entry(("allocate", d)).or_default();
            if !l.contains(&(a.function, a.statement)) {
                l.push((a.function, a.statement));
            }
        }
        if a.via != "allocate" {
            ix.alloc_via.insert(a.statement, a.via);
        }
    }
    ix
}

impl Index {
    fn kind_of(&self, kind: &'static str, via: ElementId) -> &'static str {
        if kind == "allocate" { self.alloc_via.get(&via).copied().unwrap_or(kind) } else { kind }
    }

    /// One list entry for an element reached through a link (see `entry`).
    pub fn entry_of(&self, model: &Model, other: ElementId, via: ElementId, kind: &str) -> Value {
        entry(model, other, via, kind)
    }

    /// The elements at the far end of the links of `kind` at `id`, with the link, in the order of the model.
    pub fn hop(&self, model: &Model, prop: &Prop, id: ElementId) -> Vec<(ElementId, ElementId, &'static str)> {
        if prop.kind == ABSTRACTION {
            let ix = match prop.side {
                Side::FromTarget => &self.by_tgt,
                Side::FromSource => &self.by_src,
            };
            return ABSTRACTION_KINDS.iter().flat_map(|k| ix.get(&(*k, id)).into_iter().flatten().map(move |(o, v)| (*o, *v, self.kind_of(k, *v)))).collect();
        }
        let direct = match prop.side {
            Side::FromTarget => self.by_tgt.get(&(prop.kind, id)),
            Side::FromSource => self.by_src.get(&(prop.kind, id)),
        };
        let mut out: Vec<(ElementId, ElementId, &'static str)> = direct.into_iter().flatten().map(|(o, v)| (*o, *v, self.kind_of(prop.kind, *v))).collect();
        // A trace link from a design element to a requirement stands for a satisfy link in a model that writes it that way.
        if prop.kind == "satisfy" {
            let traced = match prop.side {
                Side::FromTarget => self.by_tgt.get(&("trace", id)).into_iter().flatten().filter(|(o, _)| model.elements[*o].kind != "requirement").map(|(o, v)| (*o, *v, "trace")).collect::<Vec<_>>(),
                Side::FromSource => self.by_src.get(&("trace", id)).into_iter().flatten().filter(|(o, _)| model.elements[*o].kind == "requirement").map(|(o, v)| (*o, *v, "trace")).collect::<Vec<_>>(),
            };
            out.extend(traced);
        }
        out
    }

    /// The closure of the property from `id`: every element reached, once, nearest first, with the link of its first reach.
    pub fn closure(&self, model: &Model, prop: &Prop, id: ElementId) -> Vec<(ElementId, ElementId, &'static str)> {
        let mut seen: BTreeSet<ElementId> = BTreeSet::from([id]);
        let mut out = Vec::new();
        let mut q = VecDeque::from([id]);
        while let Some(x) = q.pop_front() {
            for (o, v, k) in self.hop(model, prop, x) {
                if seen.insert(o) {
                    out.push((o, v, k));
                    q.push_back(o);
                }
            }
        }
        out
    }
}

pub fn entry(model: &Model, other: ElementId, via: ElementId, kind: &str) -> Value {
    let e = &model.elements[other];
    let v = &model.elements[via];
    let mut r = json!({
        "ref": e.qualified_name,
        "name": e.name,
        "kind": e.keyword(),
        "file": model.files[e.file].path,
        "line": line_of(model, e),
        "basis": "direct",
        "via": { "ref": v.qualified_name, "kind": kind, "file": model.files[v.file].path, "line": line_of(model, v) },
    });
    if let Some(c) = super::implied::context_of(model, other) {
        r["context"] = c;
    }
    r
}

fn list(model: &Model, items: &[(ElementId, ElementId, &'static str)]) -> (Value, usize) {
    // The same element through two links is listed once, with the first link.
    let mut seen = BTreeSet::new();
    let rows: Vec<Value> = items.iter().filter(|(o, _, _)| seen.insert(*o)).take(MAX_LIST).map(|(o, v, k)| entry(model, *o, *v, k)).collect();
    let n = items.iter().map(|x| x.0).collect::<BTreeSet<_>>().len();
    (json!(rows), n)
}

/// `list_traceability_properties` for a model: the table, and after it the properties the model declares (`source: "model"`).
pub fn table_for(model: &Model) -> Value {
    let mut v = table();
    let decl = super::model_props::declared(model);
    if let Some(a) = v["properties"].as_array_mut() {
        for p in a.iter_mut() {
            p["source"] = json!("built in");
        }
        let built: Vec<String> = a.iter().filter_map(|p| p["name"].as_str().map(|n| n.to_ascii_lowercase())).collect();
        for d in &decl {
            let mut row = super::model_props::table_row(model, d);
            // A declaration that takes a built-in name is not that property; it is listed as the model's own.
            row["same_name_as_built_in"] = json!(built.contains(&d.name.to_ascii_lowercase()));
            a.push(row);
        }
    }
    v["model_declared"] = json!(decl.len());
    v
}

/// `list_traceability_properties`: the table.
pub fn table() -> Value {
    json!({
        "properties": PROPS.iter().map(|p| json!({
            "name": p.name,
            "all_name": p.all_name,
            "group": p.group,
            "applies_to": p.applies_to,
            "relationship": p.kind,
            "direction": match p.side { Side::FromTarget => "this element is the supplier (target) of the link; the list holds the clients (sources)", Side::FromSource => "this element is the client (source) of the link; the list holds the suppliers (targets)" },
            "rule": p.rule,
        })).collect::<Vec<_>>(),
        "groups": ["Specification", "Realization", "Other"],
        "closure_of": CLOSURE_RULE,
        "note": "Nothing is stored in the model: each list is read from the relationship statements (satisfy, verify, refine, derive, allocate, and the trace dependencies an imported SysML v1 project writes). Every entry has a file and a line, and the link it came through.",
    })
}

/// The properties of one element.
pub fn of(model: &Model, ix: &Index, id: ElementId) -> Value {
    let e = &model.elements[id];
    let is_req = e.kind == "requirement";
    let props: Vec<Value> = PROPS
        .iter()
        .map(|p| {
            let applicable = p.applies_to == "any element" || is_req;
            let (one, n1, all, n_all) = if applicable {
                let hop = ix.hop(model, p, id);
                let (a, na) = list(model, &hop);
                let clo = ix.closure(model, p, id);
                let (b, nb) = list(model, &clo);
                (a, na, b, nb)
            } else {
                (json!([]), 0, json!([]), 0)
            };
            json!({
                "name": p.name,
                "group": p.group,
                "applies_to": p.applies_to,
                "applicable": applicable,
                "relationship": p.kind,
                "rule": p.rule,
                "count": n1,
                "items": one,
                "all_name": p.all_name,
                "closure_of": p.name,
                "all_count": n_all,
                "all_items": all,
                "truncated": n1 > MAX_LIST || n_all > MAX_LIST,
            })
        })
        .collect();
    json!({
        "element": e.qualified_name,
        "kind": e.keyword(),
        "closure_rule": CLOSURE_RULE,
        "groups": ["Specification", "Realization", "Other"],
        "properties": props,
        "model_properties": super::model_props::of(model, ix, id),
    })
}

/// The one-hop and closure lists, by property name, for tests and for other readers.
pub fn names_of(v: &Value, property: &str) -> Vec<String> {
    v["properties"].as_array().into_iter().flatten().find(|p| p["name"] == property).map(|p| p["items"].as_array().into_iter().flatten().filter_map(|x| x["ref"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Mark in a reply the links that were cut short.
pub fn mark(model: &Model, reply: &mut Value, ix: &Index) {
    graph::mark_cut(model, reply, &ix.cuts);
    let _ = implied::RULE;
}
