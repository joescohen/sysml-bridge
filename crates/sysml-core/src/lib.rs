#![recursion_limit = "512"]
//! SysML v2 core: parse `.sysml` text into an element graph, resolve names,
//! and apply model edits back to the text.

pub mod agent;
pub mod api;

pub mod edit;
pub mod hidden_chars;

pub mod lexer;
pub mod model;
pub mod parser;
pub mod resolve;

pub use model::*;

/// Parse and resolve a set of files as one workspace.
pub fn analyze(files: Vec<SourceFile>) -> Model {
    analyze_with(files, true)
}

/// `analyze`; `kept` false parses every file again (what a kept parse must equal).
#[doc(hidden)]
pub fn analyze_with(files: Vec<SourceFile>, kept: bool) -> Model {
    let mut model = Model { files, ..Default::default() };
    for i in 0..model.files.len() {
        if kept {
            let parsed = parse_cache::parsed(i, &model.files[i].text);
            parse_cache::splice(&parsed, &mut model.elements, &mut model.roots, &mut model.diagnostics);
        } else {
            let text = model.files[i].text.clone();
            parser::parse_file(i, &text, &mut model.elements, &mut model.roots, &mut model.diagnostics);
        }
    }
    model.parsed_diagnostics = model.diagnostics.len();
    resolve::resolve(&mut model);
    model
}

/// The parse of each file, kept while its text is the same: an edit changes one or two files of a model, and the
/// others were parsed again on every write (at 44,000 elements in 41 files, most of a write's time). A file's parse
/// depends only on its text and its place in the list (ids are relative to the file; `splice` shifts them), so a kept
/// parse is the parse; `SYSML_PARSE_EACH=1` parses every file every time, as before.
mod parse_cache {
    use super::{parser, Diagnostic, Element, ElementId};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    pub(crate) struct Parsed {
        text: Arc<str>,
        els: Vec<Element>,
        roots: Vec<ElementId>,
        diags: Vec<Diagnostic>,
    }

    /// Files kept: well above the files of a large model, and dropped whole when full (a new file set).
    const SLOTS: usize = 8192;
    /// Text kept, in bytes: two revisions of a large model.
    const BYTES: usize = 256 << 20;

    static CACHE: Mutex<Option<(HashMap<(usize, usize), Vec<Arc<Parsed>>>, usize, usize)>> = Mutex::new(None);

    fn enabled() -> bool {
        std::env::var_os("SYSML_PARSE_EACH").is_none()
    }

    pub(crate) fn parsed(file: usize, text: &str) -> Arc<Parsed> {
        let key = (file, text.len());
        if enabled() {
            if let Ok(c) = CACHE.lock() {
                if let Some((map, _, _)) = c.as_ref() {
                    if let Some(p) = map.get(&key).and_then(|v| v.iter().find(|p| &*p.text == text)) {
                        return p.clone();
                    }
                }
            }
        }
        let (mut els, mut roots, mut diags) = (Vec::new(), Vec::new(), Vec::new());
        parser::parse_file(file, text, &mut els, &mut roots, &mut diags);
        let p = Arc::new(Parsed { text: Arc::from(text), els, roots, diags });
        if enabled() {
            if let Ok(mut c) = CACHE.lock() {
                let (map, n, bytes) = c.get_or_insert_with(|| (HashMap::new(), 0, 0));
                if *n >= SLOTS || *bytes + text.len() > BYTES {
                    map.clear();
                    *n = 0;
                    *bytes = 0;
                }
                let v = map.entry(key).or_default();
                v.insert(0, p.clone());
                v.truncate(2);
                *n += 1;
                *bytes += text.len();
            }
        }
        p
    }

    /// Append a file's parse to the workspace, its ids shifted past the elements already there.
    pub(crate) fn splice(p: &Parsed, els: &mut Vec<Element>, roots: &mut Vec<ElementId>, diags: &mut Vec<Diagnostic>) {
        let base = els.len();
        els.reserve(p.els.len());
        for e in &p.els {
            let mut e = e.clone();
            e.id += base;
            e.owner = e.owner.map(|o| o + base);
            for c in e.children.iter_mut() {
                *c += base;
            }
            debug_assert!(e.refs.iter().all(|r| r.segments.iter().all(|s| s.resolved.is_none())));
            els.push(e);
        }
        roots.extend(p.roots.iter().map(|r| r + base));
        diags.extend(p.diags.iter().cloned());
    }
}

pub fn analyze_one(path: &str, text: &str) -> Model {
    analyze(vec![SourceFile { path: path.to_string(), text: text.to_string() }])
}
