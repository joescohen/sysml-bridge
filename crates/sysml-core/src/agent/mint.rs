//! Element ids written in the model text. Every named definition and usage of a kind that carries traceability gets a
//! short name (`part def <ek7m2qd> Analyser`) when a tool creates it. A short name is SysML v2's own slot for an id: it
//! travels with the .sysml file into any conforming tool (Cameo shows it as the Declared Short Name), a reference may
//! name the element by it, and a rename of the declared name leaves it alone.
//!
//! An id is `e` and six characters from a 32-letter alphabet, taken from a hash of the element's kind, qualified name
//! at creation and a counter, so it is not a sequence an agent could guess and two branches that create different
//! elements do not collide. The counter moves on while the id is taken anywhere in the model. Requirement ids people
//! cite (`<'R-12'>`) are left as they are: an element that has a short name keeps it.

use crate::api::FileIn;
use crate::model::{quote_name, Element, ElementId, Model, RESERVED};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The kinds a tool gives an id: the concept kinds of the trace rules, and packages and views.
pub const MINTED_KINDS: &[&str] = &["part", "item", "port", "action", "state", "interface", "connection", "requirement", "attribute", "enum", "flow", "allocation", "constraint", "calc", "verification", "use case", "package", "view", "occurrence", "analysis", "concern", "viewpoint", "rendering", "case"];

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Whether an element of this keyword gets an id.
pub fn is_minted_kind(kind: &str) -> bool {
    MINTED_KINDS.contains(&kind)
}

/// Whether a short name is the shape of a minted id.
pub fn is_minted_id(s: &str) -> bool {
    s.len() == 7 && s.starts_with('e') && s.bytes().skip(1).all(|b| ALPHABET.contains(&b))
}

/// The ids (short names) taken in the model.
pub fn taken(model: &Model) -> HashSet<String> {
    model.elements.iter().filter_map(|e| e.short_name.clone()).collect()
}

/// A new id for an element of `kind` at `qn`, not in `taken`; it is added to `taken`.
pub fn mint(kind: &str, qn: &str, taken: &mut HashSet<String>) -> String {
    for n in 0u32.. {
        let mut h = fnv(&format!("{kind}|{qn}|{n}"));
        let mut id = String::from("e");
        for _ in 0..6 {
            id.push(ALPHABET[(h & 31) as usize] as char);
            h >>= 5;
        }
        if !RESERVED.contains(&id.as_str()) && taken.insert(id.clone()) {
            return id;
        }
    }
    unreachable!("the counter runs until an id is free")
}

/// Whether this element should carry an id and does not.
pub fn wants_id(e: &Element) -> bool {
    e.short_name.is_none() && !e.implied_name && e.name.is_some() && is_minted_kind(&e.kind)
}

/// The `mint_ids` edit: every named element of a minted kind without an id gets one (`scope`: only under that element),
/// or, with `ref`, that one element gets a new id in place of the one it has (a copy that shares its original's id).
/// The reply counts them and names the first ones.
pub fn mint_ids(work: &mut Vec<FileIn>, a: &Value) -> super::Res<Value> {
    let model = super::intent::model_of(work);
    let mut taken = taken(&model);
    if let Some(r) = a.get("ref").and_then(Value::as_str) {
        let e = super::locate(&model, r)?;
        if e.name.is_none() || e.implied_name {
            return Err(format!("{} has no declared name, so it has no id. Fix: give it a name first.", e.qualified_name).into());
        }
        let id = mint(&e.kind, &format!("{}|remint|{}", e.qualified_name, e.short_name.as_deref().unwrap_or("")), &mut taken);
        super::intent::step(work, crate::edit::EditOp::SetShortName { target: e.qualified_name.clone(), short: id.clone() })?;
        return Ok(json!({ "ref": e.qualified_name, "id": id, "previous_id": e.short_name, "minted": 1 }));
    }
    let under = match a.get("scope").and_then(Value::as_str) {
        Some(s) => Some(super::locate(&model, s)?.id),
        None => None,
    };
    let inside = |mut x: usize| loop {
        if Some(x) == under {
            return true;
        }
        match model.elements[x].owner {
            Some(o) => x = o,
            None => return under.is_none(),
        }
    };
    let inserts = plan(&model, &mut taken, |e| inside(e.id));
    let n = inserts.len();
    let first: Vec<Value> = inserts.iter().take(20).map(|(_, _, id, qn)| json!({ "ref": qn, "id": id })).collect();
    if json_flag(a, "dry_run") {
        return Ok(json!({ "minted": n, "first": first, "dry_run": true }));
    }
    write(work, &model, inserts)?;
    Ok(json!({ "minted": n, "first": first }))
}

/// Ids for the elements at `roots` and everything inside them that wants one (the definitions add_interface makes, as
/// create_element would have given them). The number minted.
pub(crate) fn mint_under(work: &mut [FileIn], roots: &[String]) -> usize {
    let model = super::intent::model_of(work);
    let mut taken = taken(&model);
    let under = |q: &str| roots.iter().any(|r| q == r || (q.starts_with(r.as_str()) && q[r.len()..].starts_with("::")));
    let inserts = plan(&model, &mut taken, |e| e.short_name.is_none() && under(&e.qualified_name));
    let n = inserts.len();
    if n == 0 || write(work, &model, inserts).is_err() {
        return 0;
    }
    n
}

/// `(file, offset, id, qualified name)` for each element `pick` takes that wants an id (library elements never).
fn plan(model: &Model, taken: &mut HashSet<String>, pick: impl Fn(&Element) -> bool) -> Vec<(usize, usize, String, String)> {
    let mut inserts = Vec::new();
    for e in model.elements.iter().filter(|e| wants_id(e) && pick(e) && !super::trace::in_library(model, e.id)) {
        let Some(at) = e.name_span.map(|s| s.start) else { continue };
        let id = mint(&e.kind, &e.qualified_name, taken);
        inserts.push((e.file, at, id, e.qualified_name.clone()));
    }
    inserts
}

/// Inserts `<id> ` before each declared name, from the end of each file so earlier offsets hold.
fn write(work: &mut [FileIn], model: &Model, mut inserts: Vec<(usize, usize, String, String)>) -> super::Res<()> {
    inserts.sort_by(|x, y| (y.0, y.1).cmp(&(x.0, x.1)));
    for (file, at, id, _) in &inserts {
        let path = &model.files[*file].path;
        let Some(f) = work.iter_mut().find(|f| &f.path == path) else { return Err(format!("{path} is not in the file set").into()) };
        f.text.insert_str(*at, &format!("<{}> ", quote_name(id)));
    }
    Ok(())
}

/// After an edit that is not a tool's `create_element` (the page's add_element of raw text): every element it added
/// that wants an id gets one. `before` holds the qualified names the model had before the edit. The number minted.
pub fn mint_new(work: &mut [FileIn], before: &HashSet<String>) -> usize {
    let model = super::intent::model_of(work);
    let mut taken = taken(&model);
    let inserts = plan(&model, &mut taken, |e| !before.contains(&e.qualified_name));
    let n = inserts.len();
    if n == 0 || write(work, &model, inserts).is_err() {
        return 0;
    }
    n
}

/// A write that leaves two elements with one minted id (a subtree copied with its ids) gives each copy a new id in the
/// same write, in place of a refusal. The element the model had before keeps its id; of elements that are all new, the
/// first in the text keeps it. Ids that were shared before the write are left as they are (the semantic gate reports
/// them). One note for each id given: `{rule: duplicate-id, element, id_was, id, keeps, note}`.
pub fn remint_copies(before: &Model, work: &mut [FileIn]) -> Vec<Value> {
    let after = super::intent::model_of(work);
    let mut by: BTreeMap<&str, Vec<ElementId>> = BTreeMap::new();
    for e in &after.elements {
        if let Some(s) = e.short_name.as_deref().filter(|s| is_minted_id(s)) {
            by.entry(s).or_default().push(e.id);
        }
    }
    by.retain(|_, v| v.len() > 1);
    if by.is_empty() {
        return Vec::new();
    }
    let mut was: HashMap<&str, usize> = HashMap::new();
    let mut had: HashSet<(&str, &str, &str)> = HashSet::new();
    for e in &before.elements {
        if let Some(s) = e.short_name.as_deref().filter(|s| by.contains_key(s)) {
            *was.entry(s).or_default() += 1;
            had.insert((s, e.qualified_name.as_str(), e.kind.as_str()));
        }
    }
    let mut taken = taken(&after);
    let mut patches: Vec<(usize, usize, usize, String)> = Vec::new();
    let mut notes = Vec::new();
    for (id, els) in &by {
        if els.len() <= was.get(id).copied().unwrap_or(0) {
            continue;
        }
        let old: Vec<ElementId> = els.iter().copied().filter(|x| had.contains(&(*id, after.elements[*x].qualified_name.as_str(), after.elements[*x].kind.as_str()))).collect();
        let keeps: Vec<ElementId> = if old.is_empty() { vec![els[0]] } else { old };
        for x in els.iter().copied().filter(|x| !keeps.contains(x)) {
            let e = &after.elements[x];
            let Some(name) = e.name_span.filter(|_| !e.implied_name) else { continue };
            let text = &after.files[e.file].text;
            let Some(region) = text.get(e.span.start..name.start) else { continue };
            let Some((at, len)) = [format!("<{id}>"), format!("<'{id}'>")].iter().find_map(|w| region.find(w.as_str()).map(|i| (i, w.len()))) else { continue };
            let new = mint(&e.kind, &format!("{}|copy|{id}", e.qualified_name), &mut taken);
            let keeper = &after.elements[keeps[0]].qualified_name;
            notes.push(json!({
                "rule": super::duplicates::DUPLICATE_ID_RULE, "element": e.qualified_name, "id_was": id, "id": new, "keeps": keeper,
                "note": format!("{} carried the id {id} of {keeper} (a copy); it has the new id {new}, given in this write", e.qualified_name),
            }));
            patches.push((e.file, e.span.start + at, e.span.start + at + len, format!("<{new}>")));
        }
    }
    patches.sort_by(|x, y| (y.0, y.1).cmp(&(x.0, x.1)));
    for (file, from, to, with) in patches {
        let path = &after.files[file].path;
        if let Some(f) = work.iter_mut().find(|f| &f.path == path) {
            if f.text.is_char_boundary(from) && f.text.is_char_boundary(to) {
                f.text.replace_range(from..to, &with);
            }
        }
    }
    notes
}

fn json_flag(a: &Value, k: &str) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(false)
}

/// The text with every minted id (` <ek7m2qd>`) taken out, for comparing text that does not care which ids were minted.
pub fn strip_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(" <e") {
        let tail = &rest[i + 2..];
        match tail.find('>') {
            Some(j) if is_minted_id(&tail[..j]) => {
                out.push_str(&rest[..i]);
                rest = &tail[j + 1..];
            }
            _ => {
                out.push_str(&rest[..i + 3]);
                rest = &rest[i + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_seven_characters_unique_and_not_a_sequence() {
        let mut t = HashSet::new();
        let a = mint("part", "P::a", &mut t);
        let b = mint("part", "P::b", &mut t);
        assert!(is_minted_id(&a) && is_minted_id(&b) && a != b, "{a} {b}");
        // The same element at the same place gets the next free id when the first is taken.
        let again = mint("part", "P::a", &mut t);
        assert_ne!(a, again);
        assert!(!is_minted_id("R-12") && !is_minted_id("eABCDEF"));
        assert_eq!(strip_ids("part def <e94k68v> A { port <'R-1'> p; part <eb> q; }"), "part def A { port <'R-1'> p; part <eb> q; }");
    }
}
