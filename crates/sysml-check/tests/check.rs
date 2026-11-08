//! The checker on its own: the last line, the limits, fail closed, and no writes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use sysml_check::{run, run_with, Gate};

static N: AtomicUsize = AtomicUsize::new(0);

fn fresh() -> PathBuf {
    let d = std::env::temp_dir().join(format!("sysml-check-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn last(text: &str) -> &str {
    text.trim_end().lines().last().unwrap_or("")
}

fn go(dir: &Path) -> sysml_check::Outcome {
    run(&[dir.to_string_lossy().to_string()])
}

const GOOD: &str = "package P {\n    part def A;\n    part x : A;\n}\n";

/// Every path with its bytes and modified time, so a write of any kind shows.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                stack.push(p.clone());
            }
            let bytes = if m.is_file() { std::fs::read(&p).unwrap() } else { Vec::new() };
            out.push((p.to_string_lossy().to_string(), bytes, m.modified().unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn the_last_line_is_one_of_two_forms_and_the_exit_code_follows() {
    let d = fresh();
    std::fs::write(d.join("m.sysml"), GOOD).unwrap();
    let ok = go(&d);
    assert_eq!(ok.code, 0, "{}", ok.text);
    assert_eq!(last(&ok.text), "CHECK PASS: 1 files, 3 elements, 0 errors, 0 warnings");
    std::fs::write(d.join("m.sysml"), "package P { part def A {\n").unwrap();
    let bad = go(&d);
    assert_eq!(bad.code, 1);
    let l = last(&bad.text);
    assert!(l.starts_with("CHECK FAIL: ") && l.contains(" errors; first: m.sysml:"), "{l}");
    assert!(!l.contains('\n') && l.len() < 400);
    // The json form is the report, not a text line.
    let j = run(&[d.to_string_lossy().to_string(), "--json".into()]);
    let v: serde_json::Value = serde_json::from_str(&j.text).unwrap();
    assert_eq!((v["passed"].as_bool(), j.code), (Some(false), 1));
}

#[test]
fn the_check_never_writes_into_the_folder() {
    let d = fresh();
    std::fs::write(d.join("m.sysml"), GOOD).unwrap();
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/n.sysml"), "package Q { part def B; }\n").unwrap();
    std::fs::write(d.join("PARITY.md"), "not a model").unwrap();
    let before = tree(&d);
    for args in [vec![], vec!["--json".to_string()], vec!["--gates".into(), "syntax".into()]] {
        let mut a = vec![d.to_string_lossy().to_string()];
        a.extend(args);
        run(&a);
    }
    assert_eq!(before, tree(&d), "the folder is the same after the check");
}

#[test]
fn a_file_over_the_limit_is_a_named_failure_and_never_a_pass() {
    let d = fresh();
    std::fs::write(d.join("ok.sysml"), GOOD).unwrap();
    let big = format!("// {}\n", "x".repeat(sysml_check::MAX_FILE_BYTES as usize));
    std::fs::write(d.join("big.sysml"), big).unwrap();
    let o = go(&d);
    assert_eq!(o.code, 1);
    assert!(last(&o.text).starts_with("CHECK FAIL: 1 errors; first: big.sysml:0 input-file-too-large"), "{}", last(&o.text));
}

#[test]
fn a_folder_nested_too_deep_is_a_named_failure_and_never_skipped() {
    let d = fresh();
    std::fs::write(d.join("ok.sysml"), GOOD).unwrap();
    let mut deep = d.clone();
    for i in 0..sysml_check::MAX_DEPTH + 2 {
        deep = deep.join(format!("d{i}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("hidden.sysml"), "package Broken {\n").unwrap();
    let o = go(&d);
    assert_eq!(o.code, 1, "{}", o.text);
    assert!(last(&o.text).contains("input-too-deep"), "{}", last(&o.text));
}

#[test]
fn too_many_files_an_empty_folder_and_a_link_are_failures() {
    let d = fresh();
    let o = go(&d);
    assert!(last(&o.text).contains("input-no-files") && o.code == 1);
    for i in 0..sysml_check::MAX_FILES + 1 {
        std::fs::write(d.join(format!("f{i:05}.sysml")), "package P;\n").unwrap();
    }
    let o = go(&d);
    assert!(o.text.contains("input-too-many-files") && o.code == 1);
    let e = fresh();
    std::fs::write(e.join("real.sysml"), GOOD).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(e.join("real.sysml"), e.join("alias.sysml")).unwrap();
        let o = go(&e);
        assert!(o.text.contains("input-link") && o.code == 1, "{}", o.text);
    }
}

#[test]
fn a_gate_that_fails_inside_fails_the_run() {
    let d = fresh();
    std::fs::write(d.join("m.sysml"), GOOD).unwrap();
    let args = [d.to_string_lossy().to_string()];
    let errs: &Gate = &|_, _, _| Err("the gate broke".to_string());
    let o = run_with(&args, errs);
    assert_eq!(o.code, 2);
    assert!(last(&o.text).starts_with("CHECK FAIL: ") && !o.text.contains("CHECK PASS"), "{}", o.text);
    let panics: &Gate = &|_, _, _| panic!("a gate panicked");
    let o = run_with(&args, panics);
    assert_eq!(o.code, 2);
    assert!(last(&o.text).starts_with("CHECK FAIL: ") && !o.text.contains("CHECK PASS"), "{}", o.text);
}

#[test]
fn names_from_the_pipeline_cannot_inject_terminal_controls() {
    let d = fresh();
    std::fs::write(d.join("m\u{1b}[31m.sysml"), "package P { part def A {\n").unwrap();
    let o = go(&d);
    assert!(!o.text.contains('\u{1b}') || !last(&o.text).contains('\u{1b}'), "the last line carries no control character");
    assert!(!last(&o.text).chars().any(|c| c.is_control()), "{:?}", last(&o.text));
}

#[test]
fn bad_arguments_exit_two_and_explain_and_list_work() {
    let d = fresh();
    std::fs::write(d.join("m.sysml"), GOOD).unwrap();
    let p = d.to_string_lossy().to_string();
    assert_eq!(run(&[p.clone(), "--gates".into(), "nope".into()]).code, 2);
    assert_eq!(run(&[]).code, 2);
    assert_eq!(run(&["/no/such/folder/at/all".into()]).code, 2);
    let l = run(&["--list-rules".into()]);
    assert!(l.text.contains("trace-operand-definition\tsemantic\twarning") && l.text.contains("input-file-too-large\tinput\terror"));
    let e = run(&["--explain".into(), "trace-operand-definition".into()]);
    assert_eq!(e.code, 0);
    assert!(e.text.contains("An example that fails") && e.text.contains("satisfy") && e.text.contains("The fix:"), "{}", e.text);
    assert_eq!(run(&["--explain".into(), "no-such-rule".into()]).code, 2);
    // Every rule the program lists can be explained.
    for line in l.text.lines().skip(1) {
        let id = line.split('\t').next().unwrap();
        assert_eq!(run(&["--explain".into(), id.into()]).code, 0, "{id}");
    }
}

#[test]
fn a_report_that_is_empty_or_misses_a_gate_never_reads_as_a_pass() {
    let d = fresh();
    std::fs::write(d.join("m.sysml"), GOOD).unwrap();
    let args = [d.to_string_lossy().to_string()];
    let nulls: &Gate = &|_, _, _| Ok(serde_json::Value::Null);
    let o = run_with(&args, nulls);
    assert_eq!(o.code, 2, "{}", o.text);
    assert!(last(&o.text).starts_with("CHECK FAIL: ") && !o.text.contains("CHECK PASS"), "{}", o.text);
    let empty: &Gate = &|_, _, _| Ok(serde_json::json!({}));
    assert_eq!(run_with(&args, empty).code, 2);
    // A report with the counts but only one gate of seven: the others did not run.
    let partial: &Gate = &|_, _, _| Ok(serde_json::json!({ "passed": true, "errors": 0, "warnings": 0, "findings": [], "gates": { "syntax": { "passed": true, "errors": 0 } } }));
    let o = run_with(&args, partial);
    assert_eq!(o.code, 2, "{}", o.text);
    assert!(last(&o.text).starts_with("CHECK FAIL: "));
    // Asked for one gate, a report with that one gate is whole.
    let one: &Gate = &|_, _, _| Ok(serde_json::json!({ "passed": true, "errors": 0, "warnings": 0, "infos": 0, "findings": [], "gates": { "syntax": { "passed": true, "errors": 0 } } }));
    let mut a = args.to_vec();
    a.extend(["--gates".into(), "syntax".into()]);
    assert_eq!(run_with(&a, one).code, 0);
}
