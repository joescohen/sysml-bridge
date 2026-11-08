//! Tools round 26: the mass budget counts a real model's parts once. The fixtures are invented. Each test fails on the
//! round 25 code, which summed every "top-level part" of the model.

#![allow(dead_code, unused_imports)]

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{self, budget, package};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

fn model(text: &str) -> Model {
    let mut m = api::to_model(vec![FileIn { path: "m.sysml".into(), text: text.into() }]);
    agent::add_namespace_checks(&mut m);
    assert!(!m.has_errors(), "the fixture must parse: {:?}", m.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>());
    m
}

/// Plant holds pump 10, motor 5 and a second usage of the motor definition (5): 20 kg. Around it, things that held it
/// again in the old sum: an association with two ends typed by massed definitions, a logical part typed by the physical
/// definition, a reference usage, and two context blocks that each hold the plant.
fn fixture(limit: Option<&str>) -> String {
    let req = match limit {
        Some(l) => format!("    requirement <'R-1'> cap {{\n        subject s : Plant;\n        attribute maxMass = {l} [kg];\n    }}\n"),
        None => String::new(),
    };
    format!(
        "package P {{
    private import ISQ::*;
    part def Pump {{ attribute mass = 10 [kg]; }}
    part def Motor {{ attribute mass = 5 [kg]; }}
    part def Plant {{
        part pump : Pump;
        part motor : Motor;
        part spareMotor : Motor;
        ref part watcher : Pump;
    }}
    assoc struct Drives {{
        end part a : Pump;
        end part b : Motor;
    }}
    part def LogicalPlant {{
        part drive : Plant;
    }}
    part def CtxA {{
        part sys : Plant;
    }}
    part def CtxB {{
        part sys : Plant;
    }}
{req}}}
"
    )
}

#[test]
fn an_unrooted_budget_does_not_add_the_association_the_logical_part_the_reference_and_the_contexts() {
    let m = model(&fixture(None));
    let b = budget::budget(&m, None, None, None).unwrap();
    // 20 kg, not the 75 kg of the three tops plus the two ends.
    assert_eq!(b["total"].as_f64(), Some(20.0), "{}", b["says"]);
    assert_eq!(b["roots"].as_array().unwrap().len(), 1);
    // The ends of the association and the reference usage are no mass.
    assert!(b["tree"].as_array().unwrap().iter().all(|r| r["name"] != "a" && r["name"] != "b" && r["name"] != "watcher"), "{}", b["text"]);
    // Three roots hold the same plant: they are listed with a total each, not added, and the rule picks one.
    let rc = &b["root_choice"];
    assert_eq!(rc["how"], "most mass-stating parts");
    let c = rc["candidates"].as_array().unwrap();
    assert_eq!(c.len(), 3, "{rc}");
    assert!(c.iter().all(|x| x["total"].as_f64() == Some(20.0)));
    assert_eq!(b["roots"][0]["ref"], "P::CtxA", "ties are broken by name");
    let says = b["says"].as_str().unwrap();
    assert!(says.contains("Pass root to choose another."), "{says}");
    assert!(says.contains("are not added together"), "{says}");
    // The rooted call is the same.
    assert_eq!(budget::budget(&m, None, Some("P::Plant"), None).unwrap()["total"].as_f64(), Some(20.0));
}

#[test]
fn the_part_that_holds_the_limit_is_the_root() {
    let m = model(&fixture(Some("40")));
    let b = budget::budget(&m, None, None, None).unwrap();
    assert_eq!(b["roots"][0]["ref"], "P::Plant", "{}", b["says"]);
    assert_eq!(b["root_choice"]["how"], "holds the limit");
    assert_eq!(b["total"].as_f64(), Some(20.0));
    assert_eq!((b["limit"]["verdict"].as_str(), b["limit"]["margin"].as_f64()), (Some("pass"), Some(20.0)));
}

#[test]
fn a_design_duplicate_is_counted_for_each_usage_and_listed() {
    let m = model(&fixture(Some("40")));
    let b = budget::budget(&m, None, None, None).unwrap();
    // motor + spareMotor are two usages of one definition under one owner: 5 + 5 both count.
    let d = b["counted_more_than_once"].as_array().unwrap();
    assert_eq!(d.len(), 1, "{}", b["counted_more_than_once"]);
    assert!(d[0]["definition"].as_str().unwrap().contains("Motor"));
    let places: Vec<&str> = d[0]["places"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(places, vec!["motor", "spareMotor"]);
    assert!(b["says"].as_str().unwrap().contains("counted more than once"), "{}", b["says"]);
    // A model with no duplicate lists none.
    let one = model(&fixture(Some("40")).replace("        part spareMotor : Motor;\n", ""));
    assert!(budget::budget(&one, None, None, None).unwrap()["counted_more_than_once"].as_array().unwrap().is_empty());
}

#[test]
fn the_workspace_setting_pins_the_root() {
    let mut m = model(&fixture(None));
    m.budget_root = Some("P::CtxB".into());
    let b = budget::budget(&m, None, None, None).unwrap();
    assert_eq!((b["roots"][0]["ref"].as_str(), b["root_choice"]["how"].as_str()), (Some("P::CtxB"), Some("pinned")));
    // A pin that names no part is said and is not used.
    m.budget_root = Some("P::Nothing".into());
    let b = budget::budget(&m, None, None, None).unwrap();
    assert!(b["root_choice"]["why"].as_str().unwrap().contains("names no part"), "{}", b["root_choice"]);
    assert_eq!(b["roots"][0]["ref"], "P::CtxA");
}

// ---- the worksheet opens a state on the chain

#[test]
fn an_attribute_of_the_limit_name_on_a_part_ties_the_roll_up_of_that_part_to_the_requirement() {
    // The requirement is satisfied by nothing and has no subject; the plant states `maxMass` too.
    let text = fixture(None).replace("part def Plant {", "part def Plant {\n        attribute maxMass = 40 [kg];") + "package Reqs {\n    requirement <'R-9'> cap {\n        attribute maxMass = 40 [kg];\n    }\n}\n";
    let m = model(&text);
    let b = budget::budget(&m, None, None, None).unwrap();
    assert_eq!((b["roots"][0]["ref"].as_str(), b["root_choice"]["how"].as_str()), (Some("P::Plant"), Some("holds the limit")), "{}", b["says"]);
    assert_eq!(b["total"].as_f64(), Some(20.0));
    // A part's own `mass` is no tie: with no `maxMass` on a part the rule falls back to the composition roots.
    let loose = model(&(fixture(None) + "package Reqs {\n    requirement <'R-9'> cap {\n        attribute maxMass = 40 [kg];\n    }\n}\n"));
    assert_eq!(budget::budget(&loose, None, None, None).unwrap()["root_choice"]["how"], "most mass-stating parts");
}
