//! Embeds the known-bad fixture of each rule (`sysml-core/tests/rule_fixtures/<rule id>.sysml`) so `--explain` can show an
//! example that fails without any file next to the program.
use std::fmt::Write;

fn main() {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../sysml-core/tests/rule_fixtures");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".sysml")).map(str::to_string)).collect())
        .unwrap_or_default();
    names.sort();
    let mut src = String::from("pub static FIXTURES: &[(&str, &str)] = &[\n");
    for n in names {
        let path = dir.canonicalize().unwrap().join(format!("{n}.sysml"));
        let _ = writeln!(src, "    ({:?}, include_str!({:?})),", n, path.to_string_lossy());
    }
    src.push_str("];\n");
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("fixtures.rs"), src).unwrap();
}
