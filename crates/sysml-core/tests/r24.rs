//! Tools round 24: the chain walker follows every link a model holds.
//!
//! One fixture, a greenhouse, holds each shape the walker did not follow: a requirement joined to its function by a
//! trace link, one that names no function and derives from one that does, a requirement allocated to a part, two
//! functions whose parts sit in two subsystems that a connection joins, a connection allocated to a function, a signal
//! that is an item flow on a connection, a flow that leaves its behavior through a parameter, a group function whose
//! sub-functions have the parts, and a state machine that runs a function and is fired by a signal. Every name here is
//! invented.

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{self, impact, intent, rules, text};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

const HOUSE: &str = "package G {
    package SysMLv1 {
        metadata def Trace;
        metadata def DeriveReqt;
        metadata def ItemFlow;
    }
    item def Moisture;
    item def ValveCmd;
    item def Frame;
    item def Alarm;
    port def BusPort {
        inout item payload;
    }
    requirement <'N-1'> need {
        doc /* The grower needs the beds kept moist. */
    }
    requirement <'S-1'> keepMoist {
        doc /* The house shall measure the moisture of each bed. */
    }
    requirement <'S-2'> closeOnLeak {
        doc /* The house shall close the valve when a bed leaks. */
    }
    requirement <'S-3'> housing {
        doc /* The cabinet shall keep water out. */
    }
    requirement <'S-4'> irrigate {
        doc /* The house shall decide when to water. */
    }
    requirement <'S-5'> openValve {
        doc /* The house shall open the valve on command. */
    }
    requirement <'S-6'> grow {
        doc /* The house shall run the growing cycle. */
        requirement <'S-6.1'> cycle {
            doc /* The cycle shall repeat each hour. */
        }
    }
    action def Sense {
        out reading : Moisture;
    }
    action def Decide {
        in reading : Moisture;
        out cmd : ValveCmd;
    }
    action def Open {
        in cmd : ValveCmd;
    }
    action def Grow {
        out cmd : ValveCmd;
        action sense : Sense {
            out reading :>> Sense::reading;
        }
        action judge : Decide {
            in reading :>> Decide::reading;
            out cmd :>> Decide::cmd;
        }
        first start then sense;
        first sense then judge;
        flow from sense.reading to judge.reading;
        flow from judge.cmd to cmd;
    }
    action def Water {
        in cmd : ValveCmd;
        action open : Open {
            in cmd :>> Open::cmd;
        }
        first start then open;
        flow from cmd to open.cmd;
    }
    action def Run {
        action grow : Grow {
            out cmd :>> Grow::cmd;
        }
        action water : Water {
            in cmd :>> Water::cmd;
        }
        first start then grow;
        first grow then water;
        flow from grow.cmd to water.cmd;
    }
    part def Probe;
    part def Brain;
    part def Valve;
    part def FieldUnit {
        part probe : Probe;
        port bus : BusPort;
        port aux : BusPort;
    }
    part def Cabinet {
        part brain : Brain;
        port bus : ~BusPort;
        port aux : ~BusPort;
    }
    part def Yard {
        part valve : Valve;
        port bus : BusPort;
    }
    part def Greenhouse {
        part field : FieldUnit;
        part cabinet : Cabinet;
        part yard : Yard;
        connection trunk connect field.bus to cabinet.bus;
        connection spare connect field.aux to cabinet.aux;
        connection feed connect cabinet.bus to yard.bus;
    }
    state def Modes {
        state idle;
        state watering {
            do action water {
                action water : Water {
                    in cmd :>> Water::cmd;
                }
            }
        }
        transition first idle accept ValveCmd then watering;
    }
    allocate Sense to Probe;
    allocate Decide to Brain;
    allocate Open to Valve;
    allocate housing to Cabinet;
    allocate Greenhouse::trunk to Decide;
    satisfy irrigate by Decide;
    satisfy openValve by Open;
    satisfy grow by Grow;
    #SysMLv1::Trace dependency from keepMoist to Sense;
    #SysMLv1::DeriveReqt dependency from closeOnLeak to keepMoist;
    #SysMLv1::DeriveReqt dependency from keepMoist to need;
    #SysMLv1::ItemFlow dependency 'moisture frame' from FieldUnit to Cabinet {
        comment conveyed about Frame /* the frame */
        comment realizedBy about Greenhouse::trunk /* the trunk */
    }
    #SysMLv1::ItemFlow dependency 'alarm frame' from FieldUnit to Cabinet {
        comment conveyed about Alarm /* the alarm */
        comment realizedBy about Greenhouse::spare /* the spare */
    }
    #SysMLv1::ItemFlow dependency 'valve frame' from Cabinet to Yard {
        comment conveyed about ValveCmd /* the command */
        comment realizedBy about Greenhouse::feed /* the feed */
    }
}
";

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    let mut m = api::to_model(f.to_vec());
    agent::add_namespace_checks(&mut m);
    m
}

/// HOUSE with `old` replaced by `new`. `old` must be in HOUSE exactly once, so a fixture cannot go stale without notice.
fn swap(old: &str, new: &str) -> Vec<FileIn> {
    assert_eq!(HOUSE.matches(old).count(), 1, "the fixture text '{old}' must be in HOUSE once");
    files(&HOUSE.replace(old, new))
}

fn refs(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().filter_map(|x| x["ref"].as_str()).collect()
}

fn gap_rules(c: &Value) -> Vec<(&str, &str)> {
    c["gaps"].as_array().unwrap().iter().map(|g| (g["hop"].as_str().unwrap(), g["rule"].as_str().unwrap())).collect()
}

fn change(f: &[FileIn], name: &str) -> Value {
    let m = model(f);
    let v = impact::get_impact(&m, &BTreeMap::new(), name, 1, 200).unwrap();
    assert!(v["change"].is_object(), "{name} has a change list");
    v["change"].clone()
}

fn steps_of<'a>(c: &'a Value, step: &str) -> Vec<&'a Value> {
    c["look_at"].as_array().unwrap().iter().filter(|r| r["step"] == step).collect()
}

fn gaps_of<'a>(g: &'a Value, rule: &str) -> Vec<&'a Value> {
    g["gaps"].as_array().unwrap().iter().filter(|x| x["rule"] == rule).collect()
}

/// Apply a `next` call as the server would, with the engineer's choice for each placeholder.
fn apply_next(f: &mut Vec<FileIn>, next: &Value, fill: &[(&str, &str)]) {
    let mut text = serde_json::to_string(&json!({ "tool": next["tool"], "arguments": next["arguments"] })).unwrap();
    let listed: Vec<String> = next["placeholders"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
    for (from, to) in fill {
        assert!(listed.iter().any(|p| p == from), "the call has no placeholder {from}; it lists {listed:?}");
        text = text.replace(from, to);
    }
    for p in &listed {
        assert!(fill.iter().any(|(from, _)| from == p), "placeholder {p} was not filled; the call is {text}");
    }
    let call: Value = serde_json::from_str(&text).unwrap();
    let tool = call["tool"].as_str().unwrap();
    if tool == "apply_edits" {
        let edits = call["arguments"]["edits"].as_array().unwrap().clone();
        let b = intent::apply_batch(f, &edits).unwrap_or_else(|(i, e)| panic!("edit {i} of {text} failed: {}", e.message));
        *f = b.files;
    } else {
        let mut op = call["arguments"].clone();
        op["op"] = json!(tool);
        let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("{text} failed: {}", e.message));
        *f = a.files;
    }
}

#[test]
fn r24_the_fixture_parses_and_resolves() {
    let m = model(&files(HOUSE));
    let bad: Vec<String> = m.diagnostics.iter().filter(|d| matches!(d.severity, sysml_core::Severity::Error)).map(|d| d.message.clone()).collect();
    assert!(bad.is_empty(), "{bad:?}");
}

// ------------------------------------------------------------------ b1: trace and derive

// ------------------------------------------------------------------ b2: a requirement allocated to a part

// ------------------------------------------------------------------ b3: a connection between two subsystems

// ------------------------------------------------------------------ b4: a connection allocated to a function

// ------------------------------------------------------------------ b5: the item flow on a connection

// ------------------------------------------------------------------ b7: parameters of the behavior, and group functions

// ------------------------------------------------------------------ the guide offers what the model lacks

/// The qualified name of the flow statement of HOUSE whose text names `from`.
fn m_flow<'a>(f: &[FileIn], from: &str) -> Option<&'a str> {
    let m = model(f);
    let e = m.elements.iter().find(|e| e.kind == "flow" && e.refs.iter().any(|r| r.text == from)).unwrap();
    Some(Box::leak(e.qualified_name.clone().into_boxed_str()))
}

// ------------------------------------------------------------------ (iii) two behaviors, and (iv) the style of the model

// ------------------------------------------------------------------ (i) states and transitions

// ------------------------------------------------------------------ (ii) requirement to requirement

// ------------------------------------------------------------------ bounded and the same each time
