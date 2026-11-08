//! The three-way comparison (agent::merge3): the changes two revisions made to one base, and their conflicts, in the
//! kinds of Cameo's merge mapped to SysML v2 text.

use serde_json::Value;
use sysml_core::{
    agent::merge3,
    api::{self, FileIn},
};

fn check(base: &str, ours: &str, theirs: &str) -> Value {
    let m = |t: &str| api::to_model(vec![FileIn { path: "m.sysml".into(), text: t.into() }]);
    merge3::merge_check(&m(base), &m(ours), &m(theirs))
}

fn kinds(r: &Value) -> Vec<String> {
    r["conflicts"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap().to_string()).collect()
}

const BASE: &str = "package P {
    part def Engine { attribute mass = 10; }
    part def Wheel;
    part def Car { part e : Engine; part w : Wheel; }
}
";

#[test]
fn changes_to_different_elements_do_not_conflict() {
    let ours = BASE.replace("attribute mass = 10;", "attribute mass = 12;");
    let theirs = BASE.replace("part def Wheel;", "part def Wheel { attribute radius; }");
    let r = check(BASE, &ours, &theirs);
    assert_eq!(r["clean"], true, "{r:#}");
    assert_eq!(r["ours"]["changed"], 1);
    assert_eq!(r["theirs"]["added"], 1);
}

#[test]
fn the_same_fact_changed_two_ways_conflicts_and_one_way_is_equivalent() {
    let ours = BASE.replace("attribute mass = 10;", "attribute mass = 12;");
    let theirs = BASE.replace("attribute mass = 10;", "attribute mass = 14;");
    let r = check(BASE, &ours, &theirs);
    assert_eq!(kinds(&r), ["both-changed"], "{r:#}");
    assert_eq!(r["conflicts"][0]["element"], "P::Engine::mass");
    assert_eq!(r["conflicts"][0]["field"], "value");
    let same = check(BASE, &ours, &ours);
    assert_eq!(same["clean"], true, "{same:#}");
    assert_eq!(same["equivalent"][0]["kind"], "same-change");
}

#[test]
fn a_removed_element_that_the_other_side_changed_conflicts() {
    let ours = BASE.replace("    part def Engine { attribute mass = 10; }\n", "").replace("part e : Engine; ", "");
    let theirs = BASE.replace("attribute mass = 10;", "attribute mass = 14; attribute power;");
    let r = check(BASE, &ours, &theirs);
    let k = kinds(&r);
    assert!(k.contains(&"removed-and-changed".to_string()), "{r:#}");
    let c = r["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "removed-and-changed").unwrap();
    assert_eq!(c["element"], "P::Engine");
    assert_eq!(c["removed_by"], "ours");
    assert_eq!(c["changed_count"], 2, "{c:#}");
    // Removed on both sides is the same change.
    let r2 = check(BASE, &ours, &ours);
    assert_eq!(r2["clean"], true, "{r2:#}");
}

#[test]
fn a_new_reference_to_a_removed_element_conflicts() {
    let ours = BASE.replace("    part def Wheel;\n", "").replace(" part w : Wheel;", "");
    let theirs = BASE.replace("part def Car {", "part def Car { part spare : Wheel;");
    let r = check(BASE, &ours, &theirs);
    let c = r["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "reference-to-removed").expect("reference-to-removed");
    assert_eq!(c["element"], "P::Car::spare");
    assert_eq!(c["references"], "P::Wheel");
    assert_eq!(c["side"], "theirs");
}

#[test]
fn specializations_that_cycle_only_together_conflict() {
    let ours = BASE.replace("part def Wheel;", "part def Wheel :> Engine;");
    let theirs = BASE.replace("part def Engine {", "part def Engine :> Wheel {");
    let r = check(BASE, &ours, &theirs);
    assert!(kinds(&r).contains(&"specialization-cycle".to_string()), "{r:#}");
}

#[test]
fn the_same_name_added_on_both_sides() {
    let ours = BASE.replace("part def Wheel;", "part def Wheel; part def Seat;");
    let theirs = BASE.replace("part def Wheel;", "part def Wheel; part def Seat { attribute width; }");
    let r = check(BASE, &ours, &theirs);
    assert_eq!(kinds(&r), ["both-added"], "{r:#}");
    let r2 = check(BASE, &ours, &ours);
    assert_eq!(r2["clean"], true);
    assert_eq!(r2["equivalent"][0]["kind"], "added-on-both");
}
