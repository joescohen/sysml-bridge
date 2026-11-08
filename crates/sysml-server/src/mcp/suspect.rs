//! Suspect links. The derivation is `sysml_core::agent::suspect`; the read side (the listing, the rule results a person
//! set aside, the validation results, usages and find) is `sysml_core::agent::tools_suspect`. This file reads the review
//! store and keeps the two acts of a person.
//!
//! `list_suspect_links` is an agent tool and only reads. Clearing a link is a person's act, like any review: it has
//! no agent tool. It comes from the `clear-suspect` command at a terminal, and it is written to `suspect-reviews.jsonl` beside the write log, never to the
//! model. The text a person typed (name, note) is shown quoted; the values read back from the file go through `stored_token`.

use super::revs::model_of;
use super::{out, writes, ToolErr, ToolResult};
use crate::App;
use serde_json::{json, Value};
use sysml_core::agent::suspect::{self, Reviews};
use sysml_core::agent::text::{quote_claim, quote_clip, stored_token};
pub use sysml_core::agent::tools_suspect::{list_suspect_links, result_fingerprint};
use sysml_core::agent::tools_suspect::{listing, opts_of, stable_id_of};

/// How a review reached the server: over HTTP (`route` "http") or from a command at a terminal (`route` "command").
/// `authenticated` is true only when the reviewer token was set on the server and the request carried it.
pub struct Reviewer {
    pub route: &'static str,
    pub authenticated: bool,
}

impl Reviewer {

    fn way(route: &str) -> Option<&'static str> {
        match route {
            "http" => Some("HTTP route"),
            "command" => Some("command"),
            _ => None,
        }
    }

    pub fn says(&self) -> String {
        let who = if self.authenticated { "reviewer authenticated" } else { "reviewer not authenticated" };
        match Self::way(self.route) {
            Some(w) => format!("through the {w}, {who}"),
            None => who.to_string(),
        }
    }
}

pub(super) fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// A name or a short text a person gave: 1 to `max` characters that show.
pub(super) fn said<'a>(a: &'a Value, k: &str, what: &str, max: usize) -> Result<&'a str, ToolErr> {
    let v = a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| super::writes::err(format!("missing argument '{k}': {what}.")))?;
    if v.chars().count() > max || v.chars().any(char::is_control) || sysml_core::edit::name_fault(v).is_some() {
        return Err(super::writes::err(format!("argument '{k}' must be 1 to {max} characters that show, with no control character: {what}.")));
    }
    Ok(v)
}

/// A text of a person that may run over several lines: 1 to `max` characters, line breaks and tabs kept.
pub(super) fn said_text(a: &Value, k: &str, what: &str, max: usize) -> Result<String, ToolErr> {
    let v = a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| super::writes::err(format!("missing argument '{k}': {what}.")))?;
    let v = v.replace("\r\n", "\n");
    if v.chars().count() > max {
        return Err(super::writes::err(format!("argument '{k}' must be 1 to {max} characters (it has {}): {what}.", v.chars().count())));
    }
    sysml_core::edit::check_doc_text(&format!("argument '{k}'"), &v).map_err(super::writes::err)?;
    Ok(v)
}

fn err(m: impl Into<String>) -> ToolErr {
    writes::err(m)
}

/// The review store, by link key.
pub fn reviews(app: &App) -> Reviews {
    let mut out: Reviews = Reviews::new();
    for l in app.store.read_suspect_reviews() {
        if l["event"] != "cleared" {
            continue;
        }
        if let Some(k) = l["key"].as_str() {
            out.entry(k.to_string()).or_default().push(l);
        }
    }
    out
}

/// A person clears a suspect link: records, for the fingerprint the tracked end has now, who cleared it, when, and why.
/// `key` is the key `list_suspect_links` gave. Not an agent tool.
pub fn clear(app: &App, a: &Value, rv: &Reviewer) -> ToolResult {
    // The listing reads the store under its own lock, so it is taken before this write's lock.
    let shown_early = listing(app, a)?.1;
    let _guard = app.store.lock_writes();
    let key = a.get("link").or_else(|| a.get("key")).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| err("missing argument 'link': the key of the link, as list_suspect_links gives it."))?;
    if key.chars().count() > 600 || key.chars().any(char::is_control) {
        return Err(err("argument 'link' is a key of at most 600 characters with no control character."));
    }
    let by = said(a, "by", "who clears the link: the name of a person", 80)?;
    let note = if a.get("note").is_some_and(|v| !v.is_null()) { said_text(a, "note", "why the link is still sound", 2000)? } else { String::new() };
    let opts = opts_of(a)?;
    let files = super::model_files(app);
    let m = model_of(&files);
    // The fingerprint of the tracked end now: what the person says they have looked at.
    // The listing the person read: the link must be in it for this `since`, and the clear carries both ends of its change.
    let shown = shown_early.clone();
    let row = shown["links"].as_array().into_iter().flatten().find(|l| l["key"].as_str() == Some(key)).cloned().ok_or_else(|| err(format!("the link {} is not a suspect link against that 'since' (list_suspect_links with the same 'since' gives the links and their fingerprints). Nothing was cleared.", quote_clip(key))))?;
    let seen_before = a.get("fingerprint_before").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if seen_before.is_empty() {
        return Err(err("missing argument 'fingerprint_before': the fingerprint.before that list_suspect_links showed for this link, with the same 'since'. A clear holds for the change from that 'since' to the end as it is now, and for no other."));
    }
    if row["fingerprint"]["before"].as_str() != Some(seen_before) {
        return Err(err(format!("the 'since' side of this link is not the one you cleared: you pass {}, this 'since' gives {}. Nothing was cleared. List the link again with the 'since' you mean.", quote_clip(seen_before), stored_token(&row["fingerprint"]["before"]))));
    }
    let stamp = suspect::review_stamp(&m, key, &opts).ok_or_else(|| err(format!("no link with the key {} in the current model (list_suspect_links gives the keys).", quote_clip(key))))?;
    let now = now_ms();
    let fp = stamp["fingerprint"].clone();
    // The person clears what the listing showed: the request names the fingerprint of the listing (`fingerprint.after`), and it
    // must be the one the end has now.
    let seen = a.get("fingerprint").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if seen.is_empty() {
        return Err(err(format!("missing argument 'fingerprint': the fingerprint.after that list_suspect_links showed for this link. The end has fingerprint {} now; list the link, read what changed, and pass the fingerprint you read.", stored_token(&fp))));
    }
    if fp.as_str() != Some(seen) {
        return Err(err(format!("the end changed since it was listed: the fingerprint you pass is {}, the end has {} now. Nothing was cleared. List the link again, read the change, and clear with the new fingerprint.", quote_clip(seen), stored_token(&fp))));
    }
    let entry = json!({
        "event": "cleared",
        "key": key,
        "track": stamp["track"],
        "fields": stamp["fields"],
        "form": stamp["form"],
        "fingerprint": fp,
        "before_fingerprint": seen_before,
        "since": a["since"],
        "seen_fingerprint": seen,
        "field_hashes": stamp["field_hashes"],
        "by": by,
        "note": note,
        "time": writes::rfc3339(now),
        "route": rv.route,
        "authenticated": rv.authenticated,
        "revision": sysml_core::agent::project_revision(&files),
    });
    app.store.append_suspect_review(&entry).map_err(err)?;
    let text = format!("Link {} cleared for fingerprint {}: {} ({}) at {}{}. It is suspect again when its tracked end changes. Nothing was written to the model.", quote_clip(key), stored_token(&fp), quote_claim(by), rv.says(), stored_token(&entry["time"]), if note.is_empty() { String::new() } else { format!(", note {}", quote_clip(&note)) });
    out(text, json!({ "cleared": true, "review": entry }))
}

// ---- rule results a person sets aside, and the matrix ----

/// A person sets one rule result aside: `{fingerprint, by, note?}`. The fingerprint is the one the listing showed for the result;
/// it must be the fingerprint of a result the model gives now, else the result changed and the check is read again. Not an agent tool.
pub fn ignore_result(app: &App, a: &Value, rv: &Reviewer) -> ToolResult {
    let _guard = app.store.lock_writes();
    let seen = a.get("fingerprint").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if seen.is_empty() {
        return Err(err("missing argument 'fingerprint': the fingerprint that the rule listing (check_all, validate_model) showed for the result."));
    }
    let by = said(a, "by", "who sets the result aside: the name of a person", 80)?;
    let note = if a.get("note").is_some_and(|v| !v.is_null()) { said_text(a, "note", "why the result is set aside", 2000)? } else { String::new() };
    let files = super::model_files(app);
    let m = model_of(&files);
    let all = sysml_core::agent::rules::check_all(&m, None, None).map_err(|e| err(e.message))?;
    let found = all["findings"].as_array().into_iter().flatten().find(|f| result_fingerprint(f) == seen).cloned();
    let Some(f) = found else {
        return Err(err(format!("the result {} is not one the rules give now: the model changed since it was listed. Nothing was set aside. Run the check again and use the fingerprint it shows.", quote_clip(seen))));
    };
    let entry = json!({
        "event": "ignored", "fingerprint": seen, "form": "sysml-bridge-ignore-v1",
        "rule": f["rule"], "gate": f["gate"], "element": f["element"],
        "element_id": stable_id_of(&super::ids::effective(app), f["element"].as_str().unwrap_or("")),
        "by": by, "note": note, "time": writes::rfc3339(now_ms()), "route": rv.route, "authenticated": rv.authenticated,
        "revision": sysml_core::agent::project_revision(&files),
    });
    app.store.append_rule_ignore(&entry).map_err(err)?;
    out(format!("Result {} set aside: {} ({}) at {}. It is listed as ignored until the model gives a different result, or until it has been gone and comes back. Nothing was written to the model.", stored_token(&json!(seen)), quote_claim(by), rv.says(), stored_token(&entry["time"])), json!({ "ignored": true, "record": entry }))
}
