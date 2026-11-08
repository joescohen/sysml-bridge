//! Change impact: everything that depends on one element, grouped by what a change to it would touch.
//! Built on the same extraction as the other reads: the relationship statements of `graph`, the member
//! walk. No rule here knows a project; every list follows from the model.
//!
//! Cost of one call: one pass over the elements and their references for each element examined (one at
//! depth 1, and at most `MAX_AFFECTED` more (or the caller's `expand`) for each further level.

use super::graph::{self, Stmt};
use super::{connect, locate, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Rows kept per list unless the caller asks for another limit; the counts stay exact.
pub const DEFAULT_ROWS: usize = 100;
/// Most rows a caller may ask for per list.
pub const MAX_ROWS: usize = 2000;
/// Elements each level past the first expands unless the caller gives another number; the rest are counted and
/// `truncated` is set.
pub const MAX_AFFECTED: usize = 25;
/// Rows per list of an element expanded at depth 2.
pub const AFFECTED_ROWS: usize = 5;

/// Source ids by the qualified name they name.
pub struct SourceIds<'a>(HashMap<&'a str, Vec<&'a str>>);

impl<'a> SourceIds<'a> {
    pub fn new(map: &'a BTreeMap<String, String>) -> Self {
        let mut by: HashMap<&str, Vec<&str>> = HashMap::new();
        for (id, qn) in map {
            by.entry(qn.as_str()).or_default().push(id.as_str());
        }
        SourceIds(by)
    }

    /// Add `source_ids` to a row when the element has any.
    pub fn tag(&self, row: &mut Value, qn: &str) {
        if let Some(ids) = self.0.get(qn) {
            row["source_ids"] = json!(ids);
        }
    }
}


pub(crate) fn is_connector(e: &Element) -> bool {
    !e.is_def && matches!(e.kind.as_str(), "connection" | "interface" | "flow" | "binding" | "message")
}

/// `ref`, `kind`, `name` and the source ids of an element: the row every list is made of.
pub(crate) fn tag(_model: &Model, ids: &SourceIds, e: &Element) -> Value {
    let mut v = json!({ "ref": e.qualified_name, "kind": e.keyword() });
    if let (Some(n), false) = (&e.name, e.implied_name) {
        v["name"] = json!(n);
    }
    ids.tag(&mut v, &e.qualified_name);
    v
}

/// Transitions, accept actions and send actions with the elements their text names: (the statement, what its trigger,
/// payload or send text names, that text). The names are resolved from the statement outward as the parser would.
pub(crate) fn signal_statements(model: &Model) -> Vec<(ElementId, Vec<ElementId>, String)> {
    let mut queries: Vec<(ElementId, RefRole, String)> = Vec::new();
    let mut texts: Vec<(ElementId, String)> = Vec::new();
    for e in &model.elements {
        let text = match e.kind.as_str() {
            "transition" => e.props.get("trigger"),
            "accept action" => e.props.get("payload"),
            "send action" => e.props.get("text"),
            k if k.contains("action") && matches!(e.props.get("node").map(String::as_str), Some("send" | "accept")) => e.props.get("text"),
            _ => None,
        };
        let Some(t) = text else { continue };
        for n in names_in(t) {
            queries.push((e.id, RefRole::Expr, n));
        }
        texts.push((e.id, t.clone()));
    }
    let found = if queries.is_empty() { Vec::new() } else { crate::resolve::resolve_texts(model, &queries) };
    let mut named: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for ((el, _, _), hit) in queries.iter().zip(found) {
        named.entry(*el).or_default().extend(hit);
    }
    texts.into_iter().filter_map(|(el, t)| named.remove(&el).filter(|v| !v.is_empty()).map(|v| (el, v, t))).collect()
}

/// Names in the text of a trigger, payload or send statement: `Sig`, `Pkg::Sig`, `'My Signal'`.
fn names_in(text: &str) -> Vec<String> {
    const WORDS: &[&str] = &["after", "when", "at", "via", "to", "accept", "send", "new", "true", "false", "and", "or", "not", "if", "do", "then"];
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < c.len() && out.len() < 16 {
        let start = i;
        loop {
            if c.get(i) == Some(&'\'') {
                i += 1;
                while i < c.len() && c[i] != '\'' {
                    i += if c[i] == '\\' { 2 } else { 1 };
                }
                i = (i + 1).min(c.len());
            } else {
                let from = i;
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
                if i == from {
                    break;
                }
            }
            if c.get(i) == Some(&':') && c.get(i + 1) == Some(&':') {
                i += 2;
            } else {
                break;
            }
        }
        if i == start {
            i += 1;
            continue;
        }
        let name: String = c[start..i].iter().collect();
        if !WORDS.contains(&name.as_str()) && !name.chars().next().map(|x| x.is_ascii_digit()).unwrap_or(true) {
            out.push(name);
        }
    }
    out
}

struct Ctx<'a> {
    model: &'a Model,
    ids: SourceIds<'a>,
    stmts: Vec<Stmt>,
    /// Statements whose source-target pairs were cut short.
    cuts: Vec<graph::Cut>,
    /// Definition -> the usages it types.
    typed: HashMap<ElementId, Vec<ElementId>>,
    /// General -> the elements that specialize it.
    subs: HashMap<ElementId, Vec<ElementId>>,
    /// Transitions, accept actions and send actions with the elements their text names.
    signals: Vec<(ElementId, Vec<ElementId>, String)>,
    /// Every allocation (`allocate` and `perform`).
    allocs: Vec<super::allocation::Allocation>,
}

impl<'a> Ctx<'a> {
    fn new(model: &'a Model, ids: &'a BTreeMap<String, String>) -> Self {
        let (mut typed, mut subs): (HashMap<ElementId, Vec<ElementId>>, HashMap<ElementId, Vec<ElementId>>) = (HashMap::new(), HashMap::new());
        for e in &model.elements {
            let statement = e.kind.starts_with("satisfy") || e.kind.starts_with("verify") || is_connector(e);
            for r in &e.refs {
                let Some(t) = r.resolved() else { continue };
                match r.role {
                    RefRole::Type if !statement && !e.is_def => typed.entry(t).or_default().push(e.id),
                    RefRole::Specializes => subs.entry(t).or_default().push(e.id),
                    _ => {}
                }
            }
        }
        let signals = signal_statements(model);
        let (stmts, cuts) = graph::statements_cut(model);
        let allocs = super::allocation::allocations(model);
        Ctx { model, ids: SourceIds::new(ids), stmts, cuts, typed, subs, signals, allocs }
    }

    fn tag(&self, id: ElementId) -> Value {
        tag(self.model, &self.ids, &self.model.elements[id])
    }

    fn qn(&self, id: ElementId) -> &str {
        &self.model.elements[id].qualified_name
    }
}

/// The far end of a connector: what it names, the port it reaches and the part that holds that port.
fn far_end(cx: &Ctx, via: &Element, r: &Reference) -> Value {
    let m = cx.model;
    let ids: Vec<ElementId> = r.segments.iter().filter_map(|s| s.resolved).collect();
    let port = ids.iter().position(|x| m.elements[*x].kind == "port");
    let owner = match port {
        // A port named directly belongs to the element that holds the connector.
        Some(0) => via.owner,
        Some(k) => Some(ids[k - 1]),
        None => ids.last().and_then(|l| m.elements[*l].owner),
    };
    let mut v = json!({ "text": r.text, "ref": ids.last().map(|l| cx.qn(*l)) });
    if let Some(p) = port {
        v["port"] = json!(cx.qn(ids[p]));
    }
    if let Some(o) = owner {
        let oe = &m.elements[o];
        v["owner"] = json!(oe.qualified_name);
        let types: Vec<&str> = oe.refs_of(RefRole::Type).map(|t| t.text.as_str()).collect();
        if !types.is_empty() {
            v["owner_type"] = json!(types);
        }
    }
    v
}

fn of_types(model: &Model, e: &Element) -> Vec<String> {
    e.refs_of(RefRole::Type).map(|r| r.resolved().map(|t| model.elements[t].qualified_name.clone()).unwrap_or_else(|| r.text.clone())).collect()
}

/// A connector as a row: which end reaches the subject, and where the other end goes.
fn connector_row(cx: &Ctx, via: &Element, hit_ref: usize) -> Value {
    let r = &via.refs[hit_ref];
    let mut row = cx.tag(via.id);
    row["end"] = json!(if r.role == RefRole::Source { "source" } else { "target" });
    row["end_text"] = json!(r.text);
    let far: Vec<Value> = via.refs.iter().enumerate().filter(|(i, o)| *i != hit_ref && matches!(o.role, RefRole::Source | RefRole::Target)).map(|(_, o)| far_end(cx, via, o)).collect();
    let mut far = far.into_iter();
    row["far_end"] = far.next().unwrap_or(Value::Null);
    let more: Vec<Value> = far.collect();
    if !more.is_empty() {
        row["other_far_ends"] = json!(more);
    }
    if matches!(via.kind.as_str(), "flow" | "message") {
        row["of"] = json!(of_types(cx.model, via));
    }
    if let Some(o) = via.owner {
        row["declared_in"] = json!(cx.qn(o));
    }
    row
}

struct Lists {
    limit: usize,
    counts: serde_json::Map<String, Value>,
    cut: Vec<String>,
}

impl Lists {
    /// The first `limit` rows; the count under `name` stays exact.
    fn put(&mut self, name: &str, mut rows: Vec<Value>) -> Value {
        self.counts.insert(name.to_string(), json!(rows.len()));
        if rows.len() > self.limit {
            rows.truncate(self.limit);
            self.cut.push(name.to_string());
        }
        json!(rows)
    }
}

/// The impact lists of one element, and the elements a change to it reaches directly.
fn one(cx: &Ctx, e: &Element, limit: usize) -> (Value, Vec<ElementId>) {
    let m = cx.model;
    let mut affected: Vec<ElementId> = Vec::new();
    let mut l = Lists { limit, counts: serde_json::Map::new(), cut: Vec::new() };

    // Usages: of a definition (and of the definitions that specialize it); for a usage, its definition's other usages.
    let mut usage_rows: Vec<Value> = Vec::new();
    let mut usage_ids: Vec<ElementId> = Vec::new();
    let mut definitions: Vec<Value> = Vec::new();
    if e.is_def {
        let mut family = vec![e.id];
        let mut seen: HashSet<ElementId> = HashSet::from([e.id]);
        let mut i = 0;
        while i < family.len() {
            for s in cx.subs.get(&family[i]).into_iter().flatten() {
                if m.elements[*s].is_def && seen.insert(*s) {
                    family.push(*s);
                }
            }
            i += 1;
        }
        for d in &family {
            for u in cx.typed.get(d).into_iter().flatten() {
                let mut row = cx.tag(*u);
                row["owner"] = json!(m.elements[*u].owner.map(|o| cx.qn(o)));
                if *d != e.id {
                    row["via"] = json!(cx.qn(*d));
                }
                usage_rows.push(row);
                usage_ids.push(*u);
            }
        }
    } else {
        for d in e.refs_of(RefRole::Type).filter_map(|r| r.resolved()).filter(|d| m.elements[*d].is_def) {
            definitions.push(cx.tag(d));
            affected.push(d);
            for u in cx.typed.get(&d).into_iter().flatten().filter(|u| **u != e.id) {
                let mut row = cx.tag(*u);
                row["owner"] = json!(m.elements[*u].owner.map(|o| cx.qn(o)));
                row["of"] = json!(cx.qn(d));
                usage_rows.push(row);
            }
        }
    }
    affected.extend(&usage_ids);

    let mut inside: HashSet<ElementId> = m.descendants(e.id).into_iter().collect();
    inside.insert(e.id);
    // A change to a definition reaches its usages; a statement that names one of them names the subject.
    let mut subject: HashSet<ElementId> = inside.clone();
    subject.extend(&usage_ids);

    // Ports it owns or inherits; a port usage is its own port, a port definition stands for the ports it types.
    let mut ports: Vec<ElementId> = Vec::new();
    if e.kind == "port" {
        if e.is_def {
            ports.extend(&usage_ids);
        } else {
            ports.push(e.id);
        }
    }
    ports.extend(crate::agent::text::all_members(m, e.id).into_iter().filter(|p| m.elements[*p].kind == "port" && !m.elements[*p].is_def));
    let mut port_at: HashMap<ElementId, usize> = HashMap::new();
    ports.retain(|p| {
        let n = port_at.len();
        port_at.insert(*p, n).is_none()
    });
    let mut port_lists: Vec<[Vec<Value>; 4]> = ports.iter().map(|_| Default::default()).collect();
    // For a part usage, a connector counts when it names that usage on the way to the port (or lies inside it).
    let need = (!e.is_def && e.kind != "port").then_some(e.id);

    let mut through: Vec<(ElementId, usize, ElementId)> = Vec::new();
    let mut on_port: HashSet<ElementId> = HashSet::new();
    let (mut specializations, mut redefinitions, mut performed_by, mut names_it, mut other) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut conveyed_by: Vec<Value> = Vec::new();
    let mut seen_other: HashSet<ElementId> = HashSet::new();
    for o in &m.elements {
        let within = inside.contains(&o.id);
        let conn = is_connector(o);
        for (ri, r) in o.refs.iter().enumerate() {
            let segs: Vec<ElementId> = r.segments.iter().filter_map(|s| s.resolved).collect();
            let Some(hit) = segs.iter().copied().find(|x| subject.contains(x) || port_at.contains_key(x)) else { continue };
            if conn && matches!(r.role, RefRole::Source | RefRole::Target) {
                let port = segs.iter().find_map(|x| port_at.get(x).copied());
                let mine = need.map(|n| segs.contains(&n) || within).unwrap_or(true);
                match port {
                    Some(p) if mine => {
                        let slot = match o.kind.as_str() {
                            "connection" => 0,
                            "interface" => 1,
                            "binding" => 3,
                            _ => 2,
                        };
                        port_lists[p][slot].push(connector_row(cx, o, ri));
                        on_port.insert(o.id);
                    }
                    _ if subject.contains(&hit) => through.push((o.id, ri, hit)),
                    _ => {}
                }
                continue;
            }
            if !subject.contains(&hit) {
                continue;
            }
            if conn && r.role == RefRole::Type {
                if r.resolved() == Some(e.id) {
                    let mut row = cx.tag(o.id);
                    row["declared_in"] = json!(o.owner.map(|x| cx.qn(x)));
                    conveyed_by.push(row);
                }
                continue;
            }
            if within || o.kind.starts_with("satisfy") || o.kind.starts_with("verify") || o.kind == "dependency" || (o.kind == "allocation" && !o.is_def) {
                // Inside the subject, or a trace statement: those are listed from the relationship statements below.
                continue;
            }
            let target = r.resolved().filter(|t| inside.contains(t));
            let mut row = cx.tag(o.id);
            match (r.role, target) {
                (RefRole::Specializes | RefRole::References, Some(t)) => {
                    row["of"] = json!(cx.qn(t));
                    row["relation"] = json!(if r.role == RefRole::Specializes { "specialization" } else { "reference" });
                    specializations.push(row);
                    affected.push(o.id);
                }
                (RefRole::Redefines, Some(t)) => {
                    row["of"] = json!(cx.qn(t));
                    redefinitions.push(row);
                    affected.push(o.id);
                }
                (RefRole::Type, Some(t)) if t == e.id || usage_ids.contains(&o.id) => {}
                _ if !seen_other.insert(o.id) => {}
                // What performs it is read from the allocations below.
                _ if o.kind.starts_with("perform") => {}
                _ if o.kind.starts_with("exhibit") || o.kind.starts_with("include") => {
                    row["names"] = json!(cx.qn(hit));
                    row["by"] = json!(o.owner.map(|x| cx.qn(x)));
                    performed_by.push(row);
                }
                _ => {
                    row["names"] = json!(cx.qn(hit));
                    row["role"] = json!(r.role);
                    let case = std::iter::successors(o.owner, |x| m.elements[*x].owner).find(|x| m.elements[*x].kind.starts_with("verification"));
                    match case {
                        Some(c) => {
                            row["case"] = json!(cx.qn(c));
                            row["why"] = json!("names it");
                            names_it.push(row);
                        }
                        None => other.push(row),
                    }
                }
            }
        }
    }
    through.retain(|(o, _, _)| !on_port.contains(o));
    let mut seen_through: HashSet<ElementId> = HashSet::new();
    through.retain(|(o, _, _)| seen_through.insert(*o));

    let mut port_rows: Vec<Value> = Vec::new();
    let (mut n_conn, mut n_iface, mut n_flow, mut n_bind) = (0, 0, 0, 0);
    let mut signal_types: HashSet<ElementId> = HashSet::new();
    if e.is_def && matches!(e.kind.as_str(), "item" | "attribute" | "part" | "occurrence") {
        signal_types.insert(e.id);
    }
    for (p, lists) in ports.iter().zip(port_lists) {
        let pe = &m.elements[*p];
        let mut row = cx.tag(*p);
        row["type"] = json!(pe.refs_of(RefRole::Type).map(|r| if r.conjugated { format!("~{}", r.text) } else { r.text.clone() }).collect::<Vec<_>>());
        row["direction"] = json!(connect::effective_direction(m, pe));
        row["declared_in"] = json!(pe.owner.map(|o| cx.qn(o)));
        if pe.owner != Some(e.id) && *p != e.id {
            row["inherited"] = json!(true);
        }
        for d in pe.refs_of(RefRole::Type).filter_map(|r| r.resolved()) {
            for f in crate::agent::text::all_members(m, d).into_iter().map(|f| &m.elements[f]).filter(|f| f.direction.is_some()) {
                signal_types.extend(f.refs_of(RefRole::Type).filter_map(|r| r.resolved()));
            }
        }
        let [c, i, f, b] = lists;
        for x in c.iter().chain(&i).chain(&f).chain(&b) {
            if let Some(o) = x["far_end"]["owner"].as_str().and_then(|q| m.find(q)) {
                affected.push(o.id);
            }
        }
        for x in &f {
            for t in x["of"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                signal_types.extend(m.find(t).map(|t| t.id));
            }
        }
        n_conn += c.len();
        n_iface += i.len();
        n_flow += f.len();
        n_bind += b.len();
        row["connected"] = json!(!(c.is_empty() && i.is_empty() && f.is_empty() && b.is_empty()));
        let cut = |mut v: Vec<Value>| {
            v.truncate(limit);
            v
        };
        row["connections"] = json!(cut(c));
        row["interfaces"] = json!(cut(i));
        row["flows"] = json!(cut(f));
        if !b.is_empty() {
            row["bindings"] = json!(cut(b));
        }
        port_rows.push(row);
    }
    for (name, n) in [("port_connections", n_conn), ("port_interfaces", n_iface), ("port_flows", n_flow), ("port_bindings", n_bind)] {
        l.counts.insert(name.into(), json!(n));
    }

    let (mut through_conn, mut through_flow) = (Vec::new(), Vec::new());
    for (o, ri, hit) in &through {
        let oe = &m.elements[*o];
        let mut row = connector_row(cx, oe, *ri);
        row["through"] = json!(cx.qn(*hit));
        if matches!(oe.kind.as_str(), "flow" | "message") { &mut through_flow } else { &mut through_conn }.push(row);
    }

    // Traceability, from the relationship statements.
    let hit_end = |s: &Stmt, source: bool| -> Option<ElementId> {
        let end = if source { &s.source } else { &s.target };
        if let Some(id) = end.id.filter(|id| subject.contains(id)) {
            return Some(id);
        }
        // A feature chain that passes through the subject: `satisfy R by sys.engine.power`.
        graph::through(m, s.via, end).into_iter().find(|x| subject.contains(x))
    };
    let end_name = |end: &graph::End| end.id.map(|i| cx.qn(i).to_string()).unwrap_or_else(|| end.text.clone());
    let trace_row = |s: &Stmt, hit: ElementId| {
        let mut row = json!({ "kind": s.kind, "source": end_name(&s.source), "target": end_name(&s.target), "statement": cx.qn(s.via) });
        if hit != e.id {
            row["via"] = json!(cx.qn(hit));
            // How the link reaches the element when it is not stated on it: through a usage of it, or through an element it owns.
            row["basis"] = json!(if usage_ids.contains(&hit) { "usage" } else { "owned" });
        }
        cx.ids.tag(&mut row, cx.qn(s.via));
        row
    };
    let (mut satisfies, mut satisfied_by, mut verifications, mut allocations, mut links) = (Vec::new(), Vec::new(), names_it, Vec::new(), Vec::new());
    let mut satisfied: HashSet<ElementId> = HashSet::new();
    let mut allocated_behaviour: Vec<Value> = Vec::new();
    let behavioural = |id: ElementId| {
        let k = m.elements[id].kind.as_str();
        k.contains("action") || k.contains("state") || k == "perform" || k == "exhibit"
    };
    let mut item_flows: Vec<Value> = Vec::new();
    for s in &cx.stmts {
        // Allocations are read from `allocations` below.
        if !matches!(s.kind, "satisfy" | "verify" | "derive" | "refine" | "trace" | "dependency" | "itemflow") {
            continue;
        }
        let (src, tgt) = (hit_end(s, true), hit_end(s, false));
        if s.kind == "itemflow" {
            // An item flow is a flow: it is listed by its ends, and by the connection that carries it.
            let f = graph::item_flow(m, s);
            let carried = is_connector(e) && f.carriers.iter().any(|c| c.id == Some(e.id));
            let Some(hit) = src.or(tgt).or(carried.then_some(e.id)) else { continue };
            let mut row = trace_row(s, hit);
            row["ref"] = json!(cx.qn(s.via));
            row["direction"] = json!(match (src.is_some(), tgt.is_some()) {
                (true, true) => "within",
                (true, false) => "out",
                (false, true) => "in",
                _ => "carried",
            });
            row["of"] = json!(f.items.iter().map(&end_name).collect::<Vec<_>>());
            // `carried_by` lists the connections the model names; a flow without that note says so.
            row["carried_by"] = json!(f.carriers.iter().map(&end_name).collect::<Vec<_>>());
            row["carrier"] = json!(if f.carriers.is_empty() { "not stated" } else { "stated" });
            affected.extend(if src.is_some() { s.target.id } else { s.source.id });
            item_flows.push(row);
            continue;
        }
        let Some(hit) = src.or(tgt) else { continue };
        let mut row = trace_row(s, hit);
        let other = if src.is_some() { s.target.id } else { s.source.id };
        affected.extend(other);
        match s.kind {
            "satisfy" if src.is_some() => {
                satisfied.extend(s.target.id);
                satisfies.push(row);
            }
            "satisfy" => satisfied_by.push(row),
            "verify" => {
                row["why"] = json!(if src.is_some() { "it verifies the requirement" } else { "verifies it" });
                verifications.push(row);
            }
            _ => {
                row["direction"] = json!(if src.is_some() { "out" } else { "in" });
                links.push(row);
            }
        }
    }
    // Allocations, performs and performed by: one list, `allocations`. An end is the subject, or a feature chain of an
    // `allocate` passes through it (`allocate f to sys.ctl` for `sys`).
    let alloc_stmts: HashMap<ElementId, Vec<&Stmt>> = {
        let mut h: HashMap<ElementId, Vec<&Stmt>> = HashMap::new();
        for s in cx.stmts.iter().filter(|s| s.kind == "allocate") {
            h.entry(s.via).or_default().push(s);
        }
        h
    };
    let (mut performs, mut performed_by_alloc) = (Vec::new(), Vec::new());
    // A perform written in a definition is listed once for the definition, not once for each usage it reaches.
    let mut performs_seen = HashSet::new();
    let mut rows_seen = HashSet::new();
    // An action a part owns is listed with what the part owns (behaviour.owned), not again here.
    for a in cx.allocs.iter().filter(|a| a.via != "owns") {
        // A statement in the definition reads as the definition's own for it, once, and not once for each usage.
        let part = if a.defined_on == Some(e.id) { e.id } else { a.part };
        if !rows_seen.insert((a.function, part, a.via, a.statement)) {
            continue;
        }
        let chain = |source: bool| alloc_stmts.get(&a.statement).into_iter().flatten().find_map(|s| hit_end(s, source));
        let src = Some(a.function).filter(|f| subject.contains(f)).or_else(|| if a.via == "allocate" { chain(true) } else { None });
        let tgt = Some(part).filter(|p| subject.contains(p)).or_else(|| if a.via == "allocate" { chain(false) } else { None });
        let mut row = json!({ "kind": a.via, "source": cx.qn(a.function), "target": cx.qn(part), "statement": cx.qn(a.statement), "implied": !a.is_direct() });
        if let Some(t) = a.through {
            row["through"] = json!(cx.qn(t));
        }
        if let Some(op) = a.operation {
            row["operation"] = json!(cx.qn(op));
        }
        if let Some(d) = a.defined_on.filter(|d| *d != part) {
            row["defined_on"] = json!(cx.qn(d));
        }
        cx.ids.tag(&mut row, cx.qn(a.statement));
        // What it performs: as the part (a usage typed by the definition the perform is written in, too), or as that
        // definition.
        if a.via == "perform" && (a.part == e.id || a.defined_on == Some(e.id)) && performs_seen.insert((a.function, a.statement, a.part == e.id)) {
            let mut p = cx.tag(a.statement);
            p["of"] = json!([cx.qn(a.function)]);
            p["function"] = json!(cx.qn(a.function));
            p["implied"] = json!(!a.is_direct());
            if let Some(t) = a.through {
                p["through"] = json!(cx.qn(t));
            }
            if let Some(d) = a.defined_on {
                p["defined_on"] = json!(cx.qn(d));
            }
            performs.push(p);
        }
        // The element itself wins over a member of it: `perform action plan` in `Controller` is allocated to it.
        let (src, tgt) = if tgt == Some(e.id) && src != Some(e.id) { (None, tgt) } else { (src, tgt) };
        // An operation is reached by the call actions that call it.
        let called = a.operation.filter(|o| subject.contains(o));
        let Some(hit) = src.or(tgt).or(called) else { continue };
        if hit != e.id {
            row["via"] = json!(cx.qn(hit));
        }
        row["direction"] = json!(if src.is_some() { "from it" } else if tgt.is_some() { "to it" } else { "calls it" });
        affected.push(if src.is_some() { part } else { a.function });
        if src.is_some() && a.via == "perform" {
            let mut p = cx.tag(a.statement);
            p["names"] = json!(cx.qn(a.function));
            p["by"] = json!(cx.qn(a.part));
            p["implied"] = json!(!a.is_direct());
            if let Some(t) = a.through {
                p["through"] = json!(cx.qn(t));
            }
            performed_by_alloc.push(p);
        }
        // Behaviour allocated to it is what `allocations` credits to it (or to a usage of it, for a definition): a perform
        // in a nested part, or an `allocate` whose chain only passes through it, is the nested part's, not its own.
        if tgt.is_some() && behavioural(a.function) && (part == e.id || usage_ids.contains(&part)) {
            let mut b = cx.tag(a.function);
            b["statement"] = json!(cx.qn(a.statement));
            b["direction"] = json!("to it");
            b["via"] = json!(a.via);
            b["implied"] = json!(!a.is_direct());
            if let Some(op) = a.operation {
                b["operation"] = json!(cx.qn(op));
            }
            allocated_behaviour.push(b);
        }
        allocations.push(row);
    }
    performed_by.extend(performed_by_alloc);
    for s in cx.stmts.iter().filter(|s| s.kind == "verify") {
        if s.target.id.map(|t| satisfied.contains(&t) && !subject.contains(&t)).unwrap_or(false) && hit_end(s, true).is_none() {
            let mut row = trace_row(s, e.id);
            row["why"] = json!("verifies a requirement it satisfies");
            row["basis"] = json!("satisfied requirement");
            affected.extend(s.source.id);
            verifications.push(row);
        }
    }

    // Behaviour: what it owns or performs, what is allocated to it, and the transitions its signals trigger.
    let members = crate::agent::text::all_members(m, e.id);
    let (mut exhibits, mut owned) = (Vec::new(), Vec::new());
    for b in members.iter().map(|b| &m.elements[*b]).filter(|b| !b.is_def) {
        let mut row = cx.tag(b.id);
        let target: Vec<String> = b.refs.iter().filter(|r| matches!(r.role, RefRole::Target | RefRole::Type)).map(|r| r.resolved().map(|t| cx.qn(t).to_string()).unwrap_or_else(|| r.text.clone())).collect();
        if !target.is_empty() {
            row["of"] = json!(target);
        }
        if b.kind.starts_with("exhibit") {
            exhibits.push(row.clone());
        }
        if behavioural(b.id) {
            owned.push(row);
        }
    }
    let mut transitions: Vec<Value> = Vec::new();
    if !signal_types.is_empty() || !ports.is_empty() {
        for (el, named, text) in &cx.signals {
            let Some(hit) = named.iter().find(|n| signal_types.contains(n) || port_at.contains_key(n)) else { continue };
            let te = &m.elements[*el];
            let mut row = cx.tag(*el);
            row["text"] = json!(text);
            row[if port_at.contains_key(hit) { "via_port" } else { "signal" }] = json!(cx.qn(*hit));
            row["declared_in"] = json!(te.owner.map(|o| cx.qn(o)));
            transitions.push(row);
        }
    }

    let mut head = super::brief(m, e);
    cx.ids.tag(&mut head, &e.qualified_name);
    let mut out = json!({ "element": head });
    if !definitions.is_empty() {
        out["definition"] = json!(definitions);
    }
    out["usages"] = l.put("usages", usage_rows);
    out["ports"] = l.put("ports", port_rows);
    out["through"] = json!({ "connections": l.put("through_connections", through_conn), "flows": l.put("through_flows", through_flow) });
    out["item_flows"] = l.put("item_flows", item_flows);
    if !conveyed_by.is_empty() {
        out["conveyed_by"] = l.put("conveyed_by", conveyed_by);
    }
    if is_connector(e) {
        // A connection, interface, flow or binding is itself the link: its ends, and what runs on it.
        let ends: Vec<Value> = e
            .refs
            .iter()
            .filter(|r| matches!(r.role, RefRole::Source | RefRole::Target))
            .map(|r| {
                let mut row = far_end(cx, e, r);
                row["end"] = json!(if r.role == RefRole::Source { "source" } else { "target" });
                row
            })
            .collect();
        out["ends"] = json!(ends);
        let carriers = super::connect::flow_carriers(m);
        if let Some(c) = carriers.get(&e.id) {
            out["carried_by"] = cx.tag(*c);
        }
        let mut carried: Vec<ElementId> = carriers.iter().filter(|(_, c)| **c == e.id).map(|(f, _)| *f).collect();
        carried.sort();
        let rows: Vec<Value> = carried
            .into_iter()
            .map(|f| {
                let mut row = cx.tag(f);
                row["of"] = json!(of_types(m, &m.elements[f]));
                row
            })
            .collect();
        out["carries"] = l.put("carries", rows);
    }
    out["traceability"] = json!({
        "satisfies": l.put("satisfies", satisfies),
        "satisfied_by": l.put("satisfied_by", satisfied_by),
        "verifications": l.put("verifications", verifications),
        "allocations": l.put("allocations", allocations),
        "links": l.put("links", links),
        "performs": l.put("performs", performs),
        "exhibits": l.put("exhibits", exhibits),
        "performed_by": l.put("performed_by", performed_by),
    });
    out["specializations"] = l.put("specializations", specializations);
    out["redefinitions"] = l.put("redefinitions", redefinitions);
    out["behaviour"] = json!({
        "owned": l.put("behaviour_owned", owned),
        "allocated": l.put("behaviour_allocated", allocated_behaviour),
        "transitions": l.put("transitions", transitions),
    });
    out["other_references"] = l.put("other_references", other);
    out["counts"] = Value::Object(l.counts);
    out["truncated"] = json!(!l.cut.is_empty());
    out["truncated_lists"] = json!(l.cut);
    let mut seen: HashSet<ElementId> = HashSet::new();
    affected.retain(|a| !inside.contains(a) && seen.insert(*a));
    (out, affected)
}

/// What a package relies on outside itself: every resolved reference from an element it holds (at any depth) to an
/// element it does not hold. Rows {from, to, role, package}, in model order, at most `limit`; by_package counts the
/// references per package of the target (its nearest owning package; the model root when it has none), largest first.
pub fn package_depends_on(model: &Model, pkg: &Element, limit: usize) -> Value {
    let mut inside: HashSet<ElementId> = HashSet::new();
    let mut stack = vec![pkg.id];
    while let Some(x) = stack.pop() {
        if inside.insert(x) {
            stack.extend(model.elements[x].children.iter().copied());
        }
    }
    let package_of = |mut t: ElementId| -> String {
        loop {
            let e = &model.elements[t];
            if e.kind == "package" {
                return e.qualified_name.clone();
            }
            match e.owner {
                Some(o) => t = o,
                None => return String::new(),
            }
        }
    };
    let role = |r: RefRole| match r {
        RefRole::Type => "typed by",
        RefRole::Specializes => "specializes",
        RefRole::Redefines => "redefines",
        RefRole::References => "references",
        RefRole::Source | RefRole::Target => "relationship end",
        RefRole::Import => "imports",
        RefRole::Meta => "metadata",
        RefRole::Expr | RefRole::Call | RefRole::Assign => "value expression",
        RefRole::Other => "other",
    };
    let mut ids: Vec<ElementId> = inside.iter().copied().collect();
    ids.sort_unstable();
    let (mut rows, mut total) = (Vec::new(), 0usize);
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for id in ids {
        let e = &model.elements[id];
        for r in &e.refs {
            let Some(t) = r.resolved() else { continue };
            if inside.contains(&t) {
                continue;
            }
            let p = package_of(t);
            total += 1;
            *by.entry(p.clone()).or_default() += 1;
            if rows.len() < limit {
                rows.push(json!({ "from": e.qualified_name, "to": model.elements[t].qualified_name, "role": role(r.role), "package": p }));
            }
        }
    }
    let mut counts: Vec<(String, usize)> = by.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    json!({ "count": total, "rows": rows, "truncated": total > rows.len(),
            "by_package": counts.into_iter().map(|(p, n)| json!({ "package": p, "count": n })).collect::<Vec<_>>() })
}

/// `get_impact`: what a change to `name` touches. `ids` maps a source id to a qualified name. Each level past the first
/// expands at most `MAX_AFFECTED` elements (`get_impact_with` takes another number).
pub fn get_impact(model: &Model, ids: &BTreeMap<String, String>, name: &str, depth: usize, limit: usize) -> Res<Value> {
    get_impact_with(model, ids, name, depth, limit, None)
}

/// `get_impact` to any depth: depth 1 is what depends on the element, each further level the same question for each
/// element the level before reached (each element once). `expand` is how many elements each level past the first
/// expands (default `MAX_AFFECTED`); the rest are counted and `truncated` is set. A depth of 0 is read as 1.
pub fn get_impact_with(model: &Model, ids: &BTreeMap<String, String>, name: &str, depth: usize, limit: usize, expand: Option<usize>) -> Res<Value> {
    let e = locate(model, name)?;
    let depth = depth.max(1);
    let expand = expand.unwrap_or(MAX_AFFECTED).max(1);
    let limit = limit.clamp(1, MAX_ROWS);
    let cx = Ctx::new(model, ids);
    let (mut out, affected) = one(&cx, e, limit);
    out["depth"] = json!(depth);
    out["limit"] = json!(limit);
    out["affected_total"] = json!(affected.len());
    if e.kind == "package" {
        out["depends_on"] = package_depends_on(model, e, limit);
    }
    graph::mark_cut(model, &mut out, &cx.cuts);
    if depth >= 2 {
        let mut seen: HashSet<ElementId> = HashSet::from([e.id]);
        let mut level: Vec<ElementId> = affected.into_iter().filter(|a| seen.insert(*a)).collect();
        let mut rows: Vec<Value> = Vec::new();
        let mut cut = false;
        for at in 2..=depth {
            if level.is_empty() {
                break;
            }
            if level.len() > expand {
                cut = true;
            }
            let mut next: Vec<ElementId> = Vec::new();
            for a in level.iter().take(expand) {
                let (mut row, more) = one(&cx, &model.elements[*a], AFFECTED_ROWS.min(limit));
                if depth > 2 {
                    row["level"] = json!(at);
                }
                rows.push(row);
                next.extend(more.into_iter().filter(|m| seen.insert(*m)));
            }
            level = next;
        }
        if cut {
            out["truncated"] = json!(true);
            out["truncated_lists"].as_array_mut().map(|l| l.push(json!("affected")));
        }
        out["affected"] = json!(rows);
    }
    Ok(out)
}
