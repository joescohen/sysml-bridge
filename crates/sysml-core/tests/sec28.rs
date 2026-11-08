//! Security round 28 on the proposal check (after commit 34914b6). Every fixture is invented (a small bottling line); each
//! test is written against the interface of commit a087049 and fails on its code.

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{self, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

/// A line with a filler, a capper and a checker. Three functions in one behavior; two flows, one of which is already a gap
/// of the model (the checker's). Every function is allocated to its own part, and no connection joins any two parts.
const LINE: &str = "package Q {
    package SysMLv1 {
        metadata def Trace;
        metadata def DeriveReqt;
        metadata def ItemFlow;
    }
    item def Cap;
    item def Scan;
    item def Reject;
    port def Wire {
        inout item payload;
    }
    requirement <'Q-1'> need {
        doc /* The line shall cap each bottle. */
    }
    action def Fill {
        out cap : Cap;
        out scan : Scan;
    }
    action def Seal {
        in cap : Cap;
    }
    action def Check {
        in scan : Scan;
    }
    action def Run {
        action fill : Fill {
            out cap :>> Fill::cap;
            out scan :>> Fill::scan;
        }
        action seal : Seal {
            in cap :>> Seal::cap;
        }
        action check : Check {
            in scan :>> Check::scan;
        }
        first start then fill;
        first fill then seal;
        first fill then check;
        flow from fill.scan to check.scan;
    }
    part def Filler;
    part def Capper;
    part def Checker;
    part def Line {
        part filler : Filler;
        part capper : Capper;
        part checker : Checker;
    }
    allocate Fill to Filler;
    allocate Seal to Capper;
    allocate Check to Checker;
    satisfy need by Run;
}
";

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "q.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    let mut m = api::to_model(f.to_vec());
    agent::add_namespace_checks(&mut m);
    m
}

fn kinds(a: &Value) -> Vec<&str> {
    a["check"]["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect()
}

fn says(a: &Value, kind: &str) -> String {
    a["check"]["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == kind).map(|f| f["says"].as_str().unwrap()).collect::<Vec<_>>().join(" || ")
}

fn cap_edits() -> Vec<Value> {
    vec![json!({ "op": "add_element", "owner": "Q::Run", "text": "flow from fill.cap to seal.cap;" })]
}

fn left_to_p002() -> Value {
    json!([{ "rule": "interface-needed", "ref": "Q::Cap", "reason": "the connection is the next proposal", "decided_in": "P-002" }])
}

fn doc_edit(doc: &str) -> Vec<Value> {
    vec![json!({ "op": "update_element", "ref": "Q::need", "doc": doc })]
}

// ---- 1. one rule for "this finding stands or is waived" -------------------------------------------------------------------

// ---- 2. a missing or unreadable field is not "fine" -----------------------------------------------------------------------

// ---- 3. the verdict is held against what the edits did --------------------------------------------------------------------

// ---- 4. the two documents ---------------------------------------------------------------------------------------------------

const HOSTILE: &str = "reason | <script>alert(1)</script> `tick` [x](http://y) \u{0B}vt \u{2028}ls \u{202E}rtl";

fn no_forged_structure(md: &str, what: &str) {
    assert!(!md.contains("<script>") && !md.contains("<br>"), "{what}: a tag of the text reads as markup: {md}");
    assert!(!md.contains('\u{0B}') && !md.contains('\u{2028}') && !md.contains('\u{202E}'), "{what}: a hidden or separator character stands in the document");
    for l in md.lines().filter(|l| l.contains("alert")) {
        assert!(l.contains("\\<script\\>") || l.contains("{U+"), "{what}: the text stands unescaped in a line: {l}");
    }
    // Every row of a table has as many unescaped pipes as the header of that table: a pipe of the text cuts no row.
    let mut width: Option<usize> = None;
    for l in md.lines() {
        if !l.starts_with('|') {
            width = None;
            continue;
        }
        let n = l.replace("\\|", "").matches('|').count();
        assert_eq!(*width.get_or_insert(n), n, "{what}: a table row is cut by a pipe of the text: {l}");
    }
}

#[test]
fn the_change_package_writes_a_one_line_reason_an_author_and_a_baseline_note_as_text_only() {
    let p = json!({
        "title": HOSTILE, "reason": HOSTILE, "author": HOSTILE,
        "from": { "baseline": "B1", "note": HOSTILE, "revision": "r1", "time": "t1", "author": HOSTILE },
        "to": { "is": "current", "revision": "r2" },
        "writes": { "count": 1, "first": 1, "last": 1, "first_time": "t", "last_time": "t", "authors": [HOSTILE] },
        "changed": true, "requirements": [], "changes": { "counts": { "added": 0, "changed": 0, "removed": 0 }, "stages": [] },
        "proposals": [{ "id": "P-001", "title": HOSTILE, "proposed_by": HOSTILE, "accepted_by": HOSTILE, "route": "http", "reviewer_authenticated": true, "accepted_at": "t", "note": HOSTILE, "write": 1 }],
    });
    let md = agent::package::markdown(&p);
    no_forged_structure(&md, "change package");
    // A name somebody typed stands as a claim: in quotes.
    let author = md.lines().find(|l| l.starts_with("| Author (signed as)")).unwrap();
    assert!(author.starts_with("| Author (signed as) | \""), "{author}");
}

#[test]
fn a_cell_stays_one_line_whatever_the_text_holds() {
    let cell = agent::text::md_table(&["R"], &[vec![HOSTILE.to_string()], vec!["a\u{0B}b\nc".to_string()]]);
    assert_eq!(cell.lines().count(), 4, "{cell}");
    assert!(!cell.contains("<br>") && !cell.contains('\u{0B}'), "{cell}");
}
