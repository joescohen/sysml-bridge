//! Refs by source id, and the id map as a batch changes it.
//!
//! Any argument that takes the qualified name of an element also takes `source_id:<id>`, for example
//! `{"ref": "source_id:_2022x_2_98e020b_1731884294707_421547_9612"}`. The id is looked up when the edit runs,
//! so an edit after a rename or a move of the element still finds it. A caller then never needs a positional
//! `Owner::$3` ref, which changes when an earlier sibling goes.
//!
//! A positional ref in a batch names the statement it names in the text before the batch: when an earlier edit of the
//! batch renumbers it (a delete of a sibling before it), the ref follows the statement; when an earlier edit deleted the
//! statement itself, the batch is refused instead of editing the wrong one.

use super::ids::{self, IdEvent, IdNote};
use super::{locate, Fail, Res};
use crate::model::Model;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The prefix of a ref given as a source id.
pub const ID_REF_PREFIX: &str = "source_id:";

/// Arguments whose text is a ref to an element (or a list of them).
pub const REF_KEYS: &[&str] =
    &["ref", "target", "source", "owner", "scope", "root", "typed_by", "specializes", "remove_specializes", "redefines", "general", "about", "of", "context", "from", "to", "port_def", "item", "order", "before", "after"];

/// True when `v` holds a `source_id:` ref in a ref argument.
pub fn has_id_ref(v: &Value) -> bool {
    walk_refs(v, &mut |s| s.starts_with(ID_REF_PREFIX) || s.trim_start().starts_with(ID_REF_PREFIX))
}

fn walk_refs(v: &Value, hit: &mut dyn FnMut(&str) -> bool) -> bool {
    let Some(o) = v.as_object() else { return false };
    for (k, x) in o {
        if k == "features" {
            if let Some(list) = x.as_array() {
                if list.iter().any(|f| walk_refs(f, hit)) {
                    return true;
                }
            }
        } else if REF_KEYS.contains(&k.as_str()) {
            match x {
                Value::String(s) if hit(s) => return true,
                Value::Array(l) if l.iter().filter_map(Value::as_str).any(|s| hit(s)) => return true,
                _ => {}
            }
        }
    }
    false
}

/// The id of a `source_id:<id>` ref.
fn id_of(s: &str) -> Option<&str> {
    s.trim().strip_prefix(ID_REF_PREFIX).map(str::trim)
}

fn lookup(map: &BTreeMap<String, String>, id: &str, key: &str) -> Res<String> {
    map.get(id).cloned().ok_or_else(|| {
        Fail::with(
            format!("unknown source_id {} in argument '{key}'. Fix: pass an id recorded by create_element, create_relationship, add_interface, update_element or set_source_id, or one from the importer's .ids.json file; resolve_source_id checks an id.", crate::agent::text::quote_clip(id)),
            json!({ "source_id": id }),
        )
    })
}

/// Replace every `source_id:<id>` ref of `op` with the qualified name the id names now. Returns the edit and what was resolved.
pub fn resolve_ids(op: &Value, map: &BTreeMap<String, String>) -> Res<(Value, Vec<Value>)> {
    let mut out = op.clone();
    let mut done = Vec::new();
    resolve_in(&mut out, map, &mut done, &|_, _| Ok(None))?;
    Ok((out, done))
}

fn resolve_in(v: &mut Value, map: &BTreeMap<String, String>, done: &mut Vec<Value>, check: &dyn Fn(&str, &str) -> Res<Option<String>>) -> Res<()> {
    let Some(o) = v.as_object_mut() else { return Ok(()) };
    for (k, x) in o.iter_mut() {
        if k == "features" {
            for f in x.as_array_mut().into_iter().flatten() {
                resolve_in(f, map, done, check)?;
            }
        } else if REF_KEYS.contains(&k.as_str()) {
            let mut one = |s: &mut Value| -> Res<()> {
                let Some(text) = s.as_str().map(str::to_string) else { return Ok(()) };
                if let Some(id) = id_of(&text) {
                    let qn = lookup(map, id, k)?;
                    done.push(json!({ "argument": k, "source_id": id, "ref": qn }));
                    *s = json!(qn);
                } else if let Some(now) = check(k, &text)? {
                    done.push(json!({ "argument": k, "positional": text, "ref": now }));
                    *s = json!(now);
                }
                Ok(())
            };
            match x {
                Value::String(_) => one(x)?,
                Value::Array(l) => {
                    for s in l.iter_mut() {
                        one(s)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The id map while a batch runs, and what is needed to refuse a positional ref that an earlier edit renumbered.
pub struct Track {
    pub map: BTreeMap<String, String>,
    /// Deletes and moves so far, in order: the events that renumber unnamed members.
    shifts: Vec<IdEvent>,
    freed: BTreeMap<String, String>,
    pub notes: Vec<IdNote>,
    /// Positional ref as written -> its qualified name in the text before the batch.
    pre: Arc<HashMap<String, String>>,
}

impl Track {
    pub fn new(map: BTreeMap<String, String>) -> Self {
        Track { map, shifts: Vec::new(), freed: BTreeMap::new(), notes: Vec::new(), pre: Arc::new(HashMap::new()) }
    }

    /// A track for a batch: positional refs are looked up in `before` once.
    pub fn for_batch(map: BTreeMap<String, String>, pre: Arc<HashMap<String, String>>) -> Self {
        Track { pre, ..Track::new(map) }
    }

    /// Fold the events of an edit that ran into the map.
    pub fn absorb(&mut self, events: &[IdEvent]) -> Res<()> {
        ids::apply_events_noted(&mut self.map, events, &mut self.freed, &mut self.notes)?;
        self.shifts.extend(events.iter().filter(|e| matches!(e, IdEvent::Delete { .. } | IdEvent::Move { .. } | IdEvent::Reorder { .. })).cloned());
        Ok(())
    }

    /// Resolve the id refs of `op` against the ids as they are now. A positional ref names the statement it named before
    /// the batch: it follows a renumbering, and is refused when an earlier edit deleted or moved that statement away.
    pub fn resolve(&self, op: &Value) -> Res<(Value, Vec<Value>)> {
        let mut out = op.clone();
        let mut done = Vec::new();
        resolve_in(&mut out, &self.map, &mut done, &|key, text| self.check_positional(key, text))?;
        Ok((out, done))
    }

    fn check_positional(&self, key: &str, text: &str) -> Res<Option<String>> {
        let Some(canon) = self.pre.get(text) else { return Ok(None) };
        let mut now = canon.clone();
        for ev in &self.shifts {
            match ids::step(&now, ev) {
                Some(n) => now = n,
                None => {
                    return Err(Fail::from(format!(
                        "argument '{key}': '{text}' is a positional ref, and an earlier edit of this batch deleted or moved that statement away. Fix: refer to statements by source id (\"{ID_REF_PREFIX}<id>\") or by name, which do not change inside a batch."
                    )))
                }
            }
        }
        // Renumbered by an earlier delete or move of a sibling: the ref follows the statement it named.
        Ok((&now != canon || text != canon).then_some(now))
    }
}

/// The positional refs of `edits`, each with its qualified name in `model` (the text before the batch).
pub fn positional_refs(model: &Model, edits: &[Value]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for e in edits {
        walk_refs(e, &mut |s| {
            let positional = ids::split_qn(s.trim()).iter().any(|seg| ids::is_positional(seg));
            if positional && !out.contains_key(s) {
                if let Ok(el) = locate(model, s) {
                    out.insert(s.to_string(), el.qualified_name.clone());
                }
            }
            false
        });
    }
    out
}

/// True when an edit of `edits` has a positional ref (so the text before the batch is parsed to check it).
pub fn any_positional(edits: &[Value]) -> bool {
    edits.iter().any(|e| walk_refs(e, &mut |s| ids::split_qn(s.trim()).iter().any(|seg| ids::is_positional(seg))))
}
