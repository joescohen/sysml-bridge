//! Revisions a call names, and the three-way check of two revisions against their base.
//!
//! A selector is `{baseline: NAME}`, `{revision: "<16 hex>" | "#N" | "current"}`, or nothing for the current model. A
//! bare string is read as `current`, a baseline name or a revision.

use super::{model_files, out, writes, ToolErr, ToolResult};
use crate::App;
use serde_json::{json, Value};
use sysml_core::agent::project_revision;
use sysml_core::agent::text::{model_name, quote_clip, stored_token};
use sysml_core::api::FileIn;

/// Longest text of a selector.
const MAX_SELECTOR: usize = 200;
pub use sysml_core::agent::tools::model_of;

/// One revision a call reads: its files, and what the version bar says about it.
pub struct Rev {
    pub files: Vec<FileIn>,
    pub bar: Value,
}

fn err(m: impl Into<String>) -> ToolErr {
    writes::err(m)
}

fn names_of_baselines(app: &App) -> Vec<String> {
    app.store.read_baselines().iter().filter_map(|b| b["name"].as_str().map(str::to_string)).collect()
}

fn unknown(app: &App, what: &str, given: &str) -> ToolErr {
    let b = names_of_baselines(app);
    err(format!(
        "no {what} {}. Baselines: {}. Also accepted: current, a project revision (16 hex digits) or #N (the state after write N of get_history).",
        quote_clip(given),
        if b.is_empty() { "none".to_string() } else { b.iter().map(|n| quote_clip(n)).collect::<Vec<_>>().join(", ") },
    ))
}

/// The selector of a call, as `(kind, value)`: `kind` is `baseline`, `revision` or `current`.
fn parse(app: &App, arg: &str, v: &Value) -> Result<(&'static str, String), ToolErr> {
    let text = |k: &str, x: &Value| -> Result<String, ToolErr> {
        let s = x.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| err(format!("argument '{arg}': '{k}' must be a non-empty string.")))?;
        if s.chars().count() > MAX_SELECTOR || s.chars().any(char::is_control) {
            return Err(err(format!("argument '{arg}': '{k}' is at most {MAX_SELECTOR} characters with no control character.")));
        }
        Ok(s.to_string())
    };
    match v {
        Value::Null => Ok(("current", "current".into())),
        Value::String(_) => {
            let s = text("value", v)?;
            if s.eq_ignore_ascii_case("current") {
                return Ok(("current", s));
            }
            if names_of_baselines(app).contains(&s) {
                return Ok(("baseline", s));
            }
            if s.starts_with('#') || (s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit())) {
                return Ok(("revision", s));
            }
            Err(unknown(app, "baseline or revision", &s))
        }
        Value::Object(o) => {
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "baseline" | "revision")) {
                return Err(err(format!("argument '{arg}' has an unknown key {}. A selector is {{baseline: NAME}} or {{revision: \"<16 hex digits>\" | \"#N\" | \"current\"}}.", quote_clip(k))));
            }
            let named: Vec<&str> = ["baseline", "revision"].into_iter().filter(|k| o.contains_key(*k)).collect();
            if named.len() != 1 {
                return Err(err(format!("argument '{arg}' names {} revisions; it names exactly one of baseline or revision.", named.len())));
            }
            let k = named[0];
            let value = text(k, &o[k])?;
            match k {
                "baseline" => {
                    if !names_of_baselines(app).contains(&value) {
                        return Err(unknown(app, "baseline", &value));
                    }
                    Ok(("baseline", value))
                }
                _ if value.eq_ignore_ascii_case("current") => Ok(("current", value)),
                _ => Ok(("revision", value)),
            }
        }
        _ => Err(err(format!("argument '{arg}' is an object ({{baseline: NAME}}, {{revision: ...}}) or a string."))),
    }
}

fn bar_of(info: &Value, kind: &str, name: &str) -> Value {
    let mut b = json!({ "kind": kind, "name": name, "revision": info["revision"], "is": info["is"] });
    for k in ["time", "note", "author"] {
        if info[k].is_string() {
            b[k] = info[k].clone();
        }
    }
    b
}

/// The revision a selector names.
fn resolve(app: &App, arg: &str, v: &Value) -> Result<Rev, ToolErr> {
    let (kind, value) = parse(app, arg, v)?;
    let current = model_files(app);
    if kind == "current" {
        let now = project_revision(&current);
        return Ok(Rev { bar: json!({ "kind": "current", "name": "current", "revision": now, "is": "current" }), files: current });
    }
    let history = app.store.read_history();
    let (files, info) = writes::state_named(app, &current, &history, &value)?;
    let bar = if kind == "baseline" { bar_of(&info, "baseline", &value) } else { bar_of(&info, "revision", info["revision"].as_str().unwrap_or("")) };
    Ok(Rev { files, bar })
}

/// The two revisions of a comparison: `before` and `after` (each the current model when left out).
pub(super) fn pair(app: &App, a: &Value) -> Result<(Rev, Rev), ToolErr> {
    let _guard = app.store.lock_writes();
    let before = resolve(app, "before", a.get("before").unwrap_or(&Value::Null))?;
    let after = resolve(app, "after", a.get("after").unwrap_or(&Value::Null))?;
    Ok((before, after))
}

pub(super) fn bar_line(b: &Value) -> String {
    match b["kind"].as_str() {
        Some("baseline") => format!("baseline {} ({})", quote_clip(b["name"].as_str().unwrap_or("")), stored_token(&b["revision"])),
        Some("current") => format!("current ({})", stored_token(&b["revision"])),
        _ => format!("revision {}", stored_token(&b["revision"])),
    }
}

pub(super) fn versions(before: &Rev, after: &Rev) -> Value {
    json!({ "before": before.bar, "after": after.bar })
}

/// The three-way comparison of two revisions against their common base (`sysml_core::agent::merge3`).
pub fn check_merge(app: &App, a: &Value) -> ToolResult {
    let one = |k: &str| a.get(k).filter(|v| !v.is_null());
    let need = |k: &str| one(k).ok_or_else(|| err(format!("missing argument '{k}': a baseline, a revision (16 hex digits or #N) or current.")));
    let (vb, vo, vt) = (need("base")?, need("ours")?, need("theirs")?);
    let (b, o, t) = {
        let _guard = app.store.lock_writes();
        (resolve(app, "base", vb)?, resolve(app, "ours", vo)?, resolve(app, "theirs", vt)?)
    };
    let bars = json!({ "base": b.bar, "ours": o.bar, "theirs": t.bar });
    let (bm, om, tm) = (model_of(&b.files), model_of(&o.files), model_of(&t.files));
    let mut data = sysml_core::agent::merge3::merge_check(&bm, &om, &tm);
    data["versions"] = bars;
    let side = |k: &str| format!("{k}: {} added, {} changed, {} removed", data[k]["added"], data[k]["changed"], data[k]["removed"]);
    let mut text = format!(
        "Three-way check against the base. {}; {}. {} conflict(s), {} equivalent change(s).",
        side("ours"),
        side("theirs"),
        data["conflict_count"],
        data["equivalent_count"]
    );
    for c in data["conflicts"].as_array().into_iter().flatten().take(20) {
        let what = c["element"].as_str().map(model_name).unwrap_or_else(|| c["elements"].as_array().into_iter().flatten().filter_map(Value::as_str).map(model_name).collect::<Vec<_>>().join(" -> "));
        let field = c["field"].as_str().map(|f| format!(" ({})", model_name(f))).unwrap_or_default();
        text.push_str(&format!("\n- {}: {what}{field}: {}", stored_token(&c["kind"]), model_name(c["why"].as_str().unwrap_or(""))));
    }
    if data["conflict_count"].as_u64().unwrap_or(0) > 20 {
        text.push_str("\n- ... the rest are in conflicts[].");
    }
    out(text, data)
}
