//! Source ids: the map from an id of the source project to a qualified name, kept in
//! `<workspace>/.sysml-bridge/ids.json` and updated inside the same write as the model text.

use super::ToolErr;
use crate::App;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::ids::{self, IdEvent};
use sysml_core::agent::{self, intent};
use sysml_core::api::FileIn;

/// `(id, effective value before, effective value after)`.
pub type IdChange = (String, Option<String>, Option<String>);

/// Ids from the tools. A `null` value hides an id an importer wrote.
fn own(app: &App) -> BTreeMap<String, Option<String>> {
    let Some(text) = app.store.read_ids_raw() else { return BTreeMap::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    v["ids"].as_object().into_iter().flatten().map(|(k, q)| (k.clone(), q.as_str().map(str::to_string))).collect()
}

/// The ids in force: the importer's, overridden by the tools'.
pub fn effective(app: &App) -> BTreeMap<String, String> {
    let mut map = app.store.imported_ids();
    for (k, v) in own(app) {
        match v {
            Some(q) => map.insert(k, q),
            None => map.remove(&k),
        };
    }
    map
}

/// The id file after `changes` are applied (`after` side), as text; `None` when it would be empty.
fn render(app: &App, changes: &[IdChange], forward: bool) -> Option<String> {
    let imported = app.store.imported_ids();
    let mut map = own(app);
    for (id, before, after) in changes {
        let want = if forward { after } else { before };
        match want {
            Some(q) if imported.get(id) == Some(q) => {
                map.remove(id);
            }
            Some(q) => {
                map.insert(id.clone(), Some(q.clone()));
            }
            None if imported.contains_key(id) => {
                map.insert(id.clone(), None);
            }
            None => {
                map.remove(id);
            }
        }
    }
    if map.is_empty() {
        return None;
    }
    Some(format!("{}\n", serde_json::to_string_pretty(&json!({ "ids": map })).unwrap_or_default()))
}

/// Plan the id map changes of one write. `sweep` drops ids whose element a raw edit removed. Ids that point at
/// unnamed members (`Owner::$3`) are re-anchored on every write that changes text, so an edit that moves a
/// statement without a name event (a removed `doc`, a replaced statement) does not leave them on the wrong one.
pub fn plan(app: &App, before: &[FileIn], after: &[FileIn], events: &[IdEvent], sweep: bool) -> Result<Vec<IdChange>, ToolErr> {
    let eff = effective(app);
    let positional = eff.values().any(|q| q.starts_with('$') || q.contains("::$")) && agent::project_revision(before) != agent::project_revision(after);
    if events.is_empty() && !sweep && !positional {
        return Ok(Vec::new());
    }
    if eff.is_empty() && !events.iter().any(|e| matches!(e, IdEvent::Assign { .. } | IdEvent::Reassign { .. })) {
        return Ok(Vec::new());
    }
    let mut map = eff.clone();
    ids::apply_events(&mut map, events)?;
    let after_model = intent::model_of(after);
    for e in events {
        if let IdEvent::Assign { id, qn } | IdEvent::Reassign { id, qn } = e {
            if after_model.find(qn).is_none() {
                return Err(ToolErr::from(format!("source_id '{id}' would name '{qn}', which the write does not produce. Report this as a generator bug.")));
            }
        }
    }
    if positional || sweep {
        let before_model = intent::model_of(before);
        if positional {
            ids::reanchor(&mut map, &eff, &before_model, &after_model);
        }
        if sweep {
            ids::sweep(&mut map, &eff, &before_model, &after_model);
        }
    }
    Ok(ids::changes(&eff, &map))
}

/// The id changes as the history entry stores them.
pub fn entry_json(changes: &[IdChange]) -> Value {
    json!(changes.iter().map(|(id, b, a)| json!({ "id": id, "before": b, "after": a })).collect::<Vec<_>>())
}

/// New id file text for a forward write, or for an undo (`forward: false`).
pub fn file_text(app: &App, changes: &[IdChange], forward: bool) -> Option<String> {
    render(app, changes, forward)
}

/// Undo: the changes of a logged write that still hold now (the id has the value the write gave it).
pub fn reversed(app: &App, logged: &Value) -> Vec<IdChange> {
    let eff = effective(app);
    logged
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c["id"].as_str()?.to_string();
            let (before, after) = (c["before"].as_str().map(str::to_string), c["after"].as_str().map(str::to_string));
            (eff.get(&id) == after.as_ref()).then_some((id, after, before))
        })
        .collect()
}

