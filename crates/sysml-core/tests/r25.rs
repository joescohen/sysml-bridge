//! Tools round 25: the change assessment.
//!
//! One fixture, a pottery kiln, holds an element of each of the eight aspects in the chain of one requirement: a need
//! it derives from and a requirement that derives from it, three functions in one behavior, a state machine that runs
//! one of them, two subsystems with a connection between them, the signals that flow, values in attributes and in the
//! text of the requirement, and a verification case. Every name here is invented.

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{self, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

const KILN: &str = include_str!("r25_kiln.sysml");

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "k.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    let mut m = api::to_model(f.to_vec());
    agent::add_namespace_checks(&mut m);
    m
}

fn swap(old: &str, new: &str) -> Vec<FileIn> {
    assert_eq!(KILN.matches(old).count(), 1, "the fixture text '{old}' must be in KILN once");
    files(&KILN.replace(old, new))
}

fn row<'a>(w: &'a Value, r: &str) -> &'a Value {
    w["rows"].as_array().unwrap().iter().find(|x| x["ref"] == r).unwrap_or_else(|| panic!("no row {r} in {}", w["text"].as_str().unwrap_or("")))
}

fn kinds(a: &Value) -> Vec<&str> {
    a["check"]["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect()
}

fn says(a: &Value, kind: &str) -> String {
    a["check"]["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == kind).map(|f| f["says"].as_str().unwrap()).collect::<Vec<_>>().join(" || ")
}

/// The change of the first scenario: the peak temperature rises. Two edits, two rows modified.
fn hotter() -> (Vec<Value>, Vec<Value>) {
    let edits = vec![
        json!({ "op": "update_element", "ref": "K::fire", "doc": "The kiln shall reach 1,250 °C (T) within 90 min and hold it for 15 min (O)." }),
        json!({ "op": "update_element", "ref": "K::fire::peak", "value": "1250" }),
    ];
    let given = vec![
        json!({ "ref": "K::fire", "disposition": "modify", "reason": "the text states the new peak", "edits": [1] }),
        json!({ "ref": "K::fire::peak", "disposition": "modify", "reason": "the attribute holds the peak", "edits": [2] }),
    ];
    (edits, given)
}

fn change() -> Value {
    json!({ "before": "reach 1,200 °C", "after": "reach 1,250 °C", "rationale": "the new glaze needs a hotter firing", "source": "letter of the potter, 3 May" })
}

/// The change of the second scenario: the door locks while the kiln is hot. A new function in a second behavior, fed
/// by a new function of a part in another subsystem, with no connection between the two subsystems.
fn lock_edits() -> Vec<Value> {
    vec![
        json!({ "op": "add_element", "owner": "K", "text": "action def Raise { out halt : Halt; }" }),
        json!({ "op": "add_element", "owner": "K", "text": "action def Lock { in halt : Halt; }" }),
        json!({ "op": "add_element", "owner": "K::Shutdown", "text": "action raise : Raise { out halt :>> Raise::halt; }" }),
        json!({ "op": "add_element", "owner": "K::Shutdown", "text": "action lock : Lock { in halt :>> Lock::halt; }" }),
        json!({ "op": "add_element", "owner": "K::Shutdown", "text": "first stop then raise;" }),
        json!({ "op": "add_element", "owner": "K::Shutdown", "text": "first raise then lock;" }),
        json!({ "op": "add_element", "owner": "K::Shutdown", "text": "flow from raise.halt to lock.halt;" }),
        json!({ "op": "create_relationship", "kind": "allocate", "source": "K::Raise", "target": "K::Brain", "allow_definitions": true }),
        json!({ "op": "create_relationship", "kind": "allocate", "source": "K::Lock", "target": "K::Guard", "allow_definitions": true }),
        json!({ "op": "create_relationship", "kind": "satisfy", "source": "K::Lock", "target": "K::fire", "allow_definitions": true }),
        json!({ "op": "update_element", "ref": "K::fire", "doc": "The kiln shall reach 1,200 °C (T) within 90 min, hold it for 15 min (O) and lock the door while it is hot." }),
    ]
}

fn lock_new() -> Vec<Value> {
    vec![
        json!({ "ref": "K::Raise", "aspect": "function", "reason": "the clause 'lock the door while it is hot' needs a function that says the kiln is hot", "because_of": ["K::Judge"], "edits": [1] }),
        json!({ "ref": "K::Lock", "aspect": "function", "reason": "the clause 'lock the door' is a new function", "because_of": ["K::fire"], "edits": [2] }),
        json!({ "ref": "K::Shutdown::raise", "aspect": "function", "reason": "the step that raises the halt", "because_of": ["K::Shutdown"], "attaches": { "text": "after the vent opens" }, "edits": [3] }),
        json!({ "ref": "K::Shutdown::lock", "aspect": "function", "reason": "the step that locks the door", "because_of": ["K::Shutdown", "K::Raise"], "edits": [4] }),
    ]
}

// ---- item 1: the worksheet -------------------------------------------------------------------------------------------

/// `n` requirements, each derived from the one before; the last derives from the first when `ring`.
fn derive_chain(n: usize, ring: bool) -> Vec<FileIn> {
    let mut t = String::from("package D {\n    package SysMLv1 {\n        metadata def DeriveReqt;\n    }\n");
    for i in 0..n {
        t.push_str(&format!("    requirement r{i} {{\n        doc /* Step {i} shall hold. */\n    }}\n"));
    }
    for i in 1..n {
        t.push_str(&format!("    #SysMLv1::DeriveReqt dependency from r{i} to r{};\n", i - 1));
    }
    if ring {
        t.push_str(&format!("    #SysMLv1::DeriveReqt dependency from r0 to r{};\n", n - 1));
    }
    t.push_str("}\n");
    vec![FileIn { path: "d.sysml".into(), text: t }]
}

// ---- addition H: the reach table ---------------------------------------------------------------------------------------

// ---- item 2: the check of an assessment --------------------------------------------------------------------------------

fn set(a: &mut Value, r: &str, f: impl Fn(&mut Value)) {
    let d = a["dispositions"].as_array_mut().unwrap().iter_mut().find(|d| d["ref"] == r).unwrap_or_else(|| panic!("no disposition of {r}"));
    f(d);
}

// ---- the weave ---------------------------------------------------------------------------------------------------------

fn lock_accepted() -> Value {
    json!([
        { "rule": "interface-needed", "ref": "K::Halt", "reason": "the door has no port yet: the interface is its own proposal", "decided_in": "P-002" },
        { "rule": "trace-operand-definition", "ref": "K::Brain", "reason": "this model allocates between definitions" },
        { "rule": "trace-operand-definition", "ref": "K::Guard", "reason": "this model allocates between definitions" },
        { "rule": "trace-operand-definition", "ref": "K::fire", "reason": "this model satisfies a requirement with a definition" },
    ])
}

// ---- additions E and F: the interface in the pattern of the model, in a second proposal ---------------------------------

fn latch_edits() -> Vec<Value> {
    vec![
        json!({ "op": "add_element", "owner": "K::Cabinet", "text": "port door : Wire;" }),
        json!({ "op": "add_element", "owner": "K::Door", "text": "port bus : ~Wire;" }),
        json!({ "op": "add_element", "owner": "K::Plant", "text": "connection latch connect cabinet.door to door.bus;" }),
        json!({ "op": "add_element", "owner": "K", "text": "#SysMLv1::ItemFlow dependency 'halt frame' from Cabinet to Door {\n    comment conveyed about Halt /* the halt */\n    comment realizedBy about Plant::latch /* the latch */\n}" }),
        json!({ "op": "create_relationship", "kind": "allocate", "source": "K::Plant::latch", "target": "K::Lock", "allow_definitions": true }),
    ]
}

fn latch_new() -> Value {
    json!([
        { "ref": "K::Cabinet::door", "aspect": "interface", "reason": "the cabinet side of the halt", "because_of": ["K::Shutdown::raise"] },
        { "ref": "K::Door::bus", "aspect": "interface", "reason": "the door side of the halt", "because_of": ["K::Shutdown::lock"] },
        { "ref": "K::Plant::latch", "aspect": "interface", "reason": "the connection that carries the halt", "because_of": ["K::Shutdown::lock"] },
        { "ref": "K::'halt frame'", "aspect": "interface", "reason": "the halt on the connection", "because_of": ["K::Halt"] },
    ])
}

// ---- addition F: the decisions of a person ------------------------------------------------------------------------------

// ---- addition D: twins --------------------------------------------------------------------------------------------------

const PUMPS: &str = "package T {
    package Logical {
        part def Pump {
            attribute rate = 3;
        }
        item def Tick;
    }
    package Physical {
        part def Pump {
            attribute rate = 3;
        }
        item def Tick;
        part def Skid {
            part pump : Pump;
        }
    }
    requirement <'T-1'> move {
        doc /* The pump shall move 3 l/min. */
    }
    action def Push {
        out tick : Logical::Tick;
    }
    allocate Push to Logical::Pump;
    satisfy move by Push;
}
";

// ---- item 3: the document -----------------------------------------------------------------------------------------------

// ---- a connection written without its keyword gets a name ----------------------------------------------------------------

#[test]
fn a_connection_written_as_a_bare_connect_statement_can_be_named() {
    let f = files("package B {\n    port def Plug;\n    part def Lamp {\n        port p : Plug;\n    }\n    part def Socket {\n        port p : ~Plug;\n    }\n    part def Room {\n        part lamp : Lamp;\n        part socket : Socket;\n        connect lamp.p to socket.p;\n    }\n}\n");
    let m = model(&f);
    let conn = m.elements.iter().find(|e| e.kind == "connection" && !e.is_def).expect("the bare statement is a connection").qualified_name.clone();
    assert!(conn.starts_with("B::Room::$"), "{conn}");
    let b = intent::apply_batch(&f, &[json!({ "op": "update_element", "ref": conn, "rename": "cord" })]).unwrap_or_else(|(_, e)| panic!("{}", e.message));
    assert!(b.files[0].text.contains("        connection cord connect lamp.p to socket.p;\n"), "{}", b.files[0].text);
    assert_eq!(b.results[0]["ref"], "B::Room::cord");
    common::assert_official(&b.files, "the named connection");
    let after = model(&b.files);
    let e = after.find("B::Room::cord").unwrap();
    assert_eq!(e.kind, "connection");
    assert_eq!(e.refs.iter().filter(|r| r.resolved().is_some()).count(), 2, "both ends still resolve");
    // A connection that has its keyword and no name is named as before.
    let f = files(&f[0].text.replace("connect lamp.p to socket.p;", "connection connect lamp.p to socket.p;"));
    let conn = model(&f).elements.iter().find(|e| e.kind == "connection" && !e.is_def).unwrap().qualified_name.clone();
    let b = intent::apply_batch(&f, &[json!({ "op": "update_element", "ref": conn, "rename": "cord" })]).unwrap_or_else(|(_, e)| panic!("{}", e.message));
    assert!(b.files[0].text.contains("connection cord connect lamp.p to socket.p;"), "{}", b.files[0].text);
}

// ---- addition C: states and transitions are hops ---------------------------------------------------------------------------
