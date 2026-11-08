//! Safe writes: every model change is planned first, can be previewed,
//! is checked against a revision, and is logged so it can be undone.

use super::ids::{self, IdChange};
use super::{model_files, ToolErr, ToolOut};
use crate::App;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use sysml_core::agent::diff;
use sysml_core::agent::intent;
use sysml_core::agent::change;
use sysml_core::agent::plan::{self, Plan, Proposed};
use sysml_core::agent::rules;
use sysml_core::agent::text::{model_name, quote_claim, quote_clip, stored_token};
use sysml_core::agent::{project_revision, revision_of, Fail};
use sysml_core::api::FileIn;

/// Largest model file a tool may save.
// Generous: a large model with many register interfaces can be one file of several megabytes.
const MAX_FILE_BYTES: usize = 256 << 20;
/// Largest history entry; a bigger change must be split so it can still be undone.
pub(super) const MAX_ENTRY_BYTES: usize = 1 << 30;
/// Diff text sent back to the caller is cut here; the log keeps the exact hunks.
const MAX_REPLY_DIFF: usize = 64 << 10;

fn cap(s: &str) -> String {
    if s.len() <= MAX_REPLY_DIFF {
        return s.to_string();
    }
    let cut = (0..=MAX_REPLY_DIFF).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
    format!("{}\n... diff cut at {} bytes; read the file or get_history for the rest\n", &s[..cut], MAX_REPLY_DIFF)
}

/// The one spelling of a model file path, used by every gate and by the write.
fn model_path(p: &str) -> Result<String, ToolErr> {
    let c = crate::store::canon_path(p).map_err(err)?;
    if crate::store::is_layout_path(&c) || c.ends_with(crate::store::IDS_SUFFIX) {
        return Err(err("model tools store .sysml and .kerml files. Diagram layouts and id maps are written by the importer."));
    }
    Ok(c)
}

/// What a write reports. No gate refuses a write: the model it leaves is built once and its diagnostics come back with
/// the reply, any parse fault first.
#[derive(Clone, Copy)]
pub enum Gate {
    /// A tool write: the changed files are also read by the official grammar, and a fault it finds is reported.
    Report,
    /// Undo and imports: written as given, with the diagnostics of the result.
    Open,
}

/// The result of a tool's own work, before the common checks.
pub struct Built {
    pub proposed: Vec<Proposed>,
    /// Files outside the model (diagram layouts); written with the model, not diffed or logged.
    pub side: Vec<(String, String)>,
    /// Side files that followed a rename, move or delete (layouts, `renames.jsonl`): their text before
    /// and after goes into the write log, so `undo` puts them back with the model.
    pub side_logged: Vec<(String, Option<String>, String)>,
    pub info: Value,
    pub gate: Gate,
    pub undoes: Option<u64>,
    /// Source-id map changes that are written with the files and logged for undo.
    pub ids: Vec<IdChange>,
    /// Fields added to the entry of the write log.
    pub log: Value,
}

impl Built {
    pub(super) fn of(proposed: Vec<Proposed>, info: Value, gate: Gate) -> Self {
        Built { proposed, side: Vec::new(), side_logged: Vec::new(), info, gate, undoes: None, ids: Vec::new(), log: Value::Null }
    }
}

/// The files beside the model follow the renames, moves and deletes of a write (`rekey`): the
/// layouts are rewritten, and each record goes to `renames.jsonl`, in the same transaction and log entry as the model.
/// The reply says what was renamed and how many view entries followed.
pub(super) fn follow_side(app: &App, after: &[FileIn], events: &[sysml_core::agent::ids::IdEvent], tool: &str, built: &mut Built) {
    use sysml_core::agent::rekey;
    let moves = rekey::moves_of(events);
    let side: Vec<(String, String)> = app.store.read_all().into_iter().filter(|f| rekey::is_side(&f.path)).map(|f| (f.path, f.text)).collect();
    if side.is_empty() && moves.is_empty() {
        return;
    }
    let model = intent::model_of(after);
    let mut entries = 0;
    let mut files = Vec::new();
    for (path, text, n) in rekey::rekey_all(&side, &model, &moves) {
        let before = side.iter().find(|(p, _)| *p == path).map(|(_, t)| t.clone());
        entries += n;
        files.push(path.clone());
        built.side_logged.push((path, before, text));
    }
    // With no rename, the side files only followed element ids and recorded the ids of what they name.
    if moves.is_empty() {
        return;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let old = app.store.read(rekey::RENAMES_FILE).ok();
    let log = format!("{}{}", old.clone().unwrap_or_default(), rekey::log_lines(&moves, &rfc3339(now), tool));
    built.side_logged.push((rekey::RENAMES_FILE.to_string(), old, log));
    built.info["renamed"] = json!(moves.iter().map(|m| m.to_json()).collect::<Vec<_>>());
    built.info["side_entries_rekeyed"] = json!(entries);
    built.info["side_files_rekeyed"] = json!(files);
}

/// How much of the change report a reply carries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Report {
    None,
    Summary,
    Full,
}

struct Opts {
    dry_run: bool,
    base: Option<String>,
    report: Report,
}

/// `change_report`: `summary` (the default), `full` or `none`.
pub fn report_mode(a: &Value) -> Result<Report, ToolErr> {
    match a.get("change_report") {
        None | Some(Value::Null) => Ok(Report::Summary),
        Some(Value::String(s)) => match s.trim() {
            "summary" | "" => Ok(Report::Summary),
            "full" => Ok(Report::Full),
            "none" => Ok(Report::None),
            other => Err(err(format!("change_report must be 'summary' (the default), 'full' or 'none', not {}.", quote_clip(other)))),
        },
        Some(_) => Err(err("change_report must be a string: 'summary' (the default), 'full' or 'none'.")),
    }
}

fn opts(a: &Value) -> Result<Opts, ToolErr> {
    Ok(Opts {
        dry_run: a.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
        base: a.get("base_revision").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
        report: report_mode(a)?,
    })
}

/// The model files a plan leaves behind.
fn files_after(before: &[FileIn], plan: &Plan) -> Vec<FileIn> {
    let mut out: Vec<FileIn> = before.iter().filter(|f| !plan.deltas.iter().any(|d| d.path == f.path)).cloned().collect();
    out.extend(plan.deltas.iter().filter_map(|d| d.after.as_ref().map(|t| FileIn { path: d.path.clone(), text: t.clone() })));
    out
}

/// The semantic before/after report of a planned write. A write that changes no model text has nothing to compare.
fn report_of(app: &App, before: &[FileIn], plan: &Plan, id_changes: &[IdChange], mode: Report) -> Value {
    if mode == Report::None {
        return Value::Null;
    }
    if !plan.changed() {
        return change::unchanged();
    }
    let ids_before = ids::effective(app);
    let mut ids_after = ids_before.clone();
    for (id, _, now) in id_changes {
        match now {
            Some(q) => ids_after.insert(id.clone(), q.clone()),
            None => ids_after.remove(id),
        };
    }
    change::change_report(before, &files_after(before, plan), &ids_before, &ids_after, mode == Report::Full)
}

pub(super) fn err(message: impl Into<String>) -> ToolErr {
    ToolErr { message: message.into(), data: Value::Null }
}

/// RFC 3339 UTC time from epoch milliseconds, without a date library.
pub(super) fn rfc3339(ms: u128) -> String {
    let secs = (ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from day count (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

fn stale(before: &[FileIn], base: &str, why: &str) -> ToolErr {
    let files: serde_json::Map<String, Value> = before.iter().map(|f| (f.path.clone(), json!(revision_of(&f.text)))).collect();
    ToolErr {
        message: format!("refused: base_revision {} is stale ({why}). Read the current state with get_project_state, check what changed, and retry with its revision. Nothing was written.", quote_clip(base)),
        data: json!({ "base_revision": base, "current": { "project": project_revision(before), "files": files } }),
    }
}

fn files_json(plan: &Plan) -> Vec<Value> {
    plan.deltas
        .iter()
        .map(|d| json!({ "path": d.path, "change": d.change, "revision_before": d.revision_before, "revision_after": d.revision_after, "diff": cap(&d.diff) }))
        .collect()
}

fn result(plan: &Plan, tool: &str, o: &Opts, applied: bool, refusal: Option<&str>, info: &Value, seq: Option<u64>, report: &Value) -> Value {
    json!({
        "change_report": report,
        "tool": tool,
        "dry_run": o.dry_run,
        "applied": applied,
        "changed": plan.changed(),
        "would_accept": refusal.is_none(),
        "refusal": refusal,
        "revision_before": plan.revision_before,
        "revision_after": plan.revision_after,
        "files": files_json(plan),
        "diff": cap(&plan.diff()),
        "diagnostics": {
            "parse_faults": parse_faults(plan, info),
            "errors_before": plan.errors_before,
            "errors": plan.errors_after,
            "warnings": plan.warnings_after,
            "new_errors": plan.new_errors,
            "new_warnings": plan.new_warnings,
            "items": plan.touched_diagnostics,
        },
        "history_seq": seq,
        "info": info,
    })
}

/// The parse faults of a write's result, first in every reply: the parser's, then the official grammar's on the changed
/// files. A file the parser could not read whole may show fewer elements than its text holds, so these come before
/// anything else.
fn parse_faults(plan: &Plan, info: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = plan.parse_faults.iter().map(|d| json!({ "file": d.file, "line": d.line, "col": d.col, "message": d.message, "by": "parser" })).collect();
    for n in info.get("official").and_then(Value::as_array).into_iter().flatten().filter(|n| n["official"] == "invalid") {
        out.push(json!({ "file": n["file"], "line": n["line"], "col": n["column"], "message": n["why"], "by": "official grammar" }));
    }
    out
}

fn summary(plan: &Plan, verb: &str, data: &Value, ids: &[IdChange]) -> String {
    let mut t = String::new();
    // A parse fault leads the reply: until it is fixed the model of that file may be smaller than its text.
    let faults = data["diagnostics"]["parse_faults"].as_array().cloned().unwrap_or_default();
    for f in faults.iter().take(10) {
        t.push_str(&format!("PARSE FAULT ({}) {}:{}:{} {}\n", f["by"].as_str().unwrap_or(""), model_name(f["file"].as_str().unwrap_or("")), f["line"], f["col"], model_name(f["message"].as_str().unwrap_or(""))));
    }
    if faults.len() > 10 {
        t.push_str(&format!("... {} more parse faults; structuredContent.diagnostics.parse_faults lists them.\n", faults.len() - 10));
    }
    let names: Vec<&str> = plan.deltas.iter().map(|d| d.path.as_str()).collect();
    if names.is_empty() {
        t.push_str(&format!("{verb}: no model text changes; only the source id map does.\n"));
    } else {
        t.push_str(&format!("{verb} {}.\n", names.iter().map(|n| model_name(n)).collect::<Vec<_>>().join(", ")));
    }
    t.push_str(&format!("{} errors, {} warnings (before: {} errors). Revision {} -> {}.\n", plan.errors_after, plan.warnings_after, plan.errors_before, stored_token(&json!(plan.revision_before)), stored_token(&json!(plan.revision_after))));
    if data["change_report"].is_object() {
        t.push_str(&change::summary_text(&data["change_report"]));
    }
    // Only what this write changed is listed: the errors and warnings the model did not have before it. What stood
    // before stands in one line with its totals, so a reader does not have to read past findings that have nothing to
    // do with the change to see that it added none.
    let fresh: Vec<&sysml_core::api::DiagOut> = plan.new_errors.iter().chain(plan.new_warnings.iter()).collect();
    for d in fresh.iter().take(10) {
        t.push_str(&format!("new {} {}:{}:{} {}\n", format!("{:?}", d.severity).to_lowercase(), model_name(&d.file), d.line, d.col, model_name(&d.message)));
    }
    if fresh.len() > 10 {
        t.push_str(&format!("... {} more new findings; structuredContent.diagnostics lists them.\n", fresh.len() - 10));
    }
    let standing = (plan.errors_after.saturating_sub(plan.new_errors.len()), plan.warnings_after.saturating_sub(plan.new_warnings.len()));
    if standing.0 + standing.1 > 0 {
        t.push_str(&format!("{} {} error(s) and {} warning(s) were in the model before this write and still stand; they are not listed here (validate_model and check_all list them).\n", if fresh.is_empty() { "This write added no error and no warning." } else { "Besides those," }, standing.0, standing.1));
    }
    if let Some(info) = data["info"].as_object() {
        let status = |v: Option<&Value>| v.and_then(Value::as_str).map(|s| format!(" ({})", model_name(s))).unwrap_or_default();
        // Every id the call recorded for the element, not only the first.
        let ids_of = |v: &Value| -> String {
            match v["source_ids"].as_array().filter(|l| l.len() > 1) {
                Some(l) => format!(", source ids {}", l.iter().filter_map(Value::as_str).map(quote_clip).collect::<Vec<_>>().join(", ")),
                None => v["source_id"].as_str().map(|i| format!(", source id {}", quote_clip(i))).unwrap_or_default(),
            }
        };
        if let Some(r) = info.get("ref").and_then(Value::as_str) {
            let many = info.get("source_ids").and_then(Value::as_array).filter(|l| l.len() > 1).map(|_| ids_of(&Value::Object(info.clone()))).unwrap_or_default();
            t.push_str(&format!("ref: {}{}{many}\n", model_name(r), status(info.get("status"))));
        }
        if info.get("op").and_then(Value::as_str) == Some("reorder_members") {
            t.push_str(&format!(
                "Reordered {}: {} of {} members moved, {} listed, {} not listed (kept in their order after the listed ones).\n",
                model_name(info.get("owner").and_then(Value::as_str).unwrap_or("")),
                stored_token(&info["moved"]), stored_token(&info["members"]), stored_token(&info["listed"]), stored_token(&info["unlisted"])
            ));
        }
        if let Some(p) = info.get("placement").filter(|p| p["owner"].is_string()) {
            t.push_str(&format!("Placed in {} at position {} of {}.\n", model_name(p["owner"].as_str().unwrap_or("")), p["index"].as_u64().map_or(0, |i| i + 1), stored_token(&p["members"])));
        }
        for n in info.get("official").and_then(Value::as_array).into_iter().flatten().filter(|n| n["official"] != "invalid") {
            t.push_str(&format!("official grammar: {} is {} ({}).\n", model_name(n["file"].as_str().unwrap_or("")), model_name(n["official"].as_str().unwrap_or("")), model_name(n["why"].as_str().unwrap_or(""))));
        }
        for n in info.get("reminted").and_then(Value::as_array).into_iter().flatten().take(20) {
            t.push_str(&format!("note duplicate-id: {}\n", model_name(n["note"].as_str().unwrap_or(""))));
        }
        for w in info.get("warnings").and_then(Value::as_array).into_iter().flatten() {
            t.push_str(&format!("warning {}: {} Fix: {}\n", model_name(w["rule"].as_str().unwrap_or("")), model_name(w["message"].as_str().unwrap_or("")), model_name(w["fix"].as_str().unwrap_or(""))));
        }
        // One line per edit of a batch, so the caller never has to recompute a qualified name.
        if let Some(m) = info.get("reordered").and_then(Value::as_array) {
            let moves: Vec<String> = m.iter().map(|x| format!("#{} before #{}", x["edit"], x["before"])).collect();
            t.push_str(&format!("Reordered (an edit needed an element a later edit creates): {}.\n", moves.join(", ")));
        }
        // An id that left one element for another (or was removed on request) is never silent: say who lost it.
        for m in info.get("source_id_moves").and_then(Value::as_array).into_iter().flatten().take(20) {
            match m["now_on"].as_str() {
                Some(to) => t.push_str(&format!("source id {} left {} for {} ({}); {} no longer has it.\n", quote_clip(m["id"].as_str().unwrap_or("")), model_name(m["lost_by"].as_str().unwrap_or("")), model_name(to), model_name(m["how"].as_str().unwrap_or("")), model_name(m["lost_by"].as_str().unwrap_or("")))),
                None => t.push_str(&format!("source id {} was taken off {} ({}).\n", quote_clip(m["id"].as_str().unwrap_or("")), model_name(m["lost_by"].as_str().unwrap_or("")), model_name(m["how"].as_str().unwrap_or("")))),
            }
        }
        if let Some(rs) = info.get("results").and_then(Value::as_array) {
            for (i, r) in rs.iter().enumerate().take(200) {
                if let Some(q) = r["ref"].as_str() {
                    let id = ids_of(r);
                    t.push_str(&format!("#{} ref: {}{}{id}\n", i + 1, model_name(q), status(r.get("status"))));
                }
            }
        }
    }
    // What happened to the id map, so an id-only write can be read back.
    for (id, before, after) in ids.iter().take(10) {
        t.push_str(&match (before, after) {
            (None, Some(q)) => format!("source id {} recorded for {}\n", quote_clip(id), model_name(q)),
            (Some(a), Some(b)) => format!("source id {} now names {} (was {})\n", quote_clip(id), model_name(b), model_name(a)),
            (Some(a), None) => format!("source id {} dropped (was {})\n", quote_clip(id), model_name(a)),
            (None, None) => continue,
        });
    }
    if ids.len() > 10 {
        t.push_str(&format!("... {} more source id changes\n", ids.len() - 10));
    }
    t
}

/// Plan, check and (unless dry run) apply one write under the store's write lock.
pub fn write_with(app: &App, tool: &str, args: &Value, build: impl FnOnce(&[FileIn]) -> Result<Built, ToolErr>) -> Result<ToolOut, ToolErr> {
    let _guard = app.store.lock_writes();
    write_locked(app, tool, args, build)
}

/// `write_with` for a caller that holds the write lock already. This is the one function through which a tool writes
/// model files.
pub(super) fn write_locked(app: &App, tool: &str, args: &Value, build: impl FnOnce(&[FileIn]) -> Result<Built, ToolErr>) -> Result<ToolOut, ToolErr> {
    let o = opts(args)?;
    let before = model_files(app);
    if let Some(b) = &o.base {
        let known = b == &project_revision(&before) || b == "absent" || before.iter().any(|f| &revision_of(&f.text) == b);
        if !known {
            return Err(stale(&before, b, "it matches neither the project nor any file"));
        }
    }
    let mut built = build(&before)?;
    // Every tool write saves a name that starts with a digit in quotes ('5out'): the file stays official SysML v2.
    for text in built.proposed.iter_mut().filter(|p| is_model_text(&p.path)).filter_map(|p| p.text.as_mut()) {
        if let Some(q) = sysml_core::lexer::quote_digit_names(text) {
            *text = q;
        }
    }
    if built.proposed.len() > plan::MAX_PROPOSED_FILES {
        return Err(err(format!("refused: the write changes {} files; at most {} per call. Fix: split it into several calls.", built.proposed.len(), plan::MAX_PROPOSED_FILES)));
    }
    let plan = plan::plan(&before, built.proposed);
    for d in &plan.deltas {
        // Every path was checked where it entered; this is the one the store will write.
        if model_path(&d.path)? != d.path {
            return Err(err(format!("path {} is not in canonical form", quote_clip(&d.path))));
        }
        if d.after.as_ref().map(|t| t.len() > MAX_FILE_BYTES).unwrap_or(false) {
            return Err(err(format!("refused: {} would be larger than {} MiB. Split the model into several files.", quote_clip(&d.path), MAX_FILE_BYTES >> 20)));
        }
        // A file whose bytes are not UTF-8 was read with U+FFFD in place of them. An edit would write that text back and
        // destroy the bytes without a word, so no edit touches it. Two writes stay open, because the caller says in so
        // many words that the content goes: a delete, and a forced overwrite of the whole file. Whether the file is
        // text is decided from its bytes (`Store::not_utf8`), not from a U+FFFD in the text: one such character in a
        // valid file locked it against every repair.
        if d.before.is_some() && app.store.not_utf8(&d.path) {
            let whole = d.after.is_none() || (tool == "write_file" && args.get("force").and_then(Value::as_bool) == Some(true));
            if !whole {
                return Err(err(format!("refused: {} is not UTF-8 text: it holds bytes that cannot be read as text, so the tools will not edit it. Fix: replace the whole file with write_file {{force: true}}, or remove it with delete_file.", quote_clip(&d.path))));
            }
        }
        // A file the model reader skipped is still on disk: do not treat it as new.
        if d.revision_before.is_none() && app.store.exists(&d.path) {
            return Err(err(format!("refused: {} exists on disk but cannot be read as text, so it cannot be replaced safely. Remove it by hand first.", quote_clip(&d.path))));
        }
    }
    for (p, _) in &built.side {
        crate::store::canon_path(p).map_err(err)?;
    }
    if let Some(b) = &o.base {
        let fresh = b == &plan.revision_before
            || (plan.changed() && plan.deltas.iter().all(|d| d.revision_before.as_deref().unwrap_or("absent") == b));
        if !fresh {
            let touched = plan.deltas.iter().map(|d| format!("{} is {}", quote_clip(&d.path), stored_token(&json!(d.revision_before.as_deref().unwrap_or("absent"))))).collect::<Vec<_>>().join(", ");
            return Err(stale(&before, b, &format!("the write changes {touched}")));
        }
    }
    // The official grammar reads each changed file once, in its new text. What it finds is reported with the write, a
    // fault first; it never stops the write. Undo and the importer write what they are given (`Gate::Open`).
    let official_notes: Vec<Value> = match built.gate {
        Gate::Open => Vec::new(),
        Gate::Report => {
            let written: Vec<rules::WrittenFile> = plan.deltas.iter().filter_map(|d| d.after.as_deref().map(|after| rules::WrittenFile { path: &d.path, before: d.before.as_deref(), after })).collect();
            rules::official_write_notes(&written)
        }
    };
    let mut info = built.info.clone();
    if !official_notes.is_empty() {
        info["official"] = json!(official_notes);
    }
    let report = report_of(app, &before, &plan, &built.ids, o.report);
    if !plan.changed() && built.ids.is_empty() {
        let data = result(&plan, tool, &o, false, None, &info, None, &report);
        let info = &info;
        let (r, id) = (info["ref"].as_str().unwrap_or(""), info["source_id"].as_str());
        let text = match (id, info["existed"] == true) {
            _ if info["op"] == "reorder_members" => format!("No change: {} already lists its members in that order. Nothing written.", model_name(info["owner"].as_str().unwrap_or("the owner"))),
            (Some(id), true) => format!("Existed: {} was already in the model and already carries source id {}. Nothing written.", quote_clip(&r), quote_clip(&id)),
            (Some(id), false) => format!("No change: source id {} already names {}. Nothing written.", quote_clip(&id), quote_clip(&r)),
            (None, true) => format!("Existed: {} was already in the model. Nothing written.", quote_clip(&r)),
            (None, false) => "No change: the result is identical to the current text.".to_string(),
        };
        return Ok(ToolOut { text, data });
    }
    if o.dry_run {
        let data = result(&plan, tool, &o, false, None, &info, None, &report);
        let mut text = "Dry run, nothing written. The change would be written.\n".to_string();
        text.push_str(&summary(&plan, "Would change", &data, &built.ids));
        text.push_str(&cap(&plan.diff()));
        return Ok(ToolOut { text, data });
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut entry = json!({
        "time": rfc3339(now),
        "ts_ms": now as u64,
        "tool": tool,
        "author": "agent",
        "undoes": built.undoes,
        "revision_before": plan.revision_before,
        "revision_after": plan.revision_after,
        "side_files": built.side.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "side": built.side_logged.iter().map(|(p, b, a)| json!({ "path": p, "before": b, "after": a })).collect::<Vec<_>>(),
        "ids": ids::entry_json(&built.ids),
        "files": plan.deltas.iter().map(|d| json!({
            "path": d.path, "change": d.change,
            "revision_before": d.revision_before, "revision_after": d.revision_after,
            "diff": d.diff, "hunks": d.hunks,
        })).collect::<Vec<_>>(),
    });
    for (k, v) in built.log.as_object().into_iter().flatten() {
        entry[k.as_str()] = v.clone();
    }
    if entry.to_string().len() > MAX_ENTRY_BYTES {
        return Err(err("refused: the change is too large to record in the write log, so it could not be undone. Split it into smaller writes."));
    }
    // All files, then the log entry; any failure puts every file back and reports an error.
    let mut done: Vec<(String, Option<String>)> = Vec::new();
    let mut failure: Option<String> = None;
    for d in &plan.deltas {
        let r = match &d.after {
            Some(t) => app.store.write(&d.path, t),
            None => app.store.delete(&d.path),
        };
        match r {
            Ok(()) => done.push((d.path.clone(), d.before.clone())),
            Err(e) => {
                failure = Some(format!("{}: {e}", d.path));
                break;
            }
        }
    }
    if failure.is_none() {
        for (path, text) in built.side.iter().map(|(p, t)| (p, t)).chain(built.side_logged.iter().map(|(p, _, a)| (p, a))) {
            let prev = app.store.read(path).ok();
            match app.store.write(path, text) {
                Ok(()) => done.push((path.clone(), prev)),
                Err(e) => {
                    failure = Some(format!("{path}: {e}"));
                    break;
                }
            }
        }
    }
    // The id map moves with the files; the old text is kept so a failure puts it back.
    let ids_prev = app.store.read_ids_raw();
    let mut ids_written = false;
    if failure.is_none() && !built.ids.is_empty() {
        match app.store.write_ids_raw(ids::file_text(app, &built.ids, true).as_deref()) {
            Ok(()) => ids_written = true,
            Err(e) => failure = Some(format!("ids.json: {e}")),
        }
    }
    let mut seq = None;
    if failure.is_none() {
        match app.store.append_history(&mut entry) {
            Ok(s) => seq = Some(s),
            Err(e) => failure = Some(format!("write log: {e}")),
        }
    }
    if let Some(f) = failure {
        let mut stuck = Vec::new();
        for (path, prev) in done.iter().rev() {
            let r = match prev {
                Some(t) => app.store.write(path, t),
                None => app.store.delete(path),
            };
            if r.is_err() {
                stuck.push(path.as_str());
            }
        }
        if ids_written {
            let _ = app.store.write_ids_raw(ids_prev.as_deref());
        }
        let note = if stuck.is_empty() { "Every file was restored; nothing changed.".to_string() } else { format!("WARNING: could not restore {}. Check them with get_project_state.", stuck.join(", ")) };
        return Err(err(format!("write failed ({f}). {note}")));
    }
    let mut data = result(&plan, tool, &o, true, None, &info, seq, &report);
    if !built.ids.is_empty() {
        // `{id, before, after}`: what the write did to the source id map.
        data["source_id_changes"] = ids::entry_json(&built.ids);
    }
    let text = summary(&plan, "Saved", &data, &built.ids);
    Ok(ToolOut { text, data })
}

fn proposed_from(before: &[FileIn], after: Vec<FileIn>) -> Result<Vec<Proposed>, ToolErr> {
    let mut out = Vec::new();
    for f in after {
        if before.iter().find(|b| b.path == f.path).map(|b| b.text != f.text).unwrap_or(true) {
            out.push(Proposed { path: model_path(&f.path)?, text: Some(f.text) });
        }
    }
    Ok(out)
}

/// A required text argument that keeps its spaces; the message names it and says what it is.
fn arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, ToolErr> {
    match a.get(k) {
        Some(Value::String(s)) => Ok(s),
        Some(Value::Null) | None => Err(err(format!("missing argument '{k}': {}.", intent::arg_what(k)))),
        Some(_) => Err(err(format!("argument '{k}' must be a string: {}.", intent::arg_what(k)))),
    }
}

/// `path` or its alias `file`.
pub(super) fn path_arg(a: &Value) -> Result<&str, ToolErr> {
    match a.get("path").or_else(|| a.get("file")) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        Some(Value::Null) | None => Err(err(format!("missing argument 'path' (alias 'file'): {}.", intent::arg_what("path")))),
        Some(_) => Err(err(format!("argument 'path' (alias 'file') must be a string: {}.", intent::arg_what("path")))),
    }
}

fn is_model_text(path: &str) -> bool {
    !crate::store::is_layout_path(path) && (path.ends_with(".sysml") || path.ends_with(".kerml"))
}

/// `Some(message)` when `new` holds more U+FFFD than the text it replaces. New text may never bring one in, on any
/// route and with or without force; a file that holds one already may be saved while it is being repaired.
fn gains_not_text(app: &App, path: &str, new: &str) -> Option<String> {
    let count = |t: &str| t.matches(sysml_core::lexer::NOT_TEXT).count();
    let old = if app.store.not_utf8(path) { 0 } else { app.store.read(path).map(|t| count(&t)).unwrap_or(0) };
    (count(new) > old).then(|| format!("refused: the text for {} holds the character U+FFFD, the mark of bytes that were not text; a model file must not hold it and nothing was written. Fix: remove it from the text.", quote_clip(path)))
}

pub fn write_file(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let (path, text) = (path_arg(a)?, arg(a, "text")?);
    let path = model_path(path)?;
    if text.len() > MAX_FILE_BYTES {
        return Err(err(format!("refused: the text is larger than {} MiB. Split the model into several files.", MAX_FILE_BYTES >> 20)));
    }
    write_with(app, "write_file", a, |before| {
        // The same rule every other write follows: no new control, invisible or bidi character may reach a file.
        let old = before.iter().find(|f| f.path == path).map(|f| f.text.as_str()).unwrap_or("");
        if let Some(m) = gains_not_text(app, &path, text) {
            return Err(err(m));
        }
        // Each character by itself: a new one may not take the place of one the file held.
        if let Some(c) = sysml_core::edit::gained_bad_char(old, text) {
            return Err(err(format!("refused: the text holds the character U+{:04X}, a control, invisible or direction-changing character that a model file must not hold; nothing was written. Fix: remove it (line breaks and tabs are allowed).", c as u32)));
        }
        let mut after = swap_file(before, &path, Some(text));
        // A copy pasted with the minted id of an element the model has gets a new id in this write, with a note.
        let reminted = sysml_core::agent::mint::remint_copies(&intent::model_of(before), &mut after);
        let text = after.iter().find(|f| f.path == path).map(|f| f.text.clone()).unwrap_or_else(|| text.to_string());
        let mut built = Built::of(vec![Proposed { path: path.clone(), text: Some(text) }], json!({ "path": path }), Gate::Report);
        if !reminted.is_empty() {
            built.info["reminted"] = json!(reminted);
        }
        // Raw text follows the rules every tool write follows: no element under an owner that cannot hold it, and no second
        // representation of an element the model has (invalid syntax is saved and reported, a duplicate never).
        let (m0, m1) = (intent::model_of(before), intent::model_of(&after));
        sysml_core::agent::ownership::refuse_new(&m0, &m1)?;
        sysml_core::agent::duplicates::refuse_new(&m0, &m1)?;
        let similar = sysml_core::agent::duplicates::looks_like(&m0, &m1);
        if !similar.is_empty() {
            built.info["looks_like"] = json!(similar);
        }
        built.ids = ids::plan(app, before, &after, &[], true)?;
        Ok(built)
    })
}

pub fn delete_file(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let path = model_path(path_arg(a)?)?;
    write_with(app, "delete_file", a, |before| {
        if !before.iter().any(|f| f.path == path) {
            return Err(err(format!("no file {}. Use list_files to see the files.", quote_clip(&path))));
        }
        let mut built = Built::of(vec![Proposed { path: path.clone(), text: None }], json!({ "path": path }), Gate::Open);
        built.ids = ids::plan(app, before, &swap_file(before, &path, None), &[], true)?;
        Ok(built)
    })
}

/// The file set with one file replaced or removed.
fn swap_file(before: &[FileIn], path: &str, text: Option<&str>) -> Vec<FileIn> {
    let mut out: Vec<FileIn> = before.iter().filter(|f| f.path != path).cloned().collect();
    if let Some(t) = text {
        out.push(FileIn { path: path.to_string(), text: t.to_string() });
    }
    out
}

/// One edit, raw or intent-level, through the shared edit engine.
pub fn edit_one(app: &App, tool: &str, a: &Value) -> Result<ToolOut, ToolErr> {
    let mut op = a.clone();
    if tool != "edit_model" {
        op["op"] = json!(tool);
    }
    write_with(app, tool, a, |before| {
        let applied = intent::apply_op_ids(before, &op, &ids::effective(app)).map_err(|f| refused_delete(app, before, &op, a, f))?;
        let sweep = op.get("op").and_then(Value::as_str) == Some("replace");
        let ids = ids::plan(app, before, &applied.files, &applied.events, sweep)?;
        let after = applied.files.clone();
        let mut built = Built::of(proposed_from(before, applied.files)?, applied.info, Gate::Report);
        built.ids = ids;
        follow_side(app, &after, &applied.events, tool, &mut built);
        Ok(built)
    })
}

/// A delete refused because something still refers to the element: the refusal also carries the change report of
/// the delete as if it had been made without cascade, so the caller sees what would be left dangling.
fn refused_delete(app: &App, before: &[FileIn], op: &Value, a: &Value, mut f: Fail) -> ToolErr {
    let mode = report_mode(a).unwrap_or(Report::Summary);
    if op["op"] == "delete_element" && f.data["referrers"].is_array() && mode != Report::None {
        let ids = ids::effective(app);
        if let Ok(forced) = intent::apply_op_ids(before, &json!({ "op": "delete", "target": f.data["ref"] }), &ids) {
            let mut r = change::change_report(before, &forced.files, &ids, &ids, mode == Report::Full);
            r["hypothetical"] = json!("the delete as it would be without cascade; nothing was written");
            f.data["change_report"] = r;
        }
    }
    f.into()
}

/// The file set as it was at a recorded revision, rebuilt from the current files and the write log.
/// `spec` is `current`, a project revision, or `#N` (a write's history number: the state right after it; `#0` is
/// the state before the first write).
fn state_at(current: &[FileIn], history: &[Value], spec: &str) -> Result<(Vec<FileIn>, Value), ToolErr> {
    use std::collections::BTreeMap;
    let spec = spec.trim();
    let mut files: BTreeMap<String, String> = current.iter().map(|f| (f.path.clone(), f.text.clone())).collect();
    let listed = |files: &BTreeMap<String, String>| -> Vec<FileIn> { files.iter().map(|(p, t)| FileIn { path: p.clone(), text: t.clone() }).collect() };
    let now = project_revision(current);
    if spec.eq_ignore_ascii_case("current") || spec == now {
        return Ok((current.to_vec(), json!({ "revision": now, "is": "current" })));
    }
    let seq: Option<u64> = spec.strip_prefix('#').map(|n| n.trim().parse::<u64>().map_err(|_| err(format!("{} is not a history number. Fix: write # and the seq of a write from get_history, for example #12.", quote_clip(spec))))).transpose()?;
    if seq.is_none() && !(spec.len() == 16 && spec.chars().all(|c| c.is_ascii_hexdigit())) {
        return Err(err(format!("{} is not a revision. Fix: pass 'current', a project revision (16 hex digits, from get_project_state or get_history revision_before / revision_after), or #N for the state right after write N of get_history.", quote_clip(spec))));
    }
    let mut rev = now;
    for e in history.iter().rev() {
        let n = e["seq"].as_u64().unwrap_or(0);
        if e["revision_after"].as_str() != Some(rev.as_str()) {
            return Err(err(format!("cannot rebuild the revision: the files changed outside the tools after write #{n} (an edit on disk is not in the write log), so the log no longer leads back from the current files. Revisions since the last such change can still be compared.")));
        }
        if seq == Some(n) {
            return Ok((listed(&files), json!({ "revision": rev, "is": format!("after write #{n}"), "tool": e["tool"], "time": e["time"] })));
        }
        for f in e["files"].as_array().into_iter().flatten() {
            let path = f["path"].as_str().unwrap_or("").to_string();
            if f["change"] == "created" {
                files.remove(&path);
                continue;
            }
            let hunks: Vec<diff::Hunk> = serde_json::from_value(f["hunks"].clone()).map_err(|x| err(format!("write #{n} has an unreadable log entry: {x}")))?;
            let text = diff::reverse(files.get(&path).map(String::as_str).unwrap_or(""), &hunks).map_err(|x| err(format!("cannot rebuild the state before write #{n}: {x}")))?;
            files.insert(path, text);
        }
        rev = e["revision_before"].as_str().unwrap_or("").to_string();
        if seq == Some(n.saturating_sub(1)) || (seq.is_none() && spec == rev) {
            return Ok((listed(&files), json!({ "revision": rev, "is": format!("before write #{n}") })));
        }
    }
    Err(err(match seq {
        Some(n) => format!("no write #{n} in the write log. Fix: call get_history for the numbers it holds."),
        None => format!("revision {} is not in the write log. Fix: pass a revision_before or revision_after from get_history, or 'current'.", quote_clip(spec)),
    }))
}

/// The states the write log leads back through, newest first: the current files, then the state before each write, at most
/// `cap` states. Stops, without an error, where the log no longer leads back (a save outside the tools).
pub fn states_back(current: &[FileIn], history: &[Value], cap: usize) -> Vec<(String, Vec<FileIn>)> {
    use std::collections::BTreeMap;
    let mut files: BTreeMap<String, String> = current.iter().map(|f| (f.path.clone(), f.text.clone())).collect();
    let listed = |files: &BTreeMap<String, String>| -> Vec<FileIn> { files.iter().map(|(p, t)| FileIn { path: p.clone(), text: t.clone() }).collect() };
    let mut rev = project_revision(current);
    let mut states = vec![(rev.clone(), current.to_vec())];
    for e in history.iter().rev() {
        if states.len() >= cap || e["revision_after"].as_str() != Some(rev.as_str()) {
            break;
        }
        for f in e["files"].as_array().into_iter().flatten() {
            let path = f["path"].as_str().unwrap_or("").to_string();
            if f["change"] == "created" {
                files.remove(&path);
                continue;
            }
            let Ok(hunks) = serde_json::from_value::<Vec<diff::Hunk>>(f["hunks"].clone()) else { return states };
            let Ok(text) = diff::reverse(files.get(&path).map(String::as_str).unwrap_or(""), &hunks) else { return states };
            files.insert(path, text);
        }
        rev = e["revision_before"].as_str().unwrap_or("").to_string();
        states.push((rev.clone(), listed(&files)));
    }
    states
}

/// A named baseline of the workspace, by name. Names are compared as they are written.
fn baseline_named(app: &App, name: &str) -> Option<Value> {
    app.store.read_baselines().into_iter().find(|b| b["name"].as_str() == Some(name))
}

fn baseline_files(b: &Value) -> Vec<FileIn> {
    b["files"].as_array().into_iter().flatten().filter_map(|f| Some(FileIn { path: f["path"].as_str()?.to_string(), text: f["text"].as_str()?.to_string() })).collect()
}

/// A baseline without the files it holds: what `list_baselines` and a reply show.
fn baseline_row(b: &Value) -> Value {
    let mut row = b.clone();
    if let Some(o) = row.as_object_mut() {
        let files: Vec<Value> = o.remove("files").and_then(|f| f.as_array().cloned()).unwrap_or_default().iter().map(|f| json!({ "path": f["path"], "bytes": f["text"].as_str().map_or(0, str::len) })).collect();
        o.insert("files".into(), json!(files));
    }
    row
}

/// A state of the workspace by baseline name, or as `state_at` reads it: `current`, a project revision, `#N`.
pub fn state_named(app: &App, current: &[FileIn], history: &[Value], spec: &str) -> Result<(Vec<FileIn>, Value), ToolErr> {
    let spec = spec.trim();
    if let Some(b) = baseline_named(app, spec) {
        return Ok((baseline_files(&b), json!({ "revision": b["revision"], "is": format!("baseline {}", quote_clip(spec)), "baseline": spec, "time": b["time"], "note": b["note"], "author": b["author"], "element_count": b["element_count"], "history_seq": b["history_seq"] })));
    }
    state_at(current, history, spec).map_err(|mut e| {
        let names: Vec<String> = app.store.read_baselines().iter().filter_map(|b| b["name"].as_str().map(quote_clip)).collect();
        e.message.push_str(&if names.is_empty() { " A baseline name can be given too; this workspace has no baseline (create_baseline makes one).".to_string() } else { format!(" A baseline name can be given too: {}.", names.join(", ")) });
        e
    })
}

/// `m` with the workspace setting `budget_root` (`.sysml-bridge/settings.json`) that pins the root of an unrooted budget.
pub(super) fn pinned(app: &App, mut m: sysml_core::Model) -> sysml_core::Model {
    m.budget_root = app.store.setting("budget_root").and_then(|v| v.as_str().map(str::to_string));
    m.source_ids = super::ids::effective(app);
    m
}

pub(super) use sysml_core::agent::tools::model_of_files;

/// The model of the baseline a read tool was given, with what the reply says about it.
pub fn baseline_model(app: &App, a: &Value) -> Result<Option<(sysml_core::Model, Value)>, ToolErr> {
    let Some(spec) = a.get("baseline").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else { return Ok(None) };
    let _guard = app.store.lock_writes();
    let current = model_files(app);
    let (files, info) = state_named(app, &current, &app.store.read_history(), spec)?;
    Ok(Some((model_of_files(files), info)))
}

/// `create_baseline {name, note?, author?, at?}`: record a named, immutable point of the workspace history.
pub fn create_baseline(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let name = arg(a, "name")?.trim();
    let text = |k: &str| a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) || sysml_core::edit::name_fault(name).is_some() {
        return Err(err("a baseline name is 1 to 80 characters that show, with no control character. Fix: use a short name such as B1 or PDR-2026-10."));
    }
    if name.eq_ignore_ascii_case("current") || name.starts_with('#') || (name.len() == 16 && name.chars().all(|c| c.is_ascii_hexdigit())) {
        return Err(err(format!("{} cannot be a baseline name: 'current', #N and 16 hex digits name states of the write log. Fix: use a name such as B1.", quote_clip(name))));
    }
    let _guard = app.store.lock_writes();
    if let Some(b) = baseline_named(app, name) {
        return Err(ToolErr { message: format!("refused: baseline {} exists (revision {}, recorded {}), and a baseline is never replaced. Fix: choose another name; list_baselines shows the ones taken.", quote_claim(name), stored_token(&b["revision"]), stored_token(&b["time"])), data: json!({ "baseline": baseline_row(&b) }) });
    }
    let current = model_files(app);
    let history = app.store.read_history();
    let (files, at) = match text("at") {
        Some(spec) => state_named(app, &current, &history, spec)?,
        None => (current.clone(), json!({ "revision": project_revision(&current), "is": "current" })),
    };
    let m = pinned(app, model_of_files(files.clone()));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let revision = project_revision(&files);
    // The write the state is the result of: the last one, or the one `at` names.
    let seq = history.iter().rev().find(|e| e["revision_after"].as_str() == Some(revision.as_str())).and_then(|e| e["seq"].as_u64());
    let entry = json!({
        "name": name,
        "time": rfc3339(now),
        "ts_ms": now as u64,
        "author": text("author").unwrap_or("agent"),
        "note": text("note"),
        "revision": revision,
        "state": at["is"],
        "history_seq": seq,
        "element_count": m.elements.len(),
        "gates": sysml_core::agent::package::gates_of(&m),
        "files": files.iter().map(|f| json!({ "path": f.path, "text": f.text })).collect::<Vec<_>>(),
    });
    if entry.to_string().len() > MAX_ENTRY_BYTES {
        return Err(err("refused: the model is too large to record as one baseline entry."));
    }
    app.store.append_baseline(&entry).map_err(|e| err(format!("the baseline was not recorded: {e}")))?;
    let row = baseline_row(&entry);
    let g = &row["gates"];
    let text = format!(
        "Baseline {qn} recorded: revision {revision} ({}), {} elements, gates {} ({} errors, {} warnings).{} It can be named wherever a revision is: compare_revisions {{from: {qn}}}, get_change_package {{from: {qn}}}, get_interface_table {{baseline: {qn}}}.",
        model_name(at["is"].as_str().unwrap_or("")),
        stored_token(&row["element_count"]),
        if g["passed"] == true { "passed" } else { "FAILED" },
        stored_token(&g["errors"]),
        stored_token(&g["warnings"]),
        row["note"].as_str().map(|n| format!(" Note: {}.", sysml_core::agent::text::quote_claim(n))).unwrap_or_default(),
        qn = sysml_core::agent::text::quote_claim(name)
    );
    Ok(ToolOut { text, data: json!({ "baseline": row, "recorded": true }) })
}

/// `list_baselines`: every baseline, oldest first, and which one the workspace is at now.
pub fn list_baselines(app: &App) -> Result<ToolOut, ToolErr> {
    let now = project_revision(&model_files(app));
    let rows: Vec<Value> = app
        .store
        .read_baselines()
        .iter()
        .map(|b| {
            let mut r = baseline_row(b);
            r["is_current"] = json!(b["revision"].as_str() == Some(now.as_str()));
            r
        })
        .collect();
    let text = if rows.is_empty() {
        "No baseline recorded. create_baseline {name} freezes the current state.".to_string()
    } else {
        rows.iter().map(|b| format!("{}  {}  revision {}  {} elements  signed as {}{}{}", sysml_core::agent::text::quote_claim(b["name"].as_str().unwrap_or("")), stored_token(&b["time"]), stored_token(&b["revision"]), b["element_count"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "?".into()), sysml_core::agent::text::quote_claim(b["author"].as_str().unwrap_or("")), b["note"].as_str().map(|n| format!("  {}", sysml_core::agent::text::quote_claim(n))).unwrap_or_default(), if b["is_current"] == true { "  (the workspace is at this state)" } else { "" })).collect::<Vec<_>>().join("\n")
    };
    Ok(ToolOut { text, data: json!({ "count": rows.len(), "current_revision": now, "baselines": rows }) })
}

/// `get_change_package {from, to?, title?, reason?, author?}`: the record of one change between two states.
pub fn change_package(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let text = |k: &str| a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let from = text("from").ok_or_else(|| err("missing argument 'from': the older state, as a baseline name (list_baselines), a project revision, #N for the state after write N, or 'current'."))?;
    let to = text("to").unwrap_or("current");
    let _guard = app.store.lock_writes();
    let current = model_files(app);
    let history = app.store.read_history();
    let (before, mut from_info) = state_named(app, &current, &history, from)?;
    let (after, mut to_info) = state_named(app, &current, &history, to)?;
    let (bm, am) = (pinned(app, model_of_files(before)), pinned(app, model_of_files(after)));
    from_info["element_count"] = json!(bm.elements.len());
    to_info["element_count"] = json!(am.elements.len());
    // The writes of the log between the two states, when the log holds both.
    let at = |rev: &Value| history.iter().rposition(|e| e["revision_after"] == *rev);
    let (lo, hi) = (at(&from_info["revision"]).map(|i| i + 1).or_else(|| history.iter().position(|e| e["revision_before"] == from_info["revision"])), at(&to_info["revision"]));
    let writes = match (lo, hi) {
        (Some(lo), Some(hi)) if lo <= hi => {
            let l = &history[lo..=hi];
            let mut tools: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            let mut authors: Vec<String> = Vec::new();
            for e in l {
                *tools.entry(e["tool"].as_str().unwrap_or("").to_string()).or_default() += 1;
                let who = e["author"].as_str().unwrap_or("").to_string();
                if !who.is_empty() && !authors.contains(&who) {
                    authors.push(who);
                }
            }
            if to_info["time"].is_null() {
                to_info["time"] = l[l.len() - 1]["time"].clone();
            }
            json!({ "count": l.len(), "first": l[0]["seq"], "last": l[l.len() - 1]["seq"], "first_time": l[0]["time"], "last_time": l[l.len() - 1]["time"], "authors": authors, "tools": tools })
        }
        (Some(_), Some(_)) => json!({ "count": 0 }),
        _ => Value::Null,
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let author = text("author").map(str::to_string).or_else(|| writes["authors"].as_array().filter(|l| !l.is_empty()).map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))).unwrap_or_else(|| "agent".into());
    let meta = json!({ "title": text("title").unwrap_or("Change package"), "reason": text("reason"), "author": author, "prepared": rfc3339(now), "from": from_info, "to": to_info, "writes": writes });
    let ids = ids::effective(app);
    let p = sysml_core::agent::package::change_package(&bm, &am, &ids, &meta);
    let c = &p["changes"]["counts"];
    let text = format!(
        "Change package {}: {} ({}) -> {} ({}). {} requirement(s) changed; {} element(s) added, {} changed, {} removed; gates before {} errors, after {} errors. The whole record is in structuredContent.markdown ({} characters): save it as a .md file.",
        sysml_core::agent::text::quote_claim(meta["title"].as_str().unwrap_or("")),
        stored_token(&meta["from"]["revision"]),
        model_name(meta["from"]["is"].as_str().unwrap_or("")),
        stored_token(&meta["to"]["revision"]),
        model_name(meta["to"]["is"].as_str().unwrap_or("")),
        p["requirements"].as_array().map_or(0, Vec::len),
        c["added"],
        c["changed"],
        c["removed"],
        stored_token(&p["gates"]["before"]["errors"]),
        stored_token(&p["gates"]["after"]["errors"]),
        p["markdown"].as_str().map_or(0, str::len)
    );
    Ok(ToolOut { text, data: p })
}

/// The change report between two recorded revisions.
pub fn compare_revisions(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let from = a.get("from").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| err("missing argument 'from': the older state, as a project revision (from get_history), #N for the state after write N, or 'current'."))?;
    let to = a.get("to").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or("current");
    let mode = report_mode(a)?;
    if mode == Report::None {
        return Err(err("change_report: 'none' leaves compare_revisions nothing to return. Fix: use 'summary' or 'full'."));
    }
    let _guard = app.store.lock_writes();
    let current = model_files(app);
    let history = app.store.read_history();
    let (before, from_info) = state_named(app, &current, &history, from)?;
    let (after, to_info) = state_named(app, &current, &history, to)?;
    let ids = ids::effective(app);
    let report = change::change_report(&before, &after, &ids, &ids, mode == Report::Full);
    let text = format!("{} ({}) -> {} ({}).\n{}", stored_token(&from_info["revision"]), model_name(from_info["is"].as_str().unwrap_or("")), stored_token(&to_info["revision"]), model_name(to_info["is"].as_str().unwrap_or("")), change::summary_text(&report));
    Ok(ToolOut { text, data: json!({ "from": from_info, "to": to_info, "change_report": report }) })
}

pub fn apply_edits(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let edits = a.get("edits").and_then(Value::as_array).ok_or_else(|| err("missing argument 'edits': a list of {op, ...}"))?;
    if edits.is_empty() || edits.len() > intent::MAX_EDITS {
        return Err(err(intent::too_many(edits.len())));
    }
    write_with(app, "apply_edits", a, |before| edits_built(app, before, edits).map(|(b, _)| b))
}

/// The edits of a batch run on `before`, as `apply_edits` runs them: every check of every edit, and the whole batch or
/// nothing. Returns what the write would be and the files after it.
pub(super) fn edits_built(app: &App, before: &[FileIn], edits: &[Value]) -> Result<(Built, Vec<FileIn>), ToolErr> {
    let batch = intent::apply_batch_ids(before, edits, &ids::effective(app)).map_err(|(i, f)| ToolErr {
        message: format!("edit #{} failed: [{}] {} Nothing was written.", i + 1, intent::edit_label(&edits[i]), f.message),
        // `edit #N` and `failed_index` count from 1, like `reordered` and the `#n ref:` lines; `edit` echoes the edit as sent.
        data: json!({ "failed_index": i + 1, "edit": edits[i], "detail": f.data }),
    })?;
    let mut info = json!({ "results": batch.results });
    if !batch.moved.is_empty() {
        info["reordered"] = json!(batch.moved);
    }
    if !batch.source_id_moves.is_empty() {
        info["source_id_moves"] = json!(batch.source_id_moves);
    }
    if !batch.reminted.is_empty() {
        info["reminted"] = json!(batch.reminted);
    }
    let sweep = edits.iter().any(|e| e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) == Some("replace"));
    let ids = ids::plan(app, before, &batch.files, &batch.events, sweep)?;
    let after = batch.files.clone();
    let mut built = Built::of(proposed_from(before, batch.files)?, info, Gate::Report);
    built.ids = ids;
    follow_side(app, &after, &batch.events, "apply_edits", &mut built);
    Ok((built, after))
}

/// Edits of one run of the batch engine inside `import_requirements`: under the limit of `apply_edits`, with room.
const IMPORT_CHUNK: usize = 190;

/// `import_requirements {csv, owner, ...}`: requirements from a spreadsheet. The rows become the edits `apply_edits`
/// takes (`reqimport::plan`), run through the same engine in runs of at most 190, and saved as one write with one entry
/// of the write log. Every text of the file passes the checks of that engine; nothing of the file is trusted.
pub fn import_requirements(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    use sysml_core::agent::reqimport;
    let dry = a.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let mut out = write_with(app, "import_requirements", a, |before| {
        let plan = reqimport::plan(&model_of_files(before.to_vec()), a)?;
        let ids = ids::effective(app);
        let mut files = before.to_vec();
        let mut events = Vec::new();
        for (n, chunk) in plan.edits.chunks(IMPORT_CHUNK).enumerate() {
            let b = intent::apply_batch_ids(&files, chunk, &ids).map_err(|(i, f)| {
                let at = n * IMPORT_CHUNK + i;
                let row = plan.rows_of.get(at).copied().flatten();
                let id = chunk[i].get("short_name").or_else(|| chunk[i].get("ref")).or_else(|| chunk[i].get("source")).and_then(Value::as_str).unwrap_or("");
                ToolErr {
                    message: format!("{} failed: [{}] {} Nothing was written: an import is all or nothing. Fix: correct or remove that row and import again.", row.map(|r| format!("row {r} ({})", quote_clip(id))).unwrap_or_else(|| "the set-up of the import".into()), intent::edit_label(&chunk[i]), f.message),
                    data: json!({ "row": row, "edit": chunk[i], "detail": f.data }),
                }
            })?;
            files = b.files;
            events.extend(b.events);
        }
        let id_changes = ids::plan(app, before, &files, &events, false)?;
        let info = json!({ "import": plan.report, "edits": plan.edits.len() });
        let mut built = Built::of(proposed_from(before, files)?, info, Gate::Report);
        built.ids = id_changes;
        Ok(built)
    })?;
    let import = out.data["info"]["import"].clone();
    let mut text = reqimport::report_text(&import, dry);
    text.push_str(&out.text);
    out.text = text;
    for k in ["created", "existing", "changed", "skipped", "links", "links_skipped", "counts", "columns", "rows", "owner"] {
        out.data[k] = import[k].clone();
    }
    Ok(out)
}

/// The writes of the log that changed the declaration text of one element (its own text and the members it holds),
/// newest first, each with element_change: created (absent before the write) or changed. The element is named as it is
/// now and is followed back by that qualified name: a write that renamed it or moved it reads as created. Only the writes
/// the log still leads back through are read (a save outside the tools ends them), at most `ELEMENT_SCAN` of them.
const ELEMENT_SCAN: usize = 500;

fn element_writes(app: &App, all: &[Value], target: &str) -> Result<(String, Vec<(u64, &'static str)>, usize, bool), ToolErr> {
    let current = model_files(app);
    let now = sysml_core::agent::tools::model_of(&current);
    let qn = sysml_core::agent::locate(&now, target).map_err(|f| err(format!("{} Fix: name an element of the current model; get_history follows it back from now.", f.message)))?.qualified_name.clone();
    let states = states_back(&current, all, ELEMENT_SCAN + 1);
    // The element's path and text in a state; None when the state has no element of that name.
    let text_in = |files: &[FileIn]| -> Option<(String, String)> {
        let m = sysml_core::api::to_model(files.to_vec());
        let e = sysml_core::agent::locate_strict(&m, &qn).ok()?.el;
        let f = m.files.get(e.file)?;
        Some((f.path.clone(), f.text.get(e.span.start..e.span.end)?.to_string()))
    };
    let mut hits = Vec::new();
    let mut after = text_in(&states[0].1);
    let writes: Vec<&Value> = all.iter().rev().collect();
    for (k, pair) in states.windows(2).enumerate() {
        let e = writes[k];
        // A write that touched no file holding the element leaves its text as it was: skip the parse.
        let touched = e["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str()).any(|p| after.as_ref().map_or(true, |(path, _)| path == p));
        if !touched {
            continue;
        }
        let before = text_in(&pair[1].1);
        match (&before, &after) {
            (None, Some(_)) => hits.push((e["seq"].as_u64().unwrap_or(0), "created")),
            (Some((_, b)), Some((_, a))) if a != b => hits.push((e["seq"].as_u64().unwrap_or(0), "changed")),
            _ => {}
        }
        after = before;
        if after.is_none() {
            break;
        }
    }
    let scanned = states.len() - 1;
    let whole = scanned == all.len() || after.is_none();
    Ok((qn, hits, scanned, whole))
}

pub fn get_history(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    let limit = a.get("limit").and_then(Value::as_u64).unwrap_or(20).clamp(1, 200) as usize;
    let all = app.store.read_history();
    let undone: Vec<u64> = all.iter().filter_map(|e| e["undoes"].as_u64()).collect();
    let target = a.get("ref").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let element = target.map(|t| element_writes(app, &all, t)).transpose()?;
    let change_of = |e: &Value| -> Option<&'static str> { element.as_ref().and_then(|(_, hits, _, _)| hits.iter().find(|(s, _)| Some(*s) == e["seq"].as_u64()).map(|(_, c)| *c)) };
    let rows: Vec<Value> = all
        .iter()
        .rev()
        .filter(|e| element.is_none() || change_of(e).is_some())
        .take(limit)
        .map(|e| {
            let mut e = e.clone();
            for f in e["files"].as_array_mut().into_iter().flatten() {
                f.as_object_mut().map(|o| o.remove("hunks"));
            }
            e["undone"] = json!(e["seq"].as_u64().map(|s| undone.contains(&s)).unwrap_or(false));
            if let Some(c) = change_of(&e) {
                e["element_change"] = json!(c);
            }
            e
        })
        .collect();
    if let Some((qn, hits, scanned, whole)) = &element {
        let head = format!(
            "{} writes changed {} ({} of {} writes read{}).",
            hits.len(),
            model_name(qn),
            scanned,
            all.len(),
            if *whole { "" } else { "; older writes are not reachable from the current files" }
        );
        let lines: Vec<String> = rows.iter().map(|e| format!("#{} {} {} {}", stored_token(&e["seq"]), stored_token(&e["time"]), model_name(e["tool"].as_str().unwrap_or("")), e["element_change"].as_str().unwrap_or(""))).collect();
        let text = if lines.is_empty() { head } else { format!("{head}\n{}", lines.join("\n")) };
        return Ok(ToolOut { text, data: json!({ "count": rows.len(), "total": all.len(), "entries": rows, "element": { "ref": qn, "writes": hits.len(), "scanned": scanned, "complete": whole } }) });
    }
    let text = if rows.is_empty() {
        "No writes recorded yet.".to_string()
    } else {
        rows.iter()
            .map(|e| {
                let files: Vec<&str> = e["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str()).collect();
                format!("#{} {} {} {}{}", stored_token(&e["seq"]), stored_token(&e["time"]), model_name(e["tool"].as_str().unwrap_or("")), files.iter().map(|f| model_name(f)).collect::<Vec<_>>().join(", "), if e["undone"] == true { " (undone)" } else { "" })
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(ToolOut { text, data: json!({ "count": rows.len(), "total": all.len(), "entries": rows }) })
}

pub fn undo(app: &App, a: &Value) -> Result<ToolOut, ToolErr> {
    write_with(app, "undo", a, |before| {
        let all = app.store.read_history();
        let undone: Vec<u64> = all.iter().filter_map(|e| e["undoes"].as_u64()).collect();
        let target = all
            .iter()
            .rev()
            .find(|e| e["tool"] != "undo" && !e["seq"].as_u64().map(|s| undone.contains(&s)).unwrap_or(true))
            .ok_or_else(|| err("nothing to undo: the write log has no write that is still in effect"))?;
        let seq = target["seq"].as_u64().unwrap_or(0);
        let mut proposed = Vec::new();
        for f in target["files"].as_array().into_iter().flatten() {
            let path = model_path(f["path"].as_str().unwrap_or(""))?;
            let now = before.iter().find(|b| b.path == path);
            let expected = f["revision_after"].as_str();
            let have = now.map(|n| revision_of(&n.text));
            if have.as_deref() != expected {
                return Err(err(format!(
                    "cannot undo write #{seq}: {} changed after it (revision now {}, expected {}). Review with get_history and fix by hand or with another edit.",
                    quote_clip(&path),
                    stored_token(&json!(have.as_deref().unwrap_or("absent"))),
                    stored_token(&json!(expected.unwrap_or("absent")))
                )));
            }
            if f["change"] == "created" {
                proposed.push(Proposed { path, text: None });
                continue;
            }
            let hunks: Vec<diff::Hunk> = serde_json::from_value(f["hunks"].clone()).map_err(|e| err(format!("write #{seq} has an unreadable log entry: {e}")))?;
            let text = diff::reverse(now.map(|n| n.text.as_str()).unwrap_or(""), &hunks).map_err(|e| err(format!("cannot undo write #{seq}: {e}")))?;
            proposed.push(Proposed { path, text: Some(text) });
        }
        let mut built = Built::of(proposed, json!({ "undid": seq, "tool": target["tool"] }), Gate::Open);
        // The side files that followed the write go back with it, unless they changed since.
        for f in target["side"].as_array().into_iter().flatten() {
            let (Some(path), Some(after)) = (f["path"].as_str(), f["after"].as_str()) else { continue };
            if app.store.read(path).ok().as_deref() == Some(after) {
                built.side_logged.push((path.to_string(), Some(after.to_string()), f["before"].as_str().unwrap_or("").to_string()));
            }
        }
        built.undoes = Some(seq);
        built.ids = ids::reversed(app, &target["ids"]);
        Ok(built)
    })
}
