//! The official-grammar gate's recogniser: `sysml-conform`, registered with the core. The core does not depend on it, so the
//! wasm build stays free of it.

use std::sync::{Arc, Mutex, OnceLock};
use sysml_conform::{Checker, Limits, Verdict};
use sysml_core::agent::rules::{set_official_checker, OfficialVerdict};

static CHECKER: OnceLock<Checker> = OnceLock::new();

/// The last answers, newest first, with the text each is for. A write asks about the text it found (which the write before
/// it left) and the text it makes; a gate run asks about every file again. The whole text is kept and compared, not a hash
/// of it, so no other text can take an answer. An answer is kept only when it is "valid" or "invalid".
static RECENT: Mutex<Vec<(Arc<str>, OfficialVerdict, u64)>> = Mutex::new(Vec::new());
const RECENT_SLOTS: usize = 8;
const RECENT_BYTES: usize = 32 << 20;

/// The verdict of the official grammar on one file's text and the steps it took. The checker holds the text to its own
/// limit, which is in proportion to the size of the text; there is no budget for a run.
pub fn check(text: &str) -> (OfficialVerdict, u64) {
    if let Ok(recent) = RECENT.lock() {
        if let Some((_, v, steps)) = recent.iter().find(|(t, _, _)| t.len() == text.len() && **t == *text) {
            // The steps were taken once; an answer from memory is charged a step for each 64 bytes compared.
            return (v.clone(), (*steps).min(1 + text.len() as u64 / 64));
        }
    }
    let checker = CHECKER.get_or_init(Checker::new);
    let report = checker.check_str(text, &Limits::default());
    let verdict = match report.verdict {
        Verdict::Valid => OfficialVerdict::Valid,
        Verdict::NotChecked(why) => OfficialVerdict::NotChecked(why),
        Verdict::Invalid(f) => OfficialVerdict::Invalid { line: f.line, column: f.column, found: f.found, found_kind: f.found_kind, allowed: f.allowed },
    };
    if !matches!(verdict, OfficialVerdict::NotChecked(_)) && text.len() <= RECENT_BYTES / 2 {
        if let Ok(mut recent) = RECENT.lock() {
            recent.insert(0, (Arc::from(text), verdict.clone(), report.steps));
            recent.truncate(RECENT_SLOTS);
            let mut bytes = 0usize;
            recent.retain(|(t, _, _)| {
                bytes += t.len();
                bytes <= RECENT_BYTES
            });
        }
    }
    (verdict, report.steps)
}

/// Make the gate available. Safe to call again.
pub fn register() {
    set_official_checker(check);
    #[cfg(test)]
    sysml_core::agent::intent::set_write_observer(observe_writes);
}

/// Tests only: every file a write tool saves must be valid under the official grammar. Raw edits (the caller writes the text) and
/// files that were already invalid before the write are exempt.
#[cfg(test)]
pub fn observe_writes(ops: &[&serde_json::Value], before: &[sysml_core::api::FileIn], after: &[sysml_core::api::FileIn]) {
    use sysml_core::agent::intent::RAW_OPS;
    if ops.iter().any(|o| o.get("op").and_then(|v| v.as_str()).map(|n| RAW_OPS.contains(&n)).unwrap_or(false)) {
        return;
    }
    for f in after {
        let old = before.iter().find(|b| b.path == f.path);
        if old.map(|b| b.text == f.text).unwrap_or(false) || old.map(|b| !matches!(check(&b.text).0, OfficialVerdict::Valid)).unwrap_or(false) {
            continue;
        }
        if let OfficialVerdict::Invalid { line, column, found, allowed, .. } = check(&f.text).0 {
            let shown: Vec<String> = ops.iter().take(3).map(|o| o.to_string()).collect();
            panic!("a write tool saved text the official grammar rejects: {}:{line}:{column} found '{found}', allowed {allowed:?}\nops: {}\n{}", f.path, shown.join(" | "), f.text);
        }
    }
}
