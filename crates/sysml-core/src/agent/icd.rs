//! ICDs in the model: one package per ICD document, marked `@SysMLv1::icd { ... }`, that
//! holds one item def per register, message, call, stream or signal, with its fields as attributes and their layout as
//! `@SysMLv1::icdField { ... }` tags. This module reads those marks; `get_icd` (below) reads one ICD with its traces.

use crate::model::*;
use std::collections::BTreeMap;

/// The metadata usage of `el` whose def is named `mark` (`icd`, `icdRegister`, `icdField`, ...), if any.
pub fn mark_of(model: &Model, el: ElementId, mark: &str) -> Option<ElementId> {
    model.elements[el].children.iter().copied().find(|c| {
        let ce = &model.elements[*c];
        ce.kind == "metadata"
            && !ce.is_def
            && ce.refs_of(RefRole::Type).any(|r| match r.resolved() {
                Some(t) => model.elements[t].name.as_deref() == Some(mark),
                None => r.text.rsplit("::").next() == Some(mark),
            })
    })
}

/// The tags of a metadata usage, by name: values as written, a string's quotes dropped.
pub fn tags(model: &Model, meta: ElementId) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for c in &model.elements[meta].children {
        let ce = &model.elements[*c];
        if let (Some(n), Some(v)) = (ce.name.as_deref(), ce.value.as_deref()) {
            out.insert(n.to_string(), unquote(v));
        }
    }
    out
}

pub fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

/// The ICD package an element sits in (itself when it is one).
pub fn icd_package_of(model: &Model, el: ElementId) -> Option<ElementId> {
    let mut at = Some(el);
    let mut n = 0;
    while let Some(x) = at {
        if model.elements[x].kind == "package" && mark_of(model, x, "icd").is_some() {
            return Some(x);
        }
        at = model.elements[x].owner;
        n += 1;
        if n > 64 {
            break;
        }
    }
    None
}

/// Every ICD package of the model, in file order.
pub fn icd_packages(model: &Model) -> Vec<ElementId> {
    model.elements.iter().filter(|e| e.kind == "package" && mark_of(model, e.id, "icd").is_some()).map(|e| e.id).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// get_icd: one ICD read whole (what it defines, its channels, ports, links, the steps that use each element, the
// requirements that reach it), with five badges per element and report-only findings. Validate rule I8
// and every ICD reader use this one answer.

use super::allocation;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

const MAX_ELEMENTS: usize = 5000;

fn qn(m: &Model, id: ElementId) -> String {
    m.elements[id].qualified_name.clone()
}

fn ty(m: &Model, id: ElementId) -> Option<ElementId> {
    m.elements[id].first_resolved(RefRole::Type)
}

fn conj(m: &Model, id: ElementId) -> bool {
    m.elements[id].refs_of(RefRole::Type).any(|r| r.conjugated)
}

fn int_tag(t: &BTreeMap<String, String>, k: &str) -> Option<i64> {
    t.get(k).and_then(|v| v.trim().parse::<f64>().ok()).map(|f| f as i64)
}

/// The register blocks from the outermost down to `b`: each one's base address, how many times it repeats and the bytes
/// from one copy to the next (a block of lane registers that repeats for LANE0..3).
fn block_chain(m: &Model, b: ElementId) -> Vec<(i64, i64, i64)> {
    let mut out = Vec::new();
    let mut at = Some(b);
    while let Some(x) = at {
        let Some(mk) = mark_of(m, x, "icdBlock") else { break };
        let t = tags(m, mk);
        out.push((int_tag(&t, "baseAddress").unwrap_or(0), int_tag(&t, "count").unwrap_or(1).max(1), int_tag(&t, "stride").unwrap_or(0)));
        at = m.elements[x].owner;
    }
    out.reverse();
    out
}

/// The address offset of every copy of the block `b` (one entry when no block of the chain repeats), capped at 4096 copies.
fn block_offsets(m: &Model, b: Option<ElementId>) -> Vec<i64> {
    let mut offs = vec![0i64];
    if let Some(b) = b {
        for (base, count, stride) in block_chain(m, b) {
            let mut next = Vec::new();
            for o in &offs {
                for k in 0..count {
                    if next.len() >= 4096 {
                        break;
                    }
                    next.push(o + base + k * stride);
                }
            }
            offs = next;
        }
    }
    offs
}

fn real_tag(t: &BTreeMap<String, String>, k: &str) -> Option<f64> {
    t.get(k).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse::<f64>().ok())
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

const KIND_MARKS: [(&str, &str); 6] = [("icdRegister", "register"), ("icdMessage", "message"), ("icdCall", "call"), ("icdReturn", "return"), ("icdStream", "stream"), ("icdSignal", "signal")];
const WRITABLE: [&str; 4] = ["RW", "WO", "W1C", "W1S"];
const READABLE: [&str; 5] = ["RW", "RO", "W1C", "W1S", "RC"];
const ACCESS_WORDS: [&str; 6] = ["RW", "RO", "WO", "W1C", "W1S", "RC"];

struct Field {
    id: ElementId,
    tags: BTreeMap<String, String>,
    literals: Vec<(String, Option<i64>, Option<String>)>,
    ty: Option<ElementId>,
}

struct Elem {
    id: ElementId,
    kind: &'static str,
    tags: BTreeMap<String, String>,
    fields: Vec<Field>,
    block: Option<ElementId>,
    channels: Vec<String>,
}

struct Chan {
    name: String,
    pd: ElementId,
    /// toward the device (cmd) or from it
    to_device: bool,
    nested: bool,
}

struct Link {
    conn: ElementId,
    /// the end on the device side and the other
    ends: Vec<ElementId>,
    typed: Option<ElementId>,
    flows: Vec<(ElementId, Option<ElementId>, String)>,
}

struct Pin {
    id: ElementId,
    out: bool,
    step: ElementId,
    /// the part def that does the step
    doer: Option<ElementId>,
    op: Option<ElementId>,
    sets: Vec<(ElementId, String)>,
}

fn find_icd(m: &Model, which: &str) -> Option<ElementId> {
    let w = which.trim();
    icd_packages(m).into_iter().find(|p| {
        let e = &m.elements[*p];
        e.qualified_name == w || e.short_name.as_deref() == Some(w) || e.name.as_deref() == Some(w) || e.qualified_name.ends_with(&format!("::{w}"))
    })
}

fn elements_of(m: &Model, pkg: ElementId) -> Vec<Elem> {
    let mut out = Vec::new();
    let mut todo = vec![(pkg, None)];
    while let Some((p, block)) = todo.pop() {
        for c in m.elements[p].children.iter().copied() {
            let ce = &m.elements[c];
            if ce.kind == "package" && mark_of(m, c, "icdBlock").is_some() {
                todo.push((c, Some(c)));
                continue;
            }
            if !(ce.kind == "item" && ce.is_def) {
                continue;
            }
            let Some((mark, kind)) = KIND_MARKS.iter().find_map(|(mk, k)| mark_of(m, c, mk).map(|x| (x, *k))) else { continue };
            let mut fields = Vec::new();
            for f in diagram_members(m, c) {
                let fe = &m.elements[f];
                if fe.kind != "attribute" || fe.is_def {
                    continue;
                }
                let tags = mark_of(m, f, "icdField").map(|x| tags(m, x)).unwrap_or_default();
                let t = ty(m, f);
                let literals = t
                    .filter(|t| m.elements[*t].kind == "enum")
                    .map(|t| m.elements[t].children.iter().filter(|l| m.elements[**l].kind == "enum" && !m.elements[**l].is_def).map(|l| (m.elements[*l].display_name(), m.elements[*l].value.as_deref().and_then(|v| v.trim().parse::<i64>().ok()), m.elements[*l].doc.clone())).collect())
                    .unwrap_or_default();
                fields.push(Field { id: f, tags, literals, ty: t });
            }
            out.push(Elem { id: c, kind, tags: tags(m, mark), fields, block, channels: Vec::new() });
        }
    }
    out.sort_by_key(|e| m.elements[e.id].span.start);
    out
}

/// The members of a definition with those it inherits (a message header `:>`), own ones first.
fn diagram_members(m: &Model, def: ElementId) -> Vec<ElementId> {
    crate::agent::text::all_members(m, def)
}

fn port_defs_in(m: &Model, pkg: ElementId) -> Vec<ElementId> {
    let mut out = Vec::new();
    let mut todo = vec![pkg];
    while let Some(p) = todo.pop() {
        for c in m.elements[p].children.iter().copied() {
            let ce = &m.elements[c];
            if ce.kind == "package" {
                todo.push(c);
            } else if ce.kind == "port" && ce.is_def {
                out.push(c);
            }
        }
    }
    out
}

fn nested_ports(m: &Model, pd: ElementId) -> Vec<ElementId> {
    m.elements[pd].children.iter().copied().filter(|c| m.elements[*c].kind == "port" && !m.elements[*c].is_def).collect()
}

fn out_items(m: &Model, pd: ElementId) -> Vec<(ElementId, Option<ElementId>)> {
    m.elements[pd].children.iter().copied().filter(|c| m.elements[*c].direction.is_some() && !m.elements[*c].is_def).map(|c| (c, ty(m, c))).collect()
}

/// The channels a device port def nests (`cmd`, `rpt`, `irq`, `dma.cmd`), with their port defs and directions.
fn channels(m: &Model, device_pd: ElementId, prefix: &str, flip: bool, depth: usize, out: &mut Vec<Chan>) {
    for p in nested_ports(m, device_pd) {
        let Some(t) = ty(m, p) else { continue };
        let c = conj(m, p) != flip;
        let name = format!("{prefix}{}", m.elements[p].display_name());
        if !nested_ports(m, t).is_empty() && depth < 4 {
            channels(m, t, &format!("{name}."), c, depth + 1, out);
            continue;
        }
        out.push(Chan { name, pd: t, to_device: c, nested: true });
    }
}

/// The part def a step's pin belongs to: the owner of the operation a call calls, else the first part def up the tree.
fn doer_of(m: &Model, step: ElementId) -> (Option<ElementId>, Option<ElementId>) {
    if let Some(c) = allocation::call_of(m, step) {
        let op = c.op;
        let mut at = m.elements[op].owner;
        while let Some(x) = at {
            if m.elements[x].kind == "part" && m.elements[x].is_def {
                return (Some(x), Some(op));
            }
            if m.elements[x].kind == "part" && !m.elements[x].is_def {
                return (ty(m, x), Some(op));
            }
            at = m.elements[x].owner;
        }
        return (None, Some(op));
    }
    let mut at = Some(step);
    let mut op = None;
    while let Some(x) = at {
        let e = &m.elements[x];
        if e.kind == "part" && e.is_def {
            return (Some(x), op);
        }
        if e.kind == "action" && !e.is_def && op.is_none() {
            op = Some(x);
        }
        at = e.owner;
    }
    (None, op)
}

/// A flow of `item` carries the element `e`: it is `e`, or `e` is a kind of it (a group: a link that carries many
/// items carries their groups).
fn carries_in(m: &Model, item: ElementId, e: ElementId) -> bool {
    item == e || m.elements[e].refs_of(RefRole::Specializes).any(|r| r.resolved() == Some(item))
}

fn is_icd_def(m: &Model, id: ElementId) -> bool {
    let e = &m.elements[id];
    matches!(e.kind.as_str(), "port" | "item" | "interface" | "attribute" | "enum") && icd_package_of(m, id).is_some()
}

/// Everything a satisfier's behaviour holds, every level down: its members, its method's, and those of the operations
/// its calls call.
fn behaviour_closure(m: &Model, root: ElementId) -> HashSet<ElementId> {
    let mut seen = HashSet::new();
    let mut todo = vec![root];
    while let Some(x) = todo.pop() {
        if !seen.insert(x) || seen.len() > 20000 {
            continue;
        }
        let e = &m.elements[x];
        todo.extend(e.children.iter().copied());
        if e.kind.ends_with("action") || e.kind == "action" {
            if let Some(t) = ty(m, x).filter(|t| m.elements[*t].kind == "action") {
                todo.push(t);
            }
            if let Some(c) = allocation::call_of(m, x) {
                todo.push(c.op);
            }
        }
    }
    seen
}

pub fn get_icd(model: &Model, which: Option<&str>, level: &str) -> Result<Value, String> {
    let m = model;
    let pkgs = icd_packages(m);
    // the pins of the whole model, by the item that types them (read once)
    let mut pins_by_item: HashMap<ElementId, Vec<Pin>> = HashMap::new();
    let mut guard_reads: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for e in &m.elements {
        if !e.is_def && e.direction.is_some() && !matches!(e.kind.as_str(), "port") {
            if let (Some(t), Some(o)) = (ty(m, e.id), e.owner) {
                let oe = &m.elements[o];
                let in_action = oe.kind.ends_with("action") && !oe.is_def;
                if in_action && m.elements[t].kind == "item" && icd_package_of(m, t).is_some() {
                    let (doer, op) = doer_of(m, o);
                    let sets = e.children.iter().copied().filter_map(|c| {
                        let ce = &m.elements[c];
                        let f = ce.first_resolved(RefRole::Redefines)?;
                        ce.value.as_ref().map(|v| (f, v.trim().to_string()))
                    }).collect();
                    pins_by_item.entry(t).or_default().push(Pin { id: e.id, out: e.direction.as_deref() == Some("out"), step: o, doer, op, sets });
                }
            }
        }
        for r in e.refs.iter().filter(|r| r.role == RefRole::Expr) {
            for s in &r.segments {
                if let Some(x) = s.resolved {
                    if m.elements[x].kind == "attribute" && icd_package_of(m, x).is_some() {
                        guard_reads.entry(x).or_default().push(e.id);
                    }
                }
            }
        }
    }
    // satisfy: satisfier -> requirements
    let mut satisfiers: Vec<(ElementId, ElementId)> = Vec::new();
    for e in m.elements.iter().filter(|e| e.kind == "satisfy") {
        let s = e.refs_of(RefRole::Source).next().and_then(|r| r.resolved());
        let t = e.refs_of(RefRole::Target).next().and_then(|r| r.resolved());
        if let (Some(s), Some(t)) = (s, t) {
            satisfiers.push((s, t));
        }
    }
    let one = match which {
        Some(w) if !w.trim().is_empty() => Some(find_icd(m, w).ok_or_else(|| {
            let names: Vec<String> = pkgs.iter().map(|p| m.elements[*p].short_name.clone().unwrap_or_else(|| qn(m, *p))).collect();
            format!("no ICD {w} in the model (ICDs: {}). An ICD is a package marked @SysMLv1::icd ", if names.is_empty() { "none".to_string() } else { names.join(", ") })
        })?),
        _ => None,
    };
    let mut list = Vec::new();
    let mut full = Value::Null;
    for p in &pkgs {
        let r = read_icd(m, *p, &pins_by_item, &guard_reads, &satisfiers, level);
        list.push(r["summary"].clone());
        if one == Some(*p) {
            full = r;
        }
    }
    Ok(json!({ "icds": list, "icd": full }))
}

fn read_icd(m: &Model, pkg: ElementId, pins_by_item: &HashMap<ElementId, Vec<Pin>>, guard_reads: &HashMap<ElementId, Vec<ElementId>>, satisfiers: &[(ElementId, ElementId)], level: &str) -> Value {
    let pe = &m.elements[pkg];
    let carries = |item: ElementId, e: ElementId| carries_in(m, item, e);
    let facts = mark_of(m, pkg, "icd").map(|x| tags(m, x)).unwrap_or_default();
    let scope = facts.get("scope").cloned().unwrap_or_default();
    let mut elements = elements_of(m, pkg);
    elements.truncate(MAX_ELEMENTS);
    let mut findings: Vec<Value> = Vec::new();
    let mut find = |code: &str, sev: &str, el: ElementId, msg: String, fix: Value| {
        findings.push(json!({ "code": code, "severity": sev, "ref": qn(m, el), "name": m.elements[el].display_name(), "message": msg, "fix": fix }));
    };
    let pds = port_defs_in(m, pkg);
    // the interface def (this ICD's link), and for a p2p ICD over a device ICD the port def its ends are typed by
    let iface = pe.children.iter().copied().find(|c| m.elements[*c].kind == "interface" && m.elements[*c].is_def);
    let mut over: Option<ElementId> = None;
    // the device port def: the one that nests the channels
    let mut device_pd = pds.iter().copied().find(|p| !nested_ports(m, *p).is_empty() && m.elements[*p].owner == Some(pkg));
    if let Some(i) = iface {
        let end_pd = m.elements[i].children.iter().copied().filter(|c| m.elements[*c].kind == "port" && !m.elements[*c].is_def).find_map(|c| ty(m, c));
        if let Some(epd) = end_pd {
            if icd_package_of(m, epd) != Some(pkg) {
                over = icd_package_of(m, epd);
                device_pd = Some(epd);
            }
        }
    }
    let mut chans: Vec<Chan> = Vec::new();
    if let Some(d) = device_pd {
        channels(m, d, "", false, 0, &mut chans);
    }
    let in_chan: HashSet<ElementId> = chans.iter().map(|c| c.pd).collect();
    // a port def of the ICD with items of its own that the device port def does not nest: a separate port
    let mut sep_pds = Vec::new();
    for p in &pds {
        if Some(*p) == device_pd || in_chan.contains(p) || !nested_ports(m, *p).is_empty() || out_items(m, *p).is_empty() {
            continue;
        }
        let n = m.elements[*p].display_name();
        let name = format!("port:{}", lower_first(n.strip_suffix("Port").unwrap_or(&n)));
        sep_pds.push(*p);
        chans.push(Chan { name, pd: *p, to_device: false, nested: false });
    }
    // element -> channels
    let mut on: HashMap<ElementId, Vec<String>> = HashMap::new();
    for c in &chans {
        for (_f, t) in out_items(m, c.pd) {
            if let Some(t) = t {
                on.entry(t).or_default().push(c.name.clone());
            }
        }
    }
    for e in elements.iter_mut() {
        e.channels = on.get(&e.id).cloned().unwrap_or_default();
    }
    // devices and clients: part defs with a port typed by the device port def (or a separate one)
    let ends_pds: HashSet<ElementId> = device_pd.into_iter().chain(sep_pds.iter().copied()).collect();
    let mut devices: Vec<(ElementId, ElementId, ElementId)> = Vec::new();
    let mut clients: Vec<(ElementId, ElementId, ElementId)> = Vec::new();
    let mut icd_ports: HashSet<ElementId> = HashSet::new();
    for e in m.elements.iter().filter(|e| e.kind == "port" && !e.is_def) {
        let Some(t) = ty(m, e.id) else { continue };
        if !ends_pds.contains(&t) {
            continue;
        }
        let Some(o) = e.owner.filter(|o| m.elements[*o].kind == "part" && m.elements[*o].is_def) else { continue };
        icd_ports.insert(e.id);
        if conj(m, e.id) {
            clients.push((o, e.id, t));
        } else {
            devices.push((o, e.id, t));
        }
    }
    let device_defs: HashSet<ElementId> = devices.iter().map(|d| d.0).collect();
    // links: connections whose two end ports are ICD ports (or typed by the ICD's interface def)
    let mut links: Vec<Link> = Vec::new();
    for c in m.elements.iter().filter(|e| e.kind == "connection" && !e.is_def) {
        let src = c.refs_of(RefRole::Source).next();
        let tgt = c.refs_of(RefRole::Target).next();
        let (Some(src), Some(tgt)) = (src, tgt) else { continue };
        let (Some(a), Some(b)) = (src.resolved(), tgt.resolved()) else { continue };
        // a delegation (`connect txu to drv.txu`: the holder's own border port to a part inside) is no link
        if src.segments.len() < 2 || tgt.segments.len() < 2 {
            continue;
        }
        let typed = ty(m, c.id);
        let ours = (icd_ports.contains(&a) || icd_ports.contains(&b)) && (typed.is_none() || typed == iface || typed.and_then(|t| icd_package_of(m, t)).is_some());
        let ours = if over.is_some() { typed == iface && iface.is_some() } else { ours };
        if !ours {
            continue;
        }
        // its flows: flows in the same owner whose ends run through the two end ports
        let mut flows = Vec::new();
        for f in m.elements.iter().filter(|f| f.kind == "flow" && f.owner == c.owner) {
            let fs: Vec<ElementId> = f.refs_of(RefRole::Source).next().map(|r| r.segments.iter().filter_map(|s| s.resolved).collect()).unwrap_or_default();
            let ft: Vec<ElementId> = f.refs_of(RefRole::Target).next().map(|r| r.segments.iter().filter_map(|s| s.resolved).collect()).unwrap_or_default();
            let a_side = |v: &Vec<ElementId>| v.contains(&a);
            let b_side = |v: &Vec<ElementId>| v.contains(&b);
            if !((a_side(&fs) && b_side(&ft)) || (b_side(&fs) && a_side(&ft))) {
                continue;
            }
            // the channel: the segment after the end port (a nested port), or the port itself (a separate port)
            let seg = |v: &Vec<ElementId>, end: ElementId| -> String {
                let i = v.iter().position(|x| *x == end);
                let rest: Vec<String> = i.map(|i| v[i + 1..].iter().filter(|x| m.elements[**x].kind == "port").map(|x| m.elements[*x].display_name()).collect()).unwrap_or_default();
                if rest.is_empty() && device_pd.is_some() && ty(m, end) == device_pd {
                    // a flow from end port to end port: a group of items (groups), every channel its way
                    if conj(m, end) { "*to".to_string() } else { "*from".to_string() }
                } else if rest.is_empty() {
                    let pt = ty(m, end).map(|t| m.elements[t].display_name()).unwrap_or_default();
                    format!("port:{}", lower_first(pt.strip_suffix("Port").unwrap_or(&pt)))
                } else {
                    rest.join(".")
                }
            };
            let ch = if a_side(&fs) { seg(&fs, a) } else { seg(&fs, b) };
            let item = f.first_resolved(RefRole::Type);
            flows.push((f.id, item, ch));
        }
        links.push(Link { conn: c.id, ends: vec![a, b], typed, flows });
    }
    // per element: pins, sides, reads, requirements, badges
    let link_ids: HashSet<ElementId> = links.iter().map(|l| l.conn).collect();
    let mut reach: HashMap<ElementId, Vec<(ElementId, ElementId)>> = HashMap::new(); // element -> [(req, via)]
    let el_ids: HashSet<ElementId> = elements.iter().map(|e| e.id).collect();
    for (s, req) in satisfiers {
        let se = &m.elements[*s];
        if link_ids.contains(s) || links.iter().any(|l| l.conn == *s) {
            let l = links.iter().find(|l| l.conn == *s).unwrap();
            let carried: BTreeSet<ElementId> = l.flows.iter().filter_map(|f| f.1).collect();
            for e in &elements {
                if carried.is_empty() || carried.iter().any(|g| carries(*g, e.id)) {
                    reach.entry(e.id).or_default().push((*req, *s));
                }
            }
            continue;
        }
        // a connection usage reached through a part (head.txuLink): the satisfier resolves to the connection in its def
        if se.kind == "connection" {
            continue;
        }
        if se.kind.ends_with("action") || se.kind == "action" {
            let clo = behaviour_closure(m, *s);
            let mut hit = HashSet::new();
            for x in &clo {
                if let Some(t) = ty(m, *x) {
                    if el_ids.contains(&t) && m.elements[*x].direction.is_some() {
                        hit.insert(t);
                    }
                }
            }
            for t in hit {
                reach.entry(t).or_default().push((*req, *s));
            }
            continue;
        }
    }
    let mut out_elements = Vec::new();
    let mut cov = [0usize; 5];
    let mut u_partial = 0usize;
    let n_el = elements.len();
    // address clashes and message id clashes
    let block_base = |b: Option<ElementId>| -> i64 { block_offsets(m, b)[0] };
    let mut ranges: Vec<(i64, i64, ElementId)> = Vec::new();
    let mut ids_on: HashMap<(String, i64), Vec<ElementId>> = HashMap::new();
    for e in &elements {
        let at = |c: &str| -> bool { e.channels.iter().any(|x| x == c || x.ends_with(&format!(".{c}"))) };
        let name = m.elements[e.id].display_name();
        let fix_none = Value::Null;
        // D: the facts its kind needs
        let mut d_ok = true;
        let size = int_tag(&e.tags, "size");
        match e.kind {
            "register" => {
                if int_tag(&e.tags, "address").is_none() {
                    d_ok = false;
                    find("ICD-missing-fact", "warning", e.id, format!("register {name} has no address"), json!({"op": "update_element", "ref": qn(m, e.id), "doc": "<add address = N to its @SysMLv1::icdRegister: rebuild the ICD file with the address>"}));
                }
                for f in &e.fields {
                    if int_tag(&f.tags, "bit").is_none() || int_tag(&f.tags, "width").is_none() {
                        d_ok = false;
                        find("ICD-missing-fact", "warning", f.id, format!("field {}.{} has no bit or width", name, m.elements[f.id].display_name()), fix_none.clone());
                    }
                }
                let sz = size.unwrap_or(32);
                let mut used: Vec<(i64, i64, ElementId)> = Vec::new();
                for f in &e.fields {
                    if let (Some(b), Some(w)) = (int_tag(&f.tags, "bit"), int_tag(&f.tags, "width")) {
                        if b + w > sz {
                            find("ICD-field-outside", "warning", f.id, format!("{}.{} runs to bit {} of a {}-bit register", name, m.elements[f.id].display_name(), b + w - 1, sz), fix_none.clone());
                        }
                        for (ub, uw, uf) in &used {
                            if b < ub + uw && *ub < b + w {
                                find("ICD-field-overlap", "warning", f.id, format!("{}.{} and {} share bits {}..{}", name, m.elements[f.id].display_name(), m.elements[*uf].display_name(), b.max(*ub), (b + w).min(ub + uw) - 1), fix_none.clone());
                            }
                        }
                        used.push((b, w, f.id));
                        if let Some(lit_max) = f.literals.iter().filter_map(|l| l.1).max() {
                            if w < 63 && lit_max >= (1i64 << w) {
                                find("ICD-enum-too-wide", "warning", f.id, format!("{}.{} is {} bits; its value {} does not fit", name, m.elements[f.id].display_name(), w, lit_max), fix_none.clone());
                            }
                        }
                        if f.ty.map(|t| m.elements[t].name.as_deref() == Some("Boolean")).unwrap_or(false) && w != 1 {
                            find("ICD-enum-too-wide", "warning", f.id, format!("{}.{} is a Boolean of {} bits", name, m.elements[f.id].display_name(), w), fix_none.clone());
                        }
                    }
                    if let Some(a) = f.tags.get("access").filter(|a| !ACCESS_WORDS.contains(&a.as_str())) {
                        find("ICD-access-unknown", "info", f.id, format!("access {a} is none of RW, RO, WO, W1C, W1S, RC"), fix_none.clone());
                    }
                }
                if let Some(a) = int_tag(&e.tags, "address") {
                    let bytes = (sz + 7) / 8;
                    if bytes > 0 && a % bytes.min(8).max(1) != 0 {
                        find("ICD-misaligned", "warning", e.id, format!("register {name} at 0x{a:X} is not on a {bytes}-byte boundary"), fix_none.clone());
                    }
                    let count = int_tag(&e.tags, "count").unwrap_or(1).max(1);
                    let stride = int_tag(&e.tags, "stride").unwrap_or(bytes);
                    // every copy of the block it sits in; one finding per pair of elements
                    let mut clashed: BTreeSet<ElementId> = BTreeSet::new();
                    for off in block_offsets(m, e.block) {
                        let base = off + a;
                        let end = base + (count - 1) * stride + bytes;
                        for (s0, e0, other) in &ranges {
                            if base < *e0 && *s0 < end && clashed.insert(*other) {
                                find("ICD-address-clash", "warning", e.id, format!("{name} (0x{base:X}..0x{:X}) overlaps {} (0x{s0:X}..0x{:X})", end - 1, m.elements[*other].display_name(), e0 - 1), fix_none.clone());
                            }
                        }
                        ranges.push((base, end, e.id));
                    }
                }
                // channels against the access of the fields
                let acc: Vec<String> = if e.fields.iter().any(|f| f.tags.contains_key("access")) { e.fields.iter().filter_map(|f| f.tags.get("access").cloned()).collect() } else { e.tags.get("access").cloned().into_iter().collect() };
                let writable = acc.is_empty() || acc.iter().any(|a| WRITABLE.contains(&a.as_str()));
                let readable = acc.is_empty() || acc.iter().any(|a| READABLE.contains(&a.as_str()));
                if at("cmd") && !writable {
                    find("ICD-wrong-channel", "warning", e.id, format!("{name} rides cmd but no field of it is writable"), fix_none.clone());
                }
                if at("rpt") && !readable {
                    find("ICD-wrong-channel", "warning", e.id, format!("{name} rides rpt but no field of it is readable"), fix_none.clone());
                }
            }
            "message" => {
                for f in &e.fields {
                    if int_tag(&f.tags, "bit").is_none() || int_tag(&f.tags, "width").is_none() {
                        d_ok = false;
                        find("ICD-missing-fact", "warning", f.id, format!("field {}.{} has no bit or width", name, m.elements[f.id].display_name()), fix_none.clone());
                    }
                }
                let mut used: Vec<(i64, i64, ElementId)> = Vec::new();
                for f in &e.fields {
                    if let (Some(b), Some(w)) = (int_tag(&f.tags, "bit"), int_tag(&f.tags, "width")) {
                        if let Some(sz) = size {
                            if b + w > sz * 8 {
                                find("ICD-size-mismatch", "warning", f.id, format!("{}.{} runs past the {} bytes of {name}", name, m.elements[f.id].display_name(), sz), fix_none.clone());
                            }
                        }
                        for (ub, uw, uf) in &used {
                            if b < ub + uw && *ub < b + w {
                                find("ICD-size-mismatch", "warning", f.id, format!("{}.{} overlaps {}", name, m.elements[f.id].display_name(), m.elements[*uf].display_name()), fix_none.clone());
                            }
                        }
                        used.push((b, w, f.id));
                    }
                }
                if e.tags.get("timing").map(|t| t == "periodic").unwrap_or(false) && !e.tags.contains_key("rate") {
                    find("ICD-no-rate", "warning", e.id, format!("{name} is periodic with no rate"), fix_none.clone());
                }
                if let Some(id) = int_tag(&e.tags, "id") {
                    for c in &e.channels {
                        ids_on.entry((c.clone(), id)).or_default().push(e.id);
                    }
                }
            }
            "return" => {
                let base = name.strip_suffix("_return").unwrap_or(&name).to_string();
                if !elements.iter().any(|x| x.kind == "call" && m.elements[x.id].display_name() == base) {
                    find("ICD-return-orphan", "warning", e.id, format!("{name} returns from no call {base} of this ICD"), fix_none.clone());
                }
            }
            "stream" => {
                if int_tag(&e.tags, "width").is_none() {
                    d_ok = false;
                    find("ICD-missing-fact", "warning", e.id, format!("stream {name} has no width"), fix_none.clone());
                }
            }
            "signal" => {
                if !e.tags.contains_key("form") {
                    d_ok = false;
                    find("ICD-missing-fact", "warning", e.id, format!("signal {name} has no form (interrupt, discrete, clock, reset)"), fix_none.clone());
                }
            }
            _ => {}
        }
        if !m.elements[e.id].doc.as_deref().unwrap_or("").contains("SOURCE:") {
            find("ICD-no-source", "warning", e.id, format!("{name} cites no SOURCE: line"), fix_none.clone());
        }
        // P: on the channels it should ride
        let p_ok = !e.channels.is_empty();
        if !p_ok && over.is_none() {
            find("ICD-unplaced", "warning", e.id, format!("{name} rides no channel of the ICD (no port def of it has `out item {name}`)"), fix_none.clone());
        }
        // C: a flow of it on every link, for each channel it rides
        let mut c_ok = !links.is_empty() && p_ok;
        let mut carried_on = Vec::new();
        for l in &links {
            for ch in &e.channels {
                let star = if chans.iter().any(|c| &c.name == ch && c.to_device) { "*to" } else { "*from" };
                let here = l.flows.iter().any(|(_, it, fch)| it.is_some_and(|it| carries(it, e.id)) && (fch == ch || fch == star));
                let ends_ok = if ch.starts_with("port:") { l.ends.iter().any(|x| ty(m, *x).map(|t| sep_pds.contains(&t)).unwrap_or(false)) } else { l.ends.iter().any(|x| ty(m, *x) == device_pd) };
                if !ends_ok {
                    continue;
                }
                if here {
                    carried_on.push(json!({"connection": qn(m, l.conn), "channel": ch}));
                } else {
                    c_ok = false;
                    let (a, b) = (l.ends[0], l.ends[1]);
                    find("ICD-not-carried", "warning", e.id, format!("{name} on {ch}: {} carries no flow of it", m.elements[l.conn].display_name()), json!({"op": "create_relationship", "kind": "flow", "of": qn(m, e.id), "source": format!("{}::{}", qn(m, a), ch), "target": format!("{}::{}", qn(m, b), ch), "owner": m.elements[l.conn].owner.map(|o| qn(m, o))}));
                }
            }
        }
        if carried_on.is_empty() {
            c_ok = false;
        }
        // U: per channel, sent on the sending side and taken on the receiving side
        let pins = pins_by_item.get(&e.id).map(|v| v.as_slice()).unwrap_or(&[]);
        let side_dev = |p: &Pin| p.doer.map(|d| device_defs.contains(&d)).unwrap_or(false);
        let mut used_ch = 0usize;
        let mut ch_rows = Vec::new();
        for ch in &e.channels {
            let to_dev = chans.iter().find(|c| &c.name == ch).map(|c| c.to_device).unwrap_or(false);
            let sent = pins.iter().any(|p| p.out && (side_dev(p) != to_dev || device_defs.is_empty()));
            let taken = pins.iter().any(|p| !p.out && (side_dev(p) == to_dev || device_defs.is_empty()));
            if sent && taken {
                used_ch += 1;
            } else if sent || taken {
                let sev = if matches!(e.kind, "stream" | "signal") { "info" } else { "warning" };
                find("ICD-half-used", sev, e.id, format!("{name} on {ch}: {} but {}", if sent { "a step sends it" } else { "a step takes it" }, if sent { "no step takes it" } else { "no step sends it" }), fix_none.clone());
            } else {
                find("ICD-idle", "info", e.id, format!("{name} on {ch}: no step sends or takes it"), fix_none.clone());
            }
            ch_rows.push(json!({"channel": ch, "to_device": to_dev, "sent": sent, "taken": taken}));
        }
        let u: Value = if e.channels.is_empty() || used_ch == 0 { json!(false) } else if used_ch == e.channels.len() { json!(true) } else { json!("partial") };
        // field writes and reads
        let mut writes = Vec::new();
        let mut reads = Vec::new();
        let mut set_fields: HashSet<ElementId> = HashSet::new();
        for p in pins {
            let row = json!({"step": qn(m, p.step), "step_name": m.elements[p.step].display_name(), "operation": p.op.map(|o| qn(m, o)), "part_def": p.doer.map(|d| qn(m, d)), "pin": qn(m, p.id),
                "sets": p.sets.iter().map(|(f, v)| json!({"field": m.elements[*f].display_name(), "ref": qn(m, *f), "value": v})).collect::<Vec<_>>()});
            if p.out { writes.push(row) } else { reads.push(row) }
            for (f, v) in &p.sets {
                set_fields.insert(*f);
                if let Some(fd) = e.fields.iter().find(|x| x.id == *f) {
                    if fd.tags.get("access").map(|a| a == "RO" || a == "RC").unwrap_or(false) {
                        find("ICD-writes-readonly", "warning", p.step, format!("{} sets {}.{} = {v}, a read-only field", m.elements[p.step].display_name(), name, m.elements[*f].display_name()), fix_none.clone());
                    }
                    let num = v.trim().parse::<f64>().ok();
                    let lit_ok = fd.literals.is_empty() || fd.literals.iter().any(|l| v.ends_with(&l.0));
                    let out_range = num.map(|n| real_tag(&fd.tags, "min").map(|mn| n < mn).unwrap_or(false) || real_tag(&fd.tags, "max").map(|mx| n > mx).unwrap_or(false) || int_tag(&fd.tags, "width").map(|w| w < 63 && n >= (1i64 << w) as f64).unwrap_or(false)).unwrap_or(false);
                    if out_range || !lit_ok {
                        find("ICD-value-out-of-range", "warning", p.step, format!("{} sets {}.{} = {v}: outside what the field holds", m.elements[p.step].display_name(), name, m.elements[*f].display_name()), fix_none.clone());
                    }
                }
            }
        }
        let mut field_rows = Vec::new();
        for f in &e.fields {
            let gr = guard_reads.get(&f.id).map(|v| v.as_slice()).unwrap_or(&[]);
            let acc = f.tags.get("access").cloned().unwrap_or_default();
            if e.kind == "register" && !set_fields.is_empty() && !set_fields.contains(&f.id) && WRITABLE.contains(&acc.as_str()) && acc != "W1C" {
                find("ICD-field-never-set", "info", f.id, format!("no step sets {}; {name} is written field by field ({})", m.elements[f.id].display_name(),
                    set_fields.iter().map(|x| m.elements[*x].display_name()).collect::<Vec<_>>().join(", ")), fix_none.clone());
            }
            if e.kind == "register" && int_tag(&f.tags, "width") == Some(1) && (acc == "RO" || acc == "W1C") && gr.is_empty() {
                find("ICD-flag-never-read", "info", f.id, format!("no guard reads {}.{}", name, m.elements[f.id].display_name()), fix_none.clone());
            }
            let mut tagsj = serde_json::Map::new();
            for (k, v) in &f.tags {
                tagsj.insert(k.clone(), match v.parse::<i64>() { Ok(n) => json!(n), Err(_) => match v.parse::<f64>() { Ok(x) => json!(x), Err(_) => json!(v) } });
            }
            field_rows.push(json!({"ref": qn(m, f.id), "name": m.elements[f.id].display_name(), "type": f.ty.map(|t| m.elements[t].display_name()), "type_ref": f.ty.map(|t| qn(m, t)),
                "facts": tagsj, "doc": m.elements[f.id].doc, "literals": f.literals.iter().map(|(n, v, d)| json!({"name": n, "value": v, "doc": d})).collect::<Vec<_>>(),
                "read_by": gr.iter().map(|x| json!({"ref": qn(m, *x), "owner": m.elements[*x].owner.map(|o| qn(m, o))})).collect::<Vec<_>>(),
                "set_by": pins.iter().filter(|p| p.sets.iter().any(|(sf, _)| *sf == f.id)).map(|p| json!({"step": qn(m, p.step), "value": p.sets.iter().find(|(sf, _)| *sf == f.id).map(|x| x.1.clone())})).collect::<Vec<_>>()}));
        }
        let reqs = reach.get(&e.id).cloned().unwrap_or_default();
        let r_ok = !reqs.is_empty();
        let badges = json!({"D": d_ok, "P": p_ok, "C": c_ok, "U": u, "R": r_ok});
        cov[0] += d_ok as usize;
        cov[1] += p_ok as usize;
        cov[2] += c_ok as usize;
        cov[3] += (u == json!(true)) as usize;
        u_partial += (u == json!("partial")) as usize;
        cov[4] += r_ok as usize;
        let mut tagsj = serde_json::Map::new();
        for (k, v) in &e.tags {
            tagsj.insert(k.clone(), match v.parse::<i64>() { Ok(n) => json!(n), Err(_) => match v.parse::<f64>() { Ok(x) => json!(x), Err(_) => json!(v) } });
        }
        let doc = m.elements[e.id].doc.clone().unwrap_or_default();
        let source = doc.split("SOURCE:").nth(1).map(|s| s.trim().trim_end_matches('.').to_string());
        out_elements.push(json!({"ref": qn(m, e.id), "name": name, "kind": e.kind, "short_name": m.elements[e.id].short_name.clone().filter(|s| !s.starts_with('e') || s.len() != 7),
            "facts": tagsj, "doc": doc.split("SOURCE:").next().map(|s| s.trim()).unwrap_or(""), "source": source,
            "block": e.block.map(|b| m.elements[b].display_name()), "block_base": e.block.map(|b| block_base(Some(b))),
            "block_ref": e.block.map(|b| qn(m, b)), "copies": block_offsets(m, e.block).len() as i64 * int_tag(&e.tags, "count").unwrap_or(1).max(1),
            "specializes": m.elements[e.id].first_resolved(RefRole::Specializes).map(|s| qn(m, s)),
            "fields": field_rows, "channels": e.channels, "channel_use": ch_rows, "carried_on": carried_on,
            "writes": writes, "reads": reads,
            "requirements": reqs.iter().map(|(r, via)| json!({"ref": qn(m, *r), "id": m.elements[*r].short_name, "name": m.elements[*r].display_name(), "via": qn(m, *via)})).collect::<Vec<_>>(),
            "badges": badges, "line": m.line_col(m.elements[e.id].file, m.elements[e.id].span.start).0, "file": m.files[m.elements[e.id].file].path}));
    }
    for ((ch, id), els) in ids_on.into_iter().filter(|(_, v)| v.len() > 1) {
        for e in &els {
            find("ICD-id-clash", "warning", *e, format!("{} messages on {ch} share id {id}", els.len()), Value::Null);
        }
    }
    // document level
    if facts.get("revision").map(|s| s.is_empty()).unwrap_or(true) || scope.is_empty() {
        find("ICD-missing-fact", "warning", pkg, "the ICD has no revision or scope in its @SysMLv1::icd".to_string(), Value::Null);
    }
    if !pe.doc.as_deref().unwrap_or("").contains("SOURCE:") {
        find("ICD-no-source", "warning", pkg, "the ICD package cites no SOURCE: line".to_string(), Value::Null);
    }
    if scope == "device" && devices.is_empty() && device_pd.is_some() {
        find("ICD-no-device", "warning", pkg, format!("no part def types a port by {} (the device's icd port)", device_pd.map(|d| m.elements[d].display_name()).unwrap_or_default()),
            json!({"op": "create_element", "owner": "<the device's part def>", "kind": "port", "name": "icd", "typed_by": device_pd.map(|d| qn(m, d))}));
    }
    for (o, p, _t) in devices.iter().chain(clients.iter()) {
        // a port of a part def that is used in some context and on no link
        let on_link = links.iter().any(|l| l.ends.contains(p));
        let used = m.elements.iter().any(|u| u.kind == "part" && !u.is_def && ty(m, u.id) == Some(*o));
        let delegated = m.elements.iter().any(|c| matches!(c.kind.as_str(), "connect" | "connection" | "binding" | "connector") && c.refs.iter().any(|r| r.segments.iter().any(|s| s.resolved == Some(*p))));
        if used && !on_link && !delegated {
            find("ICD-unconnected", "warning", *p, format!("{}.{} speaks the ICD and no connection reaches it", m.elements[*o].display_name(), m.elements[*p].display_name()), Value::Null);
        }
    }
    if let Some(i) = iface {
        if !links.iter().any(|l| l.typed == Some(i)) {
            find("ICD-iface-unused", "warning", i, format!("{} types no connection", m.elements[i].display_name()), Value::Null);
        }
        for l in links.iter().filter(|l| l.ends.iter().any(|x| ty(m, *x) == device_pd)) {
            let other_icd_def = l.typed.map(|t| icd_package_of(m, t).is_some() && Some(t) != iface).unwrap_or(false);
            if l.typed != Some(i) && !other_icd_def {
                find("ICD-link-untyped", "warning", l.conn, format!("{} links the ICD's ports and is not typed by {}", m.elements[l.conn].display_name(), m.elements[i].display_name()),
                    json!({"op": "update_element", "ref": qn(m, l.conn), "typed_by": qn(m, i)}));
            }
        }
    }
    if !out_elements.is_empty() && cov[4] == 0 {
        find("ICD-no-requirement", "info", pkg, "no requirement reaches any element of the ICD".to_string(), Value::Null);
    }
    let warn = findings.iter().filter(|f| f["severity"] == "warning").count();
    let info = findings.len() - warn;
    if level == "warning" {
        findings.retain(|f| f["severity"] == "warning");
    }
    let mut counts = serde_json::Map::new();
    for (_, k) in KIND_MARKS {
        let n = elements.iter().filter(|e| e.kind == k).count();
        if n > 0 {
            counts.insert(k.to_string(), json!(n));
        }
    }
    let cov_j = json!({"D": [cov[0], n_el], "P": [cov[1], n_el], "C": [cov[2], n_el], "U": [cov[3], n_el], "U_partial": u_partial, "R": [cov[4], n_el]});
    let mut factsj = serde_json::Map::new();
    for (k, v) in &facts {
        factsj.insert(k.clone(), match v.parse::<i64>() { Ok(n) => json!(n), Err(_) => json!(v) });
    }
    let link_rows: Vec<Value> = links.iter().map(|l| {
        let ce = &m.elements[l.conn];
        let lf: serde_json::Map<String, Value> = l.typed.map(|t| m.elements[t].children.iter().filter(|c| m.elements[**c].kind == "attribute" && !m.elements[**c].is_def).map(|c| (m.elements[*c].display_name(), json!(m.elements[*c].value.clone().map(|v| unquote(&v))))).collect()).unwrap_or_default();
        json!({"connection": qn(m, l.conn), "name": ce.display_name(), "short_name": ce.short_name, "interface_def": l.typed.map(|t| qn(m, t)), "owner": ce.owner.map(|o| qn(m, o)),
            "ends": l.ends.iter().map(|p| json!({"port": qn(m, *p), "port_name": m.elements[*p].display_name(), "part_def": m.elements[*p].owner.map(|o| qn(m, o)), "part_def_name": m.elements[*p].owner.map(|o| m.elements[o].display_name()), "conjugated": conj(m, *p)})).collect::<Vec<_>>(),
            "facts": lf,
            "flows": l.flows.iter().map(|(f, it, ch)| json!({"ref": qn(m, *f), "item": it.map(|i| qn(m, i)), "item_name": it.map(|i| m.elements[i].display_name()), "channel": ch})).collect::<Vec<_>>()})
    }).collect();
    // the register blocks, outermost first: base address, copies, stride, the bytes their registers span
    let mut seen_b: Vec<ElementId> = Vec::new();
    for e in &elements {
        let mut at = e.block;
        let mut chain = Vec::new();
        while let Some(x) = at {
            if mark_of(m, x, "icdBlock").is_none() {
                break;
            }
            chain.push(x);
            at = m.elements[x].owner;
        }
        for x in chain.into_iter().rev() {
            if !seen_b.contains(&x) {
                seen_b.push(x);
            }
        }
    }
    let blocks_j: Vec<Value> = seen_b.iter().map(|b| {
        let t = mark_of(m, *b, "icdBlock").map(|x| tags(m, x)).unwrap_or_default();
        let parent = m.elements[*b].owner.filter(|o| mark_of(m, *o, "icdBlock").is_some());
        let span = elements.iter().filter(|e| e.block == Some(*b)).filter_map(|e| {
            let a = int_tag(&e.tags, "address")?;
            let bytes = (int_tag(&e.tags, "size").unwrap_or(32) + 7) / 8;
            Some(a + (int_tag(&e.tags, "count").unwrap_or(1).max(1) - 1) * int_tag(&e.tags, "stride").unwrap_or(bytes) + bytes)
        }).max().unwrap_or(0);
        let doc = m.elements[*b].doc.clone().unwrap_or_default();
        json!({"ref": qn(m, *b), "name": m.elements[*b].display_name(), "short_name": m.elements[*b].short_name, "parent": parent.map(|p| qn(m, p)),
            "base": block_offsets(m, Some(*b))[0], "count": int_tag(&t, "count").unwrap_or(1).max(1), "stride": int_tag(&t, "stride").unwrap_or(0), "span": span,
            "doc": doc.split("SOURCE:").next().map(|s| s.trim()).unwrap_or("")})
    }).collect();
    let summary = json!({"ref": qn(m, pkg), "id": pe.short_name, "name": pe.display_name(), "title": facts.get("title"), "revision": facts.get("revision"), "scope": scope,
        "over": over.map(|o| m.elements[o].short_name.clone().unwrap_or_else(|| qn(m, o))),
        "devices": devices.iter().map(|d| qn(m, d.0)).collect::<BTreeSet<_>>(), "clients": clients.iter().map(|d| qn(m, d.0)).collect::<BTreeSet<_>>(),
        "counts": counts, "coverage": cov_j, "findings": {"warning": warn, "info": info}, "links": links.len(),
        "addresses": out_elements.iter().filter(|e| e["kind"] == "register").map(|e| e["copies"].as_i64().unwrap_or(1)).sum::<i64>()});
    json!({
        "summary": summary.clone(),
        "ref": qn(m, pkg), "id": pe.short_name, "name": pe.display_name(), "doc": pe.doc.as_deref().map(|d| d.split("SOURCE:").next().unwrap_or("").trim()), "source": pe.doc.as_deref().and_then(|d| d.split("SOURCE:").nth(1)).map(|s| s.trim().trim_end_matches('.')),
        "facts": factsj, "scope": scope, "over": summary["over"],
        "interface_def": iface.map(|i| qn(m, i)), "device_port_def": device_pd.map(|d| qn(m, d)),
        "channels": chans.iter().map(|c| json!({"name": c.name, "port_def": qn(m, c.pd), "port_def_name": m.elements[c.pd].display_name(), "to_device": c.to_device, "nested": c.nested})).collect::<Vec<_>>(),
        "ports": {"device": devices.iter().map(|(o, p, t)| json!({"part_def": qn(m, *o), "part_def_name": m.elements[*o].display_name(), "port": qn(m, *p), "port_name": m.elements[*p].display_name(), "port_def": qn(m, *t)})).collect::<Vec<_>>(),
                  "client": clients.iter().map(|(o, p, t)| json!({"part_def": qn(m, *o), "part_def_name": m.elements[*o].display_name(), "port": qn(m, *p), "port_name": m.elements[*p].display_name(), "port_def": qn(m, *t), "conjugated": true})).collect::<Vec<_>>()},
        "blocks": blocks_j, "addresses": summary["addresses"],
        "links": link_rows, "elements": out_elements, "findings": findings, "coverage": summary["coverage"], "counts": summary["counts"],
        "line": m.line_col(pe.file, pe.span.start).0, "file": m.files[pe.file].path,
    })
}

#[allow(dead_code)]
fn unused(m: &Model, id: ElementId) -> bool {
    is_icd_def(m, id)
}

pub fn tool_get_icd(ctx: &dyn super::tools::Ctx, a: &Value) -> super::tools::ToolResult {
    let level = super::tools::opt(a, "level").unwrap_or("info");
    let v = get_icd(&ctx.model(), super::tools::opt(a, "icd"), level).map_err(super::tools::err)?;
    let mut lines: Vec<String> = Vec::new();
    for s in v["icds"].as_array().into_iter().flatten() {
        let c = &s["coverage"];
        let pair = |k: &str| format!("{}/{}", c[k][0], c[k][1]);
        lines.push(format!("{} {} rev {} [{}]: {} | D {} P {} C {} U {} R {} | {} warnings, {} info",
            s["id"].as_str().unwrap_or("?"), s["title"].as_str().unwrap_or(""), s["revision"].as_str().unwrap_or("?"), s["scope"].as_str().unwrap_or("?"),
            s["counts"].as_object().map(|o| o.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            pair("D"), pair("P"), pair("C"), pair("U"), pair("R"), s["findings"]["warning"], s["findings"]["info"]));
    }
    if lines.is_empty() {
        lines.push("No ICD in this model (an ICD is a package marked @SysMLv1::icd).".into());
    }
    for f in v["icd"]["findings"].as_array().into_iter().flatten() {
        lines.push(format!("{} {} {} :: {}", f["severity"].as_str().unwrap_or(""), f["code"].as_str().unwrap_or(""), f["ref"].as_str().unwrap_or(""), f["message"].as_str().unwrap_or("")));
    }
    super::tools::out(lines.join("\n"), v)
}
