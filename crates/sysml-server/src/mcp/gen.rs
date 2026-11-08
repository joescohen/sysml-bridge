//! The write-side halves of the requirement tools that need the store: a renumbering plan, the comparison of a CSV
//! (which can save a column map) and the comparison of a ReqIF file. Each returns the edits that `apply_edits` takes;
//! none writes model text. The read half (relation maps, coverage history, next id) is `sysml_core::agent::tools_gen`.

use super::{out, writes, ToolErr, ToolResult};
use crate::App;
use serde_json::{json, Value};
use sysml_core::agent::numbering;
use sysml_core::agent::reqif;
use sysml_core::agent::text::quote_clip;
use sysml_core::agent::tools::model_of;
use sysml_core::agent::tools_gen::{edge, num, text_of};

fn err(m: impl Into<String>) -> ToolErr {
    writes::err(m)
}

/// `plan_renumber`: the new ids under one scope and the edits that write them. Writes nothing.
pub fn plan_renumber(app: &App, a: &Value) -> ToolResult {
    edge(a, &[], &[])?;
    let scope = a.get("scope").and_then(Value::as_str).ok_or_else(|| err("missing argument 'scope': the qualified name of a package or requirement, for example Package::Section."))?;
    let o = numbering::Opts {
        scope: scope.to_string(),
        recursive: a.get("recursive").and_then(Value::as_bool).unwrap_or(false),
        prefix: text_of(a, "prefix").unwrap_or_default(),
        separator: text_of(a, "separator"),
        start: num(a, "start"),
    };
    let m = model_of(&super::model_files(app));
    let plan = numbering::plan(&m, &o).map_err(|e| err(e.message))?;
    let changes = plan["changes"].as_array().cloned().unwrap_or_default();
    let line = format!("{} id(s) change, {} unchanged under {}; {} text(s) hold an old id as plain characters.", plan["changed"], plan["unchanged"], quote_clip(plan["scope"].as_str().unwrap_or("")), plan["texts_with_an_old_id"]);
    if changes.is_empty() {
        return out(format!("Nothing to renumber: {line}"), plan);
    }
    let edits: Vec<Value> = changes.iter().map(|c| c["edit"].clone()).collect();
    let mut data = plan;
    data["edits"] = json!(edits);
    out(format!("{line} Nothing was written: pass edits to apply_edits to write them."), data)
}

/// `plan_from_csv`: compare a CSV with the model by its key column: the counts, the rows and the edits that make the model
/// match the file. The model is never written here.
pub fn plan_from_csv(app: &App, a: &Value) -> ToolResult {
    // The file has its own cap (checked next, by bytes); every other argument is tested as usual.
    let mut rest = a.clone();
    rest.as_object_mut().map(|o| o.remove("csv"));
    edge(&rest, &[("map", sysml_core::agent::csv_sync::MAX_MAP)], &[])?;
    if a.get("csv").and_then(Value::as_str).map(|c| c.len() > sysml_core::agent::csv_sync::MAX_BYTES).unwrap_or(false) {
        return Err(err(format!("refused: the csv is longer than {} bytes. Fix: split the file.", sysml_core::agent::csv_sync::MAX_BYTES)));
    }
    let csv = a.get("csv").and_then(Value::as_str).ok_or_else(|| err("missing argument 'csv': the content of the CSV file as one string (the first row is the header)."))?;
    // The column map: given, or a saved one by name.
    let mut map: Vec<(String, String)> = Vec::new();
    if let Some(Value::Object(o)) = a.get("map") {
        if o.len() > sysml_core::agent::csv_sync::MAX_MAP {
            return Err(err(format!("argument map names {} columns; at most {}.", o.len(), sysml_core::agent::csv_sync::MAX_MAP)));
        }
        map = o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
    }
    if let Some(name) = text_of(a, "map_name") {
        if !map.is_empty() {
            return Err(err("give map or map_name, not both."));
        }
        let saved = app.store.read_csv_maps().into_iter().rev().find(|e| e["name"] == name.as_str());
        let Some(saved) = saved else { return Err(err(format!("no column map is saved under the name {}.", quote_clip(&name)))) };
        map = saved["map"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
    }
    let o = sysml_core::agent::csv_sync::Opts { csv: csv.to_string(), key_column: text_of(a, "key_column"), map: map.clone(), owner: text_of(a, "owner"), scope: text_of(a, "scope") };
    let m = model_of(&super::model_files(app));
    let ids = super::ids::effective(app);
    let mut v = sysml_core::agent::csv_sync::plan(&m, &ids, &o).map_err(|e| err(e.message))?;
    if let Some(name) = text_of(a, "save_map") {
        if name.trim().is_empty() || name.len() > 80 || name.chars().any(|c| c.is_control()) {
            return Err(err("save_map is a name of 1 to 80 characters."));
        }
        let used: serde_json::Map<String, Value> = v["mapped"].as_array().into_iter().flatten().map(|x| (x["column"].as_str().unwrap_or("").to_string(), x["property"].clone())).collect();
        let _guard = app.store.lock_writes();
        app.store.append_csv_map(&json!({ "name": name, "map": used })).map_err(err)?;
        v["map_saved"] = json!(name);
    }
    let c = &v["counts"];
    let line = format!("{} new, {} updated, {} unchanged, {} obsolete, {} refused.", c["new"], c["updated"], c["unchanged"], c["obsolete"], c["refused"]);
    let why = if v["edits"].as_array().is_none_or(Vec::is_empty) { "No edit is needed." } else { "Nothing was written: pass edits to apply_edits to write them." };
    out(format!("{line} {why}"), v)
}

/// `plan_from_reqif`: compare a ReqIF file with the requirements under an owner, and the edits that make them match.
pub fn plan_from_reqif(app: &App, a: &Value) -> ToolResult {
    let mut rest=a.clone();
    rest.as_object_mut().map(|o| {o.remove("content");});
    edge(&rest,&[],&[])?;
    let owner=a.get("owner").and_then(Value::as_str).ok_or_else(||err("owner is required"))?;
    let content=match (a.get("content").and_then(Value::as_str),a.get("file").and_then(Value::as_str)) {
        (Some(_),Some(_))=>return Err(err("give content or file, not both")),
        (Some(s),None)=>s.to_string(),
        (None,Some(p))=>app.store.read_reqif_file(p).map_err(err)?,
        _=>return Err(err("content or file is required")),
    };
    if content.len()>reqif::MAX_BYTES {return Err(err(format!("ReqIF is longer than {} bytes",reqif::MAX_BYTES)));}
    let m=model_of(&super::model_files(app));
    let ids=super::ids::effective(app);
    let v=reqif::plan(&m,&ids,&reqif::Opts {content:&content,owner,fill_empty_name:a["fill_empty_name"].as_bool().unwrap_or(false),set_custom_id_as_id:a["set_custom_id_as_id"].as_bool().unwrap_or(false)}).map_err(|e|err(e.message))?;
    let c=&v["counts"];
    let line=format!("{} new, {} updated, {} unchanged, {} obsolete, {} skipped.",c["new"],c["updated"],c["unchanged"],c["obsolete"],c["skipped"]);
    let why = if v["edits"].as_array().is_none_or(Vec::is_empty) { "No edit is needed." } else { "Nothing was written: pass edits to apply_edits to write them." };
    out(format!("{line} {why}"), v)
}
