//! Change report: what a write does to the model, as facts and not as text lines. Two parses are
//! compared element by element; ports, connectors, trace links, coverage and the gates are
//! read from each side with the same code the read tools use, so a report and a later read agree.
//!
//! Elements are matched by identity, not by position: a named element by its names from the top, a
//! statement without a name by its owner, its keyword and what it says. A deleted sibling therefore does
//! not turn every later `Owner::$n` into a change. A rename or a move of an element whose content stays
//! the same is found by content and reported as a changed `name` or `owner`.

use super::connect;
use super::graph::{self, Stmt};
use super::impact::{self, SourceIds};
use super::trace;
use crate::api::FileIn;
use crate::model::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

/// Rows per list in the default (summary) report, and in the full one. The counts stay exact.
pub const SUMMARY_ROWS: usize = 12;
pub const FULL_ROWS: usize = 1000;

const SEP: char = '\u{1}';

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Element,
    Connection,
    Interface,
    Flow,
    Trace,
}

fn group(e: &Element) -> Group {
    if e.is_def {
        return Group::Element;
    }
    match e.kind.as_str() {
        "connection" | "binding" => Group::Connection,
        "interface" => Group::Interface,
        "flow" | "message" => Group::Flow,
        "allocation" | "dependency" => Group::Trace,
        k if k.starts_with("satisfy") || k.starts_with("verify") => Group::Trace,
        _ => Group::Element,
    }
}

pub(super) fn skipped(e: &Element) -> bool {
    // The text of a `doc` is the `doc` fact of its owner.
    e.kind == "doc" && !e.is_def
}

fn doc_of(m: &Model, e: &Element) -> Option<String> {
    e.doc.clone().or_else(|| m.children(e.id).find(|c| c.kind == "doc").and_then(|c| c.doc.clone()))
}

fn declared_name(e: &Element) -> Option<&str> {
    e.name.as_deref().filter(|_| !e.implied_name)
}

/// One side of the comparison with the identity of every element.
struct Side<'a> {
    m: &'a Model,
    ids: SourceIds<'a>,
    /// Named, and every owner named: the qualified name is stable.
    stable: Vec<bool>,
    /// Identity from names and written text only; what a reference to the element is compared by.
    nid: Vec<String>,
    /// Identity used for matching: as `nid`, with the references of an unnamed statement followed.
    ident: Vec<String>,
    stmts: Vec<Stmt>,
    /// Statements the model-wide pair limit hid from `stmts`.
    hidden: Option<String>,
    by_via: HashMap<ElementId, Vec<usize>>,
}

fn raw_sig(e: &Element, refs: &dyn Fn(&Reference) -> String) -> String {
    let mut s = format!("{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}", e.prefixes.join(","), e.direction, e.multiplicity, e.value, e.short_name, e.doc, e.props);
    for r in &e.refs {
        s.push_str(&format!("|{:?}{}:{}", r.role, if r.conjugated { "~" } else { "" }, refs(r)));
    }
    s
}

fn naming_ref(e: &Element) -> Option<&Reference> {
    e.refs.iter().find(|r| matches!(r.role, RefRole::Target | RefRole::Redefines))
}

impl<'a> Side<'a> {
    /// `renamed` gives an element the identity of the element it became (a rename or a move). Without `statements` the
    /// relationship statements are not read (a comparison of the elements alone needs only the identities).
    fn build(m: &'a Model, ids: &'a BTreeMap<String, String>, renamed: &HashMap<ElementId, String>, statements: bool) -> Self {
        let n = m.elements.len();
        let (mut stable, mut nid) = (vec![false; n], vec![String::new(); n]);
        let order = |m: &Model| {
            // Owners first, whatever order the parser numbered them in.
            let mut out = Vec::with_capacity(m.elements.len());
            let mut stack: Vec<ElementId> = m.roots.iter().rev().copied().collect();
            while let Some(x) = stack.pop() {
                out.push(x);
                stack.extend(m.elements[x].children.iter().rev().copied());
            }
            out
        };
        let order = order(m);
        for &i in &order {
            let e = &m.elements[i];
            stable[i] = declared_name(e).is_some() && e.owner.map(|o| stable[o]).unwrap_or(true);
            if let Some(to) = renamed.get(&i) {
                nid[i] = to.clone();
                continue;
            }
            let local = match (declared_name(e), e.implied_name) {
                (Some(name), _) => format!("n:{name}"),
                (None, true) => format!("i:{}:{}", e.keyword(), naming_ref(e).map(|r| r.text.as_str()).unwrap_or("")),
                _ => format!("u:{}:{}", e.keyword(), raw_sig(e, &|r| r.text.clone())),
            };
            nid[i] = format!("{}{SEP}{local}", e.owner.map(|o| nid[o].as_str()).unwrap_or(""));
        }
        let mut ident = vec![String::new(); n];
        let followed = |r: &Reference| -> String {
            if r.segments.iter().all(|s| s.resolved.is_some()) {
                r.segments.iter().filter_map(|s| s.resolved).map(|t| nid[t].as_str()).collect::<Vec<_>>().join(">")
            } else {
                r.text.clone()
            }
        };
        for &i in &order {
            let e = &m.elements[i];
            if stable[i] || declared_name(e).is_some() && renamed.contains_key(&i) {
                ident[i] = nid[i].clone();
                continue;
            }
            let local = match (declared_name(e), e.implied_name) {
                (Some(name), _) => format!("n:{name}"),
                (None, true) => format!("i:{}:{}", e.keyword(), naming_ref(e).map(&followed).unwrap_or_default()),
                _ => format!("u:{}:{}", e.keyword(), raw_sig(e, &followed)),
            };
            ident[i] = format!("{}{SEP}{local}", e.owner.map(|o| ident[o].as_str()).unwrap_or(""));
        }
        let (stmts, cuts) = if statements { graph::statements_cut(m) } else { (Vec::new(), Vec::new()) };
        let hidden = graph::check_notice(&cuts);
        let mut by_via: HashMap<ElementId, Vec<usize>> = HashMap::new();
        for (i, s) in stmts.iter().enumerate() {
            by_via.entry(s.via).or_default().push(i);
        }
        Side { m, ids: SourceIds::new(ids), stable, nid, ident, stmts, hidden, by_via }
    }

    fn row(&self, e: &Element) -> Value {
        let mut v = impact::tag(self.m, &self.ids, e);
        if let Some(o) = e.owner {
            v["owner"] = json!(self.m.elements[o].qualified_name);
        }
        v
    }

    /// A connector, flow or trace statement as a row: its ends as written, and what it conveys.
    fn statement_row(&self, e: &Element) -> Value {
        let mut v = self.row(e);
        let texts = |role: RefRole| e.refs_of(role).map(|r| r.text.clone()).collect::<Vec<_>>().join(", ");
        match self.by_via.get(&e.id).and_then(|l| l.first()).map(|i| &self.stmts[*i]) {
            Some(s) => {
                v["relation"] = json!(s.kind);
                let end = |x: &graph::End| x.id.map(|i| self.m.elements[i].qualified_name.clone()).unwrap_or_else(|| x.text.clone());
                v["source"] = json!(end(&s.source));
                v["target"] = json!(end(&s.target));
            }
            None => {
                v["source"] = json!(texts(RefRole::Source));
                v["target"] = json!(texts(RefRole::Target));
            }
        }
        if matches!(e.kind.as_str(), "flow" | "message") {
            v["of"] = json!(e.refs_of(RefRole::Type).map(|r| r.text.clone()).collect::<Vec<_>>());
        }
        v
    }
}

/// Content of an element without its name and place: equal for an element before and after a rename or a move.
fn fingerprint(m: &Model, e: &Element) -> u64 {
    let mut h = DefaultHasher::new();
    (e.keyword(), &e.prefixes, &e.direction, &e.multiplicity, &e.value, &e.short_name, doc_of(m, e), &e.props).hash(&mut h);
    for r in &e.refs {
        (format!("{:?}", r.role), &r.text, r.conjugated).hash(&mut h);
    }
    for c in m.children(e.id) {
        (c.keyword(), declared_name(c)).hash(&mut h);
    }
    h.finish()
}

thread_local! {
    /// The rename log of the folder being compared (renames.jsonl, `from -> to` in order). `renames` follows it before
    /// it guesses by content, so a rename that also changed the element is still a rename.
    static LOG: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Sets the rename log for the comparisons made while the value lives.
struct LogScope;

impl LogScope {
    fn new(files: &[&[FileIn]]) -> LogScope {
        let text = files.iter().flat_map(|f| f.iter()).find(|f| f.path == super::rekey::RENAMES_FILE).map(|f| f.text.as_str()).unwrap_or("");
        let log = super::rekey::read_log(text).into_iter().filter_map(|m| m.to.map(|to| (m.from, to))).collect();
        LOG.with(|l| *l.borrow_mut() = log);
        LogScope
    }
}

impl Drop for LogScope {
    fn drop(&mut self) {
        LOG.with(|l| l.borrow_mut().clear());
    }
}

/// Named elements the rename log moved: a qualified name that is gone, followed through the log to one that is new.
/// A descendant follows the owner the log moved. `before id -> after id`.
fn logged_renames(b: &Model, a: &Model, gone: &[ElementId], new: &HashSet<ElementId>) -> HashMap<ElementId, ElementId> {
    let mut out = HashMap::new();
    LOG.with(|l| {
        let log = l.borrow();
        if log.is_empty() {
            return;
        }
        let aq: HashMap<&str, ElementId> = new.iter().map(|x| (a.elements[*x].qualified_name.as_str(), *x)).collect();
        for g in gone {
            let mut qn = b.elements[*g].qualified_name.clone();
            let mut moved = false;
            for (from, to) in log.iter() {
                if qn == *from {
                    qn = to.clone();
                    moved = true;
                } else if let Some(rest) = qn.strip_prefix(from.as_str()).filter(|r| r.starts_with("::")) {
                    qn = format!("{to}{rest}");
                    moved = true;
                }
            }
            if let Some(x) = aq.get(qn.as_str()).filter(|_| moved) {
                out.insert(*g, *x);
            }
        }
    });
    out
}

/// Named elements that left one qualified name for another: by the rename log first, else with the same content.
/// `before id -> after id`.
fn renames(b: &Model, a: &Model) -> HashMap<ElementId, ElementId> {
    let stable_names = |m: &Model| -> (Vec<bool>, HashMap<String, ElementId>) {
        let mut st = vec![false; m.elements.len()];
        let mut by = HashMap::new();
        let mut stack: Vec<ElementId> = m.roots.iter().rev().copied().collect();
        while let Some(x) = stack.pop() {
            let e = &m.elements[x];
            st[x] = declared_name(e).is_some() && e.owner.map(|o| st[o]).unwrap_or(true);
            if st[x] {
                by.entry(e.qualified_name.clone()).or_insert(x);
            }
            stack.extend(e.children.iter().rev().copied());
        }
        (st, by)
    };
    let ((bs, bq), (as_, aq)) = (stable_names(b), stable_names(a));
    // Only the top of a removed or added subtree: what is inside follows its owner.
    let tops = |m: &Model, st: &[bool], other: &HashMap<String, ElementId>| -> Vec<ElementId> {
        m.elements
            .iter()
            .filter(|e| st[e.id] && !other.contains_key(&e.qualified_name))
            .filter(|e| e.owner.map(|o| other.contains_key(&m.elements[o].qualified_name)).unwrap_or(true))
            .map(|e| e.id)
            .collect()
    };
    let (gone, new) = (tops(b, &bs, &aq), tops(a, &as_, &bq));
    if gone.is_empty() || new.is_empty() {
        return HashMap::new();
    }
    // By element id first: an element that kept its short name across the change is the same element, whatever
    // renamed or moved it (a tool, a text editor, Cameo).
    let mut by_short: HashMap<&str, Vec<ElementId>> = HashMap::new();
    for x in &new {
        if let Some(s) = a.elements[*x].short_name.as_deref() {
            by_short.entry(s).or_default().push(*x);
        }
    }
    let mut out: HashMap<ElementId, ElementId> = HashMap::new();
    for g in &gone {
        if let Some([x]) = b.elements[*g].short_name.as_deref().and_then(|s| by_short.get(s)).map(Vec::as_slice) {
            out.insert(*g, *x);
        }
    }
    let rest: Vec<ElementId> = gone.iter().copied().filter(|g| !out.contains_key(g)).collect();
    let taken: HashSet<ElementId> = out.values().copied().collect();
    out.extend(logged_renames(b, a, &rest, &new.iter().copied().filter(|x| !taken.contains(x)).collect()));
    let mut used: HashSet<ElementId> = out.values().copied().collect();
    let mut by_print: HashMap<u64, Vec<ElementId>> = HashMap::new();
    for x in new.iter().filter(|x| !used.contains(x)) {
        by_print.entry(fingerprint(a, &a.elements[*x])).or_default().push(*x);
    }
    let owner_name = |m: &Model, e: &Element| e.owner.map(|o| m.elements[o].qualified_name.clone());
    let unpaired: Vec<ElementId> = gone.into_iter().filter(|g| !out.contains_key(g)).collect();
    for g in unpaired {
        let be = &b.elements[g];
        let Some(c) = by_print.get(&fingerprint(b, be)) else { continue };
        let fits: Vec<ElementId> = c.iter().copied().filter(|x| !used.contains(x)).filter(|x| a.elements[*x].name == be.name || owner_name(a, &a.elements[*x]) == owner_name(b, be)).collect();
        if let [x] = fits[..] {
            used.insert(x);
            out.insert(g, x);
        }
    }
    out
}

type Facts = BTreeMap<&'static str, (String, Value)>;

/// The facts of an element. Each is `(what is compared, what is shown)`; `name_of` gives the name a reference
/// target is compared and shown by (for the old side: the name the element has now, when it still exists).
fn facts(m: &Model, e: &Element, name_of: &dyn Fn(ElementId) -> String) -> Facts {
    let mut f: Facts = BTreeMap::new();
    let mut put = |k: &'static str, cmp: String, show: Value| {
        f.insert(k, (cmp, show));
    };
    let plain = |k: &'static str, v: Option<String>, put: &mut dyn FnMut(&'static str, String, Value)| {
        if let Some(v) = v {
            put(k, v.clone(), json!(v));
        }
    };
    plain("name", declared_name(e).map(str::to_string), &mut put);
    put("kind", e.keyword(), json!(e.keyword()));
    plain("owner", e.owner.map(name_of), &mut put);
    plain("multiplicity", e.multiplicity.clone(), &mut put);
    plain("direction", e.direction.clone(), &mut put);
    plain("doc", doc_of(m, e), &mut put);
    plain("short_name", e.short_name.clone(), &mut put);
    if !e.prefixes.is_empty() {
        put("prefixes", e.prefixes.join(" "), json!(e.prefixes));
    }
    // A value is compared without the names inside it that resolve: a rename rewrites them and changes nothing.
    if let Some(v) = &e.value {
        let mut cmp = v.clone();
        if let Some(vs) = e.value_span {
            let mut cuts: Vec<(usize, usize, String)> = e
                .refs
                .iter()
                .filter(|r| matches!(r.role, RefRole::Expr) && r.span.start >= vs.start && r.span.end <= vs.end && r.segments.iter().all(|s| s.resolved.is_some()))
                .map(|r| (r.span.start - vs.start, r.span.end - vs.start, format!("<{}>", r.segments.iter().filter_map(|s| s.resolved).map(name_of).collect::<Vec<_>>().join("."))))
                .collect();
            cuts.sort();
            let raw = m.files.get(e.file).and_then(|fi| fi.text.get(vs.start..vs.end));
            if let Some(raw) = raw {
                let mut out = String::new();
                let mut at = 0;
                for (s, t, name) in cuts {
                    if s >= at && t <= raw.len() && raw.is_char_boundary(s) && raw.is_char_boundary(t) {
                        out.push_str(&raw[at..s]);
                        out.push_str(&name);
                        at = t;
                    }
                }
                out.push_str(&raw[at..]);
                cmp = out.split_whitespace().collect::<Vec<_>>().join(" ");
            }
        }
        put("value", cmp, json!(v));
    }
    // A reference is compared by what it resolves to; one that does not resolve is compared by its text, marked.
    let target = |r: &Reference| -> (String, String) {
        let conj = if r.conjugated { "~" } else { "" };
        match r.resolved() {
            Some(t) if r.segments.iter().all(|s| s.resolved.is_some()) => {
                let n = name_of(t);
                (format!("{conj}{n}"), format!("{conj}{n}"))
            }
            _ => (format!("?{conj}{}", r.text), format!("unresolved: {conj}{}", r.text)),
        }
    };
    let ends = matches!(e.kind.as_str(), "flow" | "message");
    for (k, role) in [(if ends { "of" } else { "type" }, RefRole::Type), ("specializes", RefRole::Specializes), ("redefines", RefRole::Redefines), ("references", RefRole::References)] {
        let v: Vec<(String, String)> = e.refs_of(role).map(&target).collect();
        if !v.is_empty() {
            put(k, v.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join(", "), json!(v.iter().map(|x| x.1.as_str()).collect::<Vec<_>>()));
        }
    }
    for (k, role) in [("source", RefRole::Source), ("target", RefRole::Target)] {
        let path = |r: &Reference| r.segments.iter().map(|s| s.resolved.map(name_of).unwrap_or_else(|| format!("?{}", s.name))).collect::<Vec<_>>().join(">");
        let cmp: Vec<String> = e.refs_of(role).map(path).collect();
        if !cmp.is_empty() {
            let text = e.refs_of(role).map(|r| r.text.clone()).collect::<Vec<_>>().join(", ");
            let to: Vec<Option<String>> = e.refs_of(role).map(|r| r.resolved().filter(|_| r.segments.iter().all(|s| s.resolved.is_some())).map(name_of)).collect();
            put(k, cmp.join(", "), json!({ "text": text, "resolves_to": if to.len() == 1 { json!(to[0]) } else { json!(to) } }));
        }
    }
    if !e.props.is_empty() {
        put("detail", format!("{:?}", e.props), json!(e.props));
    }
    f
}

struct Lists {
    limit: usize,
    cut: Vec<String>,
}

impl Lists {
    fn put(&mut self, name: &str, mut rows: Vec<Value>) -> Value {
        if rows.len() > self.limit {
            rows.truncate(self.limit);
            self.cut.push(name.to_string());
        }
        json!(rows)
    }
}

/// Parts with a connected port: `(part, port)` by the names `name_of` gives.
fn connected_ports(m: &Model, name_of: &dyn Fn(ElementId) -> String) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for c in m.elements.iter().filter(|c| impact::is_connector(c)) {
        for r in c.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target)) {
            let segs: Vec<ElementId> = r.segments.iter().filter_map(|s| s.resolved).collect();
            for (i, s) in segs.iter().enumerate().filter(|(_, s)| m.elements[**s].kind == "port") {
                if let Some(holder) = if i > 0 { Some(segs[i - 1]) } else { c.owner } {
                    out.insert((name_of(holder), name_of(*s)));
                }
            }
        }
    }
    out
}

fn gate_counts(v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for g in ["syntax", "semantic", "traceability"] {
        out.insert(g.into(), json!({ "errors": v["gates"][g]["errors"], "warnings": v["gates"][g]["warnings"] }));
    }
    out.insert("errors".into(), v["errors"].clone());
    out.insert("warnings".into(), v["warnings"].clone());
    Value::Object(out)
}

/// The report between two file sets. `ids_*` map a source id to a qualified name on each side.
pub fn change_report(before: &[FileIn], after: &[FileIn], ids_before: &BTreeMap<String, String>, ids_after: &BTreeMap<String, String>, full: bool) -> Value {
    let _log = LogScope::new(&[after, before]);
    let (mut bm, mut am) = (super::intent::model_of(before).as_ref().clone(), super::intent::model_of(after).as_ref().clone());
    super::add_namespace_checks(&mut bm);
    super::add_namespace_checks(&mut am);
    compare_models(&bm, &am, ids_before, ids_after, full)
}

/// The two sides of a comparison, and the elements that are the same element on both (`pair_b`: before to after,
/// `pair_a`: after to before).
pub(super) struct Paired<'a> {
    a: Side<'a>,
    b: Side<'a>,
    pub(super) pair_b: HashMap<ElementId, ElementId>,
    pub(super) pair_a: HashMap<ElementId, ElementId>,
}

/// The elements that carry the same id on both sides: a short name that one element holds on each side, or a source id
/// (`xmi:id`) whose qualified name names one element on each side. Short names first; an element pairs once.
fn id_pairs(bm: &Model, am: &Model, ids_before: &BTreeMap<String, String>, ids_after: &BTreeMap<String, String>) -> HashMap<ElementId, ElementId> {
    fn by_short(m: &Model) -> HashMap<&str, Option<ElementId>> {
        let mut by: HashMap<&str, Option<ElementId>> = HashMap::new();
        for e in m.elements.iter().filter(|e| !skipped(e)) {
            if let Some(s) = e.short_name.as_deref() {
                // Two elements with one short name (a copy): neither is known by it.
                by.entry(s).and_modify(|x| *x = None).or_insert(Some(e.id));
            }
        }
        by
    }
    let (sb, sa) = (by_short(bm), by_short(am));
    let mut out: HashMap<ElementId, ElementId> = HashMap::new();
    let mut taken: HashSet<ElementId> = HashSet::new();
    for (s, x) in &sb {
        if let (Some(x), Some(Some(y))) = (x, sa.get(s)) {
            if bm.elements[*x].kind == am.elements[*y].kind && taken.insert(*y) {
                out.insert(*x, *y);
            }
        }
    }
    if !ids_before.is_empty() && !ids_after.is_empty() {
        fn by_qn(m: &Model) -> HashMap<&str, Option<ElementId>> {
            let mut by: HashMap<&str, Option<ElementId>> = HashMap::new();
            for e in &m.elements {
                by.entry(e.qualified_name.as_str()).and_modify(|x| *x = None).or_insert(Some(e.id));
            }
            by
        }
        let (qb, qa) = (by_qn(bm), by_qn(am));
        for (id, qn) in ids_before {
            let (Some(Some(x)), Some(Some(y))) = (qb.get(qn.as_str()), ids_after.get(id).and_then(|q| qa.get(q.as_str()))) else { continue };
            if !out.contains_key(x) && !skipped(&bm.elements[*x]) && bm.elements[*x].kind == am.elements[*y].kind && taken.insert(*y) {
                out.insert(*x, *y);
            }
        }
    }
    out
}

pub(super) fn pair<'a>(bm: &'a Model, am: &'a Model, ids_before: &'a BTreeMap<String, String>, ids_after: &'a BTreeMap<String, String>, statements: bool) -> Paired<'a> {
    let by_id = id_pairs(bm, am, ids_before, ids_after);
    let mut renamed = renames(bm, am);
    // An id is stronger evidence than a name: it decides where both disagree.
    let id_targets: HashSet<ElementId> = by_id.values().copied().collect();
    renamed.retain(|x, y| !by_id.contains_key(x) && !id_targets.contains(y));
    renamed.extend(by_id.iter().map(|(x, y)| (*x, *y)));
    let a = Side::build(am, ids_after, &HashMap::new(), statements);
    let b = Side::build(bm, ids_before, &renamed.iter().map(|(x, y)| (*x, a.nid[*y].clone())).collect(), statements);

    let mut pair_b: HashMap<ElementId, ElementId> = HashMap::new();
    let mut pair_a: HashMap<ElementId, ElementId> = HashMap::new();
    for (x, y) in &by_id {
        pair_b.insert(*x, *y);
        pair_a.insert(*y, *x);
    }
    // Match the rest by identity; equal identities pair in text order.
    let mut waiting: HashMap<&str, VecDeque<ElementId>> = HashMap::new();
    for e in am.elements.iter().filter(|e| !skipped(e) && !pair_a.contains_key(&e.id)) {
        waiting.entry(a.ident[e.id].as_str()).or_default().push_back(e.id);
    }
    let rest: Vec<ElementId> = bm.elements.iter().filter(|e| !skipped(e) && !pair_b.contains_key(&e.id)).map(|e| e.id).collect();
    for e in rest.into_iter().map(|i| &bm.elements[i]) {
        if let Some(x) = waiting.get_mut(b.ident[e.id].as_str()).and_then(|q| q.pop_front()) {
            pair_b.insert(e.id, x);
            pair_a.insert(x, e.id);
        }
    }
    // A statement whose text is the same but whose names resolve differently now is the same statement.
    let text_key = |s: &Side, e: &Element| {
        let owner = e.owner.map(|o| s.ident[o].as_str()).unwrap_or("");
        format!("{owner}{SEP}{}{SEP}{:?}{SEP}{}", e.keyword(), declared_name(e), raw_sig(e, &|r| r.text.clone()))
    };
    let statement = |e: &Element| group(e) != Group::Element && !skipped(e);
    let mut open: HashMap<String, VecDeque<ElementId>> = HashMap::new();
    for e in am.elements.iter().filter(|e| statement(e) && !pair_a.contains_key(&e.id)) {
        open.entry(text_key(&a, e)).or_default().push_back(e.id);
    }
    let unpaired: Vec<&Element> = bm.elements.iter().filter(|e| statement(e) && !pair_b.contains_key(&e.id)).collect();
    for e in unpaired {
        if let Some(x) = open.get_mut(&text_key(&b, e)).and_then(|q| q.pop_front()) {
            pair_b.insert(e.id, x);
            pair_a.insert(x, e.id);
        }
    }
    // The name a reference target is shown and compared by: for the old side, the name the element has now.
    let name_a = |i: ElementId| am.elements[i].qualified_name.clone();
    let name_b = |i: ElementId| pair_b.get(&i).map(|x| am.elements[*x].qualified_name.clone()).unwrap_or_else(|| bm.elements[i].qualified_name.clone());
    // A connector that kept one end: re-ended, not removed and added.
    let end_paths = |m: &Model, e: &Element, name_of: &dyn Fn(ElementId) -> String| -> (String, String) {
        let f = facts(m, e, name_of);
        (f.get("source").map(|x| x.0.clone()).unwrap_or_default(), f.get("target").map(|x| x.0.clone()).unwrap_or_default())
    };
    let re_end_key = |s: &Side, e: &Element| format!("{}{SEP}{}{SEP}{:?}", e.owner.map(|o| s.ident[o].as_str()).unwrap_or(""), e.keyword(), declared_name(e));
    let connector = |e: &Element| matches!(group(e), Group::Connection | Group::Interface | Group::Flow);
    let mut loose: HashMap<String, Vec<(ElementId, (String, String))>> = HashMap::new();
    for e in am.elements.iter().filter(|e| connector(e) && !pair_a.contains_key(&e.id)) {
        loose.entry(re_end_key(&a, e)).or_default().push((e.id, end_paths(am, e, &name_a)));
    }
    let mut re_ended: Vec<(ElementId, ElementId)> = Vec::new();
    if !loose.is_empty() {
        let unpaired: Vec<&Element> = bm.elements.iter().filter(|e| connector(e) && !pair_b.contains_key(&e.id)).collect();
        for e in unpaired {
            let Some(c) = loose.get_mut(&re_end_key(&b, e)) else { continue };
            let (s, t) = end_paths(bm, e, &name_b);
            if let Some(i) = c.iter().position(|(_, (s2, t2))| (!s.is_empty() && s == *s2) || (!t.is_empty() && t == *t2)) {
                let (x, _) = c.remove(i);
                re_ended.push((e.id, x));
            }
        }
    }
    for (x, y) in &re_ended {
        pair_b.insert(*x, *y);
        pair_a.insert(*y, *x);
    }
    Paired { a, b, pair_b, pair_a }
}

/// The elements of `bm` paired with the elements of `am` that are the same element (before to after), as `compare_models`
/// pairs them: by id (short name or source id) first, then by name and text.
pub fn paired_elements(bm: &Model, am: &Model, ids_before: &BTreeMap<String, String>, ids_after: &BTreeMap<String, String>) -> HashMap<ElementId, ElementId> {
    pair(bm, am, ids_before, ids_after, false).pair_b
}

/// The facts of an element that differ between the model before and the model after: `fact -> {before, after}`. The same
/// test `compare_models` makes; a connector, flow or trace statement whose end only stopped resolving has no difference.
pub(super) fn element_diff(bm: &Model, am: &Model, pair_b: &HashMap<ElementId, ElementId>, e: &Element, ae: &Element) -> serde_json::Map<String, Value> {
    let name_a = |i: ElementId| am.elements[i].qualified_name.clone();
    let name_b = |i: ElementId| pair_b.get(&i).map(|x| am.elements[*x].qualified_name.clone()).unwrap_or_else(|| bm.elements[i].qualified_name.clone());
    let (fb, fa) = (facts(bm, e, &name_b), facts(am, ae, &name_a));
    let mut diff = serde_json::Map::new();
    let keys: BTreeSet<&&'static str> = fb.keys().chain(fa.keys()).collect();
    for k in keys {
        let (vb, va) = (fb.get(*k), fa.get(*k));
        if vb.map(|v| &v.0) != va.map(|v| &v.0) {
            // The same text that no longer resolves is a dangling end, listed there; it is not a new end.
            let name: &str = k;
            let lost = matches!(name, "source" | "target" | "type" | "of") && va.map(|v| v.0.contains('?')).unwrap_or(false) && vb.map(|v| &v.1["text"]) == va.map(|v| &v.1["text"]);
            if lost && group(ae) != Group::Element {
                continue;
            }
            diff.insert(k.to_string(), json!({ "before": vb.map(|v| &v.1), "after": va.map(|v| &v.1) }));
        }
    }
    diff
}

pub fn compare_models(bm: &Model, am: &Model, ids_before: &BTreeMap<String, String>, ids_after: &BTreeMap<String, String>, full: bool) -> Value {
    let mut l = Lists { limit: if full { FULL_ROWS } else { SUMMARY_ROWS }, cut: Vec::new() };
    let Paired { a, b, pair_b, pair_a } = pair(bm, am, ids_before, ids_after, true);
    let name_a = |i: ElementId| am.elements[i].qualified_name.clone();
    let name_b = |i: ElementId| pair_b.get(&i).map(|x| am.elements[*x].qualified_name.clone()).unwrap_or_else(|| bm.elements[i].qualified_name.clone());

    // Added, removed and changed, by group.
    #[derive(Default)]
    struct G {
        added: Vec<Value>,
        removed: Vec<Value>,
        changed: Vec<Value>,
        re_ended: Vec<Value>,
        n_added: usize,
        n_removed: usize,
    }
    let (mut elements, mut ports, mut conns, mut ifaces, mut flows, mut traces) = (G::default(), G::default(), G::default(), G::default(), G::default(), G::default());
    for e in bm.elements.iter().filter(|e| !skipped(e)) {
        let Some(x) = pair_b.get(&e.id) else {
            let top = e.owner.map(|o| pair_b.contains_key(&o)).unwrap_or(true);
            let g = match group(e) {
                Group::Element => &mut elements,
                Group::Connection => &mut conns,
                Group::Interface => &mut ifaces,
                Group::Flow => &mut flows,
                Group::Trace => &mut traces,
            };
            g.n_removed += 1;
            if group(e) != Group::Element {
                g.removed.push(b.statement_row(e));
            } else if top {
                let mut row = b.row(e);
                let inside = bm.descendants(e.id).len();
                if inside > 0 {
                    row["members"] = json!(inside);
                }
                g.removed.push(row);
            }
            if e.kind == "port" && !e.is_def {
                ports.n_removed += 1;
                ports.removed.push(b.row(e));
            }
            continue;
        };
        let ae = &am.elements[*x];
        let diff = element_diff(bm, am, &pair_b, e, ae);
        if diff.is_empty() {
            continue;
        }
        let mut row = a.row(ae);
        if e.qualified_name != ae.qualified_name && (b.stable[e.id] || a.stable[ae.id]) {
            row["previous_ref"] = json!(e.qualified_name);
        }
        let ends_only = diff.keys().all(|k| k == "source" || k == "target");
        row["changed"] = json!(diff.keys().collect::<Vec<_>>());
        row["facts"] = Value::Object(diff);
        let g = match group(ae) {
            Group::Element => &mut elements,
            Group::Connection => &mut conns,
            Group::Interface => &mut ifaces,
            Group::Flow => &mut flows,
            Group::Trace => &mut traces,
        };
        if ends_only && group(ae) != Group::Element {
            g.re_ended.push(row.clone());
        } else {
            g.changed.push(row.clone());
        }
        if ae.kind == "port" && !ae.is_def {
            ports.changed.push(row);
        }
    }
    for e in am.elements.iter().filter(|e| !skipped(e) && !pair_a.contains_key(&e.id)) {
        let top = e.owner.map(|o| pair_a.contains_key(&o)).unwrap_or(true);
        let g = match group(e) {
            Group::Element => &mut elements,
            Group::Connection => &mut conns,
            Group::Interface => &mut ifaces,
            Group::Flow => &mut flows,
            Group::Trace => &mut traces,
        };
        g.n_added += 1;
        if group(e) != Group::Element {
            g.added.push(a.statement_row(e));
        } else if top {
            let mut row = a.row(e);
            let inside = am.descendants(e.id).len();
            if inside > 0 {
                row["members"] = json!(inside);
            }
            g.added.push(row);
        }
        if e.kind == "port" && !e.is_def {
            ports.n_added += 1;
            ports.added.push(a.row(e));
        }
    }

    // Dangling: an end that resolved before and does not now; a flow that lost the connection it ran on;
    // a port that was connected and is not any more.
    let lost_ends = |e: &Element| -> Vec<Value> {
        let old = pair_a.get(&e.id).map(|x| &bm.elements[*x]);
        let mut out = Vec::new();
        for (i, r) in e.refs.iter().enumerate().filter(|(_, r)| matches!(r.role, RefRole::Source | RefRole::Target | RefRole::Type)) {
            if r.segments.iter().all(|s| s.resolved.is_some()) {
                continue;
            }
            let was = old.and_then(|o| o.refs.get(i).filter(|x| x.role == r.role).or_else(|| o.refs.iter().find(|x| x.role == r.role && x.text == r.text)));
            match (old, was) {
                // It did not resolve before either: not a consequence of this change.
                (Some(_), Some(w)) if !w.segments.iter().all(|s| s.resolved.is_some()) => continue,
                _ => {}
            }
            let mut row = a.statement_row(e);
            row["problem"] = json!("lost-end");
            row["end"] = json!(match r.role {
                RefRole::Source => "source",
                RefRole::Target => "target",
                _ => "type",
            });
            row["text"] = json!(r.text);
            row["was"] = json!(was.and_then(|w| w.resolved()).map(|t| bm.elements[t].qualified_name.clone()));
            row["new_statement"] = json!(old.is_none());
            out.push(row);
        }
        out
    };
    let (mut dangling_conn, mut dangling_flow, mut dangling_trace) = (Vec::new(), Vec::new(), Vec::new());
    for e in am.elements.iter().filter(|e| group(e) != Group::Element) {
        let rows = lost_ends(e);
        match group(e) {
            Group::Flow => dangling_flow.extend(rows),
            Group::Trace => dangling_trace.extend(rows),
            _ => dangling_conn.extend(rows),
        }
    }
    let (carried_b, carried_a) = (connect::flow_carriers(bm), connect::flow_carriers(am));
    for (flow, conn) in &carried_b {
        let Some(x) = pair_b.get(flow) else { continue };
        if !carried_a.contains_key(x) {
            let mut row = a.statement_row(&am.elements[*x]);
            row["problem"] = json!("flow-without-connection");
            row["was_on"] = json!(bm.elements[*conn].qualified_name);
            row["connection_removed"] = json!(!pair_b.contains_key(conn));
            dangling_flow.push(row);
        }
    }
    dangling_flow.sort_by(|x, y| x["ref"].as_str().cmp(&y["ref"].as_str()));
    let (linked_b, linked_a) = (connected_ports(bm, &name_b), connected_ports(am, &name_a));
    let mut unconnected: Vec<Value> = linked_b
        .difference(&linked_a)
        .filter(|(part, port)| am.find(port).is_some() && am.find(part).is_some())
        .map(|(part, port)| {
            let mut row = json!({ "port": port, "part": part, "problem": "port-unconnected" });
            a.ids.tag(&mut row, port);
            row
        })
        .collect();
    unconnected.sort_by(|x, y| (x["part"].as_str(), x["port"].as_str()).cmp(&(y["part"].as_str(), y["port"].as_str())));

    // Coverage and the requirements whose state flipped.
    let (cov_b, cov_a) = (trace::coverage_report(bm, None).unwrap_or(Value::Null), trace::coverage_report(am, None).unwrap_or(Value::Null));
    let state = |m: &Model, v: &Value, name_of: &dyn Fn(ElementId) -> String| -> BTreeMap<String, (bool, bool)> {
        v["requirements"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let id = m.find(r["ref"].as_str()?)?.id;
                let has = |k: &str| r[k].as_array().map(|x| !x.is_empty()).unwrap_or(false);
                Some((name_of(id), (has("satisfied_by"), has("verified_by"))))
            })
            .collect()
    };
    let (st_b, st_a) = (state(bm, &cov_b, &name_b), state(am, &cov_a, &name_a));
    let mut coverage_changes: Vec<Value> = Vec::new();
    for (r, (sat, ver)) in &st_a {
        let Some((sat0, ver0)) = st_b.get(r) else { continue };
        if sat0 != sat || ver0 != ver {
            let word = |on: bool, yes: &str, no: &str| if on { yes.to_string() } else { no.to_string() };
            let mut row = json!({ "ref": r });
            if sat0 != sat {
                row["satisfied"] = json!({ "before": sat0, "after": sat });
                row["change"] = json!(format!("{} -> {}", word(*sat0, "satisfied", "unsatisfied"), word(*sat, "satisfied", "unsatisfied")));
            }
            if ver0 != ver {
                row["verified"] = json!({ "before": ver0, "after": ver });
                let t = format!("{} -> {}", word(*ver0, "verified", "unverified"), word(*ver, "verified", "unverified"));
                row["change"] = json!(match row["change"].as_str() {
                    Some(c) => format!("{c}; {t}"),
                    None => t,
                });
            }
            a.ids.tag(&mut row, r);
            coverage_changes.push(row);
        }
    }
    let totals = |v: &Value| json!({ "requirements": v["totals"]["requirements"], "satisfied": v["totals"]["satisfied"], "verified": v["totals"]["verified"], "fully_covered": v["totals"]["fully_covered"] });

    // Gates.
    let (val_b, val_a) = (trace::validate_model(bm, None).unwrap_or(Value::Null), trace::validate_model(am, None).unwrap_or(Value::Null));
    let mut known: HashMap<(String, String), usize> = HashMap::new();
    let key = |f: &Value| (f["rule"].as_str().unwrap_or("").to_string(), f["message"].as_str().unwrap_or("").to_string());
    for f in val_b["findings"].as_array().into_iter().flatten().filter(|f| f["severity"] == "error") {
        *known.entry(key(f)).or_default() += 1;
    }
    let mut new_errors: Vec<Value> = Vec::new();
    for f in val_a["findings"].as_array().into_iter().flatten().filter(|f| f["severity"] == "error") {
        match known.get_mut(&key(f)) {
            Some(n) if *n > 0 => *n -= 1,
            _ => new_errors.push(json!({ "gate": f["gate"], "rule": f["rule"], "element": f["element"], "file": f["file"], "line": f["line"], "message": f["message"], "fix": f["fix"] })),
        }
    }

    // The connection checks (port directions, port types, ends between definitions, ends the owner cannot reach).
    let (conn_b, conn_a) = (connect::check_in(bm, None).1, connect::check_in(am, None).1);
    let conn_counts = |v: &[Value]| {
        let e = v.iter().filter(|f| f["severity"] == "error").count();
        json!({ "errors": e, "warnings": v.len() - e })
    };
    let mut known: HashMap<(String, String), usize> = HashMap::new();
    for f in &conn_b {
        *known.entry(key(f)).or_default() += 1;
    }
    for f in &conn_a {
        match known.get_mut(&key(f)) {
            Some(n) if *n > 0 => *n -= 1,
            _ => new_errors.push(json!({ "gate": "connections", "rule": f["rule"], "severity": f["severity"], "element": f["element"], "file": f["file"], "line": f["line"], "message": f["message"], "fix": f["fix"] })),
        }
    }

    let n_dangling = dangling_conn.len() + dangling_flow.len() + unconnected.len();
    let counts = |g: &G| json!({ "added": g.n_added, "removed": g.n_removed, "changed": g.changed.len(), "re_ended": g.re_ended.len() });
    let summary = json!({
        "elements": { "added": elements.n_added, "removed": elements.n_removed, "changed": elements.changed.len() },
        "ports": { "added": ports.n_added, "removed": ports.n_removed, "changed": ports.changed.len() },
        "connections": counts(&conns),
        "interfaces": counts(&ifaces),
        "flows": counts(&flows),
        "dangling": { "total": n_dangling, "connections": dangling_conn.len(), "flows": dangling_flow.len(), "ports_unconnected": unconnected.len() },
        "traceability": { "added": traces.n_added, "removed": traces.n_removed, "changed": traces.changed.len() + traces.re_ended.len(), "dangling": dangling_trace.len(), "coverage_changed": coverage_changes.len() },
        "coverage": { "before": totals(&cov_b), "after": totals(&cov_a) },
        "gates": { "errors": { "before": val_b["errors"], "after": val_a["errors"] }, "warnings": { "before": val_b["warnings"], "after": val_a["warnings"] }, "new_errors": new_errors.len(), "connection_findings": { "before": conn_b.len(), "after": conn_a.len() } },
    });
    let changed = summary != empty_summary(&summary);
    let section = |name: &str, g: G, l: &mut Lists, connector: bool| -> Value {
        let mut v = json!({
            "added": l.put(&format!("{name}.added"), g.added),
            "removed": l.put(&format!("{name}.removed"), g.removed),
            "changed": l.put(&format!("{name}.changed"), g.changed),
        });
        if connector {
            v["re_ended"] = l.put(&format!("{name}.re_ended"), g.re_ended);
        }
        v
    };
    let mut trace_changed = std::mem::take(&mut traces.changed);
    trace_changed.append(&mut traces.re_ended);
    traces.changed = trace_changed;
    let mut out = json!({
        "mode": if full { "full" } else { "summary" },
        "changed": changed,
        "summary": summary,
    });
    out["elements"] = section("elements", elements, &mut l, false);
    out["ports"] = section("ports", ports, &mut l, false);
    out["connections"] = section("connections", conns, &mut l, true);
    out["interfaces"] = section("interfaces", ifaces, &mut l, true);
    out["flows"] = section("flows", flows, &mut l, true);
    out["dangling"] = json!({
        "connections": l.put("dangling.connections", dangling_conn),
        "flows": l.put("dangling.flows", dangling_flow),
        "ports_unconnected": l.put("dangling.ports_unconnected", unconnected),
    });
    let mut tr = section("traceability", traces, &mut l, false);
    tr["dangling"] = l.put("traceability.dangling", dangling_trace);
    tr["coverage_changes"] = l.put("traceability.coverage_changes", coverage_changes);
    tr["coverage"] = json!({ "before": totals(&cov_b), "after": totals(&cov_a) });
    out["traceability"] = tr;
    let (mut gb, mut ga) = (gate_counts(&val_b), gate_counts(&val_a));
    gb["connections"] = conn_counts(&conn_b);
    ga["connections"] = conn_counts(&conn_a);
    out["gates"] = json!({ "before": gb, "after": ga, "new_errors": l.put("gates.new_errors", new_errors) });
    if let Some(why) = a.hidden.as_ref().or(b.hidden.as_ref()) {
        out["not_fully_compared"] = json!(why);
    }
    out["truncated"] = json!(!l.cut.is_empty());
    out["truncated_lists"] = json!(l.cut);
    if !l.cut.is_empty() && !full {
        out["hint"] = json!("Lists are cut to a summary; the counts in summary are exact. Pass change_report: \"full\" for every row.");
    }
    out
}

/// The summary with every count that says "something changed" at zero, for the `changed` flag.
fn empty_summary(s: &Value) -> Value {
    fn zero(v: &Value) -> Value {
        match v {
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), zero(x))).collect()),
            Value::Number(_) => json!(0),
            other => other.clone(),
        }
    }
    let mut z = zero(s);
    // Totals that describe a state, not a change, stay as they are.
    z["coverage"] = s["coverage"].clone();
    z["gates"]["errors"] = s["gates"]["errors"].clone();
    z["gates"]["warnings"] = s["gates"]["warnings"].clone();
    z["gates"]["connection_findings"] = s["gates"]["connection_findings"].clone();
    z
}

/// A report for a write that changes no model text.
pub fn unchanged() -> Value {
    json!({ "mode": "summary", "changed": false, "summary": Value::Null, "truncated": false })
}

/// One line per group that has something to say, for the text of a write reply.
pub fn summary_text(r: &Value) -> String {
    let s = &r["summary"];
    // On a model whose relationship pairs were cut the counts below are of the part that was read. The text says so
    // before the counts, not only the structured reply.
    let cut = r["not_fully_compared"].as_str().map(|why| format!("Change report is NOT COMPLETE, {why}\n")).unwrap_or_default();
    if !s.is_object() || r["changed"] != true {
        return format!("{cut}Change report: no change to the model facts{}.\n", if cut.is_empty() { "" } else { " that were read" });
    }
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let mut parts: Vec<String> = Vec::new();
    for g in ["elements", "ports", "connections", "interfaces", "flows"] {
        let (a, d, c, e) = (n(&s[g]["added"]), n(&s[g]["removed"]), n(&s[g]["changed"]), n(&s[g]["re_ended"]));
        if a + d + c + e > 0 {
            parts.push(format!("{g} +{a} -{d} ~{c}{}", if e > 0 { format!(" re-ended {e}") } else { String::new() }));
        }
    }
    let t = &s["traceability"];
    if n(&t["added"]) + n(&t["removed"]) + n(&t["changed"]) + n(&t["dangling"]) + n(&t["coverage_changed"]) > 0 {
        parts.push(format!("trace links +{} -{} ~{}, dangling {}, requirements whose coverage changed {}", n(&t["added"]), n(&t["removed"]), n(&t["changed"]), n(&t["dangling"]), n(&t["coverage_changed"])));
    }
    let d = &s["dangling"];
    if n(&d["total"]) > 0 {
        parts.push(format!("DANGLING: {} connections, {} flows, {} ports left unconnected", n(&d["connections"]), n(&d["flows"]), n(&d["ports_unconnected"])));
    }
    let (cb, ca) = (&s["coverage"]["before"], &s["coverage"]["after"]);
    if cb != ca {
        parts.push(format!("coverage satisfied {}/{} -> {}/{}, verified {} -> {}", cb["satisfied"], cb["requirements"], ca["satisfied"], ca["requirements"], cb["verified"], ca["verified"]));
    }
    let g = &s["gates"];
    if g["errors"]["before"] != g["errors"]["after"] || g["warnings"]["before"] != g["warnings"]["after"] || g["connection_findings"]["before"] != g["connection_findings"]["after"] || n(&g["new_errors"]) > 0 {
        parts.push(format!(
            "gates (syntax, semantic, traceability only; get_change_package counts every gate) errors {} -> {}, warnings {} -> {}, connection findings {} -> {} ({} new findings listed)",
            g["errors"]["before"], g["errors"]["after"], g["warnings"]["before"], g["warnings"]["after"], g["connection_findings"]["before"], g["connection_findings"]["after"], g["new_errors"]
        ));
    }
    format!("{cut}Change report{}: {}.\n", if cut.is_empty() { "" } else { " (of the part that was read)" }, parts.join("; "))
}
