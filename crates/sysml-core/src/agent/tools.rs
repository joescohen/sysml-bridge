//! The read tools, written once. A caller (the server, `sysml-server`, or an in-memory [`MemoryCtx`]) answers a read tool by
//! calling [`dispatch`] with a [`Ctx`]: the model, the ids and, where there is one, the stores. A tool that needs a store the
//! caller does not have (history, baselines, stored reviews) asks the `Ctx`; the default answer of a `Ctx` without that store
//! says so and never guesses.
//!
//! The tool list (`tool_schema`) holds the name, the schema and the read-only flag of every tool. [`TABLE`] holds the handler of
//! every read tool that runs on a `Ctx`. A test in the server crate checks that the two agree: every tool the list classes as a
//! read is in the table or in [`NOT_SHARED`] with its reason.

use super::suspect::Reviews;
use super::tools_gen;
use super::tools_suspect::{self, mark_ignored};
use super::text::{model_name, quote_clip};
use super::{self as agent, graph, trace};
use crate::api::{self, FileIn};
use crate::{Model, Severity};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Longest error text sent back. Messages name the text that was wrong, and that text can be as long as
/// the request; the cut is made between characters.
pub const MAX_ERROR_BYTES: usize = 16 << 10;

pub fn cap_message(mut s: String) -> String {
    if s.len() > MAX_ERROR_BYTES {
        let cut = (0..=MAX_ERROR_BYTES).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
        s.truncate(cut);
        s.push_str(" ... (message cut)");
    }
    s
}

/// A tool result: a short text for people and a JSON object for programs.
pub struct ToolOut {
    pub text: String,
    pub data: Value,
}

/// A tool failure. The message says what to do next; `data` carries detail such as referrers.
pub struct ToolErr {
    pub message: String,
    pub data: Value,
}

impl From<String> for ToolErr {
    fn from(message: String) -> Self {
        ToolErr { message, data: Value::Null }
    }
}

impl From<&str> for ToolErr {
    fn from(message: &str) -> Self {
        ToolErr { message: message.to_string(), data: Value::Null }
    }
}

impl From<agent::Fail> for ToolErr {
    fn from(f: agent::Fail) -> Self {
        ToolErr { message: f.message, data: f.data }
    }
}

pub type ToolResult = Result<ToolOut, ToolErr>;

pub fn err(message: impl Into<String>) -> ToolErr {
    ToolErr { message: message.into(), data: Value::Null }
}

pub fn out(text: impl Into<String>, data: Value) -> ToolResult {
    let mut text: String = text.into();
    // A read that was built from cut relationship pairs says so first in its text, not only in a field: a reader that
    // stops after the first lines must not take the answer for a whole one.
    if let Some(n) = data["not_checked"].as_str().filter(|n| !text.starts_with(n)) {
        text = format!("{n}\n{text}");
    }
    Ok(ToolOut { text, data })
}

/// The sentence an in-memory copy answers to a tool that changes anything.
pub const READ_ONLY_COPY: &str = "This copy is read-only: changes are made through the server.";

/// A failed write names its failing edits at `structuredContent.failures[]` (`{edit, error}`, 1-based) and groups
/// them by cause at `structuredContent.causes[]` (`{edits[], error}`), the same for `apply_edits`, `edit_model` and
/// the other write tools. A single-edit tool reports edit 1. The older place, `detail.detail`, keeps its copy.
pub fn failure_lists(e: &ToolErr, data: &mut Value) {
    let inner = &e.data["detail"];
    let failures = match inner["failures"].as_array().filter(|l| !l.is_empty()) {
        Some(l) => l.clone(),
        None => vec![json!({ "edit": e.data["failed_index"].as_u64().unwrap_or(1), "error": e.message })],
    };
    let causes = match inner["causes"].as_array().filter(|l| !l.is_empty()) {
        Some(l) => l.clone(),
        None => vec![json!({ "edits": failures.iter().filter_map(|f| f["edit"].as_u64()).collect::<Vec<_>>(), "error": failures[0]["error"] })],
    };
    data["failures"] = json!(failures);
    data["causes"] = json!(causes);
}

/// The reply of `tools/call`: `content`, `structuredContent` and `isError`. The one place it is built.
pub fn envelope(result: ToolResult, is_write: bool) -> Value {
    match result {
        Ok(o) => json!({ "content": [{ "type": "text", "text": o.text }], "structuredContent": o.data, "isError": false }),
        Err(mut e) => {
            e.message = cap_message(e.message);
            let mut data = json!({ "error": e.message });
            if is_write {
                failure_lists(&e, &mut data);
            }
            if !e.data.is_null() {
                data["detail"] = e.data;
            }
            json!({ "content": [{ "type": "text", "text": e.message }], "structuredContent": data, "isError": true })
        }
    }
}

/// The model of a file set: the files, the namespace checks. Built once for as long as its revision is one of the last few asked for.
pub fn model_of(files: &[FileIn]) -> Arc<Model> {
    static CACHE: Mutex<Vec<(String, Arc<Model>)>> = Mutex::new(Vec::new());
    const CACHE_SLOTS: usize = 4;
    const CACHE_BYTES: usize = 64 << 20;
    let rev = agent::project_revision(files);
    if let Ok(mut c) = CACHE.lock() {
        if let Some(i) = c.iter().position(|(r, _)| *r == rev) {
            let entry = c.remove(i);
            let m = entry.1.clone();
            c.insert(0, entry);
            return m;
        }
    }
    let m = Arc::new(model_of_files(files.to_vec()));
    if files.iter().map(|f| f.text.len()).sum::<usize>() <= CACHE_BYTES {
        if let Ok(mut c) = CACHE.lock() {
            c.retain(|(r, _)| *r != rev);
            c.insert(0, (rev, m.clone()));
            c.truncate(CACHE_SLOTS);
        }
    }
    m
}

/// The full rule check of one revision with one set of options, computed once: a client may ask for it twice at
/// load (check_all and get_validation_results), often at the same time. A caller that arrives while another computes
/// the same key waits for that result. Ignore marks are applied by the caller, after the cache.
pub(crate) fn check_all_cached(files: &[FileIn], options: &Value, compute: impl FnOnce() -> agent::Res<Value>) -> agent::Res<Value> {
    type Cell = Arc<std::sync::OnceLock<Result<Value, (String, Value)>>>;
    static CACHE: Mutex<Vec<(String, Cell)>> = Mutex::new(Vec::new());
    const CACHE_SLOTS: usize = 4;
    let key = format!("{}|{}", agent::project_revision(files), options);
    let cell: Cell = match CACHE.lock() {
        Ok(mut c) => {
            let cell = match c.iter().position(|(k, _)| *k == key) {
                Some(i) => c.remove(i).1,
                None => Arc::default(),
            };
            c.insert(0, (key, cell.clone()));
            c.truncate(CACHE_SLOTS);
            cell
        }
        Err(_) => Arc::default(),
    };
    match cell.get_or_init(|| compute().map_err(|f| (f.message, f.data))) {
        Ok(v) => Ok(v.clone()),
        Err((message, data)) => Err(agent::Fail { message: message.clone(), data: data.clone() }),
    }
}

pub fn model_of_files(files: Vec<FileIn>) -> Model {
    let mut m = api::to_model(files);
    agent::add_namespace_checks(&mut m);
    m
}

/// The two file sets a suspect-link listing compares, and what the version bar says about each.
pub struct SuspectPair {
    pub before: Vec<FileIn>,
    pub after: Vec<FileIn>,
    pub bar_before: String,
    pub bar_after: String,
    pub versions: Value,
}

/// Something that is held while a call reads, and let go when it drops.
pub trait Guard {}
impl<T> Guard for T {}

/// What a read tool needs from where it runs. Every method has a default for a caller that has no such store: the default says
/// so (an error, or an empty list), it never answers as if the store were there.
pub trait Ctx {
    /// The current model with the settings and the ids pinned on it. Shared, not copied: a read asks it several times per
    /// click, and a copy of a 4,000-symbol model took a fifth of a second.
    fn model(&self) -> Arc<Model>;
    /// The model files (not the layout files).
    fn files(&self) -> Vec<FileIn>;
    /// Every file of the workspace the store lists: model files, layouts, import reports.
    fn all_files(&self) -> Vec<FileIn>;
    /// The source ids in force: id to qualified name.
    fn ids(&self) -> BTreeMap<String, String>;
    fn read_file(&self, path: &str) -> Result<String, String>;
    /// True for a file that holds bytes that are not UTF-8 text.
    fn not_utf8(&self, _path: &str) -> bool {
        false
    }
    /// True where there is no history, baseline or review store (an in-memory copy).
    fn offline(&self) -> bool {
        false
    }
    /// Held while a call reads several stores, so that they agree.
    fn hold(&self) -> Box<dyn Guard + '_> {
        Box::new(())
    }
    fn baseline_names(&self) -> Vec<String> {
        Vec::new()
    }
    /// The files and the description of a named state (a baseline, `#N`, a revision) of the history.
    fn state_named(&self, _current: &[FileIn], spec: &str) -> Result<(Vec<FileIn>, Value), ToolErr> {
        Err(err(format!("{} cannot be read here: baselines and the write history are kept by the server.", quote_clip(spec))))
    }
    /// The model of the baseline a read tool was given, with what the reply says about it.
    fn baseline_model(&self, a: &Value) -> Result<Option<(Model, Value)>, ToolErr> {
        match a.get("baseline").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(spec) => self.state_named(&self.files(), spec).map(|_| None),
        }
    }
    fn reviews(&self) -> Reviews {
        Reviews::new()
    }
    fn rule_ignores(&self) -> Vec<Value> {
        Vec::new()
    }
    /// The earlier states of the write log, newest first, for the check of an ignored result.
    fn past_states(&self, _files: &[FileIn], _depth: usize) -> Vec<(String, Vec<FileIn>)> {
        Vec::new()
    }
    /// The two states a suspect-link listing compares.
    fn suspect_pair(&self, _a: &Value) -> Result<SuspectPair, ToolErr> {
        Err(err("the older state cannot be read here: baselines and the write history are kept by the server."))
    }
}

/// An argument of a tool, required text; the message names it and says what it is.
pub fn arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, ToolErr> {
    match a.get(k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        Some(Value::String(_)) => Err(ToolErr::from(format!("argument '{k}' is empty: it must be {}.", agent::intent::arg_what(k)))),
        Some(Value::Null) | None => Err(ToolErr::from(format!("missing argument '{k}': {}.", agent::intent::arg_what(k)))),
        Some(_) => Err(ToolErr::from(format!("argument '{k}' must be a string: {}.", agent::intent::arg_what(k)))),
    }
}

/// An optional list of strings; a single string is a list of one.
pub fn str_list(a: &Value, k: &str) -> Result<Option<Vec<String>>, ToolErr> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(vec![s.clone()])),
        Some(Value::Array(v)) => v.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| ToolErr::from(format!("{k} must be a list of strings.")))).collect::<Result<Vec<_>, _>>().map(Some),
        Some(_) => Err(format!("{k} must be a list of strings.").into()),
    }
}

pub fn opt<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

pub fn count(a: &Value, k: &str, default: u64) -> usize {
    a.get(k).and_then(|v| v.as_u64()).unwrap_or(default) as usize
}

/// `ref` is the documented name; `name` is what get_element took before.
pub fn target<'a>(a: &'a Value) -> Result<&'a str, ToolErr> {
    opt(a, "ref").or_else(|| opt(a, "name")).or_else(|| opt(a, "element")).ok_or_else(|| ToolErr::from("missing argument 'ref': the qualified name of the element"))
}

pub fn diag_text(m: &Model, only_file: Option<&str>) -> String {
    let d: Vec<_> = api::diagnostics_out(m).into_iter().filter(|d| only_file.map(|f| f == d.file).unwrap_or(true)).collect();
    if d.is_empty() {
        return "No problems found.".into();
    }
    let errors = d.iter().filter(|x| x.severity == Severity::Error).count();
    let mut out = format!("{} errors, {} warnings\n", errors, d.len() - errors);
    for x in d.iter().take(100) {
        out.push_str(&format!("{} {}:{}:{} {}\n", format!("{:?}", x.severity).to_lowercase(), model_name(&x.file), x.line, x.col, model_name(&x.message)));
    }
    if d.len() > 100 {
        out.push_str(&format!("... {} more\n", d.len() - 100));
    }
    out
}

fn list_files(ctx: &dyn Ctx, _a: &Value) -> ToolResult {
        let files = ctx.all_files();
        // From the bytes: a valid file that holds the character U+FFFD is text, and is not marked.
        let not_text = |f: &crate::api::FileIn| f.text.contains(crate::lexer::NOT_TEXT) && ctx.not_utf8(&f.path);
        let rows: Vec<Value> = files
            .iter()
            .map(|f| {
                let mut row = json!({ "path": f.path, "lines": f.text.lines().count(), "bytes": f.text.len(), "revision": agent::revision_of(&f.text) });
                if not_text(f) {
                    row["not_utf8"] = json!(true);
                }
                row
            })
            .collect();
        let line = |f: &crate::api::FileIn| format!("{} ({} lines){}", model_name(&f.path), f.text.lines().count(), if not_text(f) { " NOT UTF-8 TEXT: every gate fails until it is saved as UTF-8" } else { "" });
        let text = if files.is_empty() { "The workspace is empty.".to_string() } else { files.iter().map(line).collect::<Vec<_>>().join("\n") };
        out(text, json!({ "count": rows.len(), "files": rows }))
}

fn path_arg(a: &Value) -> Result<&str, ToolErr> {
    match a.get("path").or_else(|| a.get("file")) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        Some(Value::Null) | None => Err(err(format!("missing argument 'path' (alias 'file'): {}.", agent::intent::arg_what("path")))),
        Some(_) => Err(err(format!("argument 'path' (alias 'file') must be a string: {}.", agent::intent::arg_what("path")))),
    }
}

/// `get_connections`: why one element is there, what it does, who does it, what it exchanges and what proves it.
fn get_connections(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let short = |q: &str| q.rsplit("::").next().unwrap_or(q).to_string();
    let v = agent::connections::connections(&ctx.model(), arg(a, "element")?)?;
    let mut lines = vec![format!("{} ({})", short(v["element"].as_str().unwrap_or("")), v["kind"].as_str().unwrap_or(""))];
    for (k, t) in [("why", "Why"), ("breaks", "Breaks into"), ("does", "Does"), ("who", "Who"), ("exchanges", "Exchanges"), ("proof", "Proof")] {
        let rows: Vec<String> = v[k].as_array().into_iter().flatten().map(|r| {
            let base = short(r["ref"].as_str().unwrap_or(""));
            let name = match r["id"].as_str() { Some(id) => format!("{id} {base}"), None => base };
            let rel = r["relation"].as_str().unwrap_or("");
            match r["via"].as_str().filter(|x| !x.is_empty()) {
                Some(via) => format!("{name} ({rel}, via {})", short(via)),
                None => format!("{name} ({rel})"),
            }
        }).collect();
        lines.push(format!("{t}: {}", if rows.is_empty() { "-".to_string() } else { rows.join("; ") }));
    }
    out(lines.join("\n"), v)
}

fn read_file(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let path = path_arg(a)?;
        let text = ctx.read_file(path)?;
        let data = json!({ "path": path, "revision": agent::revision_of(&text), "lines": text.lines().count(), "text": text });
        out(text, data)
}

fn list_traceability_properties(ctx: &dyn Ctx, _a: &Value) -> ToolResult {
        let v = agent::trace_props::table_for(&ctx.model());
        let text = v["properties"].as_array().into_iter().flatten().map(|p| {
            if p["source"] == "model" {
                // A property the model declares: every string in the line is the model's own words, so each stands quoted.
                let applies = p["applies_to"].as_str().filter(|x| !x.is_empty()).map(|x| format!(", applies to {}", quote_clip(x))).unwrap_or_default();
                let same = if p["same_name_as_built_in"] == true { ", same name as a built-in" } else { "" };
                let how = if p["evaluated"] == true { format!("a chain of hops: {}", p["hops"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|h| h.chars().filter(|c| c.is_ascii_alphabetic() || *c == ':').collect::<String>()).collect::<Vec<_>>().join(", ")) } else { p["reason"].as_str().unwrap_or("declared in the model, not evaluated").to_string() };
                format!("- {} (declared in the model{same}{applies}; source: model): {how}", quote_clip(p["name"].as_str().unwrap_or("")))
            } else {
                format!("- {} ({}; source: built in): {}", p["name"].as_str().unwrap_or(""), p["group"].as_str().unwrap_or(""), p["rule"].as_str().unwrap_or(""))
            }
        }).collect::<Vec<_>>().join("\n");
        out(text, v)
}

fn get_budget(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let v = agent::budget::budget(&ctx.model(), opt(a, "quantity"), opt(a, "root"), opt(a, "limit_requirement"))?;
        out(v["text"].as_str().unwrap_or("").to_string(), v)
}

fn get_impact(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let m = ctx.model();
        let expand = a.get("expand").and_then(Value::as_u64).map(|n| n as usize);
        let v = agent::impact::get_impact_with(&m, &ctx.ids(), target(a)?, count(a, "depth", 1), count(a, "limit", agent::impact::DEFAULT_ROWS as u64), expand)?;
        let mut text = format!("Impact of {} [{}]:\n", model_name(v["element"]["ref"].as_str().unwrap_or("")), model_name(v["element"]["kind"].as_str().unwrap_or("")));
        for (k, n) in v["counts"].as_object().into_iter().flatten() {
            text.push_str(&format!("  {k}: {n}\n"));
        }
        if v["truncated"] == true {
            text.push_str(&format!("Lists cut at {} rows ({}); the counts are {}. Pass limit for more.\n", v["limit"], v["truncated_lists"].as_array().map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(), if v["not_checked"].is_string() { "of the pairs that were read" } else { "exact" }));
        }
        text.push_str("Read structuredContent for the rows.");
        out(text, v)
}

fn get_outline(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let depth = count(a, "depth", 4);
        let root = opt(a, "root");
        let outline = api::outline(&ctx.model(), root, depth)?;
        let text = if outline.is_empty() { "The model is empty.".to_string() } else { outline };
        out(text.clone(), json!({ "root": root, "depth": depth, "outline": text }))
}

fn find_elements(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let m = ctx.model();
        let q = opt(a, "query").unwrap_or("").to_lowercase();
        let kind = opt(a, "kind").unwrap_or("");
        let limit = count(a, "limit", 50);
        let hits: Vec<&crate::Element> = m
            .elements
            .iter()
            // Statements such as `perform x;` or a flow without a name have no declared name; with `kind` they are
            // listed too, so the total agrees with counts_by_kind of get_project_state.
            .filter(|e| !kind.is_empty() || (e.name.is_some() && !e.implied_name))
            .filter(|e| q.is_empty() || e.name.as_ref().map(|n| n.to_lowercase().contains(&q)).unwrap_or(false))
            .filter(|e| kind.is_empty() || e.keyword() == kind || e.kind == kind)
            .collect();
        let mut by_keyword: BTreeMap<String, usize> = BTreeMap::new();
        for e in &hits {
            *by_keyword.entry(e.keyword()).or_default() += 1;
        }
        if hits.is_empty() {
            return out("No matching elements.", json!({ "total": 0, "returned": 0, "by_keyword": {}, "elements": [] }));
        }
        let mut text = hits.iter().take(limit).map(|e| format!("{}  [{}]  {}:{}", model_name(&e.qualified_name), model_name(&e.keyword()), model_name(&m.files[e.file].path), agent::line_of(&m, e))).collect::<Vec<_>>().join("\n");
        if hits.len() > limit {
            text.push_str(&format!("\n... {} more", hits.len() - limit));
        }
        let rows: Vec<Value> = hits
            .iter()
            .take(limit)
            .map(|e| {
                let mut b = agent::brief(&m, e);
                if e.name.is_none() || e.implied_name {
                    b["unnamed"] = json!(true);
                }
                b
            })
            .collect();
        out(text, json!({ "total": hits.len(), "returned": rows.len(), "by_keyword": by_keyword, "elements": rows }))
}

fn get_element(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let m = ctx.model();
        let mut v = agent::element_details(&m, target(a)?, opt(a, "keyword"))?;
        if let Ok(e) = agent::locate(&m, target(a)?) {
            agent::element_links::attach(&m, &ctx.ids(), e, &mut v);
        }
        out(serde_json::to_string_pretty(&v).unwrap(), v)
}

fn get_project_state(ctx: &dyn Ctx, _a: &Value) -> ToolResult {
        let s = agent::project_state(&ctx.model());
        let text = format!(
            "Revision {}. {} files, {} elements, {} errors, {} warnings.",
            s["revision"].as_str().unwrap_or(""),
            s["files"].as_array().map(|f| f.len()).unwrap_or(0),
            s["element_count"],
            s["diagnostics"]["errors"],
            s["diagnostics"]["warnings"]
        );
        out(text, s)
}

fn query_elements(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let m = ctx.model();
        let q = agent::Query { kind: opt(a, "kind"), name: opt(a, "name"), owner: opt(a, "owner"), typed_by: opt(a, "typed_by"), limit: count(a, "limit", 50) };
        let v = agent::query_elements(&m, &q)?;
        let rows: Vec<String> = v["elements"].as_array().into_iter().flatten().map(|e| format!("{}  [{}]  {}:{}", model_name(e["ref"].as_str().unwrap_or("")), model_name(e["kind"].as_str().unwrap_or("")), model_name(e["file"].as_str().unwrap_or("")), e["line"])).collect();
        let text = if rows.is_empty() { "No matching elements.".to_string() } else { format!("{} of {} elements\n{}", rows.len(), v["total"], rows.join("\n")) };
        out(text, v)
}

fn query_relationships(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let m = ctx.model();
        let e = agent::locate(&m, target(a)?)?;
        let v = graph::query(&m, e.id, opt(a, "kind"), opt(a, "direction").unwrap_or("both"), count(a, "depth", 1).clamp(1, 10))?;
        let lines: Vec<String> = v["edges"].as_array().into_iter().flatten().map(|x| format!("{} --{}--> {}", model_name(x["source"].as_str().unwrap_or("")), model_name(x["kind"].as_str().unwrap_or("")), model_name(x["target"].as_str().unwrap_or("")))).collect();
        out(if lines.is_empty() { if v["not_checked"].is_string() { "No relationships found.".to_string() } else { "No relationships.".to_string() } } else { lines.join("\n") }, v)
}

fn get_traceability(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let v = trace::traceability(&ctx.model(), target(a)?, count(a, "depth", 3))?;
        let lines = v["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str()).collect::<Vec<_>>();
        out(if lines.is_empty() { if v["not_checked"].is_string() { "No trace links found.".to_string() } else { "No trace links.".to_string() } } else { lines.join("\n") }, v)
}

fn export_facts(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let mut v = agent::export_facts(&ctx.model(), opt(a, "scope"))?;
        let map = ctx.ids();
        let mut by_ref: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (id, qn) in &map {
            by_ref.entry(qn.as_str()).or_default().push(id.as_str());
        }
        let mut listed = serde_json::Map::new();
        for el in v["elements"].as_array_mut().into_iter().flatten() {
            if let Some(found) = el["ref"].as_str().and_then(|r| by_ref.get(r)) {
                for id in found {
                    listed.insert(id.to_string(), el["ref"].clone());
                }
                el["source_ids"] = json!(found);
            }
        }
        // An edge names the statement that declares it; that statement's source id, if it has one, rides along.
        for edge in v["edges"].as_array_mut().into_iter().flatten() {
            if let Some(found) = edge["declared_at"]["element"].as_str().and_then(|r| by_ref.get(r)) {
                for id in found {
                    listed.insert(id.to_string(), edge["declared_at"]["element"].clone());
                }
                edge["source_ids"] = json!(found);
                edge["source_id"] = json!(found[0]);
            }
        }
        v["source_ids"] = Value::Object(listed);
        // In a workspace that has source ids (an import, or a rebuild that records them), say which elements have none:
        // those cannot be joined to the source project by id.
        let mut text_tail = String::new();
        if !map.is_empty() {
            let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
            for el in v["elements"].as_array().into_iter().flatten().filter(|el| el.get("source_ids").is_none()) {
                *by_kind.entry(el["kind"].as_str().unwrap_or("").to_string()).or_default() += 1;
            }
            let total: usize = by_kind.values().sum();
            let rows: Vec<Value> = by_kind.iter().map(|(k, n)| json!({ "kind": k, "count": n })).collect();
            text_tail = format!(" {total} elements have no source id.");
            v["without_source_id"] = json!({ "total": total, "by_kind": by_kind, "kinds": rows });
        }
        out(format!("{} elements, {} edges.{text_tail} Read structuredContent.", v["elements"].as_array().map_or(0, Vec::len), v["edges"].as_array().map_or(0, Vec::len)), v)
}

fn coverage_owner(a: &Value) -> Result<Option<trace::CoverageOwner>, ToolErr> {
    match a.get("coverage_owner") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(trace::CoverageOwner::parse(Some(s))?),
        Some(_) => Err("coverage_owner must be 'requirement' or 'grouping'.".into()),
    }
}

fn coverage_report(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let owner = coverage_owner(a)?;
        let v = match owner {
            Some(owner) => trace::coverage_report_with_owner(&ctx.model(), opt(a, "scope"), owner)?,
            None => trace::coverage_report(&ctx.model(), opt(a, "scope"))?,
        };
        let t = &v["totals"];
        out(format!("{} requirements: {} satisfied, {} verified, {} fully covered ({}%).", t["requirements"], t["satisfied"], t["verified"], t["fully_covered"], t["percent_covered"]), v)
}

fn validate(ctx: &dyn Ctx, _a: &Value) -> ToolResult {
        let m = ctx.model();
        let all = api::diagnostics_out(&m);
        let errors = all.iter().filter(|d| d.severity == Severity::Error).count();
        let data = json!({ "errors": errors, "warnings": all.len() - errors, "diagnostics": all });
        out(diag_text(&m, None), data)
}

fn validate_model(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let owner = coverage_owner(a)?;
        let require = match a.get("require") {
            None | Some(Value::Null) => None,
            Some(Value::Array(v)) => Some(v.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| ToolErr::from("require must be a list of strings: satisfy, verify."))).collect::<Result<Vec<_>, _>>()?),
            Some(Value::String(s)) => Some(vec![s.clone()]),
            Some(_) => return Err("require must be a list of strings: satisfy, verify.".into()),
        };
        let sel = agent::rules::Selection { gates: None, rules: str_list(a, "rules")?, groups: str_list(a, "groups")?, require: require.as_deref() };
        let mut v = trace::validate_model_sel_with_owner(&ctx.model(), opt(a, "gate"), &sel, owner)?;
        mark_ignored(ctx, &mut v);
        let mut text = format!("Gate {}: {} ({} errors, {} warnings; traceability requires {})\n", model_name(v["gate"].as_str().unwrap_or("")), if v["passed"] == true { "passed" } else { "FAILED" }, v["errors"], v["warnings"], v["required"].as_array().map(|r| r.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" and ")).unwrap_or_default());
        for f in v["findings"].as_array().into_iter().flatten().take(50) {
            text.push_str(&format!("[{}] {} {}: {} Fix: {}\n", model_name(f["rule"].as_str().unwrap_or("")), model_name(f["severity"].as_str().unwrap_or("")), model_name(f["element"].as_str().unwrap_or("-")), model_name(f["message"].as_str().unwrap_or("")), model_name(f["fix"].as_str().unwrap_or(""))));
        }
        out(text, v)
}

fn list_rules(_ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let v = agent::rules::list_rules(opt(a, "gate"), opt(a, "group"))?;
        let text = v["rules"].as_array().into_iter().flatten().map(|r| format!("{} [{}, {}, {}] {}", model_name(r["id"].as_str().unwrap_or("")), model_name(r["gate"].as_str().unwrap_or("")), model_name(r["severity"].as_str().unwrap_or("")), model_name(r["group"].as_str().unwrap_or("")), model_name(r["meaning"].as_str().unwrap_or("")))).collect::<Vec<_>>().join("\n");
        out(text, v)
}

fn check_all(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let owner = coverage_owner(a)?;
        let gates = str_list(a, "gates")?;
        let require = str_list(a, "require")?;
        let options = json!({ "gates": gates, "require": require, "coverage_owner": a.get("coverage_owner") });
        let mut v = check_all_cached(&ctx.files(), &options, || agent::rules::check_all_with_owner(&ctx.model(), gates.as_deref(), require.as_deref(), owner))?;
        mark_ignored(ctx, &mut v);
        out(agent::rules::report_text(&v, 50), v)
}

fn find_unconnected_ports(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let v = agent::connect::find_unconnected_ports(&ctx.model(), arg(a, "scope")?)?;
        let rows: Vec<String> = v["unconnected"].as_array().into_iter().flatten().map(|p| format!("{} (on {})", model_name(p["port"].as_str().unwrap_or("")), model_name(p["part"].as_str().unwrap_or("")))).collect();
        out(format!("{} of {} ports unconnected{}{}", v["unconnected_count"], v["ports_checked"], if rows.is_empty() { "" } else { ":\n" }, rows.join("\n")), v)
}

fn check_connections(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let v = agent::connect::check_connections(&ctx.model(), arg(a, "scope")?)?;
        let verdict = match (v["errors"].as_u64().unwrap_or(0), v["warnings"].as_u64().unwrap_or(0)) {
            (0, 0) => "no problems".to_string(),
            (0, w) => format!("no errors, {w} warning(s)"),
            (e, w) => format!("problems found ({e} error(s), {w} warning(s))"),
        };
        let mut text = format!("{} connections checked: {verdict}\n", v["connections_checked"]);
        for f in v["findings"].as_array().into_iter().flatten() {
            text.push_str(&format!("[{}] {} {}: {} Fix: {}\n", model_name(f["rule"].as_str().unwrap_or("")), model_name(f["severity"].as_str().unwrap_or("")), model_name(f["element"].as_str().unwrap_or("")), model_name(f["message"].as_str().unwrap_or("")), model_name(f["fix"].as_str().unwrap_or(""))));
        }
        out(text, v)
}

fn export_sysml(ctx: &dyn Ctx, a: &Value) -> ToolResult {
        let v = agent::export_sysml(&ctx.model(), opt(a, "scope"))?;
        out(v["text"].as_str().unwrap_or("").to_string(), v)
}

fn resolve_source_id(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let sid = a.get("source_id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let rf = a.get("ref").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let map = ctx.ids();
    let m = ctx.model();
    match (sid, rf) {
        (Some(_), Some(_)) | (None, None) => Err(ToolErr::from("give exactly one of source_id (to get the element) or ref (to get its source ids).")),
        (Some(id), None) => {
            let qn = map.get(id).ok_or_else(|| ToolErr::from(format!("unknown source_id {}. Fix: pass an id given to create_element, create_relationship, add_interface, update_element, set_source_id or add_element, or one from the importer's .ids.json file.", crate::agent::text::quote_clip(id))))?;
            match m.find(qn) {
                Some(e) => {
                    let mut data = agent::brief(&m, e);
                    data["source_id"] = json!(id);
                    data["exists"] = json!(true);
                    data["source_ids"] = json!(agent::ids::ids_of(&map, &e.qualified_name));
                    Ok(ToolOut { text: format!("{id} -> {}", e.qualified_name), data })
                }
                None => Err(ToolErr {
                    message: format!("source_id '{id}' maps to '{qn}', which is not in the model. The element was removed or renamed outside the tools. Fix: find it with find_elements, then create it again with the same source_id."),
                    data: json!({ "source_id": id, "ref": qn, "exists": false }),
                }),
            }
        }
        (None, Some(r)) => {
            let e = agent::locate(&m, r)?;
            let found = agent::ids::ids_of(&map, &e.qualified_name);
            let text = if found.is_empty() { format!("{} has no source id.", e.qualified_name) } else { format!("{} <- {}", e.qualified_name, found.join(", ")) };
            let mut data = agent::brief(&m, e);
            data["source_ids"] = json!(found);
            Ok(ToolOut { text, data })
        }
    }
}

/// A workspace held in memory: the files, and a second set of files to compare with. The tests use it. It has no history,
/// baseline or review store, and it says so (`offline`).
pub struct MemoryCtx {
    pub files: Vec<FileIn>,
    /// The name and the files of the second set; `list_suspect_links` compares it with the first.
    pub other: Option<(String, Vec<FileIn>)>,
}

fn is_layout(p: &str) -> bool {
    p.ends_with(".diagrams.json")
}

impl MemoryCtx {
    fn json_of(&self, path: &str) -> Option<Value> {
        let f = self.files.iter().find(|f| f.path == path)?;
        serde_json::from_str(&f.text).ok()
    }
}

impl Ctx for MemoryCtx {
    fn model(&self) -> Arc<Model> {
        // The model of this revision, analysed once (`model_of`). With a setting or ids to pin, the pinned copy is kept too, so
        // the calls of one selection share one model (the ones a model keeps built, such as the Connections facts, with it).
        static PINNED: Mutex<Vec<(Arc<Model>, Option<String>, BTreeMap<String, String>, Arc<Model>)>> = Mutex::new(Vec::new());
        let base = model_of(&self.files());
        let root = self.json_of(".sysml-bridge/settings.json").and_then(|v| v["budget_root"].as_str().map(str::to_string));
        let ids = self.ids();
        if base.budget_root == root && base.source_ids == ids {
            return base;
        }
        if let Ok(c) = PINNED.lock() {
            if let Some(hit) = c.iter().find(|(b, r, i, _)| Arc::ptr_eq(b, &base) && *r == root && *i == ids) {
                return hit.3.clone();
            }
        }
        let mut m = (*base).clone();
        m.budget_root = root.clone();
        m.source_ids = ids.clone();
        let m = Arc::new(m);
        if let Ok(mut c) = PINNED.lock() {
            c.insert(0, (base, root, ids, m.clone()));
            c.truncate(2);
        }
        m
    }

    fn files(&self) -> Vec<FileIn> {
        self.files.iter().filter(|f| !is_layout(&f.path) && !f.path.starts_with('.') && (f.path.ends_with(".sysml") || f.path.ends_with(".kerml"))).cloned().collect()
    }

    fn all_files(&self) -> Vec<FileIn> {
        self.files.iter().filter(|f| is_layout(&f.path) || f.path == agent::rekey::RENAMES_FILE || f.path.ends_with(".sysml") || f.path.ends_with(".kerml")).cloned().collect()
    }

    fn ids(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let mut importer: Vec<&FileIn> = self.files.iter().filter(|f| f.path.ends_with(".ids.json") && !f.path.starts_with('.')).collect();
        importer.sort_by(|a, b| a.path.cmp(&b.path));
        for f in importer {
            if let Ok(v) = serde_json::from_str::<Value>(&f.text) {
                for (k, q) in v["ids"].as_object().into_iter().flatten() {
                    if let Some(q) = q.as_str() {
                        map.entry(k.clone()).or_insert_with(|| q.to_string());
                    }
                }
            }
        }
        if let Some(v) = self.json_of(".sysml-bridge/ids.json") {
            for (k, q) in v["ids"].as_object().into_iter().flatten() {
                match q.as_str() {
                    Some(q) => map.insert(k.clone(), q.to_string()),
                    None => map.remove(k),
                };
            }
        }
        map
    }

    fn read_file(&self, path: &str) -> Result<String, String> {
        self.all_files().into_iter().find(|f| f.path == path).map(|f| f.text).ok_or_else(|| format!("no file '{path}'"))
    }

    fn offline(&self) -> bool {
        true
    }

    fn suspect_pair(&self, a: &Value) -> Result<SuspectPair, ToolErr> {
        let since = opt(a, "since").unwrap_or("");
        match &self.other {
            Some((name, files)) if name == since => {
                let before: Vec<FileIn> = files.iter().filter(|f| !is_layout(&f.path) && (f.path.ends_with(".sysml") || f.path.ends_with(".kerml"))).cloned().collect();
                let rev = |fs: &[FileIn]| agent::project_revision(fs);
                let after = self.files();
                Ok(SuspectPair { bar_before: format!("file set {} ({})", quote_clip(name), rev(&before)), bar_after: format!("current ({})", rev(&after)), versions: json!({ "before": { "kind": "file set", "name": name, "revision": rev(&before) }, "after": { "kind": "current", "name": "current", "revision": rev(&after) } }), before, after })
            }
            Some((name, _)) => Err(err(format!("no file set {}. The second file set is {}; baselines and revisions of the write log are kept by the server.", quote_clip(since), quote_clip(name)))),
            None => Err(err(format!("no file set {}: this copy holds no second file set to compare with. Load one first (set_file_in).", quote_clip(since)))),
        }
    }
}

pub type Handler = fn(&dyn Ctx, &Value) -> ToolResult;

fn export_reqif(ctx: &dyn Ctx, a: &Value) -> ToolResult {
    let scope = arg(a, "scope")?;
    if a.get("columns").and_then(Value::as_array).is_some_and(|v| v.len() > 24) {
        return Err(err("export_reqif accepts at most 24 additional columns"));
    }
    let cols = str_list(a, "columns")?.unwrap_or_default();
    let v = agent::reqif::export(&ctx.model(), scope, &cols)?;
    out(format!("Exported {} requirements as one ReqIF specification.", v["objects"]), v)
}

/// The handler of every read tool that runs on a [`Ctx`]. One table for every caller.
pub static TABLE: &[(&str, Handler)] = &[
    ("list_files", list_files),
    ("read_file", read_file),
    ("get_outline", get_outline),
    ("find_elements", find_elements),
    ("get_element", get_element),
    ("get_project_state", get_project_state),
    ("query_elements", query_elements),
    ("query_relationships", query_relationships),
    ("get_traceability", get_traceability),
    ("resolve_source_id", resolve_source_id),
    ("export_reqif", export_reqif),
    ("export_facts", export_facts),
    ("coverage_report", coverage_report),
    ("validate", validate),
    ("validate_model", validate_model),
    ("list_rules", list_rules),
    ("check_all", check_all),
    ("find_unconnected_ports", find_unconnected_ports),
    ("check_connections", check_connections),
    ("export_sysml", export_sysml),
    ("list_traceability_properties", list_traceability_properties),
    ("get_connections", get_connections),
    ("get_icd", super::icd::tool_get_icd),
    ("get_budget", get_budget),
    ("get_impact", get_impact),
    ("list_suspect_links", tools_suspect::list_suspect_links),
    ("find", tools_suspect::find),
    ("get_usages", tools_suspect::get_usages),
    ("get_validation_results", tools_suspect::get_validation_results),
    ("get_relation_map", tools_gen::get_relation_map),
    ("get_coverage_history", tools_gen::get_coverage_history),
    ("get_next_id", tools_gen::get_next_id),
];

/// Read tools that are not in [`TABLE`], each with its reason. The tool list classes them as reads; they need a store that only the
/// server has (the write log, the baselines), or they save a record.
pub const NOT_SHARED: &[(&str, &str)] = &[
    ("get_history", "reads the write log of the server"),
    ("compare_revisions", "compares two states of the write log"),
    ("list_baselines", "reads the baselines the server recorded"),
    ("get_change_package", "builds a package from the write log and the baselines"),
    ("check_merge", "compares states of the write log and the baselines with their base"),
    ("plan_renumber", "plans edits in the server (the tool list classes it without a model write)"),
    ("plan_from_csv", "can save a column map in the store of the server"),
    ("plan_from_reqif", "reads a ReqIF file of the workspace"),
];

/// The names of the tools this copy answers, in the order of the table.
pub fn offline_names() -> Vec<&'static str> {
    TABLE.iter().map(|(n, _)| *n).collect()
}

pub fn handler(name: &str) -> Option<Handler> {
    TABLE.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// Run a read tool on a [`Ctx`]. `None` when the name is not in the table. The caller has already checked the arguments against the
/// tool list and resolved `source_id:` refs.
pub fn dispatch(ctx: &dyn Ctx, name: &str, a: &Value) -> Option<ToolResult> {
    handler(name).map(|h| h(ctx, a))
}

/// A read call with `source_id:<id>` refs: the same call with the qualified names the ids name now.
pub fn with_id_refs(ids: &BTreeMap<String, String>, name: &str, a: &Value) -> Result<Option<Value>, ToolErr> {
    if name == "resolve_source_id" || a.as_object().is_none() {
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
    let (resolved, _) = agent::idrefs::resolve_ids(&probe, ids)?;
    Ok(Some(resolved))
}

/// `tools/call` for an in-memory copy: the exact JSON the HTTP path returns (`content`, `structuredContent`, `isError`). A tool that
/// changes anything is answered with [`READ_ONLY_COPY`]; a read tool that needs a store this copy does not have says so.
pub fn call_offline(ctx: &dyn Ctx, name: &str, args: &Value) -> Value {
    let is_write = super::tool_schema::writes_of(name).map(|w| w != "none");
    let result = match is_write {
        None => Err(ToolErr::from(format!("unknown tool {}. Call tools/list for the tool names.", quote_clip(name)))),
        Some(true) => Err(ToolErr::from(READ_ONLY_COPY)),
        Some(false) => match handler(name) {
            None => Err(ToolErr::from(format!("{} needs the history or the baselines of the server; this copy does not hold them.", quote_clip(name)))),
            Some(h) => super::tool_schema::check_args(name, args).map_err(ToolErr::from).and_then(|_| {
                let resolved = with_id_refs(&ctx.ids(), name, args)?;
                h(ctx, resolved.as_ref().unwrap_or(args))
            }),
        },
    };
    envelope(result, false)
}
