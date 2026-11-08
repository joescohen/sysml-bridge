//! Parse the given files and print diagnostics.
use sysml_core::{api, SourceFile};
fn main() {
    let mut total = 0;
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path).unwrap();
        let m = sysml_core::analyze(vec![SourceFile { path: path.clone(), text }]);
        let diags = api::diagnostics_out(&m);
        total += diags.len();
        println!("{path}: {} elements, {} diagnostics", m.elements.len(), diags.len());
        for d in diags.iter().take(12) {
            println!("  {:?} {}:{} {}", d.severity, d.line, d.col, d.message);
        }
        if std::env::var("OUTLINE").is_ok() {
            print!("{}", api::outline(&m, None, 9).unwrap());
        }
    }
    println!("total diagnostics: {total}");
}
