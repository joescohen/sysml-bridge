//! Security round on tools round 27: the check that holds an agent's proposal must not pass what it did not check, count a
//! debt as done, match two things as one, or take a text for what it does not write. Every fixture is invented (a small
//! bottling line); each test fails on the code of commit 93aeecc.

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

// ---- gates that were not checked ----------------------------------------------------------------------------------------

// ---- gaps and exchanges known by what they are --------------------------------------------------------------------------

/// The line plus a second item called Cap in another package, and a second exchange of it between the filler and the capper.
fn two_caps() -> String {
    LINE.replace("    item def Cap;", "    item def Cap;\n    package Other {\n        item def Cap;\n    }")
        .replace("        out scan : Scan;\n    }\n    action def Seal {\n        in cap : Cap;\n    }", "        out scan : Scan;\n        out other : Other::Cap;\n    }\n    action def Seal {\n        in cap : Cap;\n        in other : Other::Cap;\n    }")
}

// ---- an edit touches what its text names, read as the writer reads it -----------------------------------------------------

// ---- a row left to this proposal and examined ---------------------------------------------------------------------------

// ---- accepted_gaps names a gap by the element it is on, an unnamed statement too ----------------------------------------

// ---- text with line breaks ----------------------------------------------------------------------------------------------

// ---- an owner the walk reached only as owner ----------------------------------------------------------------------------

// ---- the budget in the check --------------------------------------------------------------------------------------------

const STATION: &str = "package P {
    private import SI::*;
    requirement <'R-7'> stationMass {
        doc /* The station shall have a mass of at most 40 kg. */
        attribute maxMass = 40 [kg];
    }
    part def Station {
        part arm : Arm;
        part rack : Rack;
    }
    part def Arm {
        attribute mass = 12 [kg];
    }
    part def Rack {
        attribute mass = 4 [kg];
    }
}
";

// ---- the obligation to take up a deferral is inferred, not chosen by the agent ------------------------------------------

// ---- independent security round on ca6d510 ------------------------------------------------------------------------------
