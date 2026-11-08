//! JSON-facing API: files in, analysis and edits out.

use crate::edit::{self, EditOp, FileChange};
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileIn {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefOut {
    pub role: RefRole,
    pub text: String,
    pub target: Option<String>,
    /// A conjugated port type (`~Port`): written only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conjugated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementOut {
    pub id: String,
    pub kind: String,
    pub keyword: String,
    pub is_def: bool,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub owner: Option<String>,
    pub children: Vec<String>,
    pub file: String,
    /// UTF-16 offsets, as used by browser editors.
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub prefixes: Vec<String>,
    pub direction: Option<String>,
    pub multiplicity: Option<String>,
    pub value: Option<String>,
    pub doc: Option<String>,
    pub refs: Vec<RefOut>,
    pub props: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagOut {
    pub file: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct AnalysisOut {
    pub elements: Vec<ElementOut>,
    pub roots: Vec<String>,
    pub diagnostics: Vec<DiagOut>,
}

/// Per-file tables for fast offset conversion.
struct Positions {
    ascii: Vec<bool>,
    line_starts: Vec<Vec<usize>>,
    /// UTF-16 offset of each line start; empty for ASCII files.
    line_starts16: Vec<Vec<usize>>,
}

impl Positions {
    fn new(model: &Model) -> Self {
        let ascii = model.files.iter().map(|f| f.text.is_ascii()).collect();
        let line_starts = model
            .files
            .iter()
            .map(|f| std::iter::once(0).chain(f.text.match_indices('\n').map(|(i, _)| i + 1)).collect())
            .collect::<Vec<Vec<usize>>>();
        let line_starts16 = model
            .files
            .iter()
            .zip(&line_starts)
            .map(|(f, ls)| {
                if f.text.is_ascii() {
                    return Vec::new();
                }
                let mut out = Vec::with_capacity(ls.len());
                let mut acc = 0;
                for (i, s) in ls.iter().enumerate() {
                    out.push(acc);
                    let end = ls.get(i + 1).copied().unwrap_or(f.text.len());
                    acc += f.text[*s..end].encode_utf16().count();
                }
                out
            })
            .collect();
        Positions { ascii, line_starts, line_starts16 }
    }
    fn line_col(&self, file: usize, byte: usize) -> (usize, usize) {
        let ls = &self.line_starts[file];
        let line = ls.partition_point(|s| *s <= byte);
        (line, byte - ls[line - 1] + 1)
    }
    fn utf16(&self, model: &Model, file: usize, byte: usize) -> usize {
        let text = &model.files[file].text;
        let mut byte = byte.min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        if self.ascii[file] {
            byte
        } else {
            let line = self.line_starts[file].partition_point(|s| *s <= byte) - 1;
            let start = self.line_starts[file][line];
            self.line_starts16[file][line] + text[start..byte].encode_utf16().count()
        }
    }
}

pub fn to_model(files: Vec<FileIn>) -> Model {
    crate::analyze(files.into_iter().map(|f| SourceFile { path: f.path, text: f.text }).collect())
}

fn element_out(model: &Model, pos: &Positions, e: &Element) -> ElementOut {
    ElementOut {
        id: e.qualified_name.clone(),
        kind: e.kind.clone(),
        keyword: e.keyword(),
        is_def: e.is_def,
        name: e.name.clone(),
        short_name: e.short_name.clone(),
        owner: e.owner.map(|o| model.elements[o].qualified_name.clone()),
        children: e.children.iter().map(|c| model.elements[*c].qualified_name.clone()).collect(),
        file: model.files[e.file].path.clone(),
        start: pos.utf16(model, e.file, e.span.start),
        end: pos.utf16(model, e.file, e.span.end),
        line: pos.line_col(e.file, e.span.start).0,
        prefixes: e.prefixes.clone(),
        direction: e.direction.clone(),
        multiplicity: e.multiplicity.clone(),
        value: e.value.clone(),
        doc: e.doc.clone(),
        refs: e
            .refs
            .iter()
            .map(|r| RefOut {
                role: r.role,
                text: r.text.clone(),
                target: r.resolved().map(|t| model.elements[t].qualified_name.clone()),
                conjugated: r.conjugated,
            })
            .collect(),
        props: e.props.clone(),
    }
}

pub fn diagnostics_out(model: &Model) -> Vec<DiagOut> {
    let pos = Positions::new(model);
    model
        .diagnostics
        .iter()
        .map(|d| {
            let (line, col) = pos.line_col(d.file, d.span.start);
            DiagOut {
                file: model.files[d.file].path.clone(),
                start: pos.utf16(model, d.file, d.span.start),
                end: pos.utf16(model, d.file, d.span.end.max(d.span.start + 1)),
                line,
                col,
                severity: d.severity,
                message: d.message.clone(),
            }
        })
        .collect()
}

pub fn one_element(model: &Model, e: &Element) -> ElementOut {
    element_out(model, &Positions::new(model), e)
}

pub fn analysis(model: &Model) -> AnalysisOut {
    let pos = Positions::new(model);
    AnalysisOut {
        elements: model.elements.iter().map(|e| element_out(model, &pos, e)).collect(),
        roots: model.roots.iter().map(|r| model.elements[*r].qualified_name.clone()).collect(),
        diagnostics: diagnostics_out(model),
    }
}

/// Apply an edit and refuse it when it introduces a new syntax error.
pub fn apply_edit(files: Vec<FileIn>, op: &EditOp) -> Result<Vec<FileChange>, String> {
    let before = to_model(files.clone());
    let errors_before = before.diagnostics.iter().filter(|d| d.severity == Severity::Error).count();
    let changes = edit::apply(&before, op)?;
    let mut after_files = files;
    for c in &changes {
        match after_files.iter_mut().find(|f| f.path == c.path) {
            Some(f) => f.text = c.text.clone(),
            None => after_files.push(FileIn { path: c.path.clone(), text: c.text.clone() }),
        }
    }
    let after = to_model(after_files.clone());
    let errs: Vec<&Diagnostic> = after.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    if errs.len() > errors_before {
        let d = errs[0];
        let (line, _) = after.line_col(d.file, d.span.start);
        return Err(format!("edit rejected: it would not parse ({} line {}: {})", after.files[d.file].path, line, d.message));
    }
    // A pasted copy that carries the minted id of an element the model has gets a new id, as a tool write gives it.
    let mut changes = changes;
    let mut after_files = after_files;
    let after = if crate::agent::mint::remint_copies(&before, &mut after_files).is_empty() {
        after
    } else {
        for c in changes.iter_mut() {
            if let Some(f) = after_files.iter().find(|f| f.path == c.path) {
                c.text = f.text.clone();
            }
        }
        to_model(after_files.clone())
    };
    // A client's edits follow the rules every tool write follows: an owner the kind allows, and no second representation
    // of an element the model has.
    crate::agent::ownership::refuse_new(&before, &after).map_err(|f| f.message)?;
    crate::agent::duplicates::refuse_new(&before, &after).map_err(|f| f.message)?;
    // An element written as text (the page's new part, port, requirement) gets its id as a tool-created one does.
    if matches!(op, EditOp::AddElement { .. }) {
        let had: std::collections::HashSet<String> = before.elements.iter().map(|e| e.qualified_name.clone()).collect();
        let mut work = after_files;
        if crate::agent::mint::mint_new(&mut work, &had) > 0 {
            for c in changes.iter_mut() {
                if let Some(f) = work.iter().find(|f| f.path == c.path) {
                    c.text = f.text.clone();
                }
            }
        }
    }
    Ok(changes)
}

/// Indented text outline, used by agents to read a model cheaply.
pub fn outline(model: &Model, root: Option<&str>, max_depth: usize) -> Result<String, String> {
    fn walk(model: &Model, id: ElementId, depth: usize, max: usize, out: &mut String) {
        let e = &model.elements[id];
        if matches!(e.kind.as_str(), "comment" | "doc" | "expression") {
            return;
        }
        let mut line = format!("{}{} {}", "  ".repeat(depth), e.keyword(), e.display_name());
        for (role, sym) in [(RefRole::Type, ":"), (RefRole::Specializes, ":>"), (RefRole::Redefines, ":>>")] {
            let v: Vec<&str> = e.refs_of(role).map(|r| r.text.as_str()).collect();
            if !v.is_empty() {
                line.push_str(&format!(" {sym} {}", v.join(", ")));
            }
        }
        let src: Vec<&str> = e.refs_of(RefRole::Source).map(|r| r.text.as_str()).collect();
        let tgt: Vec<&str> = e.refs_of(RefRole::Target).map(|r| r.text.as_str()).collect();
        if !src.is_empty() || (!tgt.is_empty() && !e.implied_name) {
            line.push_str(&format!(" {} -> {}", src.join(", "), tgt.join(", ")));
        }
        if let Some(m) = &e.multiplicity {
            line.push_str(&format!(" [{m}]"));
        }
        if let Some(v) = &e.value {
            line.push_str(&format!(" = {v}"));
        }
        out.push_str(&line);
        out.push('\n');
        if depth < max {
            for c in &e.children {
                walk(model, *c, depth + 1, max, out);
            }
        } else if !e.children.is_empty() {
            out.push_str(&format!("{}... {} members\n", "  ".repeat(depth + 1), e.children.len()));
        }
    }
    let mut out = String::new();
    match root {
        Some(r) if !r.is_empty() => {
            let e = model.find(r).ok_or_else(|| format!("no element named {}", crate::agent::text::quote_clip(r)))?;
            walk(model, e.id, 0, max_depth, &mut out);
        }
        _ => {
            for r in &model.roots {
                walk(model, *r, 0, max_depth, &mut out);
            }
        }
    }
    Ok(out)
}
