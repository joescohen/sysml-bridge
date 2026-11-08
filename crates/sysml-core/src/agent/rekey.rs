//! The files beside the model follow a rename, a move and a delete.
//!
//! `display.json` (what each view shows, named views, legends) and the imported layouts (`*.diagrams.json`) name model
//! elements by qualified name. The model text follows a rename by itself (the rename op rewrites every reference), but
//! these files are not model text, so before this module a rename left them naming an element that no longer exists:
//! a hidden port came back, a legend lost its colour, a named view went stale, without a word. Cameo has no such files
//! because its diagrams hold the element itself; here the write that renames, moves or deletes an element rewrites the
//! names in these files in the same step, from the `IdEvent`s the write produced.
//!
//! A full qualified name is mapped exactly. A short name (a port hidden as `out`, or `Def::out`, as `display.ts` matches
//! it) is rewritten only when it no longer names anything where it is used and the renamed element is the one it named
//! there (a port of the symbol's element, its type or a supertype of either). A placed path (`front.sensor`) is walked
//! from the view's block. What cannot be decided is left as it is; `side_refs` reports it.

use super::ids::{split_qn, IdEvent};
use crate::model::{canonical_name, Element, ElementId, Model, RefRole};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// The file of what each view shows.
pub const DISPLAY_FILE: &str = "display.json";

/// The log of renames, moves and deletes beside the model: one JSON line per record (`{time, kind, from, to, tool}`).
/// It travels with the model (git, the work zip), like Terraform's `moved {}` blocks, so a change report, a baseline
/// comparison or a hand-renamed view entry can follow an element across its names.
pub const RENAMES_FILE: &str = "renames.jsonl";

/// True for a file this module rewrites: `display.json` or an imported layout.
pub fn is_side(path: &str) -> bool {
    path == DISPLAY_FILE || path.ends_with(".diagrams.json")
}

/// One name change of a write, in the order it happened. `to` is `None` for a delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub kind: &'static str,
    pub from: String,
    pub to: Option<String>,
}

impl Moved {
    pub fn to_json(&self) -> Value {
        json!({ "kind": self.kind, "from": self.from, "to": self.to })
    }
}

/// The renames, moves and deletes among a write's events.
pub fn moves_of(events: &[IdEvent]) -> Vec<Moved> {
    events
        .iter()
        .filter_map(|e| match e {
            IdEvent::Rename { from, to } if from != to => Some(Moved { kind: "rename", from: from.clone(), to: Some(to.clone()) }),
            IdEvent::Move { from, to, .. } if from != to => Some(Moved { kind: "move", from: from.clone(), to: Some(to.clone()) }),
            IdEvent::Delete { qn, .. } => Some(Moved { kind: "delete", from: qn.clone(), to: None }),
            _ => None,
        })
        .collect()
}

/// What a name became: the same, another name, or nothing (the element was deleted).
#[derive(Debug, PartialEq, Eq)]
pub enum Mapped {
    Same,
    To(String),
    Gone,
}

fn canon(s: &str) -> String {
    canonical_name(s).unwrap_or_else(|| s.trim().to_string())
}

/// A full qualified name through the moves, in order: `from` and everything inside it becomes `to`.
pub fn map_qn(qn: &str, moves: &[Moved]) -> Mapped {
    let start = canon(qn);
    let mut cur = start.clone();
    for m in moves {
        let inside = cur == m.from || (cur.starts_with(&m.from) && cur[m.from.len()..].starts_with("::"));
        if !inside {
            continue;
        }
        match &m.to {
            Some(to) => cur = format!("{to}{}", &cur[m.from.len()..]),
            None => return Mapped::Gone,
        }
    }
    if cur == start {
        Mapped::Same
    } else {
        Mapped::To(cur)
    }
}

/// The element whose members a symbol of `e` shows: `e`, its types and their supertypes (as a diagram draws ports).
fn context(m: &Model, e: ElementId) -> Vec<ElementId> {
    let mut out: Vec<ElementId> = vec![e];
    let mut i = 0;
    while i < out.len() && out.len() < 64 {
        let x = &m.elements[out[i]];
        for r in x.refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes)) {
            if let Some(t) = r.resolved() {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        i += 1;
    }
    out
}

/// Where the next segment of a placed path is looked up: the element's context, and for a call on an activity view the
/// activity it calls (`driveToGoal.planPath`: `planPath` is a step of the method `driveToGoal` opens, not a member of
/// the call).
fn path_context(m: &Model, e: ElementId) -> Vec<ElementId> {
    let mut out = context(m, e);
    if let Some(a) = super::allocation::called_activity(m, e) {
        for x in context(m, a) {
            if !out.contains(&x) {
                out.push(x);
            }
        }
    }
    out
}

/// True when `s` names `qn` the way a client matches a port or a row: the whole name or its last segments.
fn names(qn: &str, s: &str) -> bool {
    qn == s || (qn.len() > s.len() + 2 && qn.ends_with(s) && qn[..qn.len() - s.len()].ends_with("::"))
}

/// The members of a symbol `owner` draws (None: anywhere in the model) that `s` names.
fn hits<'a>(m: &'a Model, owner: Option<&[ElementId]>, s: &str) -> Vec<&'a Element> {
    match owner {
        Some(ctx) => ctx.iter().flat_map(|o| m.children(*o)).filter(|c| names(&c.qualified_name, s)).collect(),
        None => m.elements.iter().filter(|e| names(&e.qualified_name, s)).collect(),
    }
}

/// A short name (`out`, `Def::out`) after the moves: rewritten only when it names nothing now and the moved element is
/// the one it named, in the place it is used.
fn map_short(m: &Model, owner: Option<&[ElementId]>, s: &str, moves: &[Moved]) -> Option<String> {
    if let Mapped::To(n) = map_qn(s, moves) {
        return Some(n);
    }
    let c = canon(s);
    if !hits(m, owner, &c).is_empty() {
        return None;
    }
    let segs = split_qn(&c);
    let mut found: Option<String> = None;
    for mv in moves {
        let Some(to) = &mv.to else { continue };
        let from = split_qn(&mv.from);
        // `s` = the last `j` segments of `from` followed by a rest inside it.
        for j in 1..=segs.len().min(from.len()) {
            if segs[..j] != from[from.len() - j..] {
                continue;
            }
            let after = if j == segs.len() { to.clone() } else { format!("{to}::{}", segs[j..].join("::")) };
            let after = match map_qn(&after, moves) {
                Mapped::Gone => continue,
                Mapped::To(t) => t,
                Mapped::Same => after,
            };
            let Some(e) = m.find(&after) else { continue };
            if let Some(ctx) = owner {
                if !e.owner.map(|o| ctx.contains(&o)).unwrap_or(false) {
                    continue;
                }
            }
            let tail = split_qn(&e.qualified_name);
            let short = tail[tail.len().saturating_sub(segs.len())..].join("::");
            match &found {
                Some(f) if *f != short => return None,
                _ => found = Some(short),
            }
        }
    }
    found.filter(|f| *f != c)
}

/// Feature names along `path` from `scope` after the moves, or None when the path is fine or cannot be followed.
fn map_path(m: &Model, scope: Option<ElementId>, path: &str, moves: &[Moved]) -> Option<String> {
    let mut cur = scope?;
    let mut out: Vec<String> = Vec::new();
    let mut changed = false;
    for seg in path.split('.') {
        let ctx = path_context(m, cur);
        let plain = seg.trim_matches('\'');
        let here = ctx.iter().flat_map(|o| m.children(*o)).find(|c| c.name.as_deref() == Some(plain));
        let next = match here {
            Some(e) => e,
            None => {
                // A member renamed by this write: the move whose old last segment is this one and whose element now
                // sits in this place.
                let cands: Vec<&Element> = moves
                    .iter()
                    .filter(|mv| split_qn(&mv.from).last().map(|l| l.trim_matches('\'') == plain).unwrap_or(false))
                    .filter_map(|mv| mv.to.as_deref())
                    .filter_map(|to| m.find(to))
                    .filter(|e| e.owner.map(|o| ctx.contains(&o)).unwrap_or(false))
                    .collect();
                match cands[..] {
                    [e] => {
                        changed = true;
                        e
                    }
                    _ => return None,
                }
            }
        };
        out.push(next.name.clone().unwrap_or_default());
        cur = next.id;
    }
    changed.then(|| out.join("."))
}

/// `generated|KIND|SCOPE[|group|G|focus|F|view|NAME]` with SCOPE (and a focus element) mapped.
fn map_view_key(key: &str, moves: &[Moved]) -> Option<String> {
    let mut parts: Vec<String> = key.split('|').map(str::to_string).collect();
    if parts.first().map(String::as_str) != Some("generated") || parts.len() < 3 {
        return None;
    }
    let mut changed = false;
    let mut slots = vec![2usize];
    if parts.len() >= 5 && parts[3] == "focus" {
        slots.push(4);
    }
    for i in slots {
        if parts[i].is_empty() {
            continue;
        }
        if let Mapped::To(n) = map_qn(&parts[i], moves) {
            parts[i] = n;
            changed = true;
        }
    }
    changed.then(|| parts.join("|"))
}

fn scope_of_key(m: &Model, key: &str) -> Option<ElementId> {
    let parts: Vec<&str> = key.split('|').collect();
    (parts.first() == Some(&"generated")).then_some(())?;
    m.find(parts.get(2)?).map(|e| e.id)
}

/// A list of full qualified names: mapped, the deleted ones dropped.
fn map_list(v: &mut Value, moves: &[Moved], n: &mut usize) {
    let Some(a) = v.as_array_mut() else { return };
    let mut out = Vec::with_capacity(a.len());
    for x in a.drain(..) {
        match x.as_str().map(|s| map_qn(s, moves)) {
            Some(Mapped::To(t)) => {
                *n += 1;
                out.push(json!(t));
            }
            Some(Mapped::Gone) => *n += 1,
            _ => out.push(x),
        }
    }
    *a = out;
}

/// A list of short names (ports, rows) used on a symbol.
fn map_shorts(m: &Model, owner: Option<&[ElementId]>, v: &mut Value, moves: &[Moved], n: &mut usize) {
    let Some(a) = v.as_array_mut() else { return };
    for x in a.iter_mut() {
        if let Some(t) = x.as_str().and_then(|s| map_short(m, owner, s, moves)) {
            *x = json!(t);
            *n += 1;
        }
    }
}

fn rekey_entry(m: &Model, view_scope: Option<ElementId>, el: Option<ElementId>, en: &mut Map<String, Value>, moves: &[Moved], n: &mut usize) {
    let ctx = el.map(|e| context(m, e));
    if let Some(p) = en.get_mut("ports") {
        map_shorts(m, ctx.as_deref(), p, moves, n);
    }
    if let Some(rows) = en.get_mut("rows").and_then(Value::as_object_mut) {
        for l in rows.values_mut() {
            map_shorts(m, ctx.as_deref(), l, moves, n);
        }
    }
    if let Some(s) = en.get_mut("shown") {
        map_list(s, moves, n);
    }
    if let Some(a) = en.get_mut("placed").and_then(Value::as_array_mut) {
        for x in a.iter_mut() {
            if let Some(t) = x.as_str().and_then(|p| map_path(m, view_scope, p, moves)) {
                *x = json!(t);
                *n += 1;
            }
        }
    }
}

/// `display.json` after the moves (`after` is the model after the write), and how many names changed.
fn rekey_display(after: &Model, doc: &mut Value, moves: &[Moved]) -> usize {
    let mut n = 0;
    if let Some(views) = doc.get_mut("views").and_then(Value::as_object_mut) {
        let old = std::mem::take(views);
        for (key, mut entries) in old {
            let key = match map_view_key(&key, moves) {
                Some(k) => {
                    n += 1;
                    k
                }
                None => key,
            };
            let scope = scope_of_key(after, &key);
            if let Some(es) = entries.as_object_mut() {
                let old = std::mem::take(es);
                for (el, mut en) in old {
                    let el = if el == "*" {
                        el
                    } else {
                        match map_qn(&el, moves) {
                            Mapped::To(t) => {
                                n += 1;
                                t
                            }
                            Mapped::Gone => {
                                n += 1;
                                continue;
                            }
                            Mapped::Same => el,
                        }
                    };
                    let eid = if el == "*" { None } else { after.find(&el).map(|e| e.id) };
                    if let Some(o) = en.as_object_mut() {
                        rekey_entry(after, scope, eid, o, moves, &mut n);
                    }
                    // Two names that became one keep the entry written last; both said what to show of one element.
                    es.insert(el, en);
                }
            }
            views.insert(key, entries);
        }
    }
    if let Some(named) = doc.get_mut("named").and_then(Value::as_array_mut) {
        named.retain_mut(|v| match v.get("scope").and_then(Value::as_str).map(|s| map_qn(s, moves)) {
            Some(Mapped::To(t)) => {
                v["scope"] = json!(t);
                n += 1;
                true
            }
            Some(Mapped::Gone) => {
                n += 1;
                false
            }
            _ => true,
        });
    }
    // A hidden generated view stays hidden under its element's new name.
    if let Some(hidden) = doc.get_mut("hidden_views").and_then(Value::as_array_mut) {
        for k in hidden.iter_mut() {
            if let Some(t) = k.as_str().and_then(|key| map_view_key(key, moves)) {
                *k = json!(t);
                n += 1;
            }
        }
    }
    if let Some(legends) = doc.get_mut("legends").and_then(Value::as_array_mut) {
        for l in legends {
            for it in l.get_mut("items").and_then(Value::as_array_mut).into_iter().flatten() {
                if let Some(e) = it.get_mut("elements") {
                    map_list(e, moves, &mut n);
                }
            }
        }
    }
    n
}

/// An imported layout after the moves: `ref`, `r` and `owner` values and `legends` lists. A deleted element is left as
/// it is (the stored view lists it as stale, as before).
fn rekey_layout(v: &mut Value, moves: &[Moved], n: &mut usize) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                match x {
                    Value::String(s) if matches!(k.as_str(), "ref" | "r" | "owner") && !s.is_empty() => {
                        if let Mapped::To(t) = map_qn(s, moves) {
                            *s = t;
                            *n += 1;
                        }
                    }
                    Value::Array(a) if k == "legends" => {
                        for y in a.iter_mut() {
                            if let Some(Mapped::To(t)) = y.as_str().map(|s| map_qn(s, moves)) {
                                *y = json!(t);
                                *n += 1;
                            }
                        }
                    }
                    _ => rekey_layout(x, moves, n),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| rekey_layout(x, moves, n)),
        _ => {}
    }
}

/// A side file's names after the moves, in place; how many changed.
fn rekey_value(path: &str, v: &mut Value, after: &Model, moves: &[Moved]) -> usize {
    if moves.is_empty() {
        return 0;
    }
    if path == DISPLAY_FILE {
        rekey_display(after, v, moves)
    } else {
        let mut n = 0;
        rekey_layout(v, moves, &mut n);
        n
    }
}

/// `v` as text in the style of `text` (pretty or one line, the end of line kept), or None when it is the same.
fn render(text: &str, v: &Value) -> Option<String> {
    let pretty = text.contains("\n ");
    let mut out = if pretty { serde_json::to_string_pretty(v).ok()? } else { serde_json::to_string(v).ok()? };
    if text.ends_with('\n') {
        out.push('\n');
    }
    (out != text).then_some(out)
}

/// One side file after the moves: its new text and the number of names that changed, or None when nothing changed (or
/// the file is not JSON: it is left as it is).
pub fn rekey_file(path: &str, text: &str, after: &Model, moves: &[Moved]) -> Option<(String, usize)> {
    if moves.is_empty() || !is_side(path) {
        return None;
    }
    let mut v: Value = serde_json::from_str(text).ok()?;
    let n = rekey_value(path, &mut v, after, moves);
    if n == 0 {
        return None;
    }
    render(text, &v).map(|t| (t, n))
}

/// Every side file of `side` after the moves, then followed by element id and stamped (`heal_file`): `(path, new text,
/// names changed)` for each that changed.
pub fn rekey_all(side: &[(String, String)], after: &Model, moves: &[Moved]) -> Vec<(String, String, usize)> {
    side.iter()
        .filter_map(|(p, t)| {
            if !is_side(p) {
                return None;
            }
            let mut v: Value = serde_json::from_str(t).ok()?;
            let n = rekey_value(p, &mut v, after, moves) + heal_value(p, &mut v, after).0;
            render(t, &v).map(|out| (p.clone(), out, n))
        })
        .collect()
}

// Element ids in the side files. A name in display.json or a layout is a qualified name, which a rename by hand, in
// another tool (Cameo) or in a text editor leaves behind. The element's id (its short name, `agent/mint.rs`) travels
// with the element, so each side file records the id of every element it names: display.json in `"ids": {QN: ID}`, a
// layout beside each name (`"ref": QN, "ref_id": ID`). When a name no longer finds its element but the id does, the
// name is moved to where the element is now, with the same engine as a rename the tools did (`heal_file`).

/// The key of display.json that holds the id of each element the file names.
pub const IDS_KEY: &str = "ids";

/// Each short name the model gives one element only, and that element.
fn by_id(m: &Model) -> std::collections::HashMap<&str, ElementId> {
    let mut seen: std::collections::HashMap<&str, Option<ElementId>> = std::collections::HashMap::new();
    for e in &m.elements {
        if let Some(s) = e.short_name.as_deref() {
            seen.entry(s).and_modify(|x| *x = None).or_insert(Some(e.id));
        }
    }
    seen.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect()
}

/// The elements display.json names now: view scopes and focus, symbols, ports and rows on them, added and placed
/// elements, named view scopes and legend elements.
fn display_elements(m: &Model, v: &Value) -> Vec<ElementId> {
    let mut out = Vec::new();
    let add = |s: &str, out: &mut Vec<ElementId>| {
        if let Some(e) = m.find(s) {
            out.push(e.id);
        }
    };
    for (key, entries) in v.get("views").and_then(Value::as_object).into_iter().flatten() {
        let parts: Vec<&str> = key.split('|').collect();
        if parts.first() == Some(&"generated") {
            for i in [2usize, 4] {
                if let Some(p) = parts.get(i).filter(|p| !p.is_empty() && (i == 2 || parts.get(3) == Some(&"focus"))) {
                    add(p, &mut out);
                }
            }
        }
        let scope = scope_of_key(m, key);
        for (el, en) in entries.as_object().into_iter().flatten() {
            let eid = if el == "*" { None } else { m.find(el).map(|e| e.id) };
            out.extend(eid);
            let ctx = eid.map(|e| context(m, e));
            let shorts = en.get("ports").and_then(Value::as_array).into_iter().flatten().chain(en.get("rows").and_then(Value::as_object).into_iter().flat_map(|r| r.values()).filter_map(Value::as_array).flatten());
            for s in shorts.filter_map(Value::as_str) {
                if let [one] = hits(m, ctx.as_deref(), &canon(s))[..] {
                    out.push(one.id);
                }
            }
            for s in en.get("shown").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                add(s, &mut out);
            }
            for p in en.get("placed").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                out.extend(path_elements(m, scope, p));
            }
        }
    }
    for nv in v.get("named").and_then(Value::as_array).into_iter().flatten() {
        if let Some(s) = nv.get("scope").and_then(Value::as_str) {
            add(s, &mut out);
        }
    }
    for l in v.get("legends").and_then(Value::as_array).into_iter().flatten() {
        for it in l.get("items").and_then(Value::as_array).into_iter().flatten() {
            for s in it.get("elements").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                add(s, &mut out);
            }
        }
    }
    out
}

/// The elements along a placed path (`front.sensor`) from the view's block, as far as it resolves.
fn path_elements(m: &Model, scope: Option<ElementId>, path: &str) -> Vec<ElementId> {
    let mut out = Vec::new();
    let Some(mut cur) = scope else { return out };
    for seg in path.split('.') {
        let plain = seg.trim_matches('\'');
        match path_context(m, cur).iter().flat_map(|o| m.children(*o)).find(|c| c.name.as_deref() == Some(plain)) {
            Some(e) => {
                out.push(e.id);
                cur = e.id;
            }
            None => break,
        }
    }
    out
}

/// The `(name, id)` pairs a side file records.
fn recorded(path: &str, v: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if path == DISPLAY_FILE {
        for (q, id) in v.get(IDS_KEY).and_then(Value::as_object).into_iter().flatten() {
            if let Some(id) = id.as_str() {
                out.push((q.clone(), id.to_string()));
            }
        }
    } else {
        layout_pairs(v, &mut out);
    }
    out
}

fn layout_pairs(v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if let (Some(s), true) = (x.as_str(), matches!(k.as_str(), "ref" | "r" | "owner")) {
                    if let Some(id) = o.get(&format!("{k}_id")).and_then(Value::as_str) {
                        out.push((s.to_string(), id.to_string()));
                    }
                } else {
                    layout_pairs(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| layout_pairs(x, out)),
        _ => {}
    }
}

/// The renames the recorded ids show: a name whose element is not there any more while its id names an element
/// elsewhere. Deepest first, so a renamed port inside a renamed block lands on its own new name.
pub fn id_moves(m: &Model, pairs: &[(String, String)]) -> Vec<Moved> {
    let ids = by_id(m);
    let mut out: Vec<Moved> = Vec::new();
    for (q, id) in pairs {
        let q = canon(q);
        if m.find(&q).and_then(|e| e.short_name.as_deref()) == Some(id.as_str()) {
            continue;
        }
        let Some(&e) = ids.get(id.as_str()) else { continue };
        let now = &m.elements[e].qualified_name;
        if *now != q && !out.iter().any(|x| x.from == q) {
            out.push(Moved { kind: "rename", from: q, to: Some(now.clone()) });
        }
    }
    out.sort_by_key(|x| std::cmp::Reverse(split_qn(&x.from).len()));
    out
}

/// display.json's id table for the elements it names now; a recorded name that finds nothing is kept, so a later
/// look can still follow it.
fn stamp_display(m: &Model, v: &mut Value) {
    let Some(o) = v.as_object_mut() else { return };
    let mut table = Map::new();
    let doc = Value::Object(o.clone());
    for e in display_elements(m, &doc) {
        let e = &m.elements[e];
        if let Some(id) = &e.short_name {
            table.insert(e.qualified_name.clone(), json!(id));
        }
    }
    let now: HashSet<Value> = table.values().cloned().collect();
    for (q, id) in o.get(IDS_KEY).and_then(Value::as_object).into_iter().flatten() {
        if m.find(q).is_none() && !now.contains(id) {
            table.insert(q.clone(), id.clone());
        }
    }
    if table.is_empty() {
        o.remove(IDS_KEY);
    } else {
        o.insert(IDS_KEY.into(), Value::Object(table));
    }
}

/// A layout's ids beside each name it holds.
fn stamp_layout(m: &Model, v: &mut Value) {
    match v {
        Value::Object(o) => {
            let mut ids: Vec<(String, Option<String>)> = Vec::new();
            for k in ["ref", "r", "owner"] {
                if let Some(s) = o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    if let Some(e) = m.find(s) {
                        ids.push((format!("{k}_id"), e.short_name.clone()));
                    }
                }
            }
            for (k, id) in ids {
                match id {
                    Some(id) => {
                        o.insert(k, json!(id));
                    }
                    None => {
                        o.remove(&k);
                    }
                }
            }
            for (_, x) in o.iter_mut() {
                if x.is_object() || x.is_array() {
                    stamp_layout(m, x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| stamp_layout(m, x)),
        _ => {}
    }
}

/// Follows the recorded ids of a side file, in place, and records the ids of what it names now. The names that moved
/// and the moves they followed.
fn heal_value(path: &str, v: &mut Value, m: &Model) -> (usize, Vec<Moved>) {
    let moves = id_moves(m, &recorded(path, v));
    let n = rekey_value(path, v, m, &moves);
    if path == DISPLAY_FILE {
        stamp_display(m, v);
    } else {
        stamp_layout(m, v);
    }
    (n, moves)
}

/// A side file after following its recorded ids and recording the ids of what it names: `(new text, names moved, the
/// renames it followed)`, or None when the text stays the same.
pub fn heal_file(path: &str, text: &str, m: &Model) -> Option<(String, usize, Vec<Moved>)> {
    if !is_side(path) {
        return None;
    }
    let mut v: Value = serde_json::from_str(text).ok()?;
    let (n, moves) = heal_value(path, &mut v, m);
    render(text, &v).map(|t| (t, n, moves))
}

/// The lines a write appends to `renames.jsonl`.
pub fn log_lines(moves: &[Moved], time: &str, tool: &str) -> String {
    moves.iter().map(|m| format!("{}\n", json!({ "time": time, "kind": m.kind, "from": m.from, "to": m.to, "tool": tool }))).collect()
}

/// The records of a `renames.jsonl` text, oldest first; a line that is not a record is skipped.
pub fn read_log(text: &str) -> Vec<Moved> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            let from = v.get("from")?.as_str()?.to_string();
            let to = v.get("to").and_then(Value::as_str).map(str::to_string);
            let kind = match v.get("kind").and_then(Value::as_str) {
                Some("move") => "move",
                Some("delete") => "delete",
                _ => "rename",
            };
            Some(Moved { kind, from, to })
        })
        .collect()
}

/// One name a side file holds that names nothing in the model.
#[derive(Debug, Clone)]
pub struct Stale {
    pub file: String,
    /// Where in the file: `views/<view>/<element>/ports`, `named`, `legends/<legend>/<item>`, a layout's `ref`.
    pub at: String,
    pub name: String,
    /// What it became, when the element id it records or the rename log knows.
    pub now: Option<String>,
    /// True when the element id the file records knows it: the next write follows it by itself.
    pub by_id: bool,
}

/// Names in `display.json` and the imported layouts that name no element of `m`. `log` (the rename log) gives what a
/// stale name became, so the fix is one call. Read-only; a write never waits on it.
pub fn stale_refs(m: &Model, side: &[(String, String)], log: &[Moved]) -> Vec<Stale> {
    let mut out = Vec::new();
    let now_of = |s: &str| match map_qn(s, log) {
        Mapped::To(t) if m.find(&t).is_some() => Some(t),
        _ => None,
    };
    let exists = |s: &str| m.find(s).is_some();
    for (file, text) in side {
        let Ok(v) = serde_json::from_str::<Value>(text) else { continue };
        let by_ids = id_moves(m, &recorded(file, &v));
        let now_by_id = |s: &str| match map_qn(s, &by_ids) {
            Mapped::To(t) if m.find(&t).is_some() => Some(t),
            _ => None,
        };
        let mut put = |at: String, name: &str, id_now: Option<String>| {
            let by_id = id_now.is_some();
            out.push(Stale { file: file.clone(), at, now: id_now.or_else(|| now_of(name)), name: name.to_string(), by_id })
        };
        if file == DISPLAY_FILE {
            for (key, entries) in v.get("views").and_then(Value::as_object).into_iter().flatten() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.first() == Some(&"generated") && parts.len() >= 3 && !parts[2].is_empty() && !exists(parts[2]) {
                    put(format!("views/{key}"), parts[2], now_by_id(parts[2]));
                }
                let scope = scope_of_key(m, key);
                for (el, en) in entries.as_object().into_iter().flatten() {
                    let eid = if el == "*" { None } else { m.find(el).map(|e| e.id) };
                    if el != "*" && eid.is_none() {
                        put(format!("views/{key}/{el}"), el, now_by_id(el));
                        continue;
                    }
                    let ctx = eid.map(|e| context(m, e));
                    for p in en.get("ports").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                        if hits(m, ctx.as_deref(), &canon(p)).is_empty() {
                            put(format!("views/{key}/{el}/ports"), p, map_short(m, ctx.as_deref(), p, &by_ids));
                        }
                    }
                    for s in en.get("shown").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                        if !exists(s) {
                            put(format!("views/{key}/{el}/shown"), s, now_by_id(s));
                        }
                    }
                    for p in en.get("placed").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                        if scope.is_some() && !path_resolves(m, scope, p) {
                            put(format!("views/{key}/{el}/placed"), p, map_path(m, scope, p, &by_ids));
                        }
                    }
                }
            }
            for nv in v.get("named").and_then(Value::as_array).into_iter().flatten() {
                if let Some(s) = nv.get("scope").and_then(Value::as_str) {
                    if !exists(s) {
                        put(format!("named/{}", nv.get("name").and_then(Value::as_str).unwrap_or("")), s, now_by_id(s));
                    }
                }
            }
            for l in v.get("legends").and_then(Value::as_array).into_iter().flatten() {
                let ln = l.get("name").and_then(Value::as_str).unwrap_or("");
                for it in l.get("items").and_then(Value::as_array).into_iter().flatten() {
                    let iname = it.get("name").and_then(Value::as_str).unwrap_or("");
                    for s in it.get("elements").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                        if !exists(s) {
                            put(format!("legends/{ln}/{iname}"), s, now_by_id(s));
                        }
                    }
                }
            }
        } else {
            let mut seen: HashSet<String> = HashSet::new();
            let mut names_in: Vec<String> = Vec::new();
            collect_layout(&v, &mut names_in);
            for s in names_in {
                if !exists(&s) && seen.insert(s.clone()) {
                    put("ref".into(), &s, now_by_id(&s));
                }
            }
        }
    }
    out
}

fn collect_layout(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                match x {
                    Value::String(s) if matches!(k.as_str(), "ref" | "r") && !s.is_empty() => out.push(s.clone()),
                    _ => collect_layout(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_layout(x, out)),
        _ => {}
    }
}

fn path_resolves(m: &Model, scope: Option<ElementId>, path: &str) -> bool {
    let Some(mut cur) = scope else { return false };
    for seg in path.split('.') {
        let plain = seg.trim_matches('\'');
        match path_context(m, cur).iter().flat_map(|o| m.children(*o)).find(|c| c.name.as_deref() == Some(plain)) {
            Some(e) => cur = e.id,
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::FileIn;

    fn model(t: &str) -> Model {
        crate::api::to_model(vec![FileIn { path: "m.sysml".into(), text: t.into() }])
    }

    fn mv(from: &str, to: &str) -> Moved {
        Moved { kind: "rename", from: from.into(), to: Some(to.into()) }
    }

    #[test]
    fn a_full_name_follows_its_owner_and_a_prefix_is_not_a_parent() {
        let ms = [mv("P::A", "P::B")];
        assert_eq!(map_qn("P::A::dout", &ms), Mapped::To("P::B::dout".into()));
        assert_eq!(map_qn("P::A", &ms), Mapped::To("P::B".into()));
        assert_eq!(map_qn("P::A2", &ms), Mapped::Same);
        assert_eq!(map_qn("P::AB::x", &ms), Mapped::Same);
        let gone = [Moved { kind: "delete", from: "P::A".into(), to: None }];
        assert_eq!(map_qn("P::A::dout", &gone), Mapped::Gone);
        // A chain: A to B, then B to C.
        assert_eq!(map_qn("P::A::x", &[mv("P::A", "P::B"), mv("P::B", "P::C")]), Mapped::To("P::C::x".into()));
    }

    #[test]
    fn a_hidden_port_a_legend_a_named_view_and_a_placement_follow_a_rename() {
        let after = model("package P { port def D; part def Pump { port inlet : D; port drain : D; part front : Front; } part def Front { port feed : D; } part def Other { port dout : D; } part sys : Pump; }");
        let display = json!({
            "version": 1,
            "views": {
                "generated|interconnection|P::Pump": { "P::sys": { "ports": ["dout", "drain"] }, "*": { "placed": ["front.supply"] } },
                "*": { "P::sys": { "ports": ["P::Pump::dout"] } }
            },
            "named": [{ "kind": "interconnection", "scope": "P::Pump", "name": "Data" }],
            "legends": [{ "name": "L", "items": [{ "name": "I", "elements": ["P::Pump::dout", "P::Other::dout"] }] }]
        });
        // Port `out` of Pump became `inlet`; port `supply` of Front became `feed`.
        let moves = [mv("P::Pump::dout", "P::Pump::inlet"), mv("P::Front::supply", "P::Front::feed")];
        let (text, n) = rekey_file(DISPLAY_FILE, &serde_json::to_string_pretty(&display).unwrap(), &after, &moves).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let view = &v["views"]["generated|interconnection|P::Pump"];
        assert_eq!(view["P::sys"]["ports"], json!(["inlet", "drain"]), "{text}");
        assert_eq!(view["*"]["placed"], json!(["front.feed"]));
        assert_eq!(v["views"]["*"]["P::sys"]["ports"], json!(["P::Pump::inlet"]));
        // Other::dout is another element named `out`: untouched.
        assert_eq!(v["legends"][0]["items"][0]["elements"], json!(["P::Pump::inlet", "P::Other::dout"]));
        assert_eq!(n, 4);
        assert!(stale_refs(&after, &[(DISPLAY_FILE.into(), text)], &[]).is_empty());
    }

    #[test]
    fn a_renamed_block_takes_its_view_keys_named_views_and_symbols_along() {
        let after = model("package P { port def D; part def Engine { port dout : D; } part sys : Engine; }");
        let display = json!({
            "version": 1,
            "views": { "generated|interconnection|P::Pump": { "P::Pump::dout": { "hidden": true } } },
            "named": [{ "kind": "interconnection", "scope": "P::Pump", "name": "Data" }]
        });
        let moves = [mv("P::Pump", "P::Engine")];
        let (text, _) = rekey_file(DISPLAY_FILE, &display.to_string(), &after, &moves).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["views"]["generated|interconnection|P::Engine"]["P::Engine::dout"]["hidden"], true, "{text}");
        assert_eq!(v["named"][0]["scope"], "P::Engine");
    }

    #[test]
    fn a_deleted_element_leaves_display_and_an_ambiguous_short_name_is_left_alone() {
        let after = model("package P { port def D; part def A { port q : D; } part def B { port p : D; } part a : A; }");
        let display = json!({ "version": 1, "views": { "*": { "P::Gone": { "hidden": true }, "*": { "ports": ["p"] } } } });
        let moves = [Moved { kind: "delete", from: "P::Gone".into(), to: None }, mv("P::A::p", "P::A::q")];
        let (text, _) = rekey_file(DISPLAY_FILE, &display.to_string(), &after, &moves).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v["views"]["*"].get("P::Gone").is_none());
        // `p` on every symbol still names B::p: kept.
        assert_eq!(v["views"]["*"]["*"]["ports"], json!(["p"]));
    }

    #[test]
    fn a_layout_follows_and_its_formatting_style_is_kept() {
        let after = model("package P { part def B; }");
        let text = r#"[{"id":"d1","owner":"P::A","legends":["P::A"],"shapes":[{"ref":"P::A"},{"ref":"P::X"}]}]"#;
        let (out, n) = rekey_file("m.diagrams.json", text, &after, &[mv("P::A", "P::B")]).unwrap();
        assert_eq!(n, 3);
        assert!(!out.contains('\n'));
        let stale = stale_refs(&after, &[("m.diagrams.json".into(), out)], &[]);
        assert_eq!(stale.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["P::X"]);
    }

    #[test]
    fn a_placed_call_path_on_an_activity_goes_into_the_called_method() {
        let m = model("package P { item def I; part def Leaf { action step { out item i : I; } } \
            part def Sub { part leaf : Leaf; action def Run { perform action step ::> leaf.step; } action run : Run; } \
            part def Top { part sub : Sub; action def Go { perform action run ::> sub.run; } action go : Go; } }");
        let key = "generated|action|P::Top::Go";
        let ok = json!({"views": {key: {"*": {"placed": ["run.step"]}}}}).to_string();
        assert!(stale_refs(&m, &[(DISPLAY_FILE.into(), ok)], &[]).is_empty(), "a path down through the called method is not stale");
        let bad = json!({"views": {key: {"*": {"placed": ["run.nothing"]}}}}).to_string();
        assert_eq!(stale_refs(&m, &[(DISPLAY_FILE.into(), bad)], &[]).len(), 1, "a path to no step still is");
    }

    #[test]
    fn the_log_reads_back_and_tells_a_stale_name_what_it_became() {
        let after = model("package P { part def B; }");
        let ms = [mv("P::A", "P::B")];
        let log = read_log(&log_lines(&ms, "t", "test"));
        assert_eq!(log, ms);
        let s = stale_refs(&after, &[(DISPLAY_FILE.into(), json!({"views": {"*": {"P::A": {"hidden": true}}}}).to_string())], &log);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].now.as_deref(), Some("P::B"));
    }

    #[test]
    fn a_rename_by_hand_is_followed_by_the_recorded_id() {
        let before = model("package P { port def D; part def <ea0000a> Pump { port <ea0000b> dout : D; } part <ea0000c> sys : Pump; }");
        let display = json!({
            "version": 1,
            "views": { "generated|interconnection|P::Pump": { "P::sys": { "ports": ["dout"] } } },
            "named": [{ "kind": "interconnection", "scope": "P::Pump", "name": "Data" }],
            "legends": [{ "name": "L", "items": [{ "name": "I", "elements": ["P::Pump::dout"] }] }]
        });
        let (stamped, n, moves) = heal_file(DISPLAY_FILE, &display.to_string(), &before).unwrap();
        assert_eq!((n, moves.len()), (0, 0));
        let v: Value = serde_json::from_str(&stamped).unwrap();
        assert_eq!(v["ids"], json!({ "P::Pump": "ea0000a", "P::Pump::dout": "ea0000b", "P::sys": "ea0000c" }), "{stamped}");
        // The same again: nothing to do.
        assert!(heal_file(DISPLAY_FILE, &stamped, &before).is_none());
        // Pump and its port renamed in a text editor (or in Cameo): the ids came along, the side file did not.
        let after = model("package P { port def D; part def <ea0000a> Engine { port <ea0000b> dataOut : D; } part <ea0000c> sys : Engine; }");
        assert_eq!(stale_refs(&after, &[(DISPLAY_FILE.into(), stamped.clone())], &[]).iter().find(|s| s.name == "P::Pump").and_then(|s| s.now.clone()).as_deref(), Some("P::Engine"));
        let (healed, n, moves) = heal_file(DISPLAY_FILE, &stamped, &after).unwrap();
        assert_eq!(moves.len(), 2, "{moves:?}");
        assert!(n >= 4, "{n}");
        let v: Value = serde_json::from_str(&healed).unwrap();
        assert_eq!(v["views"]["generated|interconnection|P::Engine"]["P::sys"]["ports"], json!(["dataOut"]), "{healed}");
        assert_eq!(v["named"][0]["scope"], "P::Engine");
        assert_eq!(v["legends"][0]["items"][0]["elements"], json!(["P::Engine::dataOut"]));
        assert_eq!(v["ids"], json!({ "P::Engine": "ea0000a", "P::Engine::dataOut": "ea0000b", "P::sys": "ea0000c" }));
        assert!(stale_refs(&after, &[(DISPLAY_FILE.into(), healed)], &[]).is_empty());
    }

    #[test]
    fn a_layout_records_ids_beside_its_names_and_follows_them() {
        let before = model("package P { part def <ea0000a> A; }");
        let (stamped, _, _) = heal_file("m.diagrams.json", r#"[{"id":"d1","owner":"P::A","shapes":[{"ref":"P::A"}]}]"#, &before).unwrap();
        assert_eq!(stamped, r#"[{"id":"d1","owner":"P::A","owner_id":"ea0000a","shapes":[{"ref":"P::A","ref_id":"ea0000a"}]}]"#);
        let after = model("package P { part def <ea0000a> B; }");
        let (healed, n, _) = heal_file("m.diagrams.json", &stamped, &after).unwrap();
        assert_eq!(n, 2);
        assert_eq!(healed, r#"[{"id":"d1","owner":"P::B","owner_id":"ea0000a","shapes":[{"ref":"P::B","ref_id":"ea0000a"}]}]"#);
    }
}
