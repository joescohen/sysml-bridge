//! A budget roll-up over the part tree: each part's own amount, the sum of what it holds, and the limit a requirement
//! states for the whole.
//!
//! One quantity is built: mass. A quantity is a row of `QUANTITIES` (the attribute name, the unit the sums are given in,
//! and the units that are converted), so a second one is one more row; the walk, the limit, the reply and the guide gap
//! read the row and name no quantity.
//!
//! The rule of the roll-up: a part's total is its own amount plus the totals of the parts it holds, each times the
//! multiplicity of the usage when that is a number. A part that holds no parts and states no amount is "not stated"; it
//! is never counted as zero in silence, so the reply lists it and says that the total is a lower bound.
//!
//! Only composite part usages down the composition tree from one root are mass. Reference usages (`ref part`), the ends
//! of connections, interfaces and associations, the features of allocations and ports are not. With no root given, one
//! root is chosen by `choose` (the part that holds a mass limit, else the composition root that holds the most
//! mass-stating parts); the candidates are never added. A definition used under two composite owners is counted once for
//! each usage, and the reply lists it in `counted_more_than_once`.

use super::text::{csv, Names};
use super::{connect, line_of, locate, units, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

/// A quantity that is rolled up.
pub struct Quantity {
    pub id: &'static str,
    /// The name of the attribute a part states its own amount with.
    pub attribute: &'static str,
    /// The unit of every sum in the reply.
    pub unit: &'static str,
    /// The units read, each with the factor to `unit`.
    pub units: &'static [(&'static str, f64)],
}

pub const QUANTITIES: [Quantity; 1] = [Quantity { id: "mass", attribute: "mass", unit: "kg", units: &[("kg", 1.0), ("g", 0.001), ("mg", 0.000_001), ("t", 1000.0)] }];

/// Most nodes one roll-up walks, and the deepest nesting.
pub const MAX_NODES: usize = 20_000;
const MAX_DEPTH: usize = 32;

pub fn quantity(name: Option<&str>) -> Res<&'static Quantity> {
    let want = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(QUANTITIES[0].id);
    QUANTITIES.iter().find(|q| q.id.eq_ignore_ascii_case(want)).ok_or_else(|| format!("quantity {} is not built. Known: {}.", crate::agent::text::quote_clip(want), QUANTITIES.iter().map(|q| q.id).collect::<Vec<_>>().join(", ")).into())
}

/// A number as a reader writes it: at most six decimals, no trailing zeros.
pub fn number(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

fn amount_text(q: &Quantity, x: f64) -> String {
    format!("{} {}", number(x), q.unit)
}

/// The amount a value states, in the unit of the quantity: `12 [kg]`, `500 [g]`, `1.5 [SI::kg]`. `unit_of_type` is the
/// unit the type of the attribute gives when the value has none (`ISQ::MassValue` is in kg).
fn amount(q: &Quantity, value: &str, unit_of_type: Option<&str>) -> Result<f64, String> {
    let head = value.split('[').next().unwrap_or("").trim();
    let n: f64 = head.parse().map_err(|_| format!("the value {} is not one number with a unit", crate::agent::text::quote_clip(value.trim())))?;
    if !n.is_finite() || n < 0.0 {
        return Err(format!("the value {} is not a number of zero or more", crate::agent::text::quote_clip(value.trim())));
    }
    let named = units::unit_names(value);
    let unit = match named.first() {
        Some(u) => u.rsplit("::").next().unwrap_or(u).trim_matches('\'').to_string(),
        None => unit_of_type.map(str::to_string).ok_or_else(|| format!("the value {} has no unit", crate::agent::text::quote_clip(value.trim())))?,
    };
    match q.units.iter().find(|(u, _)| *u == unit) {
        Some((_, f)) => Ok(n * f),
        None => Err(format!("the unit {} is not a unit of {} that is converted ({})", crate::agent::text::quote_clip(&unit), q.id, q.units.iter().map(|(u, _)| *u).collect::<Vec<_>>().join(", "))),
    }
}

pub(super) fn amount_of(m: &Model, q: &Quantity, attr: &Element) -> Result<f64, String> {
    let value = attr.value.as_deref().ok_or_else(|| "it has no value".to_string())?;
    let by_type = super::text::unit_of(m, &Element { value: None, ..attr.clone() }).map(|u| u.0);
    amount(q, value, by_type.as_deref())
}

/// The attribute a part states its own amount with: its own member first, then a member of its definition.
fn own_attribute<'a>(m: &'a Model, q: &Quantity, part: &'a Element) -> Option<&'a Element> {
    let named = |e: &&Element| e.kind == "attribute" && !e.is_def && e.name.as_deref() == Some(q.attribute);
    m.children(part.id).find(named).or_else(|| {
        let def = if part.is_def { Some(part.id) } else { connect::typed_def(m, part) };
        def.and_then(|d| crate::agent::text::all_members(m, d).into_iter().map(|x| &m.elements[x]).find(named))
    })
}

/// The part usages a part holds: its own, then those of its definition (a name the usage states itself is not repeated).
pub(super) fn held(m: &Model, part: &Element) -> Vec<ElementId> {
    let is_usage = |e: &Element| e.kind == "part" && !e.is_def && !e.has_prefix("ref") && !e.has_prefix("end");
    let mut out: Vec<ElementId> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    let mut take = |ids: Vec<ElementId>| {
        for i in ids {
            let e = &m.elements[i];
            if !is_usage(e) || out.contains(&i) {
                continue;
            }
            if let Some(n) = &e.name {
                if !names.insert(n.clone()) {
                    continue;
                }
            }
            out.push(i);
        }
    };
    if part.is_def {
        take(crate::agent::text::all_members(m, part.id));
    } else {
        take(part.children.clone());
        if let Some(d) = connect::typed_def(m, part) {
            take(crate::agent::text::all_members(m, d));
        }
    }
    out
}

/// The multiplicity of a usage as a factor: `[2]` and `2` give 2. A range or `*` is not a number: the factor is 1 and
/// the note says so.
pub(super) fn factor(e: &Element) -> (f64, Option<String>) {
    match e.multiplicity.as_deref().map(|s| s.trim().trim_start_matches('[').trim_end_matches(']').trim()) {
        None | Some("") => (1.0, None),
        Some(s) => match s.parse::<u32>() {
            Ok(n) => (f64::from(n), None),
            Err(_) => (1.0, Some(format!("multiplicity {s} is not one number: counted once"))),
        },
    }
}

struct Walk<'a> {
    m: &'a Model,
    q: &'a Quantity,
    nm: Names<'a>,
    rows: Vec<Value>,
    cut: bool,
}

impl<'a> Walk<'a> {
    /// Adds the row of `part` and the rows of what it holds; returns the total of one instance of it.
    fn node(&mut self, part: ElementId, depth: usize, parent: Option<usize>, path: &mut Vec<ElementId>) -> f64 {
        let (m, q) = (self.m, self.q);
        let e = &m.elements[part];
        let def = if e.is_def { None } else { connect::typed_def(m, e) };
        let index = self.rows.len();
        self.rows.push(Value::Null);
        let attr = own_attribute(m, q, e);
        let (mut own, mut note) = (None, None);
        if let Some(a) = attr {
            match amount_of(m, q, a) {
                Ok(x) => own = Some(x),
                Err(why) => note = Some(format!("{} of {} is not counted: {why}", q.attribute, self.nm.show(e))),
            }
        }
        let mut sum = own.unwrap_or(0.0);
        let mut children = 0usize;
        // A definition that holds itself would never end: the second visit is not walked.
        let key = def.unwrap_or(part);
        if depth < MAX_DEPTH && !path.contains(&key) {
            path.push(key);
            for c in held(m, e) {
                if self.rows.len() >= MAX_NODES {
                    self.cut = true;
                    break;
                }
                children += 1;
                let one = self.node(c, depth + 1, Some(index), path);
                let (f, _) = factor(&m.elements[c]);
                sum += one * f;
            }
            path.pop();
        }
        let (f, f_note) = if depth == 0 { (1.0, None) } else { factor(e) };
        let state = if own.is_some() {
            "stated"
        } else if note.is_some() {
            "not readable"
        } else if children > 0 {
            "sum of its parts"
        } else {
            "not stated"
        };
        let notes: Vec<String> = [note, f_note].into_iter().flatten().collect();
        self.rows[index] = json!({
            "index": index,
            "parent": parent,
            "depth": depth,
            "ref": e.qualified_name,
            "name": e.name.clone().unwrap_or_else(|| self.nm.show(e)),
            "label": self.nm.show(e),
            "kind": e.keyword(),
            "definition": def.map(|d| self.nm.show(&m.elements[d])),
            "file": m.files[e.file].path,
            "line": line_of(m, e),
            "multiplicity": e.multiplicity,
            "count": f,
            "state": state,
            "own": own,
            "own_text": own.map(|x| amount_text(q, x)),
            "stated_as": attr.and_then(|a| a.value.clone()).map(|v| v.trim().to_string()),
            "attribute": attr.map(|a| a.qualified_name.clone()),
            "parts": children,
            "rolled_up": sum,
            "rolled_up_text": amount_text(q, sum),
            "total": sum * f,
            "total_text": amount_text(q, sum * f),
            "notes": notes,
        });
        sum
    }
}

/// A limit a requirement states: `attribute maxMass = 40 [kg];` on a requirement usage.
pub struct Limit {
    pub requirement: ElementId,
    pub attribute: ElementId,
    pub amount: f64,
}

/// Every requirement usage that states a limit of the quantity: an attribute whose name holds the name of the quantity
/// (`maxMass`, `massLimit`, `mass`) and whose value is an amount of it.
pub fn limits(m: &Model, q: &Quantity) -> Vec<Limit> {
    let mut out = Vec::new();
    for r in m.elements.iter().filter(|e| e.kind == "requirement" && !e.is_def) {
        for a in m.children(r.id).filter(|c| c.kind == "attribute" && !c.is_def) {
            if a.name.as_deref().map(|n| n.to_lowercase().contains(q.attribute)).unwrap_or(false) {
                if let Ok(x) = amount_of(m, q, a) {
                    out.push(Limit { requirement: r.id, attribute: a.id, amount: x });
                    break;
                }
            }
        }
    }
    out
}

/// A part usage that is a member of the composition tree: not a reference, not an end, and owned by a package or a part.
/// A part usage owned by an association, a connection, an interface, an action or a state is a feature of that element,
/// not a composed part.
fn composite(m: &Model, e: &Element) -> bool {
    e.kind == "part" && !e.is_def && !e.has_prefix("ref") && !e.has_prefix("end") && e.owner.map(|o| matches!(m.elements[o].kind.as_str(), "package" | "part")).unwrap_or(true)
}

/// The composition roots: each part usage that no part owns and each part definition that no composite part usage is
/// typed by. A part of an association, a connection or an interface is no root.
fn composition_roots(m: &Model) -> Vec<ElementId> {
    let used: BTreeSet<ElementId> = m.elements.iter().filter(|e| composite(m, e)).filter_map(|e| connect::typed_def(m, e)).collect();
    let mut out: Vec<ElementId> = m
        .elements
        .iter()
        .filter(|e| e.kind == "part")
        .filter(|e| if e.is_def { !used.contains(&e.id) } else { composite(m, e) && !e.owner.map(|o| m.elements[o].kind == "part").unwrap_or(false) })
        .map(|e| e.id)
        .collect();
    out.sort_by(|a, b| m.elements[*a].qualified_name.cmp(&m.elements[*b].qualified_name));
    out
}

/// The nearest part (definition or composite usage) that is `id` or owns it.
fn part_at(m: &Model, id: ElementId) -> Option<ElementId> {
    let mut at = Some(id);
    while let Some(i) = at {
        let e = &m.elements[i];
        if e.kind == "part" && !e.has_prefix("ref") && !e.has_prefix("end") {
            return Some(i);
        }
        at = e.owner;
    }
    None
}

/// The parts that hold a limit: the subject of the requirement, what satisfies it or is allocated it, the part that owns
/// the requirement, and a part that holds an attribute of the same name as the limit (`maxMass`): that attribute ties the
/// roll-up of the part to the requirement. The name of the quantity itself (`mass`) is a part's own amount, not a tie.
fn limit_holders(m: &Model, q: &Quantity, all: &[Limit]) -> Vec<ElementId> {
    let mut out: BTreeSet<ElementId> = BTreeSet::new();
    let (stmts, _) = super::graph::statements_cut(m);
    for l in all {
        let r = &m.elements[l.requirement];
        for sub in m.children(r.id).filter(|c| c.kind == "subject") {
            if let Some(t) = sub.refs_of(RefRole::Type).find_map(|x| x.resolved()).and_then(|t| part_at(m, t)) {
                out.insert(t);
            }
        }
        let mut mine = vec![r.id];
        if let Some(d) = connect::typed_def(m, r) {
            mine.push(d);
        }
        for s in stmts.iter().filter(|s| s.kind == "satisfy" || s.kind == "allocate") {
            let other = match (s.target.id, s.source.id) {
                (Some(t), Some(o)) if mine.contains(&t) => Some(o),
                (Some(o), Some(t)) if s.kind == "allocate" && mine.contains(&t) => Some(o),
                _ => None,
            };
            if let Some(p) = other.and_then(|o| part_at(m, o)) {
                out.insert(p);
            }
        }
        if let Some(p) = r.owner.and_then(|o| part_at(m, o)) {
            out.insert(p);
        }
        if let Some(name) = m.elements[l.attribute].name.as_deref().filter(|n| *n != q.attribute) {
            for p in m.elements.iter().filter(|e| e.kind == "part" && !e.has_prefix("ref") && !e.has_prefix("end")) {
                if m.children(p.id).any(|c| c.kind == "attribute" && !c.is_def && c.name.as_deref() == Some(name) && amount_of(m, q, c).is_ok()) {
                    out.insert(p.id);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// The total and the number of stated amounts of one tree, without rows. `left` bounds the nodes of all scans.
fn scan(m: &Model, q: &Quantity, part: ElementId, depth: usize, path: &mut Vec<ElementId>, left: &mut usize) -> (f64, usize) {
    if *left == 0 {
        return (0.0, 0);
    }
    *left -= 1;
    let e = &m.elements[part];
    let def = if e.is_def { None } else { connect::typed_def(m, e) };
    let own = own_attribute(m, q, e).and_then(|a| amount_of(m, q, a).ok());
    let (mut sum, mut stated) = (own.unwrap_or(0.0), usize::from(own.is_some()));
    let key = def.unwrap_or(part);
    if depth < MAX_DEPTH && !path.contains(&key) {
        path.push(key);
        for c in held(m, e) {
            let (one, s) = scan(m, q, c, depth + 1, path, left);
            sum += one * factor(&m.elements[c]).0;
            stated += s;
        }
        path.pop();
    }
    (sum, stated)
}

/// A root that was weighed when none was given.
pub struct Candidate {
    pub id: ElementId,
    /// Amounts the tree states (a part counted under two usages counts twice).
    pub stated: usize,
    pub total: Option<f64>,
}

/// How the root of an unrooted budget was chosen.
pub struct Choice {
    pub root: Option<ElementId>,
    /// `pinned` (the workspace setting budget_root), `holds the limit`, `only root that states` or `most mass-stating parts`.
    pub how: &'static str,
    pub why: String,
    pub candidates: Vec<Candidate>,
    pub cut: bool,
}

/// Most nodes all candidate scans of one choice walk.
const SCAN_NODES: usize = 400_000;

/// The root of a budget when none is given. Deterministic: the pinned root (`Model::budget_root`); else the part that
/// holds the mass limit; else the one composition root that holds every part that states an amount. When several
/// candidates remain they are not added: the one that holds the most amount-stating parts is used (ties by name).
pub fn choose(m: &Model, q: &Quantity, all: &[Limit]) -> Choice {
    let mut note = String::new();
    if let Some(p) = m.budget_root.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        match locate(m, p) {
            Ok(e) if e.kind == "part" => {
                return Choice { root: Some(e.id), how: "pinned", why: format!("the workspace setting budget_root names {}", e.qualified_name), candidates: Vec::new(), cut: false };
            }
            _ => note = format!("The workspace setting budget_root ({}) names no part, so it is not used. ", crate::agent::text::quote_clip(p)),
        }
    }
    let mut left = SCAN_NODES;
    let weigh = |ids: Vec<ElementId>, left: &mut usize| -> Vec<Candidate> {
        ids.into_iter()
            .map(|id| {
                let (total, stated) = scan(m, q, id, 0, &mut Vec::new(), left);
                Candidate { id, stated, total: if stated > 0 { Some(total) } else { None } }
            })
            .collect()
    };
    let pick = |mut c: Vec<Candidate>| -> Vec<Candidate> {
        c.sort_by(|a, b| b.stated.cmp(&a.stated).then_with(|| m.elements[a.id].qualified_name.cmp(&m.elements[b.id].qualified_name)));
        c
    };
    let mut holders = limit_holders(m, q, all);
    holders.truncate(200);
    let held_c: Vec<Candidate> = weigh(holders, &mut left).into_iter().filter(|c| c.stated > 0).collect();
    let (how, cands): (&'static str, Vec<Candidate>) = if !held_c.is_empty() {
        ("holds the limit", pick(held_c))
    } else {
        let mut roots = composition_roots(m);
        let many = roots.len() > 400;
        roots.truncate(400);
        let all_c = weigh(roots, &mut left);
        let with: Vec<Candidate> = all_c.iter().filter(|c| c.stated > 0).map(|c| Candidate { id: c.id, stated: c.stated, total: c.total }).collect();
        let cut = many || left == 0;
        if with.is_empty() {
            // Nothing is stated anywhere: the roots with the most parts are shown, and the reply says no amount is stated.
            let mut c = all_c;
            c.sort_by(|a, b| m.elements[a.id].qualified_name.cmp(&m.elements[b.id].qualified_name));
            let first = c.first().map(|x| x.id);
            return Choice { root: first, how: "first by name", why: format!("{note}no part states an amount, so the first root by name is shown"), candidates: Vec::new(), cut };
        }
        let how = if with.len() == 1 { "only root that states" } else { "most mass-stating parts" };
        return finish(m, note, how, pick(with), cut);
    };
    finish(m, note, how, cands, left == 0)
}

fn finish(m: &Model, note: String, how: &'static str, cands: Vec<Candidate>, cut: bool) -> Choice {
    let top = &m.elements[cands[0].id];
    let why = match (how, cands.len()) {
        ("holds the limit", 1) => format!("{note}{} holds the limit", top.qualified_name),
        ("only root that states", _) => format!("{note}{} is the only composition root that holds a part that states an amount", top.qualified_name),
        _ => format!("{note}{} holds the most amount-stating parts ({}) of the {} candidates; ties are broken by name", top.qualified_name, cands[0].stated, cands.len()),
    };
    Choice { root: Some(cands[0].id), how, why, candidates: cands, cut }
}

fn requirement_label(nm: &Names, r: &Element) -> String {
    match &r.short_name {
        Some(id) => format!("{id} {}", nm.show(r)),
        None => nm.show(r),
    }
}

fn limit_json(m: &Model, q: &Quantity, nm: &Names, l: &Limit, total: f64) -> Value {
    let (r, a) = (&m.elements[l.requirement], &m.elements[l.attribute]);
    let margin = l.amount - total;
    json!({
        "requirement": r.qualified_name,
        "id": r.short_name,
        "label": requirement_label(nm, r),
        "attribute": a.qualified_name,
        "name": a.name,
        "stated_as": a.value.as_deref().map(str::trim),
        "limit": l.amount,
        "limit_text": amount_text(q, l.amount),
        "total": total,
        "margin": margin,
        "margin_text": amount_text(q, margin),
        "verdict": if total <= l.amount { "pass" } else { "fail" },
        "file": m.files[a.file].path,
        "line": line_of(m, a),
    })
}

/// `get_budget {quantity?, root?, limit_requirement?}`.
pub fn budget(m: &Model, quantity_name: Option<&str>, root: Option<&str>, limit_requirement: Option<&str>) -> Res<Value> {
    let q = quantity(quantity_name)?;
    let nm = Names::new(m);
    let all = limits(m, q);
    let (root_id, choice): (Option<ElementId>, Option<Choice>) = match root.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => {
            let e = match locate(m, r) {
                Ok(e) => e,
                // A bare name of one part is enough; a name that several parts have lists them.
                Err(fail) => {
                    let bare = r.trim().trim_start_matches('<').trim_end_matches('>').trim().trim_matches('\'');
                    let hits: Vec<&Element> = m.elements.iter().filter(|e| e.kind == "part" && !e.implied_name && e.name.as_deref() == Some(bare)).collect();
                    match hits.as_slice() {
                        [one] => *one,
                        [] => return Err(fail),
                        many => {
                            let list: Vec<String> = many.iter().take(20).map(|e| format!("{} ({})", e.qualified_name, e.keyword())).collect();
                            return Err(format!("{} names {} parts, so it does not say which is the root: {}{}. Fix: pass the qualified name of the one you mean.", crate::agent::text::quote_clip(bare), many.len(), list.join("; "), if many.len() > 20 { format!("; and {} more", many.len() - 20) } else { String::new() }).into());
                        }
                    }
                }
            };
            if e.kind != "part" {
                return Err(format!("'{}' is a {}; the root of a budget is a part definition or a part usage.", e.qualified_name, e.keyword()).into());
            }
            (Some(e.id), None)
        }
        None => {
            let c = choose(m, q, &all);
            (c.root, Some(c))
        }
    };
    let mut w = Walk { m, q, nm: Names::new(m), rows: Vec::new(), cut: false };
    let mut root_rows = Vec::new();
    let mut total = 0.0;
    if let Some(r) = root_id {
        let one = w.node(r, 0, None, &mut Vec::new());
        total += one;
        root_rows.push(json!({ "ref": m.elements[r].qualified_name, "label": nm.show(&m.elements[r]), "index": 0, "total": one, "total_text": amount_text(q, one) }));
    }
    let mut rows = w.rows;
    // A part whose tree states no amount at all has no total: a sum of nothing is not 0 kg. The rows are in tree order,
    // so the tree of a row is the rows after it that lie deeper.
    for i in 0..rows.len() {
        if rows[i]["state"] != "sum of its parts" {
            continue;
        }
        let depth = rows[i]["depth"].as_u64().unwrap_or(0);
        let any = rows[i + 1..].iter().take_while(|r| r["depth"].as_u64().unwrap_or(0) > depth).any(|r| r["state"] == "stated");
        if !any {
            let r = &mut rows[i];
            r["state"] = json!("no part states it");
            r["rolled_up"] = Value::Null;
            r["total"] = Value::Null;
            r["rolled_up_text"] = json!("not stated");
            r["total_text"] = json!("not stated");
        }
    }
    let not_stated: Vec<Value> = rows.iter().filter(|r| r["state"] == "not stated" || r["state"] == "not readable").map(|r| json!({ "ref": r["ref"], "label": r["label"], "definition": r["definition"], "state": r["state"], "notes": r["notes"] })).collect();
    let stated = rows.iter().filter(|r| r["state"] == "stated").count();
    // No part states the quantity: there is no total, and nothing can be judged against a limit. A total of 0 would
    // read as "the system weighs nothing" and would pass every limit.
    let nothing = stated == 0 && !not_stated.is_empty();
    let total_json = if nothing { Value::Null } else { json!(total) };
    let total_text = if nothing { "not stated".to_string() } else { amount_text(q, total) };
    for r in root_rows.iter_mut() {
        let at = r["index"].as_u64().unwrap_or(0) as usize;
        if rows.get(at).map(|x| x["total"].is_null()).unwrap_or(false) {
            r["total"] = Value::Null;
            r["total_text"] = json!("not stated");
        }
    }

    // ---- the limit
    let chosen: Option<&Limit> = match limit_requirement.map(str::trim).filter(|r| !r.is_empty()) {
        Some(name) => {
            // The one resolver of every tool (`locate`): an id, a qualified name or a printed form.
            let req = locate(m, name)?;
            Some(all.iter().find(|l| l.requirement == req.id).ok_or_else(|| super::Fail::from(format!("requirement '{}' states no {} limit. Fix: give it an attribute with an amount, for example `attribute max{} = 40 [{}];`.", req.qualified_name, q.id, upper_first(q.attribute), q.unit)))?)
        }
        None => all.first(),
    };
    let one_limit = |l: &Limit| {
        let mut v = limit_json(m, q, &nm, l, total);
        if nothing {
            v["total"] = Value::Null;
            v["margin"] = Value::Null;
            v["margin_text"] = json!("not known");
            v["verdict"] = json!("cannot be judged");
        }
        v
    };
    let limit = chosen.map(|l| one_limit(l));
    let limits_json: Vec<Value> = all.iter().map(|l| one_limit(l)).collect();

    // ---- what a reader says in one sentence
    let what = match root_rows.as_slice() {
        [one] => one["label"].as_str().unwrap_or("").to_string(),
        [] => "the model".to_string(),
        many => format!("the {} top-level parts", many.len()),
    };
    let dups = counted_more_than_once(&rows);
    let mut says = if rows.is_empty() {
        format!("The model has no part to roll {} up over.", q.id)
    } else if nothing {
        format!("The {} is not stated for {} of {} part(s) of {what}: there is no total.", q.id, not_stated.len(), not_stated.len())
    } else {
        format!("The {} of {what} is {} ({} part(s) state it).", q.id, amount_text(q, total), stated)
    };
    if !not_stated.is_empty() && !nothing {
        says.push_str(&format!(" {} part(s) state no {}: the total is a lower bound.", not_stated.len(), q.id));
    }
    let root_choice = match &choice {
        None => json!({ "how": "root argument", "says": "The root was given." }),
        Some(c) => {
            let cands: Vec<Value> = c.candidates.iter().map(|x| {
                let e = &m.elements[x.id];
                json!({ "ref": e.qualified_name, "label": nm.show(e), "stated": x.stated, "total": x.total, "total_text": x.total.map(|t| amount_text(q, t)).unwrap_or_else(|| "not stated".into()), "used": Some(x.id) == c.root })
            }).collect();
            let mut t = format!("Root used: {}.", c.why);
            if c.candidates.len() > 1 {
                let list = c.candidates.iter().map(|x| format!("{} {}", nm.show(&m.elements[x.id]), x.total.map(|t| amount_text(q, t)).unwrap_or_else(|| "not stated".into()))).collect::<Vec<_>>().join("; ");
                t.push_str(&format!(" {} roots hold a stated {} and are not added together: {list}. The guide and the change package use the same root. Pass root to choose another.", c.candidates.len(), q.id));
            }
            if c.cut {
                t.push_str(" The search for roots stopped at a limit of the walker; pass root for one part tree.");
            }
            json!({ "how": c.how, "why": c.why, "candidates": cands, "says": t })
        }
    };
    // A root that was the plain answer is not repeated; a pinned or a chosen-among-several root is said.
    let say_root = choice.as_ref().map(|c| c.candidates.len() > 1 || c.how == "pinned" || c.how == "first by name" || c.cut || c.why.contains("names no part")).unwrap_or(false);
    if let Some(t) = root_choice["says"].as_str().filter(|_| say_root) {
        says.push(' ');
        says.push_str(t);
    }
    if !dups.is_empty() {
        says.push_str(&format!(" {} part(s) are counted more than once, once for each usage (counted_more_than_once): a design duplicate, or a leftover of the model.", dups.len()));
    }
    match &limit {
        Some(l) if nothing => says.push_str(&format!(" The limit of {} is {}: it cannot be judged until the parts state their {}.", l["label"].as_str().unwrap_or(""), l["limit_text"].as_str().unwrap_or(""), q.id)),
        Some(l) if l["verdict"] == "pass" => says.push_str(&format!(" The limit of {} is {}: the margin is {}, PASS.", l["label"].as_str().unwrap_or(""), l["limit_text"].as_str().unwrap_or(""), l["margin_text"].as_str().unwrap_or(""))),
        Some(l) => says.push_str(&format!(" The limit of {} is {}: over by {}, FAIL.", l["label"].as_str().unwrap_or(""), l["limit_text"].as_str().unwrap_or(""), amount_text(q, total - l["limit"].as_f64().unwrap_or(0.0)))),
        None => says.push_str(&format!(" No requirement states a {} limit.", q.id)),
    }

    // ---- the tree as text and as csv
    let mut text = format!("{says}\n");
    let mut lines: Vec<Vec<String>> = Vec::new();
    for r in &rows {
        let depth = r["depth"].as_u64().unwrap_or(0) as usize;
        let def = r["definition"].as_str().map(|d| format!(" : {d}")).unwrap_or_default();
        let count = r["count"].as_f64().unwrap_or(1.0);
        let times = if count != 1.0 { format!(" x{}", number(count)) } else { String::new() };
        let own = match r["state"].as_str() {
            Some("stated") => format!("own {}", r["own_text"].as_str().unwrap_or("")),
            Some("sum of its parts") => "the sum of its parts".to_string(),
            Some("not readable") => "own: NOT READABLE".to_string(),
            Some("no part states it") => "NOT STATED by any of its parts".to_string(),
            _ => "NOT STATED".to_string(),
        };
        let rolled = if r["total"].is_null() {
            String::new()
        } else if r["parts"].as_u64().unwrap_or(0) > 0 || count != 1.0 { format!(", total {}", r["total_text"].as_str().unwrap_or("")) } else { String::new() };
        let name = if depth == 0 { r["label"].as_str().unwrap_or("") } else { r["name"].as_str().unwrap_or("") };
        text.push_str(&format!("{}{name}{def}{times}: {own}{rolled}\n", "  ".repeat(depth)));
        for n in r["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            text.push_str(&format!("{}  note: {n}\n", "  ".repeat(depth)));
        }
        lines.push(vec![
            depth.to_string(),
            format!("{}{name}", "  ".repeat(depth)),
            r["definition"].as_str().unwrap_or("").to_string(),
            number(count),
            r["own"].as_f64().map(number).unwrap_or_default(),
            r["rolled_up"].as_f64().map(number).unwrap_or_default(),
            r["total"].as_f64().map(number).unwrap_or_default(),
            r["stated_as"].as_str().unwrap_or("").to_string(),
            r["state"].as_str().unwrap_or("").to_string(),
            r["notes"].as_array().map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")).unwrap_or_default(),
        ]);
    }
    let unit = q.unit;
    let headers: Vec<String> = ["Level".to_string(), "Part".into(), "Definition".into(), "Count".into(), format!("Own {} ({unit})", q.id), format!("Rolled-up {} of one ({unit})", q.id), format!("Total with count ({unit})"), "Stated as".into(), "State".into(), "Notes".into()].to_vec();
    let tail = |name: &str, value: String, state: String| vec![String::new(), name.to_string(), String::new(), String::new(), String::new(), String::new(), value, String::new(), state, String::new()];
    lines.push(tail("Total", if nothing { String::new() } else { number(total) }, if nothing { format!("no total: not stated for {} of {} part(s)", not_stated.len(), not_stated.len()) } else if not_stated.is_empty() { "every part states it".into() } else { format!("{} part(s) not stated", not_stated.len()) }));
    if let Some(l) = &limit {
        lines.push(tail(&format!("Limit ({})", l["label"].as_str().unwrap_or("")), number(l["limit"].as_f64().unwrap_or(0.0)), String::new()));
        lines.push(tail("Margin", l["margin"].as_f64().map(number).unwrap_or_default(), l["verdict"].as_str().unwrap_or("").to_string()));
    }
    let mut out = json!({
        "quantity": q.id,
        "unit": q.unit,
        "roots": root_rows,
        "root_choice": root_choice,
        "counted_more_than_once": dups,
        "tree": rows,
        "total": total_json,
        "total_text": total_text,
        "stated": stated,
        "parts_that_should_state": stated + not_stated.len(),
        "not_stated": not_stated,
        "complete": not_stated.is_empty(),
        "limit": limit,
        "limits": limits_json,
        "verdict": limit.as_ref().map(|l| l["verdict"].clone()).unwrap_or(json!("no limit")),
        "says": says,
        "text": text,
        "csv": csv(&headers, &lines),
    });
    if w.cut {
        out["truncated"] = json!(true);
        out["hint"] = json!(format!("The roll-up stopped at {MAX_NODES} nodes; the total covers those. Pass root for one part tree."));
    }
    Ok(out)
}

/// The definitions that are counted for more than one usage of the roll-up. The rows are in tree order. A row is listed
/// once, at its outermost place: what lies under an already listed part repeats with it.
fn counted_more_than_once(rows: &[Value]) -> Vec<Value> {
    let amount = |r: &Value| !r["total"].is_null() && r["total"].as_f64().unwrap_or(0.0) > 0.0;
    let key = |r: &Value| r["definition"].as_str().or_else(|| r["ref"].as_str()).unwrap_or("").to_string();
    let mut groups: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, r) in rows.iter().enumerate().filter(|(_, r)| r["depth"].as_u64().unwrap_or(0) > 0 && amount(r)) {
        groups.entry(key(r)).or_default().push(i);
    }
    groups.retain(|_, v| v.len() > 1);
    let flagged = |i: usize| groups.contains_key(&key(&rows[i]));
    let under_flagged = |mut i: usize| {
        while let Some(p) = rows[i]["parent"].as_u64() {
            i = p as usize;
            if rows[i]["depth"].as_u64().unwrap_or(0) > 0 && amount(&rows[i]) && flagged(i) {
                return true;
            }
        }
        false
    };
    let mut out = Vec::new();
    for (k, v) in &groups {
        if v.iter().all(|i| under_flagged(*i)) {
            continue;
        }
        let places: Vec<Value> = v
            .iter()
            .filter(|i| !under_flagged(**i))
            .map(|i| {
                let r = &rows[*i];
                let owner = r["parent"].as_u64().map(|p| &rows[p as usize]);
                json!({ "ref": r["ref"], "name": r["name"], "owner": owner.map(|o| o["label"].clone()), "owner_ref": owner.map(|o| o["ref"].clone()), "count": r["count"], "total_text": r["total_text"], "file": r["file"], "line": r["line"] })
            })
            .collect();
        if places.len() > 1 {
            out.push(json!({ "definition": k, "places": places, "says": format!("{k} is counted {} times: once for each of its {} usages.", places.len(), places.len()) }));
        }
    }
    out
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// What the guide asks of a budget: for each requirement that states a limit, the roll-up over the top-level part
/// definitions against it, and the part that states the largest amount. A model without such a requirement costs nothing.
pub struct Check {
    pub requirement: ElementId,
    pub attribute: ElementId,
    pub limit: f64,
    pub total: f64,
    pub what: String,
    /// The attribute that states the largest own amount, the part that holds it, and the amount.
    pub heaviest: Option<(String, String, f64)>,
    pub quantity: &'static Quantity,
    /// When several roots were candidates: a sentence that names the one used, and why.
    pub root_note: String,
    /// False when no part states the quantity: there is no total, so the limit can be neither met nor missed.
    pub judged: bool,
}

pub fn checks(m: &Model) -> Vec<Check> {
    let mut out = Vec::new();
    for q in &QUANTITIES {
        let all = limits(m, q);
        if all.is_empty() {
            continue;
        }
        let Ok(b) = budget(m, Some(q.id), None, None) else { continue };
        let heaviest = b["tree"].as_array().into_iter().flatten().filter(|r| r["own"].is_number() && r["attribute"].is_string()).max_by(|a, b| a["own"].as_f64().partial_cmp(&b["own"].as_f64()).unwrap_or(std::cmp::Ordering::Equal)).map(|r| (r["attribute"].as_str().unwrap_or("").to_string(), r["label"].as_str().unwrap_or("").to_string(), r["own"].as_f64().unwrap_or(0.0)));
        let what = match b["roots"].as_array().map(Vec::as_slice) {
            Some([one]) => one["ref"].as_str().unwrap_or("").to_string(),
            _ => "the top-level parts".to_string(),
        };
        let root_note = match b["root_choice"]["candidates"].as_array() {
            Some(c) if c.len() > 1 => format!(" Root used: {}. Pass root to get_budget to choose another.", b["root_choice"]["why"].as_str().unwrap_or("")),
            _ => String::new(),
        };
        for l in &all {
            out.push(Check { root_note: root_note.clone(), requirement: l.requirement, attribute: l.attribute, limit: l.amount, total: b["total"].as_f64().unwrap_or(0.0), what: what.clone(), heaviest: heaviest.clone(), quantity: q, judged: b["total"].is_number() });
        }
    }
    out
}

pub fn text_of(q: &Quantity, x: f64) -> String {
    amount_text(q, x)
}

/// The budget of two states side by side, for the change package: totals, limit and verdict before and after, and each
/// part whose own amount or total differs.
pub fn delta(bm: &Model, am: &Model) -> Value {
    let q = &QUANTITIES[0];
    let (b, a) = (budget(bm, None, None, None).unwrap_or(Value::Null), budget(am, None, None, None).unwrap_or(Value::Null));
    let side = |v: &Value| json!({ "root": v["roots"][0]["label"], "root_why": v["root_choice"]["why"], "total": v["total"], "total_text": v["total_text"], "stated": v["stated"], "not_stated": v["not_stated"].as_array().map_or(0, Vec::len), "limit": v["limit"], "verdict": v["verdict"], "says": v["says"] });
    let cell = |r: &Value| match r["state"].as_str() {
        Some("stated") if r["parts"].as_u64().unwrap_or(0) == 0 && r["count"].as_f64() == Some(1.0) => r["own_text"].as_str().unwrap_or("").to_string(),
        Some("stated") => format!("own {}, total {}", r["own_text"].as_str().unwrap_or(""), r["total_text"].as_str().unwrap_or("")),
        Some("sum of its parts") => format!("total {}", r["total_text"].as_str().unwrap_or("")),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let list = |v: &Value| -> Vec<(String, String, String)> { v["tree"].as_array().into_iter().flatten().map(|r| (r["ref"].as_str().unwrap_or("").to_string(), r["label"].as_str().unwrap_or("").to_string(), cell(r))).collect() };
    let (lb, la) = (list(&b), list(&a));
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    let (in_b, in_a): (HashMap<&str, &(String, String, String)>, BTreeSet<&str>) = (lb.iter().rev().map(|x| (x.0.as_str(), x)).collect(), la.iter().map(|x| x.0.as_str()).collect());
    for (qn, label, now) in &la {
        if !seen.insert(qn.clone()) {
            continue;
        }
        match in_b.get(qn.as_str()).copied() {
            Some((_, _, was)) if was == now => {}
            Some((_, _, was)) => rows.push(json!({ "change": "changed", "ref": qn, "label": label, "before": was, "after": now })),
            None => rows.push(json!({ "change": "added", "ref": qn, "label": label, "before": "", "after": now })),
        }
    }
    let mut gone = BTreeSet::new();
    for (qn, label, was) in &lb {
        if !in_a.contains(qn.as_str()) && gone.insert(qn.clone()) {
            rows.push(json!({ "change": "removed", "ref": qn, "label": label, "before": was, "after": "" }));
        }
    }
    let relevant = b["stated"].as_u64().unwrap_or(0) + a["stated"].as_u64().unwrap_or(0) > 0 || b["limit"].is_object() || a["limit"].is_object();
    json!({ "quantity": q.id, "unit": q.unit, "relevant": relevant, "before": side(&b), "after": side(&a), "rows": rows })
}
