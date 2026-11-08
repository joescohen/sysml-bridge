//! `sysml-conform <file-or-directory>...`: official-grammar check per `.sysml` file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use sysml_conform::{Checker, Limits, Report, Verdict, GRAMMAR_RELEASE};

const USAGE: &str = "usage: sysml-conform [options] <file-or-directory>...

Checks each .sysml file against the official OMG SysML v2 textual grammar.
A directory is searched for *.sysml files. Exit code: 0 when every file is
valid, 1 when a file is invalid, 2 when a file was not checked or on a usage error.

options:
  --json              print one JSON document instead of text
  --max-tokens N      token limit (default 4000000)
                      (a file gets no more than 100000 + 200 recogniser
                      steps a byte; there is no file size limit)
  --max-items N       stored-item limit (default 10000000)
  --grammar-report    print how the grammar was assembled, then exit
";

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn collect(path: &Path, out: &mut Vec<PathBuf>, problems: &mut Vec<(PathBuf, String)>) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => return problems.push((path.to_path_buf(), format!("cannot read: {e}"))),
    };
    if meta.is_dir() {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(path) {
            Ok(dir) => dir.filter_map(|e| e.ok().map(|e| e.path())).collect(),
            Err(e) => return problems.push((path.to_path_buf(), format!("cannot read: {e}"))),
        };
        entries.sort();
        for entry in entries {
            // Symbolic links inside a directory are not followed.
            let Ok(meta) = std::fs::symlink_metadata(&entry) else {
                continue;
            };
            if meta.is_dir() {
                collect(&entry, out, problems);
            } else if meta.is_file() && entry.extension().is_some_and(|e| e == "sysml") {
                out.push(entry);
            }
        }
    } else {
        out.push(path.to_path_buf());
    }
}

fn grammar_report(checker: &Checker) {
    let report = &checker.grammar.report;
    println!("official grammar release: {GRAMMAR_RELEASE}");
    println!(
        "rules taken from SysML-textual-bnf.kebnf: {}",
        report.sysml_rules.len()
    );
    println!(
        "rules taken from KerML-textual-bnf.kebnf: {}",
        report.kerml_rules.len()
    );
    for name in &report.kerml_rules {
        println!("  {name}");
    }
    println!("corrections: {}", report.corrections.len());
    for fix in &report.corrections {
        println!(
            "  {} ({})",
            fix.rule,
            if fix.replaces {
                "replaces the official rule"
            } else {
                "added"
            }
        );
        for line in fix.reason.lines() {
            println!("      {line}");
        }
    }
    println!(
        "word terminals that are not SysML reserved words: {:?}",
        report.unmatchable_words
    );
    println!("rewrites that keep the language: {}", report.rewrites.len());
    for line in &report.rewrites {
        println!("  {line}");
    }
    println!(
        "compiled: {} productions, {} nonterminals, {} terminals",
        report.compiled_rules,
        report.compiled_nonterminals,
        checker.grammar.terminals.len()
    );
}

fn main() -> ExitCode {
    let mut limits = Limits::default();
    let mut json = false;
    let mut show_grammar = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut number = |name: &str| -> Result<u64, String> {
            args.next()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| format!("{name} needs a whole number"))
        };
        let parsed = match arg.as_str() {
            "--json" => {
                json = true;
                Ok(())
            }
            "--grammar-report" => {
                show_grammar = true;
                Ok(())
            }
            "--max-tokens" => number("--max-tokens").map(|n| limits.max_tokens = n as usize),
            "--max-items" => number("--max-items").map(|n| limits.max_items = n as usize),
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other if other.starts_with("--") => Err(format!("unknown option {other}")),
            other => {
                paths.push(PathBuf::from(other));
                Ok(())
            }
        };
        if let Err(message) = parsed {
            eprintln!("sysml-conform: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let checker = Checker::new();
    if show_grammar {
        grammar_report(&checker);
        return ExitCode::SUCCESS;
    }
    if paths.is_empty() {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }

    let mut files = Vec::new();
    let mut problems = Vec::new();
    for path in &paths {
        collect(path, &mut files, &mut problems);
    }
    let mut results: Vec<(PathBuf, Report)> = Vec::new();
    for (path, reason) in problems {
        results.push((
            path,
            Report {
                verdict: Verdict::NotChecked(reason),
                bytes: 0,
                tokens: 0,
                steps: 0,
            },
        ));
    }
    for path in files {
        let report = match std::fs::read(&path) {
            Ok(bytes) => checker.check_bytes(&bytes, &limits),
            Err(e) => Report {
                verdict: Verdict::NotChecked(format!("cannot read: {e}")),
                bytes: 0,
                tokens: 0,
                steps: 0,
            },
        };
        results.push((path, report));
    }

    let (mut valid, mut invalid, mut not_checked) = (0, 0, 0);
    let mut out = String::new();
    if json {
        out.push_str("{\n  \"grammar_release\": ");
        out.push_str(&json_string(GRAMMAR_RELEASE));
        out.push_str(",\n  \"files\": [");
    }
    for (index, (path, report)) in results.iter().enumerate() {
        let shown = path.display().to_string();
        match &report.verdict {
            Verdict::Valid => valid += 1,
            Verdict::Invalid(_) => invalid += 1,
            Verdict::NotChecked(_) => not_checked += 1,
        }
        if json {
            out.push_str(if index == 0 { "\n    {" } else { ",\n    {" });
            let _ = write!(out, "\"path\": {}, ", json_string(&shown));
            match &report.verdict {
                Verdict::Valid => out.push_str("\"status\": \"valid\""),
                Verdict::Invalid(fault) => {
                    let allowed: Vec<String> =
                        fault.allowed.iter().map(|a| json_string(a)).collect();
                    let _ = write!(
                        out,
                        "\"status\": \"invalid\", \"line\": {}, \"column\": {}, \"found\": {}, \"found_kind\": {}, \"allowed\": [{}]",
                        fault.line,
                        fault.column,
                        json_string(&fault.found),
                        json_string(&fault.found_kind),
                        allowed.join(", ")
                    );
                }
                Verdict::NotChecked(reason) => {
                    let _ = write!(
                        out,
                        "\"status\": \"not_checked\", \"reason\": {}",
                        json_string(reason)
                    );
                }
            }
            let _ = write!(
                out,
                ", \"bytes\": {}, \"tokens\": {}, \"steps\": {}}}",
                report.bytes, report.tokens, report.steps
            );
        } else {
            match &report.verdict {
                Verdict::Valid => {
                    let _ = writeln!(out, "{shown}: valid");
                }
                Verdict::Invalid(fault) => {
                    let _ = writeln!(
                        out,
                        "{shown}:{}:{}: invalid: found {} ({}); the grammar allows here: {}",
                        fault.line,
                        fault.column,
                        if fault.found == "end of file" {
                            fault.found.clone()
                        } else {
                            format!("`{}`", fault.found)
                        },
                        fault.found_kind,
                        fault.allowed.join(" ")
                    );
                }
                Verdict::NotChecked(reason) => {
                    let _ = writeln!(out, "{shown}: not checked: {reason}");
                }
            }
        }
    }
    if json {
        let _ = write!(
            out,
            "\n  ],\n  \"summary\": {{\"files\": {}, \"valid\": {valid}, \"invalid\": {invalid}, \"not_checked\": {not_checked}}}\n}}\n",
            results.len()
        );
    } else {
        let _ = writeln!(
            out,
            "{} files: {valid} valid, {invalid} invalid, {not_checked} not checked",
            results.len()
        );
    }
    print!("{out}");
    if invalid > 0 {
        ExitCode::from(1)
    } else if not_checked > 0 || results.is_empty() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}
