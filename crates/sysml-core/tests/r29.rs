//! Tools round 29: the guide's fix for a connection end that is not a port keeps a port that is already there, and the ports of
//! the ends of an association are not counted a second time. Every fixture is invented (a level sensor and a radio).

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, connect, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

const UNIT: &str = "package P {
    item def Level;
    port def LevelPort {
        out item level : Level;
    }
    part def Sensor {
        port levelOut : LevelPort;
    }
    part def Radio;
    part def Unit {
        part sensor : Sensor;
        part radio : Radio;
        connection link connect sensor.levelOut to radio;
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


fn gap<'a>(g: &'a Value, rule: &str, element: &str) -> Option<&'a Value> {
    g["gaps"].as_array().unwrap().iter().find(|x| x["rule"] == rule && x["element"]["ref"] == element)
}

fn apply(f: &mut Vec<FileIn>, next: &Value) {
    assert_eq!(next["tool"], "apply_edits", "{next}");
    let edits = next["arguments"]["edits"].as_array().unwrap().clone();
    let b = intent::apply_batch(f, &edits).unwrap_or_else(|(i, e)| panic!("edit {} of {} failed: {}", i + 1, next["arguments"], e.message));
    *f = b.files;
}

#[test]
fn a_lifeline_of_a_sequence_is_not_a_part_with_open_ports() {
    let text = format!("{}\n", UNIT.replace("part def Radio;", "part def Radio;\n    action def Seq {\n        ref part s : Sensor;\n        ref part r : Radio;\n        message m from s to r;\n    }"));
    let m = model(&files(&text));
    let r = connect::find_unconnected_ports(&m, "P").unwrap();
    let ports: Vec<&str> = r["unconnected"].as_array().unwrap().iter().map(|u| u["part"].as_str().unwrap()).collect();
    assert!(ports.iter().all(|p| !p.starts_with("P::Seq")), "{ports:?}");
}
