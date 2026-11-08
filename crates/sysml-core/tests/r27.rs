//! Tools round 27: the faults the first agent run on a real model met. Every fixture is invented (a small bottling line);
//! each test fails on the round 26 code. No name, text or number of the customer model is here.

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

// ---- item 1: a deferred gap on an unnamed flow --------------------------------------------------------------------------

/// The first proposal: the capper takes the cap from the filler. A flow with no name, in a behavior that already holds
/// another flow with no name, and no connection between the parts yet.
fn cap_edits() -> Vec<Value> {
    vec![json!({ "op": "add_element", "owner": "Q::Run", "text": "flow from fill.cap to seal.cap;" })]
}

fn left_to_p002() -> Value {
    json!([{ "rule": "interface-needed", "ref": "Q::Cap", "reason": "the connection is the next proposal", "decided_in": "P-002" }])
}

/// The second proposal: a connection with an item flow that carries the cap.
fn link_edits() -> Vec<Value> {
    vec![
        json!({ "op": "add_element", "owner": "Q::Filler", "text": "port capOut : Wire;" }),
        json!({ "op": "add_element", "owner": "Q::Capper", "text": "port capIn : ~Wire;" }),
        json!({ "op": "add_element", "owner": "Q::Line", "text": "connection capLink connect filler.capOut to capper.capIn;" }),
        json!({ "op": "add_element", "owner": "Q", "text": "#SysMLv1::ItemFlow dependency 'cap frame' from Filler to Capper {\n    comment conveyed about Cap /* the cap */\n    comment realizedBy about Line::capLink /* the link */\n}" }),
    ]
}


// ---- item 2: the gates of a check, by rule ------------------------------------------------------------------------------

fn orphan_flow_edit() -> Value {
    json!({ "op": "add_element", "owner": "Q", "text": "#SysMLv1::ItemFlow dependency 'orphan frame' from Filler to Capper {\n    comment conveyed about Cap /* the cap */\n}" })
}

fn extra_pin_edit() -> Value {
    json!({ "op": "add_element", "owner": "Q::Run::fill", "text": "out reject : Reject;" })
}

// ---- item 3: accepted_gaps matches one gap ------------------------------------------------------------------------------

fn two_gap_edits() -> Vec<Value> {
    vec![
        json!({ "op": "create_relationship", "kind": "allocate", "source": "Q::Fill", "target": "Q::Line", "allow_definitions": true }),
        json!({ "op": "create_relationship", "kind": "satisfy", "source": "Q::Fill", "target": "Q::need", "allow_definitions": true }),
    ]
}

// ---- item 4: a verdict 'deferred' ---------------------------------------------------------------------------------------

// ---- item 5: an edit touches what its text names ------------------------------------------------------------------------

/// The line with a port on each of two parts, a connection between them, and the connection allocated to a function.
fn linked_line() -> String {
    LINE.replace("part def Filler;", "part def Filler {\n        port capOut : Wire;\n    }")
        .replace("part def Capper;", "part def Capper {\n        port capIn : ~Wire;\n    }")
        .replace("        part checker : Checker;\n    }", "        part checker : Checker;\n        connection capLink connect filler.capOut to capper.capIn;\n    }")
        .replace("    satisfy need by Run;", "    allocate Line::capLink to Seal;\n    satisfy need by Run;")
}

// ---- item 6: the walker -------------------------------------------------------------------------------------------------

/// Two group functions, each with a leaf that a part performs and a leaf that another part performs; the parts of the other
/// two leaves are joined by a connection that carries nothing; the first two have no connection.
const GROUPS: &str = "package G {
    item def Msg;
    requirement <'G-1'> need {
        doc /* The system shall pass the message. */
    }
    action def Inner1 { out msg : Msg; }
    action def Inner1b;
    action def Inner2 { in msg : Msg; }
    action def Inner2b;
    action def GroupA {
        out msg : Msg;
        action i1 : Inner1 { out msg :>> Inner1::msg; }
        action i1b : Inner1b;
        flow from i1.msg to msg;
    }
    action def GroupB {
        in msg : Msg;
        action i2 : Inner2 { in msg :>> Inner2::msg; }
        action i2b : Inner2b;
        flow from msg to i2.msg;
    }
    action def Top {
        action ga : GroupA { }
        action gb : GroupB { }
    }
    part def P1;
    part def P2;
    part def P3;
    part def P4;
    part def Sys {
        part p1 : P1;
        part p2 : P2;
        part p3 : P3;
        part p4 : P4;
        connection b34 connect p3 to p4;
    }
    allocate Inner1 to P1;
    allocate Inner2 to P2;
    allocate Inner1b to P3;
    allocate Inner2b to P4;
    satisfy need by Top;
}
";

fn group_edits() -> Vec<Value> {
    vec![
        json!({ "op": "add_element", "owner": "G::Top::ga", "text": "out msg :>> GroupA::msg;" }),
        json!({ "op": "add_element", "owner": "G::Top::gb", "text": "in msg :>> GroupB::msg;" }),
        json!({ "op": "add_element", "owner": "G::Top", "text": "flow from ga.msg to gb.msg;" }),
    ]
}

// ---- item 7: each stated value in at least one verification case ----------------------------------------------------------

const CLAUSES: &str = "package V {
    requirement <'V-1'> limits {
        doc /* The line shall fill within 5 s, reject within 2 s and cap within 7 s. */
        attribute fillTime = 5;
        attribute rejectTime = 2;
        attribute capTime = 7;
    }
    verification verifyFill {
        objective { verify limits; }
        attribute fillTime = 5;
    }
    verification verifyReject {
        objective { verify limits; }
        attribute rejectTime = 2;
    }
}
";

// ---- item 8: smaller points ---------------------------------------------------------------------------------------------

#[test]
fn a_budget_root_may_be_a_bare_name_when_one_part_has_it_and_several_are_listed() {
    let text = "package B {
    private import ISQ::*;
    part def Pump { attribute mass = 4 [kg]; }
    package Other { part def Valve { attribute mass = 1 [kg]; } }
    package Third { part def Valve { attribute mass = 2 [kg]; } }
}
";
    let m = model(&files(text));
    let one = agent::budget::budget(&m, None, Some("Pump"), None).unwrap();
    assert_eq!(one["total"].as_f64(), Some(4.0), "{}", one["says"]);
    let many = agent::budget::budget(&m, None, Some("Valve"), None).unwrap_err().message;
    assert!(many.contains("B::Other::Valve") && many.contains("B::Third::Valve") && many.contains("names 2 parts"), "{many}");
    assert!(agent::budget::budget(&m, None, Some("Nothing"), None).unwrap_err().message.contains("no element named"));
}

// ---- item 9: the document -----------------------------------------------------------------------------------------------
