//! Shared by the test files: the official-grammar recogniser, and a check on every file the write tools save.
//!
//! `mod common;` in a test file is enough. The registration runs before the test binary starts (an init-array entry), so no
//! test has to call it. A write that saves text the official grammar rejects fails the test that made it, with the op and the
//! fault. A raw edit (`add_element`, `replace`, ...) is exempt, because the caller writes the text; so is a file that was
//! already invalid before the write, because the tool did not make that fault.

#![allow(dead_code)]

use serde_json::Value;
use std::sync::OnceLock;
use sysml_conform::{Checker, Limits, Verdict};
use sysml_core::agent::intent::{self, RAW_OPS};
use sysml_core::agent::rules::{self, OfficialVerdict};
use sysml_core::api::FileIn;

static CHECKER: OnceLock<Checker> = OnceLock::new();

/// The verdict of the official grammar on one text.
pub fn verdict(text: &str) -> Verdict {
    CHECKER.get_or_init(Checker::new).check_str(text, &Limits::default()).verdict
}

/// The registered recogniser: never more than the checker's own limit for a text of this size.
pub fn official(text: &str) -> (OfficialVerdict, u64) {
    let report = CHECKER.get_or_init(Checker::new).check_str(text, &Limits::default());
    let verdict = match report.verdict {
        Verdict::Valid => OfficialVerdict::Valid,
        Verdict::NotChecked(w) => OfficialVerdict::NotChecked(w),
        Verdict::Invalid(f) => OfficialVerdict::Invalid { line: f.line, column: f.column, found: f.found, found_kind: f.found_kind, allowed: f.allowed },
    };
    (verdict, report.steps)
}

/// `None` when the text is valid, else the fault in one line.
pub fn fault(text: &str) -> Option<String> {
    match verdict(text) {
        Verdict::Valid => None,
        Verdict::Invalid(f) => Some(format!("{}:{} found {} '{}', allowed {:?}", f.line, f.column, f.found_kind, f.found, f.allowed.iter().take(12).collect::<Vec<_>>())),
        Verdict::NotChecked(w) => Some(format!("not checked: {w}")),
    }
}

/// Panics when any file in `files` is invalid under the official grammar.
pub fn assert_official(files: &[FileIn], what: &str) {
    for f in files {
        if let Some(why) = fault(&f.text) {
            panic!("{what}: {} is not valid under the official grammar: {why}\n{}", f.path, f.text);
        }
    }
}

pub static OBSERVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn observe(ops: &[&Value], before: &[FileIn], after: &[FileIn]) {
    OBSERVED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if ops.iter().any(|o| o.get("op").and_then(Value::as_str).map(|n| RAW_OPS.contains(&n)).unwrap_or(false)) {
        return;
    }
    for f in after {
        let old = before.iter().find(|b| b.path == f.path);
        if old.map(|b| b.text == f.text).unwrap_or(false) || old.map(|b| fault(&b.text).is_some()).unwrap_or(false) {
            continue;
        }
        if let Some(why) = fault(&f.text) {
            let shown: Vec<String> = ops.iter().take(3).map(|o| o.to_string()).collect();
            panic!("a write tool saved text the official grammar rejects: {} {why}\nops: {}\n{}", f.path, shown.join(" | "), f.text);
        }
    }
}

pub fn init() {
    rules::set_official_checker(official);
    intent::set_write_observer(observe);
}

extern "C" fn init_at_start() {
    init();
}

#[used]
#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
static INIT: extern "C" fn() = init_at_start;
