//! Analyze all given files as one workspace and summarize diagnostics.
use sysml_core::{api, SourceFile};
fn main() {
    let files: Vec<SourceFile> = std::env::args().skip(1).map(|p| SourceFile { text: std::fs::read_to_string(&p).unwrap(), path: p }).collect();
    let t = std::time::Instant::now();
    let m = sysml_core::analyze(files);
    let ms = t.elapsed().as_millis();
    let d = api::diagnostics_out(&m);
    let refs: usize = m.elements.iter().map(|e| e.refs.len()).sum();
    let unresolved: usize = m.elements.iter().flat_map(|e| &e.refs).filter(|r| r.resolved().is_none()).count();
    println!("{} files, {} elements, {} refs ({} unresolved), {} diagnostics, {} ms", m.files.len(), m.elements.len(), refs, unresolved, d.len(), ms);
    let mut c = std::collections::BTreeMap::new();
    for x in &d { *c.entry(x.message.clone()).or_insert(0) += 1; }
    let mut v: Vec<_> = c.into_iter().collect(); v.sort_by(|a, b| b.1.cmp(&a.1));
    for (m, n) in v.iter().take(25) { println!("{n:5} {m}"); }
}
