//! Member order: the order in which an owner lists its members is part of the text, and a compartment or a wide
//! view shows members in that order. This module moves members inside one body without touching any other byte:
//! `reorder_members`, and `before` / `after` / `position` on the tools that create a member.
//!
//! A member moves as a block: the statement, the `//` note lines directly above it (no blank line between), and a
//! `//` note that ends its last line. The text between two blocks (new lines, blank lines, indentation) stays in
//! its slot, so the new text is the old text with its blocks permuted.

use super::ids::IdEvent;
use super::intent::model_of;
use super::{brief, locate, qshow, Fail, Res};
use crate::api::FileIn;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Largest `order` list one call takes.
pub const MAX_ORDER: usize = 20_000;

/// Where a created member goes among the members of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    First,
    Last,
    Before(String),
    After(String),
}

/// `before`, `after` or `position` of an edit. At most one of them.
pub fn placement_of(a: &Value) -> Res<Option<Placement>> {
    let get = |k: &str| -> Res<Option<String>> {
        match a.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
            Some(Value::String(_)) => Err(Fail::from(format!("argument '{k}' is empty: it must be {}.", if k == "position" { "'first' or 'last'" } else { "the ref of a member of the same owner" }))),
            Some(_) => Err(Fail::from(format!("argument '{k}' must be a string: {}.", if k == "position" { "'first' or 'last'" } else { "the ref of a member of the same owner" }))),
        }
    };
    let (b, af, p) = (get("before")?, get("after")?, get("position")?);
    if [b.is_some(), af.is_some(), p.is_some()].iter().filter(|x| **x).count() > 1 {
        return Err("give one of before, after and position, not several. Fix: before / after take a sibling ref; position takes 'first' or 'last'.".into());
    }
    Ok(match (b, af, p) {
        (Some(b), _, _) => Some(Placement::Before(b)),
        (_, Some(a), _) => Some(Placement::After(a)),
        (_, _, Some(p)) => match p.to_lowercase().as_str() {
            "first" => Some(Placement::First),
            "last" => Some(Placement::Last),
            other => return Err(format!("position must be 'first' or 'last', not {}. Fix: use before / after with a sibling ref for any other place.", crate::agent::text::quote_clip(other)).into()),
        },
        _ => None,
    })
}

/// True when the edit asks for a place other than the end.
pub fn asks_for_place(a: &Value) -> bool {
    ["before", "after", "position"].iter().any(|k| a.get(*k).map(|v| !v.is_null()).unwrap_or(false))
}

fn leading_ws(s: &str) -> &str {
    &s[..s.len() - s.trim_start_matches([' ', '\t']).len()]
}

/// A line that is only a `//` note (not the `//* ... */` form, which can run over several lines).
fn is_note_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("//") && !t.starts_with("//*")
}

/// The blocks of an owner's members: `(start, end, ends_with_note)` in the file text, in member order, with the
/// bounds of the body text between the braces.
struct Layout {
    lo: usize,
    hi: usize,
    blocks: Vec<(usize, usize, bool)>,
}

fn layout(model: &Model, owner: &Element) -> Res<Layout> {
    let name = qshow(&owner.qualified_name);
    let text = &model.files[owner.file].text;
    let body = owner.body_span.ok_or_else(|| Fail::from(format!("{name} has no body, so it has no members to order.")))?;
    let braces = body.end >= body.start + 2 && text.get(body.start..).is_some_and(|t| t.starts_with('{')) && text.get(..body.end).is_some_and(|t| t.ends_with('}'));
    if !braces {
        return Err(format!("{name} has a '{{' with no closing '}}', so its members cannot be ordered. Fix: close the body first.").into());
    }
    let (lo, hi) = (body.start + 1, body.end - 1);
    let kids: Vec<&Element> = owner.children.iter().map(|c| &model.elements[*c]).collect();
    let mut prev_end = lo;
    for k in &kids {
        if k.file != owner.file || k.span.start < prev_end || k.span.end <= k.span.start || k.span.end > hi {
            return Err(Fail::from(format!(
                "{name} cannot be ordered: the member {} is not written as a statement of its own inside the body (it overlaps another member, or comes from a declaration written elsewhere). Nothing was written.",
                qshow(&k.qualified_name)
            )));
        }
        prev_end = k.span.end;
    }
    let mut blocks: Vec<(usize, usize, bool)> = Vec::with_capacity(kids.len());
    for (i, k) in kids.iter().enumerate() {
        let s = k.span;
        let limit = kids.get(i + 1).map(|n| n.span.start).unwrap_or(hi);
        let eol = text[s.end..limit].find('\n').map(|p| s.end + p).unwrap_or(limit);
        let seg = &text[s.end..eol];
        let note = is_note_line(seg);
        let end = if note { s.end + seg.trim_end().len() } else { s.end };
        let prev_block_end = blocks.last().map(|b| b.1).unwrap_or(lo);
        // The first line start where a note line may begin: the line after the end of the previous block.
        let floor = text[prev_block_end..s.start].find('\n').map(|p| prev_block_end + p + 1).unwrap_or(usize::MAX);
        let ls = text[..s.start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let mut start = s.start;
        if ls >= floor && text[ls..s.start].chars().all(|c| c == ' ' || c == '\t') {
            start = ls;
            while start > floor {
                let pls = text[..start - 1].rfind('\n').map(|p| p + 1).unwrap_or(0);
                if pls >= floor && is_note_line(&text[pls..start - 1]) {
                    start = pls;
                } else {
                    break;
                }
            }
        }
        blocks.push((start, end, note));
    }
    Ok(Layout { lo, hi, blocks })
}

/// The file text with the members of `owner` in `order` (`order[j]` is the old position of the member that goes to
/// position `j`), and the new position of every old position.
fn permuted(model: &Model, owner: &Element, order: &[usize]) -> Res<(String, Vec<usize>)> {
    let n = owner.children.len();
    let mut to = vec![usize::MAX; n];
    for (j, old) in order.iter().enumerate() {
        match to.get_mut(*old) {
            Some(t) if *t == usize::MAX => *t = j,
            _ => return Err("the new order is not a permutation of the members. Report this as a bug.".into()),
        }
    }
    if order.len() != n || to.contains(&usize::MAX) {
        return Err("the new order does not list every member once. Report this as a bug.".into());
    }
    let text = &model.files[owner.file].text;
    let lay = layout(model, owner)?;
    let b = &lay.blocks;
    let indent = {
        let ls = text[..owner.span.start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        leading_ws(&text[ls..]).to_string()
    };
    let child_indent = {
        let first = &model.elements[owner.children[0]];
        let ls = text[..first.span.start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if ls >= lay.lo { leading_ws(&text[ls..]).to_string() } else { format!("{indent}    ") }
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..b[0].0]);
    for (j, &old) in order.iter().enumerate() {
        let (s, e, note) = b[old];
        out.push_str(&text[s..e]);
        // The text after slot `j` stays where it was; a note that runs to the end of its line needs a line break behind it.
        let (sep_from, sep_to) = if j + 1 < n { (b[j].1, b[j + 1].0) } else { (b[j].1, lay.hi) };
        let sep = &text[sep_from..sep_to];
        if note && !sep.contains('\n') {
            out.push('\n');
            out.push_str(if j + 1 < n { &child_indent } else { &indent });
        } else {
            out.push_str(sep);
        }
    }
    out.push_str(&text[lay.hi..]);
    Ok((out, to))
}

/// What a member says apart from where it sits (kind and text, white space collapsed): the check that a move
/// changed nothing but the order.
fn same_statement(a: &Model, ea: &Element, b: &Model, eb: &Element) -> bool {
    ea.kind == eb.kind && ea.is_def == eb.is_def && ea.name == eb.name && super::ids::signature(a, ea) == super::ids::signature(b, eb)
}

/// Put the members of `owner` in `order` (old positions, one for each member) by rewriting the file in `work`.
/// Returns the new position of each old position. The result is parsed again and must hold the same members.
fn apply_order(work: &mut [FileIn], model: &Model, owner: &Element, order: &[usize]) -> Res<Vec<usize>> {
    let (text, to) = permuted(model, owner, order)?;
    let path = &model.files[owner.file].path;
    let slot = work.iter().position(|f| &f.path == path).ok_or_else(|| Fail::from("the file of the owner is gone. Report this as a bug."))?;
    let old_text = std::mem::replace(&mut work[slot].text, text);
    let after = model_of(work);
    let fine = after.elements.get(owner.id).filter(|o| o.qualified_name == owner.qualified_name).map(|o| {
        o.children.len() == owner.children.len()
            && order.iter().zip(&o.children).all(|(old, new)| same_statement(model, &model.elements[owner.children[*old]], &after, &after.elements[*new]))
    });
    if fine != Some(true) {
        work[slot].text = old_text;
        return Err(Fail::from(format!(
            "the members of {} could not be moved without changing a statement (the text between them is not plain white space and notes); nothing was written. Fix: order that owner by hand with edit_model replace, or report the text.",
            qshow(&owner.qualified_name)
        )));
    }
    Ok(to)
}

/// `Owner::$3` after the members of `owner` moved by `to` (new position of each old position).
pub fn reposition(qn: &str, owner: &str, to: &[usize]) -> Option<String> {
    let rest = qn.strip_prefix(owner)?.strip_prefix("::")?;
    let seg = super::ids::split_qn(rest).into_iter().next()?;
    if !super::ids::is_positional(seg) {
        return None;
    }
    let k: usize = seg[1..].parse().ok()?;
    let new = *to.get(k.checked_sub(1)?)?;
    (new + 1 != k).then(|| format!("{owner}::${}{}", new + 1, &rest[seg.len()..]))
}

/// Positional refs that changed: `[{from, to}]`, at most `cap`, and how many there are.
fn renumbered(model: &Model, owner: &Element, to: &[usize], cap: usize) -> (Vec<Value>, usize) {
    let mut rows = Vec::new();
    let mut count = 0;
    for (old, c) in owner.children.iter().enumerate() {
        let e = &model.elements[*c];
        if to[old] == old || !(e.name.is_none() || e.implied_name) {
            continue;
        }
        count += 1;
        if rows.len() < cap {
            if let Some(now) = reposition(&e.qualified_name, &owner.qualified_name, to) {
                rows.push(json!({ "from": e.qualified_name, "to": now }));
            }
        }
    }
    (rows, count)
}

fn renumber_warning(count: usize, owner: &str) -> Value {
    json!({
        "rule": "positional-refs-renumbered",
        "severity": "warning",
        "message": format!("{count} unnamed member(s) of {} got a new $n ref, because $n is the position among the members.", qshow(owner)),
        "fix": "Refer to unnamed statements by source_id:<id> or read the new refs from export_facts; renumbered[] lists the old and new refs."
    })
}

fn push_warning(info: &mut Value, w: Value) {
    match info.get_mut("warnings").and_then(Value::as_array_mut) {
        Some(l) => l.push(w),
        None => info["warnings"] = json!([w]),
    }
}

/// `reorder_members {owner, order}`: the listed members come first, in the given order; members not listed keep their
/// relative order after them.
pub fn reorder_members(work: &mut Vec<FileIn>, a: &Value, events: &mut Vec<IdEvent>) -> Res<Value> {
    let model = model_of(work);
    let owner_ref = super::intent::req(a, "owner")?;
    let owner = locate(&model, owner_ref)?;
    let oname = qshow(&owner.qualified_name);
    let list = match a.get("order") {
        Some(Value::Array(l)) if !l.is_empty() => l,
        Some(Value::Array(_)) => return Err(format!("order is empty. Fix: list the members of {oname} in the order they should come, as refs or source_id:<id>.").into()),
        None | Some(Value::Null) => return Err(format!("missing argument 'order': the members of {oname} in the order they should come, as refs or source_id:<id>.").into()),
        Some(_) => return Err("argument 'order' must be a list of refs (or source_id:<id>).".into()),
    };
    if list.len() > MAX_ORDER {
        return Err(format!("order lists {} members; at most {MAX_ORDER} per call.", list.len()).into());
    }
    let n = owner.children.len();
    let by_name: HashMap<&str, usize> = owner.children.iter().enumerate().map(|(i, c)| (model.elements[*c].qualified_name.as_str(), i)).collect();
    let mut listed: Vec<usize> = Vec::with_capacity(list.len());
    let mut seen = vec![false; n];
    for (i, v) in list.iter().enumerate() {
        let r = v.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| Fail::from(format!("order[{}] must be a ref (or source_id:<id>) as text.", i + 1)))?;
        let pos = match by_name.get(r) {
            Some(p) => *p,
            None => {
                let rel = format!("{}::{}", owner.qualified_name, r);
                let el = match locate(&model, r) {
                    Ok(e) => Some(e),
                    Err(_) => model.find(&rel),
                };
                let Some(el) = el else {
                    return Err(Fail::with(
                        format!("order[{}]: no member {} in {oname}, which has {n} members. Fix: list refs of direct members of the owner (query_elements {{owner}}).", i + 1, crate::agent::text::quote_clip(r)),
                        json!({ "index": i + 1, "ref": r, "owner": owner.qualified_name, "members": n }),
                    ));
                };
                match owner.children.iter().position(|c| *c == el.id) {
                    Some(p) => p,
                    None => {
                        let home = el.owner.map(|o| format!("a member of {}", qshow(&model.elements[o].qualified_name))).unwrap_or_else(|| "a top-level element".into());
                        return Err(Fail::with(
                            format!("order[{}]: {} is {home}, not a member of {oname}, which has {n} members. Fix: list only direct members of the owner.", i + 1, qshow(&el.qualified_name)),
                            json!({ "index": i + 1, "ref": el.qualified_name, "owner": owner.qualified_name, "members": n }),
                        ));
                    }
                }
            }
        };
        if seen[pos] {
            return Err(format!("order[{}]: {} is listed twice. Fix: list each member once.", i + 1, qshow(&model.elements[owner.children[pos]].qualified_name)).into());
        }
        seen[pos] = true;
        listed.push(pos);
    }
    let mut order = listed.clone();
    order.extend((0..n).filter(|p| !seen[*p]));
    let mut info = json!({ "owner": owner.qualified_name, "members": n, "listed": listed.len(), "unlisted": n - listed.len() });
    if order.iter().enumerate().all(|(j, o)| j == *o) {
        info["moved"] = json!(0);
        info["unchanged"] = json!(n);
        return Ok(info);
    }
    let to = apply_order(work, &model, owner, &order)?;
    let moved = to.iter().enumerate().filter(|(old, new)| old != *new).count();
    let (rows, count) = renumbered(&model, owner, &to, 50);
    info["moved"] = json!(moved);
    info["unchanged"] = json!(n - moved);
    if count > 0 {
        info["renumbered"] = json!(rows);
        info["renumbered_count"] = json!(count);
        push_warning(&mut info, renumber_warning(count, &owner.qualified_name));
    }
    events.push(IdEvent::Reorder { owner: owner.qualified_name.clone(), to });
    Ok(info)
}

/// The refs of the members a creating edit made, in the order they were made.
fn made_refs(name: &str, info: &Value, after: &Model) -> Vec<String> {
    let one = |k: &str| info[k].as_str().map(str::to_string);
    match name {
        // A connection and the flow beside it.
        "add_interface" => [one("connection"), one("flow")].into_iter().flatten().collect(),
        // Every top-level member of the text that was added.
        "add_element" => {
            let rows: Vec<String> = info["created"].as_array().into_iter().flatten().filter_map(|c| c["ref"].as_str().map(str::to_string)).collect();
            let owner = rows.first().and_then(|r| after.find(r)).map(|e| e.owner);
            rows.into_iter().filter(|r| after.find(r).map(|e| Some(e.owner) == owner).unwrap_or(false)).collect()
        }
        _ => one("ref").into_iter().collect(),
    }
}

fn remap_strings(v: &mut Value, owner: &str, to: &[usize]) {
    match v {
        Value::String(s) => {
            if let Some(n) = reposition(s, owner, to) {
                *s = n;
            }
        }
        Value::Array(l) => l.iter_mut().for_each(|x| remap_strings(x, owner, to)),
        Value::Object(o) => o.values_mut().for_each(|x| remap_strings(x, owner, to)),
        _ => {}
    }
}

/// Move what a creating edit just made to the place the edit asked for. `before` is the parse of the files before the
/// edit, `info` the edit's reply (its refs follow the move), `work` the files after it.
pub fn place_made(work: &mut Vec<FileIn>, before: &Model, name: &str, p: &Placement, info: &mut Value, events: &mut Vec<IdEvent>) -> Res<()> {
    if info["existed"] == true {
        info["placement"] = json!({ "applied": false, "why": "the element existed, so nothing was written or moved" });
        return Ok(());
    }
    if *p == Placement::Last {
        return Ok(());
    }
    let after = model_of(work);
    let refs = made_refs(name, info, &after);
    let mut made: Vec<usize> = Vec::new();
    let mut owner_id: Option<Option<ElementId>> = None;
    for r in &refs {
        let Some(e) = after.find(r).filter(|_| before.find(r).is_none()) else { continue };
        if owner_id.is_some_and(|o| o != e.owner) {
            continue;
        }
        owner_id = Some(e.owner);
        made.push(e.id);
    }
    let Some(Some(owner_id)) = owner_id else {
        return Err(if owner_id.is_some() {
            "before, after and position: 'first' place members of an owner; a top-level element goes at the end of its file. Fix: drop the argument, or create the element inside a package.".into()
        } else {
            Fail::from(format!("{name} made no new member, so there is nothing to place. Fix: drop before / after / position."))
        });
    };
    let owner = &after.elements[owner_id];
    let oname = qshow(&owner.qualified_name);
    let kids = &owner.children;
    made.sort_by_key(|id| kids.iter().position(|c| c == id).unwrap_or(usize::MAX));
    let others: Vec<ElementId> = kids.iter().copied().filter(|c| !made.contains(c)).collect();
    let at = match p {
        Placement::First => 0,
        Placement::Last => others.len(),
        Placement::Before(r) | Placement::After(r) => {
            let sib = locate(&after, r).ok().or_else(|| after.find(&format!("{}::{}", owner.qualified_name, r)));
            let hit = sib.and_then(|s| others.iter().position(|c| *c == s.id));
            let Some(i) = hit else {
                let key = if matches!(p, Placement::Before(_)) { "before" } else { "after" };
                let why = match sib {
                    Some(s) if made.contains(&s.id) => "it is the new member itself".to_string(),
                    Some(s) => format!("it is {}", s.owner.map(|o| format!("a member of {}", qshow(&after.elements[o].qualified_name))).unwrap_or_else(|| "a top-level element".into())),
                    None => "no such element".to_string(),
                };
                return Err(Fail::with(
                    format!("{key}: {} is not a member of {oname}: {why}. {oname} has {} members besides the new one. Fix: pass a direct member of that owner (query_elements {{owner}}).", crate::agent::text::quote_clip(r), others.len()),
                    json!({ "owner": owner.qualified_name, "members": others.len(), key: r }),
                ));
            };
            if matches!(p, Placement::Before(_)) { i } else { i + 1 }
        }
    };
    let mut new_kids: Vec<ElementId> = others[..at].to_vec();
    new_kids.extend(&made);
    new_kids.extend(&others[at..]);
    if new_kids == *kids {
        info["placement"] = json!({ "owner": owner.qualified_name, "index": at, "members": kids.len() });
        return Ok(());
    }
    let order: Vec<usize> = new_kids.iter().map(|id| kids.iter().position(|c| c == id).unwrap_or(0)).collect();
    let to = apply_order(work, &after, owner, &order)?;
    let (_, count) = renumbered(&after, owner, &to, 0);
    let now = model_of(work);
    remap_strings(info, &owner.qualified_name, &to);
    for row in info["created"].as_array_mut().into_iter().flatten() {
        if let Some(e) = row["ref"].as_str().and_then(|r| now.find(r)) {
            let fresh = brief(&now, e);
            for (k, v) in fresh.as_object().into_iter().flatten() {
                row[k] = v.clone();
            }
        }
    }
    info["placement"] = json!({ "owner": owner.qualified_name, "index": at, "members": kids.len() });
    if count > 0 {
        push_warning(info, renumber_warning(count, &owner.qualified_name));
    }
    events.push(IdEvent::Reorder { owner: owner.qualified_name.clone(), to });
    Ok(())
}
