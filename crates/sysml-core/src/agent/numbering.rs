//! Requirement numbering (`propose_renumber`): the new ids of the requirements under one package or requirement, as edits the
//! caller stores as a proposal. Nothing is written here. An id is the short name of the requirement, or the value of its `id`
//! attribute when it has one. Ids are names, not positions: only this command changes them.
//!
//! Scheme: the children of the scope, in the order of the model, are numbered from `start` (1 by default). A child of a package
//! is `prefix` followed by the number; a child of a requirement is the new id of its parent, `separator`, the number. With
//! `recursive` the requirements under each child are numbered the same way.

use super::text::id_of;
use super::{Fail, Res};
use crate::model::*;
use serde_json::{json, Value};

/// Most changes one call plans: the edit limit of a proposal.
pub const MAX_CHANGES: usize = super::intent::MAX_EDITS;
/// Most requirements read.
pub const MAX_READ: usize = 20_000;
/// Most mentions of an old id searched for in text, and the most texts read.
const MAX_MENTION_ID: usize = 50;

#[derive(Default)]
pub struct Opts {
    pub scope: String,
    pub recursive: bool,
    pub prefix: String,
    pub separator: Option<String>,
    pub start: usize,
}

fn is_req(e: &Element) -> bool {
    e.kind == "requirement" && !e.is_def
}

pub(crate) fn id_attr(m: &Model, e: &Element) -> Option<ElementId> {
    e.children.iter().copied().find(|c| {
        let c = &m.elements[*c];
        c.kind == "attribute" && c.name.as_deref().map(|n| n.eq_ignore_ascii_case("id") || n.eq_ignore_ascii_case("reqid")).unwrap_or(false)
    })
}

fn safe(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 60 && !id.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
}

pub fn plan(m: &Model, o: &Opts) -> Res<Value> {
    if o.scope.len() > 500 || o.prefix.len() > 60 || o.separator.as_ref().map(|x| x.len() > 60).unwrap_or(false) {
        return Err(Fail::from("scope is at most 500 characters; prefix and separator at most 60.".to_string()));
    }
    if o.start > 1_000_000_000 {
        return Err(Fail::from("start is a number up to 1000000000.".to_string()));
    }
    if !safe(&o.prefix) && !o.prefix.is_empty() {
        return Err(Fail::from("prefix is 1 to 60 characters without quotes, angle brackets, backslash or line breaks, for example SR-.".to_string()));
    }
    let sep = o.separator.clone().unwrap_or_else(|| ".".into());
    if !sep.is_empty() && !safe(&sep) {
        return Err(Fail::from("separator is a short text without quotes, angle brackets, backslash or line breaks, for example .".to_string()));
    }
    let scope = m.find(o.scope.trim()).ok_or_else(|| Fail::from(format!("scope {} is not an element of the model. Give the qualified name of a package or requirement in the argument scope, for example Package::Section.", super::text::quote_clip(&o.scope))))?;
    let start = o.start.max(1);
    // The base for the first level: the scope's own id when it is a requirement, else the prefix.
    let base_of_scope = if is_req(scope) { id_of(m, scope) } else { String::new() };
    let mut changes: Vec<Value> = Vec::new();
    let (mut unchanged, mut read) = (0usize, 0usize);
    let mut cut = false;
    let mut planned: Vec<(ElementId, String)> = Vec::new();
    // (parent, new id of the parent or the prefix, is the parent a requirement)
    let mut stack: Vec<(ElementId, String, bool)> = vec![(scope.id, if is_req(scope) { base_of_scope } else { o.prefix.clone() }, is_req(scope))];
    while let Some((parent, base, under_req)) = stack.pop() {
        let kids: Vec<ElementId> = m.elements[parent].children.iter().copied().filter(|c| is_req(&m.elements[*c])).collect();
        let mut kid_bases = Vec::new();
        for (k, c) in kids.iter().enumerate() {
            read += 1;
            if read > MAX_READ {
                cut = true;
                break;
            }
            let n = start + k;
            let new = if under_req { format!("{base}{sep}{n}") } else { format!("{base}{n}") };
            if !safe(&new) {
                return Err(Fail::from("a new id would be empty or hold a character an id cannot (quotes, angle brackets, backslash, line break): change prefix or separator.".to_string()));
            }
            planned.push((*c, new.clone()));
            kid_bases.push((*c, new));
        }
        if o.recursive {
            for (c, nb) in kid_bases.into_iter().rev() {
                stack.push((c, nb, true));
            }
        }
        if cut {
            break;
        }
    }
    // A scope cut at the read bound would be numbered in part: the ids after the cut would clash with the new ones. Refused, not planned.
    if cut {
        return Err(Fail::from(format!("the scope holds more than {MAX_READ} requirements; a renumber of part of them could clash with the rest. Fix: renumber a smaller scope (a section).")));
    }
    for (c, new) in &planned {
        let e = &m.elements[*c];
        let old = id_of(m, e);
        if old == *new {
            unchanged += 1;
            continue;
        }
        let edit = match id_attr(m, e) {
            Some(a) => json!({ "op": "update_element", "ref": m.elements[a].qualified_name, "value": format!("\"{new}\"") }),
            None => json!({ "op": "update_element", "ref": e.qualified_name, "short_name": new }),
        };
        changes.push(json!({ "ref": e.qualified_name, "from": old, "to": new, "edit": edit }));
    }
    if changes.len() > MAX_CHANGES {
        return Err(Fail::from(format!("{} ids change, and a proposal holds at most {MAX_CHANGES} edits. Fix: renumber a smaller scope (a section), or without recursive.", changes.len())));
    }
    // Texts that hold an old id as literal characters are not updated by a renumber: say how many.
    let olds: Vec<&str> = changes.iter().filter_map(|c| c["from"].as_str()).filter(|f| f.chars().count() >= 3).take(MAX_MENTION_ID).collect();
    let mut mentions = 0usize;
    let mut texts_cut = false;
    if !olds.is_empty() {
        texts_cut = m.elements.iter().filter(|e| e.kind != "doc" && e.kind != "comment").nth(MAX_READ).is_some();
        for e in m.elements.iter().filter(|e| e.kind != "doc" && e.kind != "comment").take(MAX_READ) {
            if let Some(d) = e.doc.as_deref() {
                if olds.iter().any(|f| d.contains(f)) {
                    mentions += 1;
                }
            }
        }
    }
    let mut bounds = Vec::new();
    if cut {
        bounds.push(format!("requirements: the first {MAX_READ} were read; renumber a smaller scope"));
    }
    if texts_cut {
        bounds.push(format!("mentions: the first {MAX_READ} elements were searched; texts_with_an_old_id is a lower bound"));
    }
    if olds.len() < changes.len() {
        bounds.push(format!("mentions: the old ids searched for in text are the first {MAX_MENTION_ID} of three characters or more"));
    }
    Ok(json!({
        "scope": scope.qualified_name,
        "scheme": { "prefix": o.prefix, "separator": sep, "start": start, "recursive": o.recursive },
        "changes": changes,
        "changed": changes.len(),
        "unchanged": unchanged,
        "texts_with_an_old_id": mentions,
        "texts_with_an_old_id_is_lower_bound": texts_cut || olds.len() < changes.len(),
        "truncated": cut,
        "bounds": bounds,
        "limits": { "changes": MAX_CHANGES, "read": MAX_READ },
    }))
}

/// Most requirement ids read to find the next free one, and most numbers tried.
const MAX_NEXT_READ: usize = 50_000;
const MAX_NEXT_TRIES: usize = 10_000;

/// The text before the trailing digits of an id, the number, and how many digits it was written with.
fn split_number(id: &str) -> Option<(String, usize, usize)> {
    let digits = id.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let cut = id.len() - digits;
    Some((id[..cut].to_string(), id[cut..].parse().ok()?, digits))
}

/// `get_next_id`: the next free requirement id under a scope, by the prefix rule of the ids the scope already holds (the commonest
/// text before the trailing number, one more than the highest number written with it, with the same padding). A scope with no ids
/// needs `prefix`; a requirement scope numbers its children after its own id and the separator. Read-only: nothing is reserved.
pub fn next_id(m: &Model, scope: &str, prefix: Option<&str>, separator: Option<&str>) -> Res<Value> {
    if scope.len() > 500 || prefix.map(|p| p.len() > 60).unwrap_or(false) || separator.map(|p| p.len() > 60).unwrap_or(false) {
        return Err(Fail::from("scope is at most 500 characters; prefix and separator at most 60.".to_string()));
    }
    for t in prefix.iter().chain(separator.iter()) {
        if !t.is_empty() && !safe(t) {
            return Err(Fail::from("prefix and separator are short texts without quotes, angle brackets, backslash or line breaks.".to_string()));
        }
    }
    let sc = m.find(scope.trim()).ok_or_else(|| Fail::from(format!("scope {} is not an element of the model. Give the qualified name of a package or requirement in the argument scope, for example Package::Section.", super::text::quote_clip(scope))))?;
    // Every id in use in the model, so the answer is free everywhere.
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cut = false;
    for e in m.elements.iter().filter(|e| is_req(e)) {
        if taken.len() >= MAX_NEXT_READ {
            cut = true;
            break;
        }
        let id = id_of(m, e);
        if !id.is_empty() {
            taken.insert(id);
        }
    }
    // A bound that was hit means a free id cannot be confirmed: the answer is refused, never "free".
    if cut {
        return Err(Fail::from(format!("the model holds more than {MAX_NEXT_READ} requirement ids, so an id cannot be confirmed free. Fix: number by hand under a prefix of your own, or renumber a smaller scope.")));
    }
    if sc.children.len() > MAX_NEXT_READ {
        return Err(Fail::from(format!("the scope has more than {MAX_NEXT_READ} children; give a smaller scope.")));
    }
    let kids: Vec<String> = sc.children.iter().map(|c| &m.elements[*c]).filter(|c| is_req(c)).map(|c| id_of(m, c)).filter(|i| !i.is_empty()).take(MAX_NEXT_READ).collect();
    // The commonest prefix among the ids that end in a number.
    let mut by_prefix: std::collections::BTreeMap<String, (usize, usize, usize)> = std::collections::BTreeMap::new();
    for id in &kids {
        if let Some((p, n, w)) = split_number(id) {
            let e = by_prefix.entry(p).or_insert((0, 0, 0));
            e.0 += 1;
            e.1 = e.1.max(n);
            e.2 = e.2.max(if id.len() > 1 && id[id.len() - w..].starts_with('0') { w } else { 0 });
        }
    }
    let (basis, base, mut n, width) = match by_prefix.iter().max_by_key(|(p, v)| (v.0, std::cmp::Reverse((*p).clone()))) {
        Some((p, v)) => ("the prefix most of the ids under the scope share".to_string(), p.clone(), v.1 + 1, v.2),
        None => {
            let sep = separator.unwrap_or(".");
            if is_req(sc) && !id_of(m, sc).is_empty() {
                ("the id of the scope and the separator".to_string(), format!("{}{sep}", id_of(m, sc)), 1, 0)
            } else if let Some(p) = prefix.filter(|p| !p.is_empty()) {
                ("the prefix given".to_string(), p.to_string(), 1, 0)
            } else {
                return Err(Fail::from("the scope holds no requirement id to follow: give prefix, for example SR-.".to_string()));
            }
        }
    };
    let mut tries = 0usize;
    loop {
        let cand = format!("{base}{n:0width$}", width = width);
        if !taken.contains(&cand) {
            if !safe(&cand) {
                return Err(Fail::from("the next id would be empty or hold a character an id cannot: change prefix.".to_string()));
            }
            return Ok(json!({ "scope": sc.qualified_name, "next": cand, "prefix": base, "basis": basis, "ids_under_scope": kids.len(), "ids_in_model": taken.len(), "skipped_taken": tries, "truncated": cut, "reserved": false, "bounds": if cut { json!([format!("ids: the first {MAX_NEXT_READ} are read; an id beyond them may be taken")]) } else { json!([]) } }));
        }
        n += 1;
        tries += 1;
        if tries > MAX_NEXT_TRIES {
            return Err(Fail::from(format!("no free id after {MAX_NEXT_TRIES} tries: the numbers under this prefix are all taken.")));
        }
    }
}
