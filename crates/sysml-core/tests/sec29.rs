//! Security round 29 on the proposal check (after commit ad9365f): a verdict "modify" or "replace" on a usage stands only on a
//! change the model with the proposal holds at the row, or on a change of its definition that the usage gains. Every fixture is
//! invented (the small bottling line of sec28).

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


const SEAL: &str = "Q::Run::seal";
