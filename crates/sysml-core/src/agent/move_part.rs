//! `move_part {part, into}`: re-own a part that was modelled beside the part that owns it (rule O1, The part usage moves under the new holder (the definition that types `into`, or
//! `into` itself when it has none); every connection to it follows:
//!
//! - a connection or flow written beside the two parts that joins the part to a part inside `into` moves with it, into the
//!   holder, and its other end loses the `into.` step (`connect board.eth to router.sw.eth` becomes `connect board.eth to
//!   sw.eth` inside the router);
//! - a connection that joins the part to anything outside `into` goes through a port on the holder's border, typed and
//!   conjugated as the part's own port, with a delegation inside the holder (`connect eth to board.eth`), as
//!   `add_interface` writes a link to a nested part (abstraction rule A2);
//! - any other name that reaches the part through a feature chain gets the `into.` step; plain names follow the move as
//!   `update_element {owner}` makes them follow.
//!
//! The text of every element is kept, so short ids (`<'id'>`) stay; source ids follow the moves. The ownership rules of
//! The ownership rules apply (the move is `update_element {owner}`'s, which refuses what Cameo refuses). If the
//! result leaves a name unresolved that resolved before, nothing is written.

use super::connect::typed_def;
use super::ids::{self, IdEvent};
use super::intent::{add, model_of, move_element, req, step};
use super::{locate, Fail, Res};
use crate::api::FileIn;
use crate::edit::EditOp;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

fn unresolved(m: &Model) -> usize {
    m.diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).count()
}

fn is_connector(e: &Element) -> bool {
    !e.is_def && matches!(e.kind.as_str(), "connection" | "interface" | "flow" | "binding" | "connect")
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

struct BorderPort {
    name: String,
    ty: Option<String>,
    conj: bool,
    inner: String,
}

pub fn move_part(work: &mut Vec<FileIn>, a: &Value, events: &mut Vec<IdEvent>) -> Res<Value> {
    let m0 = model_of(work);
    let p = locate(&m0, req(a, "part")?)?;
    let s = locate(&m0, req(a, "into")?)?;
    for (role, e) in [("part", p), ("into", s)] {
        if e.kind != "part" || e.is_def {
            return Err(format!("{role} '{}' is a {}; move_part moves a part usage into a part usage beside it.", e.qualified_name, e.keyword()).into());
        }
    }
    if p.id == s.id {
        return Err("part and into are the same part; pick the part that should hold it".into());
    }
    if p.owner != s.owner {
        return Err(Fail::from(format!(
            "'{}' and '{}' are not beside each other (they have different owners). move_part moves a part into a part of the same owner; use update_element {{owner}} for any other move.",
            p.qualified_name, s.qualified_name
        )));
    }
    let holder_id = typed_def(&m0, s).filter(|d| m0.elements[*d].kind == "part").unwrap_or(s.id);
    if std::iter::successors(Some(holder_id), |i| m0.elements[*i].owner).any(|i| i == p.id) {
        return Err(format!("'{}' lives inside '{}', so it cannot hold it.", m0.elements[holder_id].qualified_name, p.qualified_name).into());
    }
    let holder = m0.elements[holder_id].qualified_name.clone();
    // The parts the holder types, lifelines of sequences (`ref part`) left out.
    let holder_usages = m0.elements.iter().filter(|e| e.kind == "part" && !e.is_def && !e.has_prefix("ref") && typed_def(&m0, e) == Some(holder_id)).count();
    let (p_id, s_id) = (p.id, s.id);
    let (p_name, s_name) = (quote_name(&p.display_name()), quote_name(&s.display_name()));
    let p_qn = p.qualified_name.clone();
    let inside_p: HashSet<ElementId> = m0.descendants(p_id).into_iter().chain([p_id]).collect();
    let holder_names: HashSet<String> = crate::agent::text::all_members(&m0, holder_id).into_iter().filter_map(|c| m0.elements[c].name.clone()).collect();

    // The plan, read from the model before anything changes.
    let mut inplace: Vec<(usize, Span, String)> = Vec::new();
    let mut moving: Vec<(ElementId, String)> = Vec::new();
    let mut rewired: Vec<Value> = Vec::new();
    let mut crossings: Vec<String> = Vec::new();
    let mut ports: BTreeMap<ElementId, BorderPort> = BTreeMap::new();
    let mut taken: HashSet<String> = holder_names.clone();
    for e in m0.elements.iter().filter(|e| !inside_p.contains(&e.id)) {
        let ends: Vec<&Reference> = e.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target)).collect();
        for r in &e.refs {
            let Some(k) = r.segments.iter().position(|g| g.resolved == Some(p_id)) else { continue };
            let connector = is_connector(e) && matches!(r.role, RefRole::Source | RefRole::Target);
            if connector {
                // Beside the two parts, joining the part to a part inside `into`: the statement moves into the holder.
                let other = ends.iter().find(|o| !std::ptr::eq(**o, r));
                let into_inside = other.filter(|o| o.segments.len() >= 2 && o.segments[0].resolved == Some(s_id));
                if let (0, Some(o), true) = (k, into_inside, e.owner == m0.elements[p_id].owner) {
                    if let Some(n) = e.name.as_ref().filter(|_| !e.implied_name) {
                        if holder_names.contains(n) {
                            return Err(format!("'{holder}' already has a member named '{n}', the name of '{}', which would move there. Fix: rename one of them first.", e.qualified_name).into());
                        }
                    }
                    let file = &m0.files[e.file].text;
                    let mut text = file[e.span.start..e.span.end].to_string();
                    let (a0, a1) = (o.segments[0].span.start - e.span.start, o.segments[1].span.start - e.span.start);
                    let before = o.text.clone();
                    text.replace_range(a0..a1, "");
                    rewired.push(json!({ "statement": e.qualified_name, "before": before, "after": o.text.replacen(&format!("{}.", o.segments[0].name), "", 1), "how": format!("moved into {holder}") }));
                    moving.push((e.id, text));
                    break;
                }
                // To anything else: through a port on the holder's border.
                if let Some(port) = r.segments.get(k + 1).and_then(|g| g.resolved).filter(|x| m0.elements[*x].kind == "port") {
                    let pe = &m0.elements[port];
                    let bp = ports.entry(port).or_insert_with(|| {
                        let pn = pe.display_name();
                        let mut name = pn.clone();
                        if taken.contains(&name) {
                            name = format!("{}{}", super::connect::ident(&p.display_name(), false), cap(&super::connect::ident(&pn, false)));
                        }
                        let mut i = 2;
                        while taken.contains(&name) {
                            name = format!("{}{}{i}", super::connect::ident(&p.display_name(), false), cap(&super::connect::ident(&pn, false)));
                            i += 1;
                        }
                        taken.insert(name.clone());
                        let ty = pe.refs_of(RefRole::Type).next();
                        BorderPort {
                            name,
                            ty: ty.and_then(|t| t.resolved()).map(|t| m0.elements[t].qualified_name.clone()).or(ty.map(|t| t.text.clone())),
                            conj: ty.map(|t| t.conjugated).unwrap_or(false),
                            inner: quote_name(&pn),
                        }
                    });
                    let span = Span { start: r.segments[k].span.start, end: r.segments[k + 1].span.end };
                    let new = format!("{s_name}.{}", quote_name(&bp.name));
                    let file = &m0.files[e.file].text;
                    let after = format!("{}{}{}", &file[r.span.start..span.start], new, &file[span.end..r.span.end]);
                    rewired.push(json!({ "statement": e.qualified_name, "before": r.text, "after": after, "how": format!("through the border port {holder}::{}", bp.name) }));
                    inplace.push((e.file, span, new));
                    continue;
                }
                crossings.push(format!("{} reaches {} itself, not a port of it: it now crosses the border of {} (connector-crosses-boundary)", e.qualified_name, p.display_name(), s.display_name()));
            } else if !r.text.contains('.') {
                // A plain name follows the move (update_element {owner}).
                continue;
            }
            let at = r.segments[k].span.start;
            inplace.push((e.file, Span { start: at, end: at }, format!("{s_name}.")));
            if connector {
                rewired.push(json!({ "statement": e.qualified_name, "before": r.text, "after": format!("{}{s_name}.{}", &r.text[..at - r.span.start], &r.text[at - r.span.start..]), "how": "through the new holder, no port" }));
            }
        }
    }

    let snapshot = work.clone();
    let fail = |work: &mut Vec<FileIn>, msg: String| -> Res<Value> {
        *work = snapshot.clone();
        Err(Fail::from(format!("{msg} Nothing was written.")))
    };
    // 1. Ends and names rewritten where they stand.
    let mut by_file: HashMap<usize, Vec<(Span, String)>> = HashMap::new();
    for (f, sp, t) in inplace {
        by_file.entry(f).or_default().push((sp, t));
    }
    for (f, mut edits) in by_file {
        let path = m0.files[f].path.clone();
        let Some(file) = work.iter_mut().find(|x| x.path == path) else { continue };
        edits.sort_by_key(|(sp, _)| std::cmp::Reverse((sp.start, sp.end)));
        edits.dedup_by_key(|(sp, _)| (sp.start, sp.end));
        for (sp, t) in edits {
            file.text.replace_range(sp.start..sp.end, &t);
        }
    }
    // 2. The statements that move into the holder leave their place, last first so the positional names of the others hold.
    moving.sort_by_key(|(id, _)| m0.elements[*id].span.start);
    let mut left: Vec<(String, Option<String>, usize)> = Vec::new();
    for (id, _) in moving.iter().rev() {
        // Names did not change in step 1, so the statement is found by its name in the model before.
        let qn = m0.elements[*id].qualified_name.clone();
        let m = model_of(work);
        let Some(e) = m.find(&qn) else { return fail(work, format!("'{qn}' was not found after its ends were rewritten.")) };
        let (owner, index) = ids::place_of(&m, e);
        if let Err(x) = step(work, EditOp::Delete { target: qn.clone() }) {
            return fail(work, format!("removing '{qn}' failed: {}.", x.message));
        }
        left.push((qn, owner, index));
    }
    // 3. The part itself.
    let m2 = model_of(work);
    let Some(pt) = m2.find(&p_qn) else { return fail(work, format!("'{p_qn}' was not found before the move.")) };
    let mut part_events = Vec::new();
    let moved = match move_element(work, &m2, pt, &holder, &mut part_events) {
        Ok(v) => v,
        Err(x) => return fail(work, format!("moving '{p_qn}' into '{holder}' was refused: {}", x.message)),
    };
    // The statements in the holder, in their order.
    let mut to: Vec<String> = Vec::new();
    for (id, text) in &moving {
        let e = &m0.elements[*id];
        let text = super::intent::dedent_after_first(text, &m0.files[e.file].text[..e.span.start]);
        if let Err(x) = add(work, Some(&holder), None, &text) {
            return fail(work, format!("writing '{}' into '{holder}' failed: {}.", e.qualified_name, x.message));
        }
        let m = model_of(work);
        to.push(m.find(&holder).and_then(|h| h.children.last()).map(|c| m.elements[*c].qualified_name.clone()).unwrap_or_default());
    }
    let mut moved_stmts: Vec<Value> = Vec::new();
    for (k, (from, owner, index)) in left.into_iter().enumerate() {
        let t = to[moving.len() - 1 - k].clone();
        events.push(IdEvent::Move { from: from.clone(), to: t.clone(), owner, index });
        moved_stmts.insert(0, json!({ "from": from, "to": t }));
    }
    events.extend(part_events);
    // 4. The border ports and their delegations.
    let mut ports_added = Vec::new();
    let mut delegations = Vec::new();
    for bp in ports.values() {
        let m = model_of(work);
        let exists = m.find(&holder).map(|h| m.children(h.id).any(|c| c.name.as_deref() == Some(bp.name.as_str()))).unwrap_or(false);
        if !exists {
            let text = match &bp.ty {
                Some(t) => format!("port {} : {}{t};", quote_name(&bp.name), if bp.conj { "~" } else { "" }),
                None => format!("port {};", quote_name(&bp.name)),
            };
            if let Err(x) = add(work, Some(&holder), None, &text) {
                return fail(work, format!("adding the border port '{}' to '{holder}' failed: {}.", bp.name, x.message));
            }
            ports_added.push(format!("{holder}::{}", quote_name(&bp.name)));
        }
        let link = format!("connect {} to {p_name}.{};", quote_name(&bp.name), bp.inner);
        if let Err(x) = add(work, Some(&holder), None, &link) {
            return fail(work, format!("adding the delegation '{link}' failed: {}.", x.message));
        }
        let m = model_of(work);
        if let Some(c) = m.find(&holder).and_then(|h| h.children.last()) {
            delegations.push(m.elements[*c].qualified_name.clone());
        }
    }
    let after = model_of(work);
    if unresolved(&after) > unresolved(&m0) {
        let first = after.diagnostics.iter().find(|d| d.message.starts_with("cannot resolve")).map(|d| d.message.clone()).unwrap_or_default();
        return fail(work, format!("moving '{p_qn}' into '{holder}' would leave {} name(s) unresolved (first: {first}).", unresolved(&after) - unresolved(&m0)));
    }
    let new_ref = moved["ref"].as_str().unwrap_or_default().to_string();
    let mut out = json!({
        "ref": new_ref,
        "previous_ref": p_qn,
        "into": m0.elements[s_id].qualified_name,
        "holder": holder,
        "statements_moved": moved_stmts,
        "ends_rewired": rewired,
        "ports_added": ports_added,
        "delegations_added": delegations,
        "crossings": crossings,
        "references_followed": moved.get("references_followed").cloned().unwrap_or(json!(0)),
    });
    if holder_usages > 1 {
        out["note"] = json!(format!("{holder} types {holder_usages} part usages: each of them now holds {}.", m0.elements[p_id].display_name()));
    }
    Ok(out)
}
