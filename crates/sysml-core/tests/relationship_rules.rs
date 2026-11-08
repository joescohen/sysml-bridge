//! Relationship legality as Cameo enforces it (Magic Systems of Systems Architect 2022x Refresh2, read from its SysML
//! customization and validation suites). Only the ends Cameo restricts are restricted:
//! a satisfy, verify or derive names a requirement as its supplier, a derive a requirement as its client, a connector
//! joins properties, ports and parameters. Refine, trace, allocate, dependency and item flow ends are free in Cameo, so
//! they are free here. A requirement takes part in no generalization and holds no classifier but a requirement.

use serde_json::{json, Value};
use sysml_core::agent::{intent, rules};
use sysml_core::api::{self, FileIn};

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn semantic(text: &str) -> Vec<String> {
    let m = api::to_model(files(text));
    rules::run_gates(&m, &["semantic"], None, None, &[]).findings.iter().filter(|f| f["severity"] == "error").map(|f| f["rule"].as_str().unwrap().to_string()).collect()
}

fn ends_errors(text: &str) -> usize {
    semantic(text).iter().filter(|r| *r == "relationship-end-kind").count()
}

const BASE: &str = "package P {\n    part def A { port p; attribute m : ScalarValues::Real; constraint c; }\n    part a : A;\n    part b : A;\n    action act;\n    requirement def RD;\n    requirement r1 : RD;\n    requirement r2 : RD;\n    verification def V;\n";

fn with(extra: &str) -> String {
    format!("{BASE}{extra}\n}}\n")
}

#[test]
fn ends_left_free_are_free() {
    // Refine, Trace, Allocate: Cameo's customization sets no typesForSource or typesForTarget.
    for link in [
        "#SysMLv1::Refine dependency from a to b;",
        "#SysMLv1::Refine dependency from act to a;",
        "#SysMLv1::Trace dependency from a to act;",
        "allocate r1 to a.m;",
        "allocate a.p to act;",
        // Satisfy: any client (the four requirement kinds narrow it only as cameo-gate warnings).
        "satisfy r1 by a.p;",
        "satisfy r2 by a.m;",
        "satisfy r1 by a.c;",
        // A connector may join a constraint property.
        "connect a.c to b.c;",
    ] {
        assert_eq!(ends_errors(&with(link)), 0, "{link}: {:?}", semantic(&with(link)));
    }
}

#[test]
fn restricted_ends_are_errors() {
    // Satisfy and Verify: typesForTarget = AbstractRequirement. DeriveReqt: both ends.
    for link in ["satisfy b by a;", "#SysMLv1::DeriveReqt dependency from a to r1;", "#SysMLv1::DeriveReqt dependency from r1 to a;"] {
        assert_eq!(ends_errors(&with(link)), 1, "{link}: {:?}", semantic(&with(link)));
    }
}

fn apply(f: &mut Vec<FileIn>, op: Value) -> Result<Value, String> {
    match intent::apply_op(f, &op) {
        Ok(a) => {
            *f = a.files;
            Ok(a.info)
        }
        Err(e) => Err(e.message),
    }
}

#[test]
fn create_relationship_follows_cameo() {
    let mut f = files(&with(""));
    // A refine or trace between two parts: Cameo draws it.
    apply(&mut f, json!({"op": "create_relationship", "kind": "refine", "source": "P::act", "target": "P::a"})).unwrap();
    apply(&mut f, json!({"op": "create_relationship", "kind": "trace", "source": "P::a", "target": "P::b"})).unwrap();
    // A derive whose supplier is a part: Cameo's path tool does not take it.
    let e = apply(&mut f, json!({"op": "create_relationship", "kind": "derive", "source": "P::r1", "target": "P::a"})).unwrap_err();
    assert!(e.contains("must be a requirement"), "{e}");
    // A requirement takes part in no generalization (Requirement[4]).
    apply(&mut f, json!({"op": "create_element", "owner": "P", "kind": "requirement def", "name": "RD2"})).unwrap();
    let e = apply(&mut f, json!({"op": "create_relationship", "kind": "specialize", "source": "P::RD2", "target": "P::RD"})).unwrap_err();
    assert!(e.contains("Requirement[4]") && e.contains("derive"), "{e}");
    let m = api::to_model(f.clone());
    assert!(semantic(&m.files.iter().map(|x| x.text.clone()).collect::<String>()).iter().all(|r| r != "relationship-end-kind"));
}

