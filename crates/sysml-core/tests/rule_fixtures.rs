//! The ratchet: every rule of every validation gate has a known-bad fixture in `tests/rule_fixtures/<rule id>.sysml`,
//! the fixture raises that rule in that gate, and nothing else in the same gate except what its first line allows
//! (`// also: id, id`). The clean model raises nothing in any gate. A new rule cannot land without its fixture.

mod common;

use std::collections::BTreeSet;
use sysml_core::agent::rules::{self, registry, GATES};
use sysml_core::api::{self, FileIn};

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/rule_fixtures")
}

fn findings(text: &str, gate: &str) -> Vec<(String, String)> {
    let mut m = api::to_model(vec![FileIn { path: "fixture.sysml".into(), text: text.into() }]);
    sysml_core::agent::add_namespace_checks(&mut m);
    let owner = text.lines().find_map(|l| l.strip_prefix("// coverage_owner: "));
    let owner = sysml_core::agent::trace::CoverageOwner::parse(owner).unwrap();
    let run = rules::run_gates_with_owner(&m, &[gate], None, None, &["satisfy", "verify"], owner);
    run.findings.iter().map(|f| (f["rule"].as_str().unwrap().to_string(), f["severity"].as_str().unwrap().to_string())).collect()
}

#[test]
fn every_rule_has_a_fixture_that_raises_it() {
    let reg = registry();
    assert!(reg.len() >= 75, "the registry holds {} rules", reg.len());
    let mut problems = Vec::new();
    for r in &reg {
        let path = dir().join(format!("{}.sysml", r.id));
        let Ok(text) = std::fs::read_to_string(&path) else {
            problems.push(format!("{} ({}): no fixture {}", r.id, r.gate, path.display()));
            continue;
        };
        let also: BTreeSet<String> = text
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// also:"))
            .map(|l| l.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        let got = findings(&text, r.gate);
        if !got.iter().any(|(id, _)| *id == r.id) {
            problems.push(format!("{} ({}): its fixture does not raise it; the gate raised {:?}", r.id, r.gate, got));
            continue;
        }
        let extra: BTreeSet<String> = got.iter().map(|(id, _)| id.clone()).filter(|id| *id != r.id && !also.contains(id)).collect();
        if !extra.is_empty() {
            problems.push(format!("{} ({}): its fixture also raises {:?}", r.id, r.gate, extra));
        }
    }
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn no_fixture_is_without_a_rule() {
    let ids: BTreeSet<String> = registry().into_iter().map(|r| r.id).collect();
    for e in std::fs::read_dir(dir()).unwrap().flatten() {
        if !e.file_type().unwrap().is_file() { continue; }
        let name = e.file_name().to_string_lossy().to_string();
        let stem = name.strip_suffix(".sysml").unwrap_or(&name).to_string();
        assert!(stem == "clean" || ids.contains(&stem), "fixture {name} names no rule id");
    }
}

#[test]
fn the_clean_model_raises_nothing_in_any_gate() {
    let text = std::fs::read_to_string(dir().join("clean.sysml")).unwrap();
    for g in GATES {
        let got = findings(&text, g);
        assert!(got.is_empty(), "gate {g} raised {got:?} on the clean model");
    }
}

#[test]
fn the_registry_has_one_row_per_gate_and_rule() {
    let reg = registry();
    let mut seen = BTreeSet::new();
    for r in &reg {
        assert!(GATES.contains(&r.gate), "{}", r.id);
        assert!(seen.insert((r.gate, r.id.clone())), "{} twice in {}", r.id, r.gate);
        assert!(!r.meaning.is_empty() && !r.group.is_empty());
    }
}
