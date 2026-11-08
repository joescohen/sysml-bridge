//! Connections: the five questions each pillar of a model exists to answer, asked of one element:
//! why does it exist, which interfaces, behaviours and requirements hook it to the rest of the model.
//!
//! - why: the requirements it is there for (for a requirement: the ones above it; `breaks` holds the ones below)
//! - does: the behaviour (the functions it performs, its steps, the flows around it)
//! - who: the structure (the parts that do it, or that it is part of, or that it joins)
//! - exchanges: the interfaces (the items it sends or takes, the connectors they ride, the parts at the far ends)
//! - proof: what verifies the requirements it is there for
//!
//! Every answer row is {ref, relation, via}: `via` is null for a link the element states itself, else the element the row
//! is reached through (NAV-1 for a test that verifies the requirement a part satisfies). Direct rows come first. Each
//! kind of element is answered by its own rule, from the relationships the model states (satisfy, derive, refine,
//! verify, perform, allocate, calls, connect, flow, succession), never by a walk over everything: a connector's structure
//! is its two end parts, not every part of the behaviour around it. Read-only; one call answers one element.

use super::graph;
use super::conn_index::{type_of, Conn, Ix};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};

/// Most rows one answer holds.
const MAX: usize = 120;

#[derive(Clone)]
struct Row {
    id: ElementId,
    relation: String,
    via: Option<ElementId>,
}

#[derive(Default)]
struct List(Vec<Row>);

impl List {
    fn add(&mut self, id: ElementId, relation: &str, via: Option<ElementId>) {
        if let Some(r) = self.0.iter_mut().find(|r| r.id == id) {
            if r.via.is_some() && via.is_none() {
                r.via = None;
                r.relation = relation.to_string();
            }
            return;
        }
        if self.0.len() < MAX {
            self.0.push(Row { id, relation: relation.to_string(), via });
        }
    }
    /// As `add`, but a second relation of the same element is kept beside the first ("in from host, out to host").
    fn add_all(&mut self, id: ElementId, relation: &str, via: Option<ElementId>) {
        if let Some(r) = self.0.iter_mut().find(|r| r.id == id) {
            if !r.relation.split(", ").any(|x| x == relation) {
                r.relation = format!("{}, {relation}", r.relation);
            }
            return;
        }
        self.add(id, relation, via);
    }
    fn ids(&self) -> Vec<ElementId> {
        self.0.iter().map(|r| r.id).collect()
    }
    fn json(&self, model: &Model, me: ElementId) -> Value {
        let mut rows: Vec<&Row> = self.0.iter().filter(|r| r.id != me).collect();
        rows.sort_by_key(|r| r.via.is_some());
        Value::Array(rows.into_iter().map(|r| json!({ "ref": qn(model, r.id), "id": model.elements[r.id].short_name, "relation": r.relation, "via": r.via.map(|v| qn(model, v)) })).collect())
    }
}

fn qn(model: &Model, id: ElementId) -> String {
    model.elements[id].qualified_name.clone()
}
fn is_req(model: &Model, id: ElementId) -> bool {
    model.elements[id].kind.starts_with("requirement")
}
fn is_action(model: &Model, id: ElementId) -> bool {
    let k = model.elements[id].kind.as_str();
    k.ends_with("action") || k == "perform"
}
fn is_part(model: &Model, id: ElementId) -> bool {
    model.elements[id].kind == "part"
}
fn name(model: &Model, id: ElementId) -> String {
    model.elements[id].display_name()
}

/// What the answers read, built once per model.
pub(crate) struct Facts {
    ix: Ix,
    stmts: Vec<graph::Stmt>,
    allocs: Vec<super::allocation::Allocation>,
    /// Requirement -> its satisfiers (usages and definitions as stated).
    satisfiers: HashMap<ElementId, BTreeSet<ElementId>>,
    /// Requirement -> the requirements above it, with the relation word.
    up: HashMap<ElementId, Vec<(ElementId, &'static str)>>,
    /// Requirement -> the requirements below it.
    down: HashMap<ElementId, Vec<(ElementId, &'static str)>>,
    /// Requirement -> the cases that verify it.
    verified: HashMap<ElementId, Vec<ElementId>>,
    /// Action definition (or action) -> the action usages typed by it (an operation by its method, a step by its activity).
    typed_users: HashMap<ElementId, Vec<ElementId>>,
    /// Operation -> the steps that call it (`perform action x ::> part.op`).
    callers: HashMap<ElementId, Vec<ElementId>>,
}

fn facts(model: &Model) -> Facts {
    // Its own index (not the thread's shared one): the facts are kept on the model, which threads may share.
    let ix = Ix::new(model);
    let stmts = graph::statements(model);
    let allocs = super::allocation::allocations(model);
    let mut satisfiers: HashMap<ElementId, BTreeSet<ElementId>> = HashMap::new();
    for (x, reqs) in &ix.seq.satisfied {
        for r in reqs {
            satisfiers.entry(*r).or_default().insert(*x);
        }
    }
    let mut up: HashMap<ElementId, Vec<(ElementId, &'static str)>> = HashMap::new();
    let mut down: HashMap<ElementId, Vec<(ElementId, &'static str)>> = HashMap::new();
    let mut verified: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    let link = |lower: ElementId, upper: ElementId, rel_up: &'static str, rel_down: &'static str, up: &mut HashMap<ElementId, Vec<(ElementId, &'static str)>>, down: &mut HashMap<ElementId, Vec<(ElementId, &'static str)>>| {
        let u = up.entry(lower).or_default();
        if !u.iter().any(|x| x.0 == upper) {
            u.push((upper, rel_up));
        }
        let d = down.entry(upper).or_default();
        if !d.iter().any(|x| x.0 == lower) {
            d.push((lower, rel_down));
        }
    };
    for s in &stmts {
        let (Some(a), Some(b)) = (s.source.id, s.target.id) else { continue };
        match s.kind {
            // `derive` and `refine` go from the lower requirement to the upper one.
            "derive" | "refine" | "trace" if is_req(model, a) && is_req(model, b) && a != b => {
                let (u, d) = match s.kind { "derive" => ("derived from", "derives"), "refine" => ("refines", "refined by"), _ => ("traces to", "traced from") };
                link(a, b, u, d, &mut up, &mut down);
            }
            "verify" if is_req(model, b) => {
                let case = if model.elements[a].kind.contains("verification") || model.elements[a].kind.contains("case") { a } else { graph::verifier_of(model, &model.elements[s.via]).unwrap_or(a) };
                let l = verified.entry(b).or_default();
                if !l.contains(&case) {
                    l.push(case);
                }
            }
            _ => {}
        }
    }
    for e in model.elements.iter().filter(|e| e.kind.starts_with("requirement")) {
        if let Some(o) = e.owner.filter(|o| is_req(model, *o)) {
            link(e.id, o, "part of", "holds", &mut up, &mut down);
        }
    }
    let mut typed_users: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    let mut callers: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for e in model.elements.iter().filter(|e| !e.is_def && is_action(model, e.id)) {
        if let Some(t) = type_of(model, e.id).filter(|t| is_action(model, *t)) {
            typed_users.entry(t).or_default().push(e.id);
        }
        if let Some(c) = super::allocation::call_of(model, e.id) {
            callers.entry(c.op).or_default().push(e.id);
        }
    }
    Facts { ix, stmts, allocs, satisfiers, up, down, verified, typed_users, callers }
}

/// The usages of a definition, or the element itself when it is a usage.
fn usages_of(model: &Model, f: &Facts, x: ElementId) -> Vec<ElementId> {
    if model.elements[x].is_def {
        let u = f.ix.usages.get(&x).cloned().unwrap_or_default();
        if !u.is_empty() {
            return u;
        }
    }
    vec![x]
}

/// The requirements an element (and its definition) satisfies.
fn satisfied_by(model: &Model, f: &Facts, x: ElementId) -> Vec<ElementId> {
    let mut out: Vec<ElementId> = Vec::new();
    let mut keys = vec![x];
    keys.extend(type_of(model, x));
    for k in keys {
        for r in f.ix.seq.satisfied.get(&k).into_iter().flatten() {
            if !out.contains(r) {
                out.push(*r);
            }
        }
    }
    out
}

/// The functions a part performs or is allocated (stated on it, its definition, or the operations its block owns).
fn functions_of_part(model: &Model, f: &Facts, p: ElementId) -> Vec<(ElementId, &'static str)> {
    let mut keys = vec![p];
    keys.extend(type_of(model, p));
    let mut out: Vec<(ElementId, &'static str)> = Vec::new();
    for a in &f.allocs {
        if !a.is_direct() {
            continue;
        }
        if keys.contains(&a.part) || a.defined_on.is_some_and(|d| keys.contains(&d)) {
            let func = a.operation.unwrap_or(a.function);
            let rel = match a.via { "allocate" => "allocated", "calls" => "called on it", "owns" => "operation", _ => "performs" };
            if !out.iter().any(|x| x.0 == func) {
                out.push((func, rel));
            }
        }
    }
    for k in keys {
        for c in &model.elements[k].children {
            if super::allocation::is_operation(model, *c) && !out.iter().any(|x| x.0 == *c) {
                out.push((*c, "operation"));
            }
        }
    }
    out
}

/// The parts that do a function: performers and allocations, a usage of a call's operation, an operation's block.
fn parts_of_function(model: &Model, f: &Facts, func: ElementId) -> Vec<(ElementId, &'static str)> {
    let def = type_of(model, func).filter(|t| is_action(model, *t));
    let op = super::allocation::call_of(model, func).map(|c| c.op);
    let mut out: Vec<(ElementId, &'static str)> = Vec::new();
    let mut push = |p: ElementId, rel: &'static str| {
        if !out.iter().any(|x| x.0 == p) {
            out.push((p, rel));
        }
    };
    for a in &f.allocs {
        let hit = a.function == func || Some(a.function) == def || a.operation.is_some_and(|o| o == func || Some(o) == op);
        if hit {
            push(a.part, if a.via == "allocate" { "allocated to" } else { "performed by" });
        }
    }
    if let Some(c) = super::allocation::call_of(model, func) {
        if let Some(p) = c.part {
            push(p, "called on");
        }
    }
    let e = &model.elements[func];
    if super::allocation::is_operation(model, func) {
        if let Some(o) = e.owner {
            for u in usages_of(model, f, o) {
                push(u, "operation of");
            }
        }
    }
    out
}

/// The items a function's parameters (pins) carry: (item definition, direction).
fn pins_of(model: &Model, func: ElementId) -> Vec<(ElementId, String)> {
    let mut holders = vec![func];
    holders.extend(type_of(model, func));
    let mut out = Vec::new();
    for h in holders {
        for c in &model.elements[h].children {
            let ce = &model.elements[*c];
            let (Some(d), Some(t)) = (ce.direction.clone(), type_of(model, *c)) else { continue };
            if model.elements[t].kind == "item" && !out.iter().any(|x: &(ElementId, String)| x.0 == t) {
                out.push((t, d));
            }
        }
    }
    out
}

/// The connectors at a part (any end that names it, its usages or its definition): (connection index, my end, far end).
fn conns_at(model: &Model, f: &Facts, p: ElementId) -> Vec<(usize, usize)> {
    let mut keys: BTreeSet<ElementId> = BTreeSet::new();
    keys.insert(p);
    keys.extend(type_of(model, p));
    let mut out = Vec::new();
    for (i, c) in f.ix.conns.iter().enumerate() {
        for (k, end) in c.ends.iter().enumerate() {
            if end.last().is_some_and(|l| keys.contains(&l) || type_of(model, l).is_some_and(|t| keys.contains(&t))) && !out.iter().any(|x: &(usize, usize)| x.0 == i) {
                out.push((i, k));
            }
        }
    }
    out
}

/// The requirements a function serves: what it, its definition or its call's operation satisfies (direct), then what the
/// functions it is a step of satisfy (through them: a step of operateVessel serves SYS-2 too).
fn why_of_function(model: &Model, f: &Facts, func: ElementId) -> Vec<(ElementId, Option<ElementId>)> {
    let mut out: Vec<(ElementId, Option<ElementId>)> = Vec::new();
    let mut keys = vec![func];
    keys.extend(type_of(model, func));
    keys.extend(super::allocation::call_of(model, func).map(|c| c.op));
    for k in keys {
        for r in satisfied_by(model, f, k) {
            if !out.iter().any(|x| x.0 == r) {
                out.push((r, None));
            }
        }
    }
    // Climb every level: the functions it sits in, the steps that call it (an operation), the usages of it or its
    // definition (a method's operation, an activity's steps), then the same from each of those, up to the top
    // activity. A step of an FPGA's method serves what the radar's top activity satisfies, levels up.
    let ancestors = |x: ElementId| std::iter::successors(model.elements[x].owner, |o| model.elements[*o].owner).take(8).filter(|o| is_action(model, *o)).collect::<Vec<_>>();
    let mut seen: BTreeSet<ElementId> = BTreeSet::new();
    seen.insert(func);
    let mut queue: std::collections::VecDeque<ElementId> = std::collections::VecDeque::new();
    queue.push_back(func);
    let mut n = 0usize;
    while let Some(x) = queue.pop_front() {
        n += 1;
        if n > 4000 {
            break;
        }
        let mut next: Vec<ElementId> = ancestors(x);
        let mut users: Vec<ElementId> = Vec::new();
        for k in std::iter::once(x).chain(type_of(model, x).filter(|t| is_action(model, *t))) {
            users.extend(f.typed_users.get(&k).into_iter().flatten().copied().filter(|u| *u != x));
            users.extend(f.callers.get(&k).into_iter().flatten().copied());
        }
        for u in users {
            next.extend(ancestors(u));
            next.push(u);
        }
        for o in next {
            if !seen.insert(o) {
                continue;
            }
            queue.push_back(o);
            let mut keys = vec![o];
            keys.extend(type_of(model, o));
            for k in keys {
                for r in satisfied_by(model, f, k) {
                    if !out.iter().any(|x| x.0 == r) {
                        out.push((r, Some(o)));
                    }
                }
            }
        }
    }
    out
}

/// The operations that make or take what a connector carries: its end parts' own first; then, in models where an
/// end is an assembly (a carrier card), those of the parts inside it ("inside": the FPGA step that fills the buffer).
fn connector_does(model: &Model, f: &Facts, c: &Conn) -> Vec<(ElementId, &'static str, ElementId)> {
    let items: Vec<ElementId> = c.items.iter().map(|x| x.0).collect();
    let mut out: Vec<(ElementId, &'static str, ElementId)> = Vec::new();
    for end in &c.ends {
        let Some(p) = end.last() else { continue };
        let mut parts = vec![p];
        let mut i = 0;
        while i < parts.len() && parts.len() < 400 {
            let x = parts[i];
            i += 1;
            let mut kids: Vec<ElementId> = model.elements[x].children.clone();
            if let Some(t) = type_of(model, x) {
                kids.extend(model.elements[t].children.iter().copied());
            }
            for k in kids {
                if is_part(model, k) && !model.elements[k].is_def && !parts.contains(&k) {
                    parts.push(k);
                }
            }
        }
        for (depth, q) in parts.into_iter().enumerate() {
            for (func, _) in functions_of_part(model, f, q) {
                let pins = pins_of(model, func);
                if let Some((_, d)) = pins.iter().find(|(t, _)| items.contains(t)) {
                    let w = match (d.as_str(), depth == 0) { ("out", true) => "sends on it", (_, true) => "takes from it", ("out", false) => "sends on it, inside", _ => "takes from it, inside" };
                    if !out.iter().any(|x| x.0 == func) {
                        out.push((func, w, q));
                    }
                }
            }
        }
    }
    out
}

/// The steps that call an operation (`perform action acquireFix ::> gps.acquireFix`).
fn callers_of(model: &Model, f: &Facts, op: ElementId) -> Vec<ElementId> {
    if !super::allocation::is_operation(model, op) {
        return Vec::new();
    }
    let _ = model;
    f.callers.get(&op).cloned().unwrap_or_default()
}

/// Whether a part sends or takes an item: the direction of the pin of one of its functions that carries it.
fn dir_at(model: &Model, f: &Facts, p: ElementId, item: ElementId) -> Option<&'static str> {
    for (func, _) in functions_of_part(model, f, p) {
        for (t, d) in pins_of(model, func) {
            if t == item {
                return match d.as_str() { "out" => Some("out"), "in" => Some("in"), _ => None };
            }
        }
    }
    None
}

/// The way `item` goes through `port` as its features state it (`in item cmd : CTRL;`), turned round on a conjugated port.
fn port_dir(model: &Model, port: ElementId, item: ElementId) -> Option<&'static str> {
    let pe = &model.elements[port];
    let tref = pe.refs.iter().find(|r| r.role == RefRole::Type);
    let conj = tref.is_some_and(|r| r.conjugated);
    let mut feats: Vec<ElementId> = pe.children.clone();
    if let Some(d) = tref.and_then(|r| r.resolved()) {
        feats.extend(model.elements[d].children.iter().copied());
    }
    for c in feats {
        if type_of(model, c) != Some(item) {
            continue;
        }
        return match (model.elements[c].direction.as_deref(), conj) {
            (Some("in"), false) | (Some("out"), true) => Some("in"),
            (Some("out"), false) | (Some("in"), true) => Some("out"),
            _ => None,
        };
    }
    None
}

/// "out to comms", "in from comms" or "with comms" for one item at the port of part `p` (as the port states it, else as the
/// functions of `p` take or make it).
fn item_way(model: &Model, f: &Facts, p: ElementId, port: Option<ElementId>, item: ElementId, flow: ElementId, far: &str) -> String {
    // A flow says it outright: it starts at p (out) or ends at p (in).
    let at = |role: RefRole| model.elements[flow].refs_of(role).any(|r| r.segments.iter().any(|s| s.resolved == Some(p)));
    let by_flow = if at(RefRole::Source) { Some("out") } else if at(RefRole::Target) { Some("in") } else { None };
    match by_flow.or_else(|| port.and_then(|x| port_dir(model, x, item))).or_else(|| dir_at(model, f, p, item)) {
        Some("out") => format!("out to {far}"),
        Some("in") => format!("in from {far}"),
        _ => format!("with {far}"),
    }
}

/// "out to comms", "in from comms" or "with comms" for the items a connector carries, seen from part `p`.
fn way(model: &Model, f: &Facts, p: ElementId, items: &[(ElementId, ElementId)], far: &str) -> String {
    let dirs: BTreeSet<&str> = items.iter().filter_map(|(it, _)| dir_at(model, f, p, *it)).collect();
    match (dirs.contains("out"), dirs.contains("in")) {
        (true, false) => format!("out to {far}"),
        (false, true) => format!("in from {far}"),
        _ => format!("with {far}"),
    }
}

/// Answers derived from the requirements: why rows lead to proof rows (each verifier, through the requirement), and a
/// requirement reached directly brings the ones above it (through it).
fn close_why(model: &Model, f: &Facts, why: &mut List, proof: &mut List) {
    let mut i = 0;
    while i < why.0.len() && why.0.len() < MAX {
        let r = why.0[i].id;
        for (u, rel) in f.up.get(&r).into_iter().flatten() {
            why.add(*u, rel, Some(r));
        }
        i += 1;
    }
    for r in why.ids() {
        for c in f.verified.get(&r).into_iter().flatten() {
            proof.add(*c, "verifies", Some(r));
        }
    }
    let _ = model;
}

pub fn connections(model: &Model, element: &str) -> Result<Value, String> {
    let e = model.find(element).ok_or_else(|| format!("no element {}", super::text::quote_clip(element)))?;
    let me = e.id;
    let f = model.conn_facts.get(|| facts(model));
    let (mut why, mut breaks, mut does, mut who, mut ex, mut proof) = (List::default(), List::default(), List::default(), List::default(), List::default(), List::default());
    let kind = e.kind.as_str();
    let rule;
    if is_req(model, me) {
        rule = "requirement";
        for (u, rel) in f.up.get(&me).into_iter().flatten() {
            why.add(*u, rel, None);
        }
        // The ones above the parents, through them.
        let mut i = 0;
        while i < why.0.len() && why.0.len() < MAX {
            let r = why.0[i].id;
            for (u, rel) in f.up.get(&r).into_iter().flatten() {
                why.add(*u, rel, Some(r));
            }
            i += 1;
        }
        for (d, rel) in f.down.get(&me).into_iter().flatten() {
            breaks.add(*d, rel, None);
        }
        let sats: Vec<ElementId> = f.satisfiers.get(&me).into_iter().flatten().copied().collect();
        // The usage the satisfy names, not also its definition (the index holds both).
        for s in sats.iter().filter(|s| !(model.elements[**s].is_def && is_action(model, **s) && sats.iter().any(|u| type_of(model, *u) == Some(**s)))) {
            if let Some(ci) = f.ix.conn_at.get(s) {
                // An interface requirement satisfied by a link: the link, what it carries, and its two ends.
                let c = &f.ix.conns[*ci];
                ex.add(*s, "satisfies it", None);
                for (it, _) in &c.items {
                    ex.add(*it, "carried", Some(*s));
                }
                for end in &c.ends {
                    if let Some(p) = end.last() {
                        who.add(p, "an end of the link", Some(*s));
                    }
                }
            } else if is_action(model, *s) {
                does.add(*s, "satisfies it", None);
                for (p, rel) in parts_of_function(model, &f, *s) {
                    who.add(p, rel, Some(*s));
                }
                // Its steps carry it out too, and the parts that do them (gps, fuser for NAV-1 inside NavContext).
                let mut holders = vec![*s];
                holders.extend(type_of(model, *s).filter(|t| is_action(model, *t)));
                for hd in holders {
                    for st in model.elements[hd].children.iter().copied().filter(|c| is_action(model, *c)) {
                        does.add(st, "its step", Some(*s));
                        for (p, rel) in parts_of_function(model, &f, st) {
                            who.add(p, rel, Some(st));
                        }
                    }
                }
            } else if model.elements[*s].kind != "connection" {
                // The requirement's own subject (`subject nav : NavContext`) is named as such.
                let subj = model.elements[me].children.iter().any(|c| type_of(model, *c) == Some(*s) || *c == *s);
                for u in usages_of(model, &f, *s) {
                    who.add(u, if subj { "its subject" } else { "satisfies it" }, if u == *s { None } else { Some(*s) });
                    for (func, _) in functions_of_part(model, &f, u) {
                        does.add(func, "performed by", Some(u));
                    }
                }
            }
        }
        // The interfaces: the connectors between two parts that do it.
        let parts: Vec<ElementId> = who.ids().into_iter().filter(|p| is_part(model, *p)).collect();
        for p in &parts {
            for (ci, k) in conns_at(model, &f, *p) {
                let c = &f.ix.conns[ci];
                let far = c.ends[1 - k].last();
                if far.is_some_and(|x| parts.contains(&x)) {
                    ex.add(c.id, &format!("joins {} and {}", name(model, *p), far.map(|x| name(model, x)).unwrap_or_default()), None);
                    for (it, _) in &c.items {
                        ex.add(*it, &format!("over {}", name(model, c.id)), Some(c.id));
                    }
                }
            }
        }
        // And the connectors the satisfying functions' items ride, at the parts that do them.
        for func in does.ids() {
            let pins = pins_of(model, func);
            for (p, _) in parts_of_function(model, &f, func) {
                for (ci, k) in conns_at(model, &f, p) {
                    let c = &f.ix.conns[ci];
                    if c.items.iter().any(|(it, _)| pins.iter().any(|x| x.0 == *it)) {
                        let far = c.ends[1 - k].last().map(|x| name(model, x)).unwrap_or_default();
                        ex.add(c.id, &way(model, &f, p, &c.items, &far), Some(func));
                        for (it, fl) in &c.items {
                            ex.add(*it, &way(model, &f, p, &[(*it, *fl)], &far), Some(c.id));
                        }
                    }
                }
            }
        }
        for c in f.verified.get(&me).into_iter().flatten() {
            proof.add(*c, "verifies it", None);
        }
    } else if !e.is_def && kind == "flow" && !f.ix.conn_at.contains_key(&me) {
        // An item flow drawn on an IBD: the item, the part that sends it and the one that takes it, the link it rides on,
        // the functions that make and use it, and what those serve.
        rule = "connector";
        let item = e.first_resolved(RefRole::Type);
        let end = |role: RefRole| e.refs_of(role).next().map(|r| r.segments.iter().filter_map(|s| s.resolved).collect::<Vec<_>>()).unwrap_or_default();
        let part_at = |segs: &[ElementId]| segs.iter().rev().find(|x| is_part(model, **x)).copied();
        let port_at = |segs: &[ElementId]| segs.last().copied().filter(|x| model.elements[*x].kind == "port");
        if let Some(i) = item {
            ex.add(i, "carries", None);
        }
        for c in f.ix.conns.iter().filter(|c| c.items.iter().any(|(_, fl)| *fl == me)) {
            ex.add(c.id, "carried on", None);
        }
        for (segs, says, pin, verb) in [(end(RefRole::Source), "sends it", "out", "sends it"), (end(RefRole::Target), "takes it", "in", "takes it")] {
            let Some(p) = part_at(&segs) else { continue };
            who.add(p, &port_at(&segs).map(|x| format!("{says}, at {}", name(model, x))).unwrap_or_else(|| says.to_string()), None);
            if let Some(i) = item {
                for (func, _) in functions_of_part(model, &f, p) {
                    if pins_of(model, func).iter().any(|(t, d)| *t == i && d == pin) {
                        does.add(func, verb, Some(p));
                    }
                }
            }
        }
        for r in satisfied_by(model, &f, me) {
            why.add(r, "satisfies", None);
        }
        for func in does.ids() {
            for (r, _) in why_of_function(model, &f, func) {
                why.add(r, "served by what it carries", Some(func));
            }
        }
    } else if !e.is_def && matches!(kind, "connection" | "interface" | "flow") || f.ix.conn_at.contains_key(&me) {
        rule = "connector";
        if let Some(ci) = f.ix.conn_at.get(&me) {
            let c = &f.ix.conns[*ci];
            for end in &c.ends {
                if let Some(p) = end.last() {
                    let at = end.port.map(|x| format!("end, at {}", name(model, x))).unwrap_or_else(|| "end".into());
                    who.add(p, &at, None);
                }
                if let Some(port) = end.port {
                    ex.add(port, "end port", None);
                }
            }
            if let Some(o) = c.owner {
                who.add(o, "in", None);
            }
            for (it, _) in &c.items {
                ex.add(*it, "carries", None);
            }
            // The functions at the two ends that make or take what it carries.
            for (func, w, q) in connector_does(model, &f, c) {
                does.add(func, w, Some(q));
            }
        }
        for r in satisfied_by(model, &f, me) {
            why.add(r, "satisfies", None);
        }
        // Why: only what the functions that send or take its items serve (not everything its end parts do).
        for func in does.ids() {
            for (r, _) in why_of_function(model, &f, func) {
                why.add(r, "served by what it carries", Some(func));
            }
        }
    } else if kind == "port" {
        rule = "port";
        let owner = e.owner;
        if let Some(o) = owner {
            who.add(o, "port of", None);
        }
        for c in &f.ix.conns {
            for (k, end) in c.ends.iter().enumerate() {
                if end.port == Some(me) || end.port.is_some_and(|x| model.elements[x].name == e.name && owner.is_some_and(|o| end.last().is_some_and(|l| type_of(model, l) == Some(o) || l == o))) {
                    ex.add(c.id, "connector", None);
                    if let Some(far) = c.ends[1 - k].last() {
                        who.add(far, &format!("at the far end of {}", name(model, c.id)), Some(c.id));
                    }
                    for (it, _) in &c.items {
                        ex.add(*it, "carries", Some(c.id));
                    }
                }
            }
        }
        let items: Vec<ElementId> = ex.ids().into_iter().filter(|x| model.elements[*x].kind == "item").collect();
        if let Some(o) = owner {
            for u in usages_of(model, &f, o) {
                for (func, _) in functions_of_part(model, &f, u) {
                    if pins_of(model, func).iter().any(|(t, _)| items.contains(t)) {
                        does.add(func, "uses it", Some(u));
                    }
                }
            }
        }
        for func in does.ids() {
            for (r, _) in why_of_function(model, &f, func) {
                why.add(r, "served by what it carries", Some(func));
            }
        }
    } else if is_action(model, me) && !e.children.iter().any(|c| model.elements[*c].kind == "message") {
        rule = "function";
        // Steps, the function it is a step of, the call's operation, the flows and successions around it.
        for c in &e.children {
            if is_action(model, *c) {
                does.add(*c, "step", None);
            }
        }
        if let Some(def) = type_of(model, me).filter(|t| is_action(model, *t)) {
            does.add(def, "is", None);
            for c in &model.elements[def].children {
                if is_action(model, *c) {
                    does.add(*c, "step", Some(def));
                }
            }
        }
        if let Some(call) = super::allocation::call_of(model, me) {
            does.add(call.op, "calls", None);
        }
        if let Some(o) = e.owner.filter(|o| is_action(model, *o)) {
            does.add(o, "step of", None);
        }
        // The steps that call this operation stand for it on the activity (lit there); the row names the function
        // they are a step of, not a second "acquireFix".
        for c in callers_of(model, &f, me) {
            if let Some(o) = model.elements[c].owner.filter(|o| is_action(model, *o)) {
                does.add(o, "a step of it", None);
            }
        }
        for s in &f.stmts {
            let (Some(a), Some(b)) = (s.source.id, s.target.id) else { continue };
            if !matches!(s.kind, "succession" | "flow") {
                continue;
            }
            let word = if s.kind == "flow" { ("sends to", "takes from") } else { ("then", "after") };
            // A flow names pins (acquireFix.gnss_fix): the step is the pin's owner.
            let step = |x: ElementId| if is_action(model, x) { Some(x) } else { model.elements[x].owner.filter(|o| is_action(model, *o)) };
            let (Some(a), Some(b)) = (step(a), step(b)) else { continue };
            let mine = |x: ElementId| x == me || super::allocation::call_of(model, x).is_some_and(|c| c.op == me);
            if mine(a) && !mine(b) {
                does.add(b, word.0, if a == me { None } else { Some(a) });
            } else if mine(b) && !mine(a) {
                does.add(a, word.1, if b == me { None } else { Some(b) });
            }
        }
        let mut doers = parts_of_function(model, &f, me);
        if let Some(o) = e.owner.filter(|o| is_action(model, *o)) {
            for (p, _) in parts_of_function(model, &f, o) {
                if !doers.iter().any(|x| x.0 == p) {
                    doers.push((p, "does the function it is a step of"));
                }
            }
        }
        for (p, rel) in &doers {
            who.add(*p, rel, None);
        }
        // Exchanges: the items of its pins, and the connectors at its doers that carry them.
        let pins = pins_of(model, me);
        for (it, d) in &pins {
            ex.add(*it, if d == "out" { "sends" } else if d == "in" { "takes" } else { "passes" }, None);
        }
        for (p, _) in &doers {
            for (ci, k) in conns_at(model, &f, *p) {
                let c = &f.ix.conns[ci];
                let carried: Vec<ElementId> = c.items.iter().map(|x| x.0).filter(|it| pins.iter().any(|x| x.0 == *it)).collect();
                if !carried.is_empty() {
                    let far = c.ends[1 - k].last().map(|x| name(model, x)).unwrap_or_default();
                    let dirs: BTreeSet<&str> = carried.iter().filter_map(|it| pins.iter().find(|x| x.0 == *it).map(|x| x.1.as_str())).collect();
                    let w = match (dirs.contains("out"), dirs.contains("in")) { (true, false) => format!("out to {far}"), (false, true) => format!("in from {far}"), _ => format!("with {far}") };
                    ex.add(c.id, &w, Some(*p));
                }
            }
        }
        for (r, via) in why_of_function(model, &f, me) {
            why.add(r, if via.is_some() { "served by the function it is in" } else { "satisfies" }, via);
        }
    } else if kind == "part" || (e.is_def && kind == "part") {
        rule = "part";
        // Who: the parts it is connected to first (what the drawing lights), then its owner, then what it is made of.
        for (ci, k) in conns_at(model, &f, me) {
            let c = &f.ix.conns[ci];
            let far = c.ends[1 - k].last();
            let farw = far.map(|x| name(model, x)).unwrap_or_default();
            if let Some(x) = far.filter(|x| *x != me) {
                who.add(x, &format!("connected over {}", name(model, c.id)), None);
            }
            let port = c.ends[k].port;
            let ways: BTreeSet<String> = c.items.iter().map(|(it, fl)| item_way(model, &f, me, port, *it, *fl, &farw)).collect();
            ex.add(c.id, &if ways.len() == 1 { ways.into_iter().next().unwrap_or_default() } else { format!("with {farw}") }, None);
            // One row per item; an item that goes both ways (or on two links) keeps every way.
            for (it, fl) in &c.items {
                ex.add_all(*it, &item_way(model, &f, me, port, *it, *fl, &farw), Some(c.id));
            }
        }
        if let Some(o) = e.owner.filter(|o| model.elements[*o].kind == "part") {
            who.add(o, "part of", None);
        }
        let mut kids: Vec<ElementId> = e.children.clone();
        if let Some(t) = type_of(model, me) {
            kids.extend(model.elements[t].children.iter().copied());
        }
        for k in kids {
            if is_part(model, k) && !model.elements[k].is_def {
                who.add(k, "made of", None);
            }
        }
        for (func, rel) in functions_of_part(model, &f, me) {
            does.add(func, rel, None);
            for c in callers_of(model, &f, func) {
                does.add(c, "its step", Some(func));
            }
        }
        for func in does.ids() {
            for (r, _) in why_of_function(model, &f, func) {
                why.add(r, "served by what it does", Some(func));
            }
        }
        for (ci, _) in conns_at(model, &f, me) {
            let c = f.ix.conns[ci].id;
            for r in satisfied_by(model, &f, c) {
                why.add(r, "satisfied by its link", Some(c));
            }
        }
        let reached = why.ids();
        for r in satisfied_by(model, &f, me).into_iter().filter(|r| !reached.contains(r)) {
            why.add(r, "satisfies", None);
        }
    } else {
        rule = if e.kind.contains("verification") || e.kind.contains("case") { "verification" } else if e.kind == "item" && e.is_def { "item" } else { "other" };
        for r in satisfied_by(model, &f, me) {
            why.add(r, "satisfies", None);
        }
        // A test: the requirements it verifies, and who and what carries them out.
        for (r, cases) in &f.verified {
            if cases.contains(&me) {
                why.add(*r, "verifies", None);
                for s in f.satisfiers.get(r).into_iter().flatten() {
                    if is_action(model, *s) {
                        does.add(*s, "carries it out", Some(*r));
                        for (p, _) in parts_of_function(model, &f, *s) {
                            who.add(p, "does it", Some(*r));
                        }
                    } else {
                        let subj = model.elements[*r].children.iter().any(|c| type_of(model, *c) == Some(*s));
                        for u in usages_of(model, &f, *s) {
                            who.add(u, if subj { "subject of it" } else { "satisfies it" }, Some(*r));
                        }
                    }
                }
            }
        }
        // An item: the functions that send and take it (their requirements are its Why).
        if e.kind == "item" && e.is_def {
            for a in model.elements.iter().filter(|a| is_action(model, a.id) && !a.is_def) {
                if let Some((_, d)) = pins_of(model, a.id).into_iter().find(|(t, _)| *t == me) {
                    does.add(a.id, if d == "out" { "sends it" } else if d == "in" { "takes it" } else { "passes it" }, None);
                }
            }
            for func in does.ids() {
                for (r, _) in why_of_function(model, &f, func) {
                    why.add(r, "served by what carries it", Some(func));
                }
            }
        }
        if e.kind == "item" && e.is_def {
            for c in &f.ix.conns {
                if c.items.iter().any(|(i, _)| *i == me) {
                    ex.add(c.id, "carried on", None);
                    for end in &c.ends {
                        if let Some(p) = end.last() {
                            who.add(p, &format!("end of {}", name(model, c.id)), Some(c.id));
                        }
                    }
                }
            }
        }
    }
    if rule != "requirement" {
        close_why(model, &f, &mut why, &mut proof);
    }
    // A test is the proof: other tests of the same requirements are not its proof.
    if rule == "verification" {
        proof = List::default();
    }
    // What a drawing lights for it, in any view: the element and its direct answers, each with the symbols that stand
    // for it elsewhere (a function's steps, the steps that call an operation, a part's functions on an activity).
    let mut seeds: Vec<ElementId> = vec![me];
    for l in [&why, &breaks, &does, &who, &ex] {
        seeds.extend(l.0.iter().filter(|r| r.via.is_none() || rule == "requirement" || rule == "verification").map(|r| r.id));
    }
    if rule == "part" {
        seeds.extend(does.ids());
    }
    let mut lights: Vec<ElementId> = Vec::new();
    for x in seeds {
        let mut more = vec![x];
        if is_action(model, x) {
            more.extend(callers_of(model, &f, x));
            // An item lights the pins that carry it, not every step of the functions that send it.
            if rule == "item" {
                let mut holders = vec![x];
                holders.extend(type_of(model, x));
                for hd in holders {
                    more.extend(model.elements[hd].children.iter().copied().filter(|c| type_of(model, *c) == Some(me)));
                }
                for m in more {
                    if !lights.contains(&m) && lights.len() < 400 {
                        lights.push(m);
                    }
                }
                continue;
            }
            // Its steps light only for the selection itself and for what a requirement or test is carried out by: a
            // step's "step of ProvideNavigation" lights that function, not its five sibling steps.
            if x == me || rule == "requirement" || rule == "verification" {
                let mut holders = vec![x];
                holders.extend(type_of(model, x).filter(|t| is_action(model, *t)));
                for hd in holders {
                    more.extend(model.elements[hd].children.iter().copied().filter(|c| is_action(model, *c)));
                }
            }
        }
        for m in more {
            if !lights.contains(&m) && lights.len() < 400 {
                lights.push(m);
            }
        }
    }
    let answers = [("why", &why), ("does", &does), ("who", &who), ("exchanges", &ex), ("proof", &proof)];
    // The links each kind of element is expected to have (one shared table, computed, never written into the model):
    // an empty expected answer is a gap; an empty answer the kind does not need is simply not asked.
    // Only usages are traced (a definition takes its usages' answers), and only the links the traceability rules
    // count: a part serves a requirement; a function serves one and has a doer; a link that carries items serves one
    // (a delegation through a border port carries nothing of its own); a requirement is carried out and verified.
    let carries = f.ix.conn_at.get(&me).is_some_and(|ci| !f.ix.conns[*ci].items.is_empty() && f.ix.conns[*ci].ends.iter().all(|x| !x.parts.is_empty()));
    let expected: &[&str] = if e.is_def && rule != "requirement" && rule != "verification" && rule != "item" {
        &[]
    } else {
        match rule {
            "part" => &["why"],
            "function" => &["why", "who"],
            "connector" if carries => &["why", "does"],
            "requirement" => &["does", "proof"],
            "verification" => &["why"],
            "item" => &["does"],
            _ => &[],
        }
    };
    let gaps: Vec<&str> = answers.iter().filter(|(k, l)| expected.contains(k) && l.0.iter().all(|r| r.id == me)).map(|(k, _)| *k).collect();
    Ok(json!({
        "element": e.qualified_name,
        "kind": e.kind,
        "rule": rule,
        "why": why.json(model, me),
        "breaks": breaks.json(model, me),
        "does": does.json(model, me),
        "who": who.json(model, me),
        "exchanges": ex.json(model, me),
        "proof": proof.json(model, me),
        "expected": expected,
        "gaps": gaps,
        "lights": lights.iter().map(|x| qn(model, *x)).collect::<Vec<_>>(),
    }))
}
