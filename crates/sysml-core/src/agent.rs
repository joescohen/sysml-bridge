//! Agent interface logic: structured reads, intent-level writes, traceability
//! and connection checks. Everything here is a pure function over the model
//! or over a set of files, so the server only adds storage and transport.

pub mod allocation;
pub mod budget;
pub mod capture;
pub mod change;
pub mod connect;
pub mod conn_index;
pub mod connections;
pub mod coverage_history;
pub mod csv_sync;
pub mod diff;
pub mod duplicates;
pub mod element_links;
pub mod expr;
pub mod find;
pub mod fold;
pub mod grammar;
pub mod graph;
pub mod icd;
pub mod idrefs;
pub mod ids;
pub mod impact;
pub mod implied;
pub mod intent;
pub mod merge3;
pub mod mint;
pub mod model_props;
pub mod move_part;
pub mod numbering;
pub mod order;
pub mod ownership;
pub mod package;
pub mod plan;
pub mod rekey;
pub mod relmap;
pub mod reqif;
pub mod reqimport;
pub mod results;
pub mod rules;
mod spec;
pub mod suspect;
pub mod text;
pub mod tool_schema;
pub mod tools;
pub mod tools_gen;
pub mod tools_suspect;
pub mod trace;
pub mod trace_props;
pub mod units;
pub mod usages;

use crate::api::{self, FileIn};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// A tool failure: a message that says what to do next, plus optional structured detail.
#[derive(Debug)]
pub struct Fail {
    pub message: String,
    pub data: Value,
}

impl Fail {
    pub fn with(message: impl Into<String>, data: Value) -> Self {
        Fail { message: message.into(), data }
    }
}

impl From<String> for Fail {
    fn from(message: String) -> Self {
        Fail { message, data: Value::Null }
    }
}

impl From<&str> for Fail {
    fn from(message: &str) -> Self {
        Fail { message: message.to_string(), data: Value::Null }
    }
}

pub type Res<T> = Result<T, Fail>;

/// Stable revision of a text: FNV-1a 64 over the bytes, as 16 hex digits.
pub fn revision_of(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Revision of a whole file set. Independent of the order of `files`.
pub fn project_revision(files: &[FileIn]) -> String {
    let mut parts: Vec<String> = files.iter().map(|f| format!("{}\u{0}{}", f.path, revision_of(&f.text))).collect();
    parts.sort();
    revision_of(&parts.join("\n"))
}

/// True when two names are equal or one typo apart (two for long names).
fn close(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let max = if a.len().max(b.len()) <= 5 { 1 } else { 2 };
    if a.len().abs_diff(b.len()) > max {
        return false;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur.push(sub.min(prev[j] + 1).min(cur[j - 1] + 1));
        }
        prev = cur;
    }
    prev[b.len()] <= max
}

/// A qualified name for a message: in quotes, unless its own quotes already show where each name starts and ends.
pub fn qshow(qn: &str) -> String {
    let qn = crate::agent::text::model_name(qn);
    if qn.contains('\'') { qn } else { format!("'{qn}'") }
}

/// A name resolved to an element, with the text to write for it.
pub struct Located<'a> {
    pub el: &'a Element,
    /// What a statement should say: the qualified name, or the feature chain as the caller wrote it.
    pub text: String,
    /// The element itself, or for a chain the last element found by exact name. Statements go beside it.
    pub anchor: &'a Element,
}

/// Member `name` of `cur`: declared in it, or inherited through its types and supertypes.
pub(crate) fn member_named<'a>(model: &'a Model, cur: &'a Element, name: &str, seen: &mut BTreeSet<ElementId>) -> Option<&'a Element> {
    if !seen.insert(cur.id) || seen.len() > 64 {
        return None;
    }
    if let Some(m) = model.children(cur.id).find(|c| c.name.as_deref() == Some(name)) {
        return Some(m);
    }
    cur.refs
        .iter()
        .filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines))
        .filter_map(|r| r.resolved())
        .find_map(|t| member_named(model, &model.elements[t], name, seen))
}

/// `Top::usage::member`: the longest exact prefix, then members found through typing and specialization.
fn locate_chain<'a>(model: &'a Model, name: &str) -> Option<Located<'a>> {
    let segs: Vec<&str> = name.split("::").map(str::trim).collect();
    for k in (1..segs.len()).rev() {
        let Some(anchor) = model.find(&segs[..k].join("::")) else { continue };
        if let Some(el) = chain_end(model, anchor, &segs[k..]) {
            return Some(Located { el, text: canonical_name(&segs.join("::")).unwrap_or_else(|| el.qualified_name.clone()), anchor });
        }
    }
    None
}

/// Walk `rest` from `from`; `None` when a step has no member.
fn chain_end<'a>(model: &'a Model, from: &'a Element, rest: &[&str]) -> Option<&'a Element> {
    let mut cur = from;
    for seg in rest {
        cur = member_named(model, cur, seg.trim_matches('\''), &mut BTreeSet::new())?;
    }
    Some(cur)
}

/// The elements a chain passes through after `from`, one per step; stops at the first step with no member.
pub(crate) fn chain_steps<'a>(model: &'a Model, from: &'a Element, rest: &[&str]) -> Vec<&'a Element> {
    let mut cur = from;
    let mut out = Vec::new();
    for seg in rest {
        match member_named(model, cur, seg.trim_matches('\''), &mut BTreeSet::new()) {
            Some(m) => {
                out.push(m);
                cur = m;
            }
            None => break,
        }
    }
    out
}

/// Find an element by qualified name, or fail with near matches.
pub fn locate<'a>(model: &'a Model, name: &str) -> Res<&'a Element> {
    locate_ref(model, name).map(|l| l.el)
}

/// Like `locate`, and also accepts a feature chain through typing: `Top::usage::member`
/// where `member` belongs to the definition that types `usage` (or to one of its supertypes).
///
/// This is the one resolver of every tool argument that names an element. It accepts, in this order:
///
/// 1. the qualified name, with or without the quotes of a segment, and a feature chain (`locate_strict`);
/// 2. an id, which is the short name of an element (`R-4`, `'R-4'`, `<R-4>`), when one element has it;
/// 3. a path in which a segment is an id (`Requirements::'R-4'`), and a path without the one root package every name of
///    the model starts with (`station::arm`, as the tables print it);
/// 4. the forms the tools print: an id with the name (`R-4 swapTime`) and a name with its owner
///    (`swapStatus in SwapStatusPort`).
///
/// A form that names more than one element is refused, and the refusal lists the candidates.
pub fn locate_ref<'a>(model: &'a Model, name: &str) -> Res<Located<'a>> {
    locate_ref_as(model, name, true)
}

/// `locate_ref` for an end of a statement a write makes: every form but the path without the root package. A name
/// written below the root is read there from the owner of the statement outward, as the parser reads it, and without
/// an owner it is not a name.
pub(crate) fn locate_end_ref<'a>(model: &'a Model, name: &str) -> Res<Located<'a>> {
    locate_ref_as(model, name, false)
}

fn locate_ref_as<'a>(model: &'a Model, name: &str, below_root: bool) -> Res<Located<'a>> {
    let strict = locate_strict(model, name);
    let Err(fail) = strict else { return strict };
    match locate_other(model, name.trim(), below_root) {
        Ok(Some(e)) => Ok(Located { el: e, text: e.qualified_name.clone(), anchor: e }),
        Ok(None) => Err(fail),
        Err(ambiguous) => Err(ambiguous),
    }
}

fn unquoted(s: &str) -> &str {
    let s = s.trim();
    let s = s.strip_prefix('<').and_then(|x| x.strip_suffix('>')).unwrap_or(s).trim();
    s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).unwrap_or(s)
}

/// The refusal for a reference that names several elements: it lists them, so the caller can pass one.
fn ambiguous(name: &str, what: &str, hits: &[&Element]) -> Fail {
    let list: Vec<String> = hits.iter().take(10).map(|e| format!("{} ({})", e.qualified_name, e.keyword())).collect();
    Fail::with(
        format!(
            "{} is {what} of {} elements, so it does not say which one: {}{}. Fix: pass the qualified name of the one you mean.",
            qshow(name),
            hits.len(),
            list.join("; "),
            if hits.len() > 10 { format!("; and {} more", hits.len() - 10) } else { String::new() }
        ),
        json!({ "ref": name, "ambiguous": true, "candidates": hits.iter().take(50).map(|e| json!({ "ref": e.qualified_name, "kind": e.keyword(), "id": e.short_name, "name": e.name })).collect::<Vec<_>>() }),
    )
}

/// The elements a path names when a segment may be an id (short name) as well as a name, and when the path may start
/// below the one root package.
fn walk_path<'a>(model: &'a Model, segs: &[&str], below_root: bool) -> Vec<&'a Element> {
    let is = |e: &Element, seg: &str| !e.implied_name && (e.name.as_deref() == Some(seg) || e.short_name.as_deref() == Some(seg));
    let roots: Vec<&Element> = model.roots.iter().map(|r| &model.elements[*r]).collect();
    let packages: Vec<&&Element> = roots.iter().filter(|e| e.kind == "package").collect();
    let mut starts: Vec<&Element> = roots.iter().copied().filter(|e| is(e, segs[0])).collect();
    if let ([root], true) = (packages.as_slice(), below_root) {
        starts.extend(model.children(root.id).filter(|e| is(e, segs[0])));
    }
    let mut at = starts;
    for seg in &segs[1..] {
        let mut next: Vec<&Element> = Vec::new();
        for e in &at {
            next.extend(model.children(e.id).filter(|c| is(c, seg)));
        }
        at = next;
        if at.is_empty() {
            break;
        }
    }
    let mut seen = BTreeSet::new();
    at.into_iter().filter(|e| seen.insert(e.id)).collect()
}

/// Forms 2 to 4 of `locate_ref`. `Ok(None)`: the text is none of them. `Err`: it is one, and names several elements.
fn locate_other<'a>(model: &'a Model, name: &str, below_root: bool) -> Res<Option<&'a Element>> {
    if name.is_empty() {
        return Ok(None);
    }
    let one = |hits: Vec<&'a Element>, what: &str| -> Res<Option<&'a Element>> {
        match hits.as_slice() {
            [] => Ok(None),
            [e] => Ok(Some(*e)),
            many => Err(ambiguous(name, what, many)),
        }
    };
    // 2. An id.
    let id = unquoted(name);
    if !id.is_empty() {
        let hits: Vec<&Element> = model.elements.iter().filter(|e| e.short_name.as_deref() == Some(id)).collect();
        if let Some(e) = one(hits, "the id (short name)")? {
            return Ok(Some(e));
        }
    }
    // 3. A path with an id in it, or a path below the root package.
    let segs: Vec<&str> = split_path(name);
    if !segs.is_empty() && segs.iter().all(|s| !s.is_empty()) {
        if let Some(e) = one(walk_path(model, &segs, below_root), "a path to each")? {
            return Ok(Some(e));
        }
    }
    // A root package, then an id of an element anywhere below it (`Model::'R-4'`).
    if let [root, id] = segs.as_slice() {
        if let Some(r) = model.roots.iter().map(|r| &model.elements[*r]).find(|r| r.name.as_deref() == Some(*root)) {
            let prefix = format!("{}::", r.qualified_name);
            let hits: Vec<&Element> = model.elements.iter().filter(|e| e.short_name.as_deref() == Some(unquoted(id)) && e.qualified_name.starts_with(&prefix)).collect();
            if let Some(e) = one(hits, "the id (short name) below that package")? {
                return Ok(Some(e));
            }
        }
    }
    // 4. "R-4 swapTime": an id, then the name the tools print after it.
    if let Some((first, rest)) = name.split_once(char::is_whitespace) {
        let rest = rest.trim();
        let hits: Vec<&Element> = model.elements.iter().filter(|e| e.short_name.as_deref() == Some(unquoted(first))).filter(|e| e.name.as_deref() == Some(unquoted(rest)) || e.qualified_name.ends_with(rest) || split_path(rest).last().map(|l| e.name.as_deref() == Some(*l)).unwrap_or(false)).collect();
        if let Some(e) = one(hits, "the id and name")? {
            return Ok(Some(e));
        }
    }
    // 4. "swapStatus in SwapStatusPort": a name, then the owner it is printed with.
    if let Some(i) = name.rfind(" in ") {
        let (n, owner) = (unquoted(&name[..i]), name[i + 4..].trim());
        let owner_last = split_path(owner).last().copied().unwrap_or("");
        let owner_el = locate_strict(model, owner).ok().map(|l| l.el.id).or_else(|| match walk_path(model, &split_path(owner), below_root).as_slice() {
            [e] => Some(e.id),
            _ => None,
        });
        let hits: Vec<&Element> = model
            .elements
            .iter()
            .filter(|e| !e.implied_name && (e.name.as_deref() == Some(n) || e.short_name.as_deref() == Some(n)))
            .filter(|e| match (e.owner, owner_el) {
                (Some(o), Some(want)) => o == want,
                (Some(o), None) => model.elements[o].name.as_deref() == Some(owner_last),
                (None, _) => false,
            })
            .collect();
        if let Some(e) = one(hits, "a name in that owner")? {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

/// The segments of a path, without their quotes. A `::` inside a quoted name does not split.
pub(crate) fn split_path(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut quoted) = (0usize, false);
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if quoted => i += 1,
            b'\'' => quoted = !quoted,
            b':' if !quoted && b.get(i + 1) == Some(&b':') => {
                out.push(unquoted(&text[start..i]));
                start = i + 2;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(unquoted(text.get(start..).unwrap_or("")));
    out
}

/// The qualified name of an element, or a feature chain through typing, exactly as it was read before the other forms
/// of `locate_ref` existed. A write that looks a name up from a scope outward tries this first, so a name written
/// relative to the scope still means what it meant.
pub fn locate_strict<'a>(model: &'a Model, name: &str) -> Res<Located<'a>> {
    let name = name.trim();
    if name.len() > 2000 {
        return Err(Fail::from(format!("name of {} bytes is too long to be an element name; at most 2000. Fix: pass the qualified name of an element.", name.len())));
    }
    let whole = |e: &'a Element| Located { el: e, text: e.qualified_name.clone(), anchor: e };
    if let Some(e) = model.find(name) {
        return Ok(whole(e));
    }
    // Names with spaces need quotes in SysML text; accept the unquoted form when it is unambiguous.
    let bare = |q: &str| q.split("::").map(|s| s.trim().trim_matches('\'')).collect::<Vec<_>>().join("::");
    let want = bare(name);
    let mut same = model.elements.iter().filter(|e| bare(&e.qualified_name) == want);
    if let (Some(e), None) = (same.next(), same.next()) {
        return Ok(whole(e));
    }
    if let Some(l) = locate_chain(model, name) {
        return Ok(l);
    }
    if let Some(f) = missing_member(model, name) {
        return Err(f);
    }
    let last = name.rsplit("::").next().unwrap_or(name).trim_matches('\'').to_lowercase();
    let near: Vec<&str> = model
        .elements
        .iter()
        .filter(|e| !e.implied_name && e.name.as_ref().map(|n| close(&n.to_lowercase(), &last)).unwrap_or(false))
        .take(5)
        .map(|e| e.qualified_name.as_str())
        .collect();
    let hint = if near.is_empty() { String::new() } else { format!(" Did you mean: {}?", near.iter().map(|n| crate::agent::text::model_name(n)).collect::<Vec<_>>().join(", ")) };
    Err(Fail::with(
        format!("no element named {}.{hint} Use query_elements {{name}} to find the qualified name.", crate::agent::text::quote_clip(name)),
        json!({ "ref": name, "near": near }),
    ))
}

/// Every member of `cur` a chain can name: declared in it, or inherited through its types and supertypes.
fn members_through<'a>(model: &'a Model, cur: &'a Element) -> Vec<&'a Element> {
    let mut seen: BTreeSet<ElementId> = BTreeSet::new();
    let mut stack = vec![cur.id];
    let mut out: Vec<&Element> = Vec::new();
    while let Some(x) = stack.pop() {
        if !seen.insert(x) || seen.len() > 64 {
            continue;
        }
        let e = &model.elements[x];
        out.extend(model.children(x).filter(|c| !c.implied_name && c.name.is_some()));
        stack.extend(e.refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines)).filter_map(|r| r.resolved()));
    }
    out
}

/// The refusal for a feature chain whose front part is an element of the model and whose next step is not a member of it:
/// `Pkg::'gPS Satellite'::p1` where the part resolves and its type has no `p1` yet. It says what is missing and offers only
/// names that element has. `None` when no front part resolves, so the caller gives the general refusal.
fn missing_member(model: &Model, name: &str) -> Option<Fail> {
    let segs: Vec<&str> = name.split("::").map(str::trim).collect();
    for k in (1..segs.len()).rev() {
        let Some(anchor) = model.find(&segs[..k].join("::")) else { continue };
        let steps = chain_steps(model, anchor, &segs[k..]);
        let at = k + steps.len();
        if at >= segs.len() {
            continue;
        }
        let holder = steps.last().copied().unwrap_or(anchor);
        let missing = segs[at].trim_matches('\'');
        let have = members_through(model, holder);
        let low = missing.to_lowercase();
        let mut near: Vec<&Element> = have.iter().copied().filter(|m| m.name.as_deref().is_some_and(|n| close(&n.to_lowercase(), &low))).collect();
        if near.is_empty() {
            near = have.iter().copied().take(8).collect();
        }
        near.truncate(5);
        let typed = holder.refs_of(RefRole::Type).next().map(|r| r.text.clone());
        if typed.is_none() {
            continue;
        }
        let names: Vec<String> = near.iter().map(|m| m.qualified_name.clone()).collect();
        let tables = crate::agent::text::model_name;
        let held = tables(&holder.qualified_name);
        let listing = if names.is_empty() { format!("'{held}' has no members at all") } else { format!("members of '{held}' are: {}", names.iter().map(|n| tables(n)).collect::<Vec<_>>().join(", ")) };
        let (qmissing, qname) = (crate::agent::text::quote_clip(missing), crate::agent::text::quote_clip(name));
        let where_ = format!("its type {} has no member {qmissing} yet", tables(typed.as_deref().unwrap_or("")));
        return Some(Fail::with(
            format!(
                "no element named {qname}: '{held}' resolves, but {where_} ({listing}). Fix: declare {qmissing} there first (for a port: create_element {{owner: <the type>, kind: 'port', name: {qmissing}, typed_by: <port def>}}), then pass this end again."
            ),
            json!({ "ref": name, "resolved": holder.qualified_name, "missing": missing, "near": names }),
        ));
    }
    None
}

thread_local! {
    static NAMESPACE_STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Elements `add_namespace_checks` compared on this thread since the last call, for a test that counts work.
pub fn take_namespace_steps() -> u64 {
    NAMESPACE_STEPS.with(|w| w.replace(0))
}

/// Add the checks the parser cannot make: one namespace declared twice (a package split over
/// several declarations or files) must not hold two members of the same name.
pub fn add_namespace_checks(model: &mut Model) {
    let mut seen: BTreeMap<&str, Vec<ElementId>> = BTreeMap::new();
    for e in &model.elements {
        if !e.implied_name && e.name.is_some() {
            seen.entry(e.qualified_name.as_str()).or_default().push(e.id);
        }
    }
    let mut extra = Vec::new();
    for ids in seen.values().filter(|v| v.len() > 1) {
        // One pass. For each element the first earlier one with another owner is wanted, and whether an earlier one has
        // the same owner. The first element of the list, and the first with another owner than it, answer the first
        // question for every owner; they are kept twice, once for elements that are not packages (two packages of one
        // name are one namespace, not a clash).
        let mut owners: HashSet<Option<ElementId>> = HashSet::new();
        let mut firsts: [[Option<ElementId>; 2]; 2] = [[None; 2]; 2];
        for id in ids {
            NAMESPACE_STEPS.with(|w| w.set(w.get() + 1));
            let e = &model.elements[*id];
            let [first, other] = firsts[(e.kind == "package") as usize];
            let clash = [first, other].into_iter().flatten().map(|p| &model.elements[p]).find(|p| p.owner != e.owner);
            let same_owner = !owners.insert(e.owner);
            if let (Some(p), false) = (clash, same_owner) {
                let (line, _) = model.line_col(p.file, p.span.start);
                extra.push(Diagnostic {
                    file: e.file,
                    span: e.name_span.unwrap_or(e.span),
                    severity: Severity::Warning,
                    message: format!("duplicate name '{}' in this namespace (also declared at {}:{line})", e.name.clone().unwrap_or_default(), model.files[p.file].path),
                });
            }
            // `firsts[0]` is read by an element that is not a package: every earlier element counts. `firsts[1]` is read
            // by a package: only earlier elements that are not packages count.
            for (slot, counts) in [(0, true), (1, e.kind != "package")] {
                if !counts {
                    continue;
                }
                match firsts[slot] {
                    [None, _] => firsts[slot][0] = Some(*id),
                    [Some(f), None] if model.elements[f].owner != e.owner => firsts[slot][1] = Some(*id),
                    _ => {}
                }
            }
        }
    }
    model.diagnostics.extend(extra);
}

pub fn line_of(model: &Model, e: &Element) -> usize {
    model.line_col(e.file, e.span.start).0
}

/// Short description of an element for lists.
pub fn brief(model: &Model, e: &Element) -> Value {
    json!({
        "ref": e.qualified_name,
        "name": e.name,
        "kind": e.keyword(),
        "file": model.files[e.file].path,
        "line": line_of(model, e),
        "owner": e.owner.map(|o| model.elements[o].qualified_name.clone()),
    })
}

fn error_count(model: &Model) -> usize {
    model.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
}

pub fn diagnostics_summary(model: &Model, sample: usize) -> Value {
    let errors = error_count(model);
    let all = api::diagnostics_out(model);
    json!({
        "errors": errors,
        "warnings": model.diagnostics.len() - errors,
        "items": all.iter().take(sample).collect::<Vec<_>>(),
    })
}

pub fn project_state(model: &Model) -> Value {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &model.elements {
        *counts.entry(e.keyword()).or_default() += 1;
    }
    let mut per_file = vec![0usize; model.files.len()];
    for e in &model.elements {
        per_file[e.file] += 1;
    }
    let files: Vec<Value> = model
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| json!({ "path": f.path, "lines": f.text.lines().count(), "elements": per_file[i], "revision": revision_of(&f.text) }))
        .collect();
    let file_ins: Vec<FileIn> = model.files.iter().map(|f| FileIn { path: f.path.clone(), text: f.text.clone() }).collect();
    json!({
        "revision": project_revision(&file_ins),
        "element_count": model.elements.len(),
        "counts_by_kind": counts,
        "files": files,
        "diagnostics": diagnostics_summary(model, 10),
    })
}

pub struct Query<'a> {
    pub kind: Option<&'a str>,
    pub name: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub typed_by: Option<&'a str>,
    pub limit: usize,
}

pub fn query_elements(model: &Model, q: &Query) -> Res<Value> {
    let owner = q.owner.filter(|s| !s.is_empty()).map(|o| locate(model, o)).transpose()?.map(|e| e.id);
    let typed = q.typed_by.filter(|s| !s.is_empty()).map(|o| locate(model, o)).transpose()?.map(|e| e.id);
    let name = q.name.map(str::to_lowercase).filter(|s| !s.is_empty());
    let kind = q.kind.filter(|s| !s.is_empty());
    let limit = q.limit.clamp(1, 500);
    let hits: Vec<&Element> = model
        .elements
        .iter()
        // Statements such as `satisfy` have no declared name; they show up only when asked for by kind.
        .filter(|e| kind.is_some() || (e.name.is_some() && !e.implied_name))
        .filter(|e| kind.map(|k| e.keyword() == k || e.kind == k).unwrap_or(true))
        // The name, or the id (short name): a requirement is asked for by `R-4` as often as by its name.
        .filter(|e| name.as_ref().map(|n| e.name.as_ref().map(|x| x.to_lowercase().contains(n)).unwrap_or(false) || e.short_name.as_ref().map(|x| x.to_lowercase().contains(n)).unwrap_or(false)).unwrap_or(true))
        .filter(|e| owner.map(|o| e.owner == Some(o)).unwrap_or(true))
        .filter(|e| typed.map(|t| e.refs_of(RefRole::Type).any(|r| r.resolved() == Some(t))).unwrap_or(true))
        .collect();
    let rows: Vec<Value> = hits
        .iter()
        .take(limit)
        .map(|e| {
            let mut b = brief(model, e);
            b["typed_by"] = json!(e.refs_of(RefRole::Type).map(|r| r.text.clone()).collect::<Vec<_>>());
            if let Some(id) = &e.short_name {
                b["id"] = json!(id);
            }
            b
        })
        .collect();
    Ok(json!({ "total": hits.len(), "returned": rows.len(), "truncated": hits.len() > rows.len(), "elements": rows }))
}

/// Elements whose references point at `target` or at anything inside it.
pub fn referrers(model: &Model, target: ElementId, include_inside: bool) -> Vec<(ElementId, RefRole)> {
    let mut inside: BTreeSet<ElementId> = model.descendants(target).into_iter().collect();
    inside.insert(target);
    let mut out = Vec::new();
    for o in &model.elements {
        if !include_inside && inside.contains(&o.id) {
            continue;
        }
        let hit = o.refs.iter().find(|r| r.segments.iter().any(|s| s.resolved.map(|x| inside.contains(&x)).unwrap_or(false)));
        if let Some(r) = hit {
            out.push((o.id, r.role));
        }
    }
    out
}

/// Full detail of one element, with every referrer (optionally limited by keyword).
pub fn element_details(model: &Model, name: &str, keyword: Option<&str>) -> Res<Value> {
    let e = locate(model, name)?;
    let mut out = serde_json::to_value(api::one_element(model, e)).unwrap();
    let src = model.files.get(e.file).and_then(|f| f.text.get(e.span.start..e.span.end)).unwrap_or("");
    out["source"] = json!(if src.len() > 6000 {
        // Cut at a character boundary at or before byte 6000.
        let cut = (0..=6000).rev().find(|i| src.is_char_boundary(*i)).unwrap_or(0);
        format!("{}\n... (truncated)", &src[..cut])
    } else {
        src.to_string()
    });
    let kw = keyword.filter(|k| !k.is_empty());
    let all: Vec<(ElementId, RefRole)> = referrers(model, e.id, true).into_iter().filter(|(o, _)| *o != e.id).collect();
    let rows: Vec<Value> = all
        .iter()
        .filter(|(o, _)| kw.map(|k| model.elements[*o].keyword() == k || model.elements[*o].kind == k).unwrap_or(true))
        .map(|(o, role)| {
            let mut b = brief(model, &model.elements[*o]);
            b["role"] = json!(role);
            b
        })
        .collect();
    out["referencedBy"] = json!(rows);
    out["referencedByCount"] = json!(rows.len());
    out["referencedByTotal"] = json!(all.len());
    Ok(out)
}

/// The position of every element among the members of its owner, counting every statement kind together (the number
/// after `$` in an unnamed member's ref is this plus one). A top-level element counts among the top-level elements
/// of its own file. Sorting a list of members by it gives the order in which the text states them.
pub fn member_index(model: &Model) -> Vec<usize> {
    let mut index = vec![0usize; model.elements.len()];
    for e in &model.elements {
        for (i, c) in e.children.iter().enumerate() {
            index[*c] = i;
        }
    }
    let mut per_file = vec![0usize; model.files.len()];
    for r in &model.roots {
        let f = model.elements[*r].file;
        index[*r] = per_file[f];
        per_file[f] += 1;
    }
    index
}

/// Text of the whole model, or of one subtree with the references that leave it.
/// Every named element and every resolved edge, for comparing two models as plain facts.
pub fn export_facts(model: &Model, scope: Option<&str>) -> Res<Value> {
    let root = scope.map(|s| locate(model, s)).transpose()?;
    let inside = |id: ElementId| match root {
        Some(r) => std::iter::successors(Some(id), |i| model.elements[*i].owner).any(|i| i == r.id),
        None => true,
    };
    // Every element is listed. Members without a declared name are listed by their positional ref (`Owner::$3`)
    // and flagged `unnamed`; statements named after what they point at (`satisfy R`) are flagged `implied_name`.
    // One kind is left out, and `not_listed` says how many: the text of a `doc` is the `doc` field of its owner.
    // An `import` is listed: its `target` is the text create_element {kind: 'import'} takes.
    let reason = |k: &str| match k {
        "doc" => Some("documentation text; it is the 'doc' field of its owner"),
        _ => None,
    };
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut elements: Vec<Value> = Vec::new();
    let index = member_index(model);
    for e in model.elements.iter().filter(|e| inside(e.id)) {
        if reason(e.kind.as_str()).is_some() && !e.is_def {
            *skipped.entry(e.kind.clone()).or_default() += 1;
            continue;
        }
        let mut b = brief(model, e);
        b["index"] = json!(index[e.id]);
        if e.name.is_none() || e.implied_name {
            b["unnamed"] = json!(true);
        }
        if let Some(v) = &e.short_name {
            b["short_name"] = json!(v);
        }
        if !e.prefixes.is_empty() {
            b["prefixes"] = json!(e.prefixes);
        }
        if let Some(v) = &e.direction {
            b["direction"] = json!(v);
        }
        if let Some(v) = &e.multiplicity {
            b["multiplicity"] = json!(v);
        }
        if let Some(v) = &e.value {
            b["value"] = json!(v);
        }
        if let Some(d) = &e.doc {
            b["doc"] = json!(d);
        } else if let Some(d) = model.children(e.id).find(|c| c.kind == "doc").and_then(|c| c.doc.clone()) {
            b["doc"] = json!(d);
        }
        // Typing, specialization, redefinition and reference subsetting, each as the text the write tools take.
        let mut listed = |key: &str, role: RefRole| {
            let v: Vec<String> = e.refs_of(role).map(|r| tool_text(model, r)).collect();
            if !v.is_empty() {
                b[key] = json!(v);
            }
        };
        if !matches!(e.kind.as_str(), "flow" | "message") {
            listed("typed_by", RefRole::Type);
        } else {
            listed("of", RefRole::Type);
        }
        listed("specializes", RefRole::Specializes);
        listed("redefines", RefRole::Redefines);
        listed("references", RefRole::References);
        match e.kind.as_str() {
            "alias" => {
                if let Some(r) = e.refs_of(RefRole::Target).next() {
                    b["target"] = json!(marker_or(r, tool_text(model, r)));
                }
            }
            // What a comment or an annotation is about (`@Tag about a, b;`).
            "comment" | "metadata" => {
                let about: Vec<String> = e.refs_of(RefRole::Other).map(|r| tool_text(model, r)).collect();
                if !about.is_empty() {
                    b["about"] = json!(about);
                }
            }
            "import" => {
                if let Some(r) = e.refs_of(RefRole::Import).next() {
                    let wild = e.props.get("wildcard").map(String::as_str).unwrap_or("");
                    // The qualified name when it resolves, then the `::*` or `::**` as written.
                    let target = match r.resolved() {
                        Some(t) if r.segments.iter().all(|s| s.resolved.is_some()) => {
                            let base = model.elements[t].qualified_name.clone();
                            if wild.is_empty() { base } else { format!("{base}::{wild}") }
                        }
                        _ => r.text.clone(),
                    };
                    b["target"] = json!(target);
                }
                if e.props.get("all").map(String::as_str) == Some("true") {
                    b["all"] = json!(true);
                }
            }
            "textual representation" => {
                if let Some(v) = e.props.get("language") {
                    b["language"] = json!(v);
                }
                if let Some(v) = e.props.get("body") {
                    b["body"] = json!(v);
                }
            }
            _ => {}
        }
        for k in ["trigger", "guard", "effect"] {
            if matches!(e.kind.as_str(), "transition" | "succession") {
                if let Some(v) = e.props.get(k) {
                    b[k] = json!(v);
                }
            }
        }
        // What the text of the value, trigger, guard and effect names: each is a reference, renamed and counted as one.
        let named: Vec<Value> = e
            .refs
            .iter()
            .filter(|r| r.role == RefRole::Expr || (r.role == RefRole::Call && r.resolved().is_some()))
            .map(|r| json!({ "text": r.text, "ref": r.resolved().map(|t| model.elements[t].qualified_name.clone()) }))
            .collect();
        if !named.is_empty() {
            b["names"] = json!(named);
        }
        // The operations a call action calls: the importer writes `comment calledOperation about <operation>` in the action.
        let calls: Vec<String> = model
            .children(e.id)
            .filter(|c| c.kind == "comment" && c.name.as_deref() == Some("calledOperation"))
            .flat_map(|c| c.refs_of(RefRole::Other).map(|r| r.resolved().map(|t| model.elements[t].qualified_name.clone()).unwrap_or_else(|| r.text.clone())))
            .collect();
        if !calls.is_empty() {
            b["calls"] = json!(calls);
        }
        // A call operation action (`perform action c ::> part.op`): the operation, the part its chain names, the chain.
        if let Some(c) = allocation::call_of(model, e.id) {
            b["calls"] = json!([model.elements[c.op].qualified_name]);
            let target = c.part.or_else(|| model.elements[c.op].owner);
            b["target"] = json!(target.map(|t| model.elements[t].qualified_name.clone()));
            b["chain"] = json!(allocation::chain_text(model, e.id));
        } else if allocation::is_call_site(model, e.id) {
            b["chain"] = json!(allocation::chain_text(model, e.id));
        }
        // An accept or a send action: a step of an activity that is not a call (a step may be one of these besides a call).
        if !e.is_def && allocation::is_accept_or_send(e) {
            let send = e.kind == "send action" || e.props.get("node").map(String::as_str) == Some("send");
            b["node"] = json!(if send { "send" } else { "accept" });
        }
        // The ends of a connector, flow, transition, succession or binding, in the form the write tools take.
        let owner_qn = e.owner.map(|o| model.elements[o].qualified_name.clone());
        let end = |role: RefRole| {
            e.refs_of(role).next().map(|r| {
                let mut path: Vec<Value> = owner_qn.iter().map(|o| json!(o)).collect();
                // Leading names of the owner or of its owners (a qualified name written from the top) add nothing.
                let up: BTreeSet<ElementId> = e.owner.map(|o| std::iter::successors(Some(o), |i| model.elements[*i].owner).collect()).unwrap_or_default();
                let skip = r.segments.iter().take_while(|s| s.resolved.map(|i| up.contains(&i)).unwrap_or(false)).count();
                path.extend(r.segments.iter().skip(skip).map(|s| json!(s.resolved.map(|i| model.elements[i].qualified_name.clone()).unwrap_or_else(|| s.name.clone()))));
                // `start` and `done` stay bare: succession, transition and allocate take them bare. When an element
                // named start exists in scope (an alias, say), the name that resolves is printed instead.
                let mut rf = end_ref_text(model, r);
                if matches!(rf.as_str(), "start" | "done") && !matches!(e.kind.as_str(), "succession" | "transition") {
                    if let Some(full) = r.segments.first().and_then(|s| s.resolved).map(|i| model.elements[i].qualified_name.clone()) {
                        rf = full;
                    }
                }
                json!({ "text": r.text, "ref": rf, "path": path })
            })
        };
        if let (Some(s), Some(t)) = (end(RefRole::Source), end(RefRole::Target)) {
            b["ends"] = json!({ "source": s, "target": t });
        }
        elements.push(b);
    }
    let not_listed: Vec<Value> = skipped.iter().map(|(k, n)| json!({ "kind": k, "count": n, "reason": reason(k).unwrap_or("") })).collect();
    let (every_edge, cuts) = graph::edges_cut(model);
    let edges: Vec<Value> = every_edge
        .iter()
        .filter(|(_, a, b)| inside(*a) || inside(*b))
        .map(|(s, a, b)| {
            // `declared_at` names the statement that declares the edge: its ref is the key for a source id.
            let mut at = graph::declared_at(model, s.via);
            at["index"] = json!(index[s.via]);
            json!({ "kind": s.kind, "source": model.elements[*a].qualified_name, "target": model.elements[*b].qualified_name, "index": index[s.via], "declared_at": at })
        })
        .collect();
    let mut facts = json!({ "elements": elements, "edges": edges, "not_listed": not_listed });
    graph::mark_cut(model, &mut facts, &cuts);
    Ok(facts)
}

/// `start` and `done` as written: the markers of a behavior, which a statement may name whatever they resolve to.
fn marker_or(r: &Reference, otherwise: String) -> String {
    match r.text.trim() {
        t @ ("start" | "done") if r.segments.len() == 1 => t.to_string(),
        _ => otherwise,
    }
}

/// A reference as the write tools take it: the qualified name when it resolves, else the text as written;
/// `~` first for a conjugated port type.
fn tool_text(model: &Model, r: &Reference) -> String {
    let base = match r.resolved() {
        Some(t) if r.segments.iter().all(|s| s.resolved.is_some()) => model.elements[t].qualified_name.clone(),
        _ => r.text.clone(),
    };
    if r.conjugated { format!("~{base}") } else { base }
}

/// An end of a statement as a chain that `locate_ref` takes: the qualified name of the first usage it names,
/// then each later step by name (`Pkg::car::engine::power`). `start` and `done` stay as written.
fn end_ref_text(model: &Model, r: &Reference) -> String {
    if let t @ ("start" | "done") = r.text.trim() {
        if r.segments.len() == 1 {
            return t.to_string();
        }
    }
    let Some(first) = r.segments.first().and_then(|s| s.resolved) else { return r.text.clone() };
    let (mut cur, mut prev) = (model.elements[first].qualified_name.clone(), Some(first));
    for seg in &r.segments[1..] {
        match (seg.resolved, prev) {
            (Some(id), Some(p)) if model.elements[id].owner == Some(p) => {
                cur = model.elements[id].qualified_name.clone();
                prev = Some(id);
            }
            _ => {
                cur = format!("{cur}::{}", quote_name(&seg.name));
                prev = seg.resolved;
            }
        }
    }
    cur
}

pub fn export_sysml(model: &Model, scope: Option<&str>) -> Res<Value> {
    let Some(scope) = scope.filter(|s| !s.is_empty()) else {
        let mut text = String::new();
        for f in &model.files {
            text.push_str(&format!("// ---- file: {} ----\n{}", f.path, f.text));
            if !f.text.ends_with('\n') {
                text.push('\n');
            }
        }
        let files: Vec<Value> = model.files.iter().map(|f| json!({ "path": f.path, "text": f.text })).collect();
        return Ok(json!({ "scope": Value::Null, "element_count": model.elements.len(), "files": files, "text": text }));
    };
    let e = locate(model, scope)?;
    let file_text = &model.files[e.file].text;
    let line_start = file_text.get(..e.span.start).unwrap_or("").rfind('\n').map(|i| i + 1).unwrap_or(0);
    let indent = file_text.get(line_start..e.span.start).unwrap_or("");
    let indent = if indent.trim().is_empty() { indent } else { "" };
    let raw = file_text.get(e.span.start..e.span.end).unwrap_or("");
    let text: String = raw
        .lines()
        .enumerate()
        .map(|(i, l)| if i == 0 { l } else { l.strip_prefix(indent).unwrap_or(l) })
        .collect::<Vec<_>>()
        .join("\n");
    let mut inside: BTreeSet<ElementId> = model.descendants(e.id).into_iter().collect();
    inside.insert(e.id);
    let mut external: BTreeSet<String> = BTreeSet::new();
    for id in &inside {
        for r in &model.elements[*id].refs {
            if let Some(t) = r.resolved() {
                if !inside.contains(&t) {
                    external.insert(model.elements[t].qualified_name.clone());
                }
            }
        }
    }
    Ok(json!({
        "scope": e.qualified_name,
        "file": model.files[e.file].path,
        "element_count": inside.len(),
        "text": format!("{text}\n"),
        "external_references": external,
    }))
}

