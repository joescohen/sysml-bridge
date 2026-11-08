//! Model Context Protocol endpoint: JSON-RPC over standard input and output, or over `POST /mcp` (JSON responses).
//!
//! Agents read and edit the workspace files. Every write goes through the store.

mod ctx;
mod gen;
mod ids;
mod revs;
mod schema;
pub mod suspect;
#[cfg(test)]
mod tests;
pub mod writes;

use crate::{authorized, App};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};
use std::sync::Arc;
use sysml_core::agent::text::{model_name, quote_clip};
use sysml_core::agent;
use sysml_core::api;
use sysml_core::Model;

const PROTOCOL: &str = "2025-06-18";

pub use sysml_core::agent::tools::{out, ToolErr, ToolOut, ToolResult};
use sysml_core::agent::tools;

pub(crate) fn model_files(app: &App) -> Vec<api::FileIn> {
    app.store.read_all().into_iter().filter(|f| !crate::store::is_layout_path(&f.path)).collect()
}

pub(crate) fn model(app: &App) -> Model {
    let mut m = api::to_model(model_files(app));
    agent::add_namespace_checks(&mut m);
    writes::pinned(app, m)
}

/// Tools that run edits: the edit engine resolves `source_id:<id>` refs itself, one edit at a time.
const WRITE_TOOLS: &[&str] = &["edit_model", "create_element", "update_element", "delete_element", "create_relationship", "add_interface", "set_source_id", "reorder_members", "fold_copy", "mint_ids", "apply_edits"];

/// A read call with `source_id:<id>` refs: the same call with the qualified names the ids name now.
fn with_id_refs(app: &App, name: &str, a: &Value) -> Result<Option<Value>, ToolErr> {
    if WRITE_TOOLS.contains(&name) || name == "resolve_source_id" || a.as_object().is_none() {
        return Ok(None);
    }
    let mut probe = a.clone();
    // `name` is a ref only where `ref` has an older spelling.
    if matches!(name, "get_element" | "query_relationships" | "get_traceability") {
        if let Some(n) = probe.get("name").cloned() {
            probe["ref"] = n;
            probe.as_object_mut().map(|o| o.remove("name"));
        }
    }
    if !agent::idrefs::has_id_ref(&probe) {
        return Ok(None);
    }
    let (resolved, _) = agent::idrefs::resolve_ids(&probe, &ids::effective(app))?;
    Ok(Some(resolved))
}

fn call(app: &App, name: &str, a: &Value) -> ToolResult {
    crate::official::register();
    schema::check_args(name, a)?;
    let resolved = with_id_refs(app, name, a)?;
    let a = resolved.as_ref().unwrap_or(a);
    // The read tools: one table, in sysml_core::agent::tools.
    if let Some(r) = tools::dispatch(app, name, a) {
        return r;
    }
    match name {
        "write_file" => writes::write_file(app, a),
        "delete_file" => writes::delete_file(app, a),
        "edit_model" | "create_element" | "update_element" | "delete_element" | "create_relationship" | "add_interface" | "set_source_id" | "reorder_members" | "fold_copy" | "mint_ids" => writes::edit_one(app, name, a),
        "apply_edits" => writes::apply_edits(app, a),
        "import_requirements" => writes::import_requirements(app, a),
        "plan_renumber" => gen::plan_renumber(app, a),
        "plan_from_csv" => gen::plan_from_csv(app, a),
        "plan_from_reqif" => gen::plan_from_reqif(app, a),
        "get_history" => writes::get_history(app, a),
        "compare_revisions" => writes::compare_revisions(app, a),
        "create_baseline" => writes::create_baseline(app, a),
        "list_baselines" => writes::list_baselines(app),
        "get_change_package" => writes::change_package(app, a),
        "undo" => writes::undo(app, a),
        "check_merge" => revs::check_merge(app, a),
        other => Err(format!("unknown tool {}. Call tools/list for the tool names.", quote_clip(other)).into()),
    }
}

fn rpc(app: &App, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));
    // Notifications carry no id and get no reply.
    let id = id?;
    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "sysml-bridge-engine", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "SysML v2 modeling workspace. Text files are the source of truth. Start with get_project_state and get_outline. Change the model with create_element, update_element, delete_element, create_relationship and add_interface (or apply_edits for several at once); a new member can take before, after or position, and reorder_members puts an owner's members in a given order; every write accepts dry_run and base_revision, and undo reverts the last one. get_impact says what to look at when an element changes. Check your work with validate_model and coverage_report. Every result has structuredContent."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": schema::tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(tools::envelope(call(app, name, &args), WRITE_TOOLS.contains(&name)))
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err((-32601, format!("method not found: {}", quote_clip(method)))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    })
}

pub async fn handle(State(app): State<Arc<App>>, headers: HeaderMap, Json(body): Json<Value>) -> axum::response::Response {
    if !authorized(&app, &headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "token required" }))).into_response();
    }
    // File and parse work is blocking; keep it off the async workers.
    let out = tokio::task::spawn_blocking(move || match &body {
        Value::Array(batch) => {
            let replies: Vec<Value> = batch.iter().filter_map(|r| rpc(&app, r)).collect();
            (!replies.is_empty()).then(|| Value::Array(replies))
        }
        single => rpc(&app, single),
    })
    .await
    .unwrap_or(None);
    match out {
        Some(v) => Json(v).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// MCP over standard input and output: one JSON-RPC message per line, for a local agent
/// that starts this program itself. Nothing but replies is written to standard output.
pub fn stdio(app: &App) {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.iter().filter_map(|r| rpc(app, r)).collect();
                (!replies.is_empty()).then(|| Value::Array(replies))
            }
            Ok(single) => rpc(app, &single),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {}", model_name(&sysml_core::edit::clip(&e.to_string()))) } })),
        };
        if let Some(v) = reply {
            if writeln!(stdout, "{v}").and_then(|_| stdout.flush()).is_err() {
                break;
            }
        }
    }
}
