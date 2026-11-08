//! Tools round 22: what a run on a real imported model objected to, each on a small model made for the fault.
//!
//! One fixture, a depot with an arm and a desk, holds every shape that went wrong: two ports of one port definition with
//! neither conjugated (one signal crosses once in each direction, so two rows of the interface table had one key), a
//! requirement that is named by its id, parts that state no mass, a connection whose port definition names no item
//! beside a flow that needs one, an item whose name has a space, and two signals of one name in two packages.

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use sysml_core::agent::{self, budget, impact, intent, package, text};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

const DEPOT: &str = "package D {
    private import SI::*;
    requirement <'R-4'> swapTime {
        doc /* The depot shall swap a pack within 45 s. */
        attribute maxTime = 45 [s];
    }
    requirement <'R-5'> report {
        doc /* The depot shall report the state of a pack. */
    }
    item def Status {
        attribute code : ScalarValues::Integer;
    }
    item def 'Pack Report';
    port def StatusPort {
        out item status : Status;
    }
    port def BarePort;
    action def Run {
        action def Swap {
            out status : Status;
            out packReport : 'Pack Report';
        }
        action def Show {
            in status : Status;
            in packReport : 'Pack Report';
        }
        action swap : Swap {
            out status :>> Swap::status;
            out packReport :>> Swap::packReport;
        }
        action show : Show {
            in status :>> Show::status;
            in packReport :>> Show::packReport;
        }
        first start then swap;
        first swap then show;
        flow from swap.status to show.status;
        flow from swap.packReport to show.packReport;
    }
    satisfy swapTime by Run::Swap;
    satisfy report by Run::Show;
    part def Depot {
        part def Arm {
            port statusOut : StatusPort;
            port link : BarePort;
        }
        part def Desk {
            port statusIn : StatusPort;
            port link : BarePort;
        }
        part arm : Arm;
        part desk : Desk;
        connection statusLink connect arm.statusOut to desk.statusIn;
        connection bare connect arm.link to desk.link;
    }
    allocate Run::Swap to Depot::Arm;
    allocate Run::Show to Depot::Desk;
    package Spare {
        item def Status;
    }
}
";

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model_of(f: &[FileIn]) -> Model {
    let mut m = api::to_model(f.to_vec());
    agent::add_namespace_checks(&mut m);
    assert!(!m.has_errors(), "the fixture must parse: {:?}", m.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>());
    m
}

fn model(text: &str) -> Model {
    model_of(&files(text))
}

/// A fixture with `old` replaced by `new`; `old` must be there once.
fn swap(base: &str, old: &str, new: &str) -> String {
    assert_eq!(base.matches(old).count(), 1, "the fixture must hold `{old}` once");
    base.replace(old, new)
}

fn info() -> Value {
    json!({ "revision": "aaaaaaaaaaaaaaaa", "is": "baseline B1", "baseline": "B1" })
}

fn meta() -> Value {
    json!({ "title": "CR-1", "from": { "revision": "a", "is": "current" }, "to": { "revision": "b", "is": "current" } })
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("not a count: {v}"))
}

// ---------------------------------------------------------------- A1: a table against itself has no change

// ---------------------------------------------------------------- A2: one resolver, and an id is a reference

// ---------------------------------------------------------------- A3: no stated mass is not 0 kg

// ---------------------------------------------------------------- A4: complete the connection that is there

fn connections(m: &Model) -> usize {
    m.elements.iter().filter(|e| e.kind == "connection" && !e.is_def).count()
}

// ---------------------------------------------------------------- B6, B9, B10: the change package reads short and honest

fn changed() -> String {
    swap(&swap(&swap(DEPOT, "within 45 s.", "within 30 s."), "attribute maxTime = 45 [s];", "attribute maxTime = 30 [s];"), "attribute code : ScalarValues::Integer;", "attribute code : ScalarValues::Real;")
}

// ---------------------------------------------------------------- B7: a thin table says that it is thin

// ---------------------------------------------------------------- B8: every gap names its source

