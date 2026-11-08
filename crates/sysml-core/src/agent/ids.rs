//! Source ids: a caller-supplied id (for example a Cameo xmi id) for an element, kept as a map to
//! qualified names. Writes produce `IdEvent`s; this module keeps the map right as names change.

use super::{Fail, Res};
use crate::model::*;
use std::collections::{BTreeMap, HashSet};

/// Longest source id.
pub const MAX_SOURCE_ID: usize = 256;

/// What a write did to names, in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdEvent {
    /// `source_id` now names the element `qn`.
    Assign { id: String, qn: String },
    /// `from` and everything inside it is now `to`.
    Rename { from: String, to: String },
    /// `qn` and everything inside it is gone. Later siblings of `owner` move up by one.
    Delete { qn: String, owner: Option<String>, index: usize },
    /// `from` and everything inside it is now `to`, under another owner. Later siblings of the old place
    /// (`owner`, `index`) move up by one.
    Move { from: String, to: String, owner: Option<String>, index: usize },
    /// `id` now names `qn`, and the element that held it loses it. Only `set_source_id {move: true}` makes this.
    Reassign { id: String, qn: String },
    /// `id` no longer names `qn`.
    Unset { id: String, qn: String },
    /// The members of `owner` changed places: `to[old]` is the new position of the member that was at `old`
    /// (both 0-based, counting every member). Only a ref `Owner::$n` of an unnamed member changes.
    Reorder { owner: String, to: Vec<usize> },
}

/// What a write did to an id besides following a rename: it left one element for another, or it was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdNote {
    pub id: String,
    /// The element that lost the id.
    pub from: String,
    /// The element that holds it now, or `None` when it was removed.
    pub to: Option<String>,
    pub how: &'static str,
}

impl IdNote {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "id": self.id, "lost_by": self.from, "now_on": self.to, "how": self.how })
    }
}

/// A source id: 1 to 256 printable ASCII characters, no spaces.
pub fn valid_source_id(s: &str) -> Res<String> {
    let s = s.trim();
    if s.is_empty() || s.len() > MAX_SOURCE_ID || !s.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(Fail::from(format!(
            "source_id '{}' is not valid. Fix: use 1 to {MAX_SOURCE_ID} printable ASCII characters without spaces, for example _2022x_2_98e020b_1736299733949_262442_3044.",
            s.chars().take(40).collect::<String>().escape_debug()
        )));
    }
    Ok(s.to_string())
}

/// Later siblings of a removed place move up by one: `Owner::$4` becomes `Owner::$3` after `Owner::$2` goes.
fn shift_after(qn: &str, owner: &Option<String>, index: usize) -> String {
    let prefix = owner.as_ref().map(|o| format!("{o}::")).unwrap_or_default();
    let Some(rest) = qn.strip_prefix(&prefix) else { return qn.to_string() };
    let Some(num) = rest.strip_prefix('$') else { return qn.to_string() };
    let digits: String = num.chars().take_while(char::is_ascii_digit).collect();
    let tail = &num[digits.len()..];
    match digits.parse::<usize>() {
        Ok(k) if k > index + 1 && (tail.is_empty() || tail.starts_with("::")) => format!("{prefix}${}{tail}", k - 1),
        _ => qn.to_string(),
    }
}

/// `qn` at `from` or inside it, renamed to `to`.
fn rebase(qn: &str, from: &str, to: &str) -> Option<String> {
    if qn == from {
        Some(to.to_string())
    } else {
        qn.strip_prefix(&format!("{from}::")).map(|rest| format!("{to}::{rest}"))
    }
}

/// Move `qn` through one event. `None` when the element is gone.
pub(crate) fn step(qn: &str, ev: &IdEvent) -> Option<String> {
    match ev {
        IdEvent::Assign { .. } | IdEvent::Reassign { .. } | IdEvent::Unset { .. } => Some(qn.to_string()),
        IdEvent::Rename { from, to } => Some(rebase(qn, from, to).unwrap_or_else(|| qn.to_string())),
        IdEvent::Delete { qn: gone, owner, index } => {
            if qn == gone || qn.starts_with(&format!("{gone}::")) {
                return None;
            }
            Some(shift_after(qn, owner, *index))
        }
        IdEvent::Move { from, to, owner, index } => Some(rebase(qn, from, to).unwrap_or_else(|| shift_after(qn, owner, *index))),
        IdEvent::Reorder { owner, to } => Some(super::order::reposition(qn, owner, to).unwrap_or_else(|| qn.to_string())),
    }
}

/// Apply events to the id map. A source id that already names another element is refused.
pub fn apply_events(map: &mut BTreeMap<String, String>, events: &[IdEvent]) -> Res<()> {
    apply_events_noted(map, events, &mut BTreeMap::new(), &mut Vec::new())
}

/// Like `apply_events`, and records in `notes` every id that left one element for another or was removed on request.
/// `freed` holds the ids that a delete in this write took off their element, so a later assign of one of them is reported.
pub fn apply_events_noted(map: &mut BTreeMap<String, String>, events: &[IdEvent], freed: &mut BTreeMap<String, String>, notes: &mut Vec<IdNote>) -> Res<()> {
    for ev in events {
        match ev {
            IdEvent::Assign { id, qn } => {
                match map.get(id) {
                    Some(have) if have != qn => {
                        return Err(Fail::from(format!(
                            "source_id '{id}' already names '{have}'. Fix: use another source_id, delete that element first, or move the id with set_source_id {{ref: '{qn}', source_id: '{id}', move: true}} (the reply then names '{have}' as the element that lost it)."
                        )))
                    }
                    Some(_) => {}
                    None => {
                        if let Some(prev) = freed.remove(id) {
                            if &prev != qn {
                                notes.push(IdNote { id: id.clone(), from: prev, to: Some(qn.clone()), how: "its element was deleted earlier in this write" });
                            }
                        }
                        map.insert(id.clone(), qn.clone());
                    }
                }
                continue;
            }
            IdEvent::Reassign { id, qn } => {
                let prev = map.insert(id.clone(), qn.clone()).or_else(|| freed.remove(id));
                if let Some(prev) = prev.filter(|p| p != qn) {
                    notes.push(IdNote { id: id.clone(), from: prev, to: Some(qn.clone()), how: "moved on request (set_source_id move: true)" });
                }
                continue;
            }
            IdEvent::Unset { id, qn } => {
                match map.get(id) {
                    Some(have) if have == qn => {
                        map.remove(id);
                        notes.push(IdNote { id: id.clone(), from: qn.clone(), to: None, how: "removed on request (set_source_id unset: true)" });
                    }
                    Some(have) => return Err(Fail::from(format!("source_id '{id}' names '{have}', not '{qn}', so there is nothing to unset on '{qn}'. Fix: pass ref '{have}', or move the id with move: true."))),
                    None => return Err(Fail::from(format!("source_id '{id}' is not recorded for any element, so there is nothing to unset."))),
                }
                continue;
            }
            IdEvent::Delete { qn: gone, .. } => {
                for (k, v) in map.iter() {
                    if v == gone || v.starts_with(&format!("{gone}::")) {
                        freed.insert(k.clone(), v.clone());
                    }
                }
            }
            _ => {}
        }
        let moved: BTreeMap<String, String> = map.iter().filter_map(|(k, v)| step(v, ev).map(|n| (k.clone(), n))).collect();
        *map = moved;
    }
    Ok(())
}

/// Drop entries whose element existed before a write and is not in the result, when no event said where it went.
/// Entries that never matched an element (an import's aliases) are kept.
pub fn sweep(map: &mut BTreeMap<String, String>, original: &BTreeMap<String, String>, before: &Model, after: &Model) {
    fn names(m: &Model) -> HashSet<&str> {
        m.elements.iter().map(|e| e.qualified_name.as_str()).collect()
    }
    let (b, a) = (names(before), names(after));
    map.retain(|id, qn| {
        if a.contains(qn.as_str()) {
            return true;
        }
        !original.get(id).map(|o| b.contains(o.as_str())).unwrap_or(false)
    });
}

/// `(id, before, after)` for every id whose entry differs.
pub fn changes(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<(String, Option<String>, Option<String>)> {
    let mut out = Vec::new();
    for (k, v) in after {
        if before.get(k) != Some(v) {
            out.push((k.clone(), before.get(k).cloned(), Some(v.clone())));
        }
    }
    for (k, v) in before {
        if !after.contains_key(k) {
            out.push((k.clone(), Some(v.clone()), None));
        }
    }
    out
}

/// The source ids of one element.
pub fn ids_of(map: &BTreeMap<String, String>, qn: &str) -> Vec<String> {
    map.iter().filter(|(_, v)| v.as_str() == qn).map(|(k, _)| k.clone()).collect()
}

/// Position of `e` among its owner's members, and the owner's qualified name.
pub fn place_of(model: &Model, e: &Element) -> (Option<String>, usize) {
    match e.owner {
        Some(o) => (Some(model.elements[o].qualified_name.clone()), model.elements[o].children.iter().position(|c| *c == e.id).unwrap_or(0)),
        None => (None, model.roots.iter().position(|c| *c == e.id).unwrap_or(0)),
    }
}

/// The segments of a qualified name, quotes kept. A `::` inside quotes does not split.
pub(crate) fn split_qn(qn: &str) -> Vec<&str> {
    let (mut out, mut start, mut in_q) = (Vec::new(), 0usize, false);
    let b = qn.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if in_q => i += 1,
            b'\'' => in_q = !in_q,
            b':' if !in_q && b.get(i + 1) == Some(&b':') => {
                out.push(&qn[start..i]);
                i += 1;
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&qn[start..]);
    out
}

pub(crate) fn is_positional(seg: &str) -> bool {
    seg.strip_prefix('$').map(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).unwrap_or(false)
}

/// What an element is, apart from where it sits: its kind and its text with white space collapsed.
pub(crate) fn signature(model: &Model, e: &Element) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    };
    e.kind.bytes().for_each(&mut feed);
    feed(0);
    let text = model.files.get(e.file).and_then(|f| f.text.get(e.span.start..e.span.end)).unwrap_or("");
    let mut gap = false;
    for b in text.bytes() {
        if b.is_ascii_whitespace() {
            gap = true;
        } else {
            if gap {
                feed(b' ');
                gap = false;
            }
            feed(b);
        }
    }
    h
}

fn members(model: &Model, owner: Option<ElementId>) -> &[ElementId] {
    match owner {
        Some(o) => &model.elements[o].children,
        None => &model.roots,
    }
}

/// Keep ids that point at unnamed members (`Owner::$3`) on the same statement after an edit moved it. The name
/// events already move them for a delete or a rename; this also covers an edit that shifts positions without
/// saying so (a removed `doc`, a replaced statement, a rewritten file). An id stays where it is when the member
/// at its place still has the text it had. Otherwise it follows the sibling with that text: the same
/// number of identical siblings before it, so parallel copies of a statement keep their order.
pub fn reanchor(map: &mut BTreeMap<String, String>, original: &BTreeMap<String, String>, before: &Model, after: &Model) {
    fn index(m: &Model) -> std::collections::HashMap<&str, ElementId> {
        let mut ix = std::collections::HashMap::new();
        for e in &m.elements {
            ix.entry(e.qualified_name.as_str()).or_insert(e.id);
        }
        ix
    }
    let (ib, ia) = (index(before), index(after));
    let mut sigs: std::collections::HashMap<(bool, ElementId), u64> = std::collections::HashMap::new();
    let mut sig = |is_after: bool, id: ElementId| -> u64 {
        *sigs.entry((is_after, id)).or_insert_with(|| signature(if is_after { after } else { before }, &(if is_after { after } else { before }).elements[id]))
    };
    for (id, now) in map.iter_mut() {
        let Some(orig) = original.get(id) else { continue };
        let (so, sn) = (split_qn(orig), split_qn(now));
        let (Some(jo), Some(jn)) = (so.iter().position(|s| is_positional(s)), sn.iter().position(|s| is_positional(s))) else { continue };
        let Some(&eb) = ib.get(so[..=jo].join("::").as_str()) else { continue };
        let want = sig(false, eb);
        let here = sn[..=jn].join("::");
        if ia.get(here.as_str()).map(|&ea| sig(true, ea) == want).unwrap_or(false) {
            continue;
        }
        let parent = if jn == 0 { None } else { ia.get(sn[..jn].join("::").as_str()).copied() };
        if jn > 0 && parent.is_none() {
            continue;
        }
        let rank = members(before, before.elements[eb].owner).iter().filter(|c| sig(false, **c) == want).position(|c| *c == eb).unwrap_or(0);
        let same: Vec<ElementId> = members(after, parent).iter().copied().filter(|c| sig(true, *c) == want).collect();
        if let Some(c) = same.get(rank) {
            let mut moved = after.elements[*c].qualified_name.clone();
            for rest in &sn[jn + 1..] {
                moved.push_str("::");
                moved.push_str(rest);
            }
            *now = moved;
        }
    }
}
