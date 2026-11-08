//! Tools round 25: the bounds of the chain walker hold before the work, and every recursion ends.
//!
//! A review of round 24 named three faults in `agent/guide.rs`. Each test here builds a synthetic model (every name is
//! invented) that, without the fix, runs for minutes or overflows the stack:
//!
//! 1. a limit applied after the work it should bound: the search of a flow through parameters was limited by what it
//!    found, so parameters wired to each other with no function behind them were searched without end; and a requirement
//!    that many others derive from was read once for each of them;
//! 2. a recursion with no bound on its depth, and a search with no visited set: a long chain of group functions
//!    overflowed the stack, and two subsystems that hold each other were searched once for each path;
//! 3. the same family: the connections that join two parts were looked up again for every flow between them.

#![allow(dead_code)]

mod common;

use serde_json::Value;
use std::time::{Duration, Instant};
use sysml_core::agent::{self};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

/// The time a call on one of these models may take. With the fix each takes well under a second; without it, minutes.
const BUDGET: Duration = Duration::from_secs(20);

fn model(text: String) -> Model {
    let mut m = api::to_model(vec![FileIn { path: "m.sysml".into(), text }]);
    agent::add_namespace_checks(&mut m);
    m
}

fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let v = f();
    assert!(t.elapsed() < BUDGET, "{what} took {:?}, over the budget of {BUDGET:?}", t.elapsed());
    v
}

/// Two subsystems that hold each other (`Hall` holds 150 rooms, each `Room` holds 150 halls), under a site that also
/// holds two parts six levels down, in two branches, whose functions exchange a signal.
fn site(m_usages: usize) -> String {
    let mut t = String::from("package Site {\n    item def Note;\n    requirement <'B-1'> pass {\n        doc /* The site shall pass a note. */\n    }\n    action def Send {\n        out note : Note;\n    }\n    action def Take {\n        in note : Note;\n    }\n    action def Pass {\n        action hand : Send {\n            out note :>> Send::note;\n        }\n        action grab : Take {\n            in note :>> Take::note;\n        }\n        first start then hand;\n        first hand then grab;\n        flow from hand.note to grab.note;\n    }\n    part def Desk;\n    part def Shelf;\n");
    t.push_str("    part def Hall {\n");
    for i in 0..m_usages {
        t.push_str(&format!("        part room{i} : Room;\n"));
    }
    t.push_str("    }\n    part def Room {\n");
    for i in 0..m_usages {
        t.push_str(&format!("        part hall{i} : Hall;\n"));
    }
    t.push_str("    }\n");
    for (branch, leaf, kind) in [("West", "desk", "Desk"), ("East", "shelf", "Shelf")] {
        t.push_str(&format!("    part def {branch}5 {{\n        part {leaf} : {kind};\n    }}\n"));
        for i in (1..5).rev() {
            t.push_str(&format!("    part def {branch}{i} {{\n        part down : {branch}{};\n    }}\n", i + 1));
        }
    }
    t.push_str("    part def Grounds {\n");
    for i in 0..m_usages {
        t.push_str(&format!("        part wing{i} : Hall;\n"));
    }
    t.push_str("        part west : West1;\n        part east : East1;\n    }\n    allocate Send to West5::desk;\n    allocate Take to East5::shelf;\n    satisfy pass by Pass;\n}\n");
    t
}

fn rules_of(v: &Value) -> Vec<&str> {
    v["gaps"].as_array().unwrap().iter().filter_map(|g| g["rule"].as_str()).collect()
}
