//! Preview of a write: which files change, the diff, and the diagnostics
//! the change would leave behind. Nothing here touches storage, and nothing
//! here refuses: a write always lands, and what it leaves is reported (any
//! parse fault first, so a file the parser could not read whole is never
//! taken for a smaller model).

use super::diff::{self, Hunk};
use super::{project_revision, revision_of};
use crate::api::{self, DiagOut, FileIn};
use crate::model::Severity;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Most files one write may propose; the work below is linear in this.
pub const MAX_PROPOSED_FILES: usize = 1000;
/// Diagnostics kept in a reply per list; the counts stay exact.
const MAX_LISTED: usize = 200;

/// A file after the write. `text: None` deletes the file.
#[derive(Debug, Clone)]
pub struct Proposed {
    pub path: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDelta {
    pub path: String,
    /// `created`, `modified` or `deleted`.
    pub change: &'static str,
    pub revision_before: Option<String>,
    pub revision_after: Option<String>,
    pub diff: String,
    #[serde(skip)]
    pub before: Option<String>,
    #[serde(skip)]
    pub after: Option<String>,
    #[serde(skip)]
    pub hunks: Vec<Hunk>,
}

#[derive(Debug)]
pub struct Plan {
    pub deltas: Vec<FileDelta>,
    pub revision_before: String,
    pub revision_after: String,
    pub errors_before: usize,
    pub errors_after: usize,
    pub warnings_after: usize,
    /// Errors in the result that the old state did not have.
    pub new_errors: Vec<DiagOut>,
    /// Errors in files this write touches.
    pub errors_in_touched: usize,
    /// Diagnostics of the touched files, capped. New ones come first, so a large file cannot hide them.
    pub touched_diagnostics: Vec<DiagOut>,
    /// Warnings in the result that the old state did not have, capped.
    pub new_warnings: Vec<DiagOut>,
    /// Faults of the parser in the result (text it could not read as SysML v2), those in the touched files first, capped.
    /// A reply lists them before anything else: the model of a file with a parse fault may hold fewer elements than its
    /// text, so the other findings can mislead until it is fixed.
    pub parse_faults: Vec<DiagOut>,
}

impl Plan {
    pub fn changed(&self) -> bool {
        !self.deltas.is_empty()
    }

    pub fn diff(&self) -> String {
        self.deltas.iter().map(|d| d.diff.as_str()).collect::<Vec<_>>().join("")
    }
}

/// Parser messages can quote a whole token of the file; keep replies short.
fn short(mut d: DiagOut) -> DiagOut {
    if d.message.chars().count() > 200 {
        d.message = format!("{}...", d.message.chars().take(200).collect::<String>());
    }
    d
}

/// The diagnostics of a file set, the error and warning counts, and how many of the diagnostics (they come first) are the
/// parser's.
fn errors(files: &[FileIn]) -> (Vec<DiagOut>, usize, usize, usize) {
    let mut m = super::intent::model_of(files).as_ref().clone();
    super::add_namespace_checks(&mut m);
    let mut all = api::diagnostics_out(&m);
    let parsed = m.parsed_diagnostics.min(all.len());
    // The warnings of validate_model's semantic gate (for example trace-operand-definition) count here too, so a
    // write reply and a later validate_model agree. Only warnings: an error rule keeps its own refusal path.
    let seen: HashSet<String> = all.iter().map(|d| d.message.clone()).collect();
    for f in super::trace::semantic_findings(&m).into_iter().filter(|f| f["severity"] == "warning" && !seen.contains(f["message"].as_str().unwrap_or(""))) {
        let (file, line) = (f["file"].as_str().unwrap_or("").to_string(), f["line"].as_u64().unwrap_or(1) as usize);
        let span = f["element"].as_str().and_then(|q| m.find(q)).map(|e| (e.span.start, e.span.end)).unwrap_or((0, 0));
        all.push(DiagOut { file, start: span.0, end: span.1, line, col: 1, severity: Severity::Warning, message: format!("{}: {}", f["rule"].as_str().unwrap_or(""), f["message"].as_str().unwrap_or("")) });
    }
    let e = all.iter().filter(|d| d.severity == Severity::Error).count();
    let w = all.len() - e;
    (all, e, w, parsed)
}

pub fn plan(before: &[FileIn], proposed: Vec<Proposed>) -> Plan {
    let old: BTreeMap<&str, &str> = before.iter().map(|f| (f.path.as_str(), f.text.as_str())).collect();
    // One pass over the files: the proposed paths replace or remove theirs, so the cost does not grow with files x proposals.
    let changed_paths: HashSet<&str> = proposed.iter().map(|p| p.path.as_str()).collect();
    let mut after_files: Vec<FileIn> = before.iter().filter(|f| !changed_paths.contains(f.path.as_str())).cloned().collect();
    let mut deltas = Vec::new();
    for p in proposed {
        let was = old.get(p.path.as_str()).map(|s| s.to_string());
        if was == p.text {
            if let Some(t) = &p.text {
                after_files.push(FileIn { path: p.path.clone(), text: t.clone() });
            }
            continue;
        }
        let change = match (&was, &p.text) {
            (None, _) => "created",
            (_, None) => "deleted",
            _ => "modified",
        };
        let hunks = diff::hunks(was.as_deref().unwrap_or(""), p.text.as_deref().unwrap_or(""));
        let text = diff::unified(&p.path, was.as_deref(), p.text.as_deref(), &hunks);
        if let Some(t) = &p.text {
            after_files.push(FileIn { path: p.path.clone(), text: t.clone() });
        }
        deltas.push(FileDelta {
            revision_before: was.as_deref().map(revision_of),
            revision_after: p.text.as_deref().map(revision_of),
            path: p.path,
            change,
            diff: text,
            before: was,
            after: p.text,
            hunks,
        });
    }
    let (diag_before, errors_before, _, _) = errors(before);
    let (diag_after, errors_after, warnings_after, parsed_after) = errors(&after_files);
    // Match by message and file so a moved diagnostic is not counted as new.
    let new_of = |severity: Severity| {
        let mut known: BTreeMap<(String, String), usize> = BTreeMap::new();
        for d in diag_before.iter().filter(|d| d.severity == severity) {
            *known.entry((d.file.clone(), d.message.clone())).or_default() += 1;
        }
        let mut fresh = Vec::new();
        for d in diag_after.iter().filter(|d| d.severity == severity) {
            match known.get_mut(&(d.file.clone(), d.message.clone())) {
                Some(n) if *n > 0 => *n -= 1,
                _ => fresh.push(short(d.clone())),
            }
        }
        fresh
    };
    let touched: BTreeSet<&str> = deltas.iter().map(|d| d.path.as_str()).collect();
    // The parser's faults, those in the files this write touches first.
    let mut parse_faults: Vec<DiagOut> = diag_after[..parsed_after].iter().filter(|d| d.severity == Severity::Error).cloned().map(short).collect();
    parse_faults.sort_by_key(|d| !touched.contains(d.file.as_str()));
    parse_faults.truncate(50);
    let faulty: HashSet<(String, usize, String)> = parse_faults.iter().map(|d| (d.file.clone(), d.line, d.message.clone())).collect();
    let is_fault = |d: &DiagOut| faulty.contains(&(d.file.clone(), d.line, d.message.clone()));
    let mut new_errors: Vec<DiagOut> = new_of(Severity::Error);
    new_errors.sort_by_key(|d| !is_fault(d));
    new_errors.truncate(MAX_LISTED);
    let new_warnings: Vec<DiagOut> = new_of(Severity::Warning).into_iter().take(50).collect();
    let fresh: HashSet<(&str, usize, &str)> = new_errors.iter().chain(new_warnings.iter()).map(|n| (n.file.as_str(), n.line, n.message.as_str())).collect();
    let is_new = |d: &DiagOut| fresh.contains(&(d.file.as_str(), d.line, short(d.clone()).message.as_str()));
    let mut touched_diagnostics: Vec<DiagOut> = diag_after.iter().filter(|d| touched.contains(d.file.as_str())).cloned().map(short).collect();
    touched_diagnostics.sort_by_cached_key(|d| (!is_fault(d), !is_new(d)));
    touched_diagnostics.truncate(50);
    let errors_in_touched = diag_after.iter().filter(|d| d.severity == Severity::Error && touched.contains(d.file.as_str())).count();
    Plan {
        revision_before: project_revision(before),
        revision_after: project_revision(&after_files),
        deltas,
        errors_before,
        errors_after,
        warnings_after,
        new_errors,
        errors_in_touched,
        touched_diagnostics,
        new_warnings,
        parse_faults,
    }
}
