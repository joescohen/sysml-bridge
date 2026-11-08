//! Tools round 18: the fourth security review. Each test fails without its fix.

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use sysml_core::agent::rules::{self, OfficialVerdict};
use sysml_core::agent::{self, intent, trace};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    let mut m = api::to_model(f.to_vec());
    agent::add_namespace_checks(&mut m);
    m
}

fn sel<'a>() -> rules::Selection<'a> {
    rules::Selection { gates: None, rules: None, groups: None, require: None }
}

fn gate(f: &[FileIn], g: &str) -> Value {
    trace::validate_model_sel(&model(f), Some(g), &sel()).unwrap()
}

fn apply(f: &mut Vec<FileIn>, op: Value) -> Value {
    let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    *f = a.files;
    a.info
}

fn refused(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(a) => panic!("op {op} should have failed; it wrote:\n{}", a.files[0].text),
        Err(e) => e.message,
    }
}

/// Every reference of the model: (holder, text, what it resolves to).
fn targets(f: &[FileIn]) -> Vec<(String, String, Option<String>)> {
    let m = model(f);
    let mut v: Vec<(String, String, Option<String>)> = m.elements.iter().flat_map(|e| e.refs.iter().map(|r| (e.qualified_name.clone(), r.text.clone(), r.resolved().map(|t| m.elements[t].qualified_name.clone())))).collect();
    v.sort();
    v
}

fn target_of(f: &[FileIn], holder: &str, text: &str) -> Option<String> {
    targets(f).into_iter().find(|(h, t, _)| h == holder && t == text).unwrap_or_else(|| panic!("no reference '{text}' in {holder}")).2
}

// ------------------------------------------------------------------ item 2: name capture, every reference compared

const CAP2: &str = "package P {\n    part def Engine {\n        attribute mass = 1;\n    }\n    part def Base {\n        part bs;\n    }\n    part def Base2 {\n        part s;\n        part def Engine;\n    }\n    requirement def RD;\n    requirement r : RD;\n    part s;\n    part def Other;\n    part def Car {\n        part engine : Engine;\n        attribute x = 1;\n        satisfy r by s;\n        part def Inner {\n            part e2 : Engine;\n        }\n    }\n    part def Truck :> Base {\n        part t1 : Engine;\n        satisfy r by s;\n    }\n    package Lib {\n        part def Engine;\n        part s;\n    }\n}\n";

/// N2. A batch that deletes one `s` and creates another one nearer to a reference: over the whole batch the count of
/// the name `s` does not change, so the old check saw nothing. The wave is compared by itself now, reference by reference.
#[test]
fn r18_n2_a_delete_in_a_batch_does_not_hide_a_create_of_the_same_name() {
    let text = "package P {\n    part def Engine;\n    part s2;\n    part s;\n    requirement r;\n    part def Car {\n        part s2;\n        part def Inner {\n            part e2 : Engine;\n            satisfy r by s2;\n            satisfy r by s;\n        }\n    }\n    package Lib {\n        part s;\n        part def Engine;\n        part zz;\n    }\n}\n";
    let f = files(text);
    let before = targets(&f);
    assert_eq!(target_of(&f, "P::Car::Inner::$3", "s"), Some("P::s".into()));
    let fill: Vec<Value> = (0..7).map(|i| json!({"op": "create_element", "owner": "P::Lib", "kind": "part", "name": format!("f{i}")})).collect();
    for (first, second, at) in [
        (json!({"op": "delete_element", "ref": "P::Lib::s"}), json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "s"}), 1),
        (json!({"op": "delete", "target": "P::Lib::s"}), json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "s"}), 1),
        (json!({"op": "delete_element", "ref": "P::Lib::Engine"}), json!({"op": "create_element", "owner": "P::Car", "kind": "part def", "name": "Engine"}), 1),
    ] {
        let mut edits = vec![first.clone(), second.clone()];
        edits.extend(fill.iter().cloned());
        let (i, fail) = intent::apply_batch(&f, &edits).err().unwrap_or_else(|| panic!("the batch with {first} and {second} must fail"));
        assert_eq!(i, at, "{second}");
        assert!(fail.message.contains("would change what") && fail.message.contains("would name P::Car::"), "{}", fail.message);
        // The create last, after the fill: the same.
        let mut edits = fill.clone();
        edits.push(second.clone());
        edits.push(first.clone());
        let (i, fail) = intent::apply_batch(&f, &edits).err().expect("must fail");
        assert_eq!(i, 7);
        assert!(fail.message.contains("would change what"), "{}", fail.message);
    }
    assert_eq!(targets(&f), before);
    // The same batch without the capturing create goes through, as a wave.
    let mut edits = vec![json!({"op": "delete_element", "ref": "P::Lib::s"})];
    edits.extend(fill.iter().cloned());
    assert!(intent::apply_batch(&f, &edits).is_ok());
}

/// N3. A rename rewrites the text of every reference to the renamed element. When the new name is one that a nearer
/// element has, the rewritten reference names that element. The old check looked a reference up by its text, so it
/// never compared a rewritten one.
#[test]
fn r18_n3_a_rename_onto_a_name_that_a_nearer_element_has_is_refused() {
    let f = files(CAP2);
    let before = targets(&f);
    for (op, want) in [
        (json!({"op": "update_element", "ref": "P::Engine", "rename": "Inner"}), "named P::Inner and would name P::Car::Inner"),
        (json!({"op": "rename", "target": "P::Engine", "new_name": "Inner"}), "named P::Inner and would name P::Car::Inner"),
        (json!({"op": "update_element", "ref": "P::s", "rename": "engine"}), "named P::engine and would name P::Car::engine"),
        (json!({"op": "update_element", "ref": "P::s", "rename": "bs"}), "named P::bs and would name P::Base::bs"),
    ] {
        let msg = refused(&f, op.clone());
        assert!(msg.starts_with("refused: this edit would change what") && msg.contains(want), "{op}: {msg}");
        let (at, fail) = intent::apply_batch(&f, &[json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Free"}), op.clone()]).err().expect("the batch must fail");
        assert_eq!(at, 1);
        assert!(fail.message.contains(want), "{op}: {}", fail.message);
    }
    assert_eq!(targets(&f), before);
    // A rename onto a free name takes every reference along, in every scope.
    let mut g = files(CAP2);
    apply(&mut g, json!({"op": "update_element", "ref": "P::Engine", "rename": "Motor"}));
    for holder in ["P::Car::engine", "P::Car::Inner::e2", "P::Truck::t1"] {
        assert_eq!(target_of(&g, holder, "Motor"), Some("P::Motor".into()), "{holder}");
    }
    apply(&mut g, json!({"op": "update_element", "ref": "P::s", "rename": "station"}));
    assert_eq!(target_of(&g, "P::Car::$3", "station"), Some("P::station".into()));
}

/// N4. A move takes the references the moved element holds to another place, where their names may mean other elements.
/// The old check compared only references that use the name of the moved element.
#[test]
fn r18_n4_a_move_that_changes_what_the_moved_elements_own_references_mean_is_refused() {
    let text = "package P {\n    part def Engine;\n    part s;\n    requirement r;\n    part def Car {\n        part w;\n    }\n    package Lib {\n        part def Engine;\n        part s;\n        part def User {\n            part u : Engine;\n            satisfy r by s;\n        }\n        part lone : Engine;\n    }\n    package Other {\n    }\n}\n";
    let f = files(text);
    let before = targets(&f);
    assert_eq!(target_of(&f, "P::Lib::User::u", "Engine"), Some("P::Lib::Engine".into()));
    for (op, want) in [
        (json!({"op": "update_element", "ref": "P::Lib::User", "owner": "P"}), "'Engine' in P::User::u named P::Lib::Engine and would name P::Engine"),
        (json!({"op": "update_element", "ref": "P::Lib::User", "owner": "P::Car"}), "'s' in P::Car::User::$2 named P::Lib::s and would name P::s"),
        (json!({"op": "update_element", "ref": "P::Lib::lone", "owner": "P::Car"}), "'Engine' in P::Car::lone named P::Lib::Engine and would name P::Engine"),
    ] {
        let msg = refused(&f, op.clone());
        assert!(msg.contains(want), "{op}: {msg}");
    }
    assert_eq!(targets(&f), before);
    // A move to a place where every name means the same element is written.
    let mut g = files(text);
    apply(&mut g, json!({"op": "update_element", "ref": "P::Car::w", "owner": "P::Lib"}));
    apply(&mut g, json!({"op": "update_element", "ref": "P::Lib::lone", "owner": "P::Lib::User"}));
    assert_eq!(target_of(&g, "P::Lib::User::lone", "Engine"), Some("P::Lib::Engine".into()));
}

/// N5. A redefinition without a name is found under the name of the feature it redefines. Inside a definition that
/// inherits that feature this is what a redefinition is for: the reference follows it. Inside one that does not, the
/// name was an outer element's, and the edit is refused.
#[test]
fn r18_n5_a_redefinition_without_a_name_may_not_take_an_outer_name() {
    let text = "package P {\n    part bs;\n    requirement r;\n    part def Base {\n        part bs;\n    }\n    part def Truck :> Base {\n        satisfy r by bs;\n    }\n    part def Van {\n        satisfy r by bs;\n    }\n}\n";
    let f = files(text);
    assert_eq!(target_of(&f, "P::Truck::$1", "bs"), Some("P::Base::bs".into()));
    assert_eq!(target_of(&f, "P::Van::$1", "bs"), Some("P::bs".into()));
    // In Van the name `bs` is the outer part: a member that takes the name is a capture, named or not.
    for op in [
        json!({"op": "create_element", "owner": "P::Van", "kind": "part", "unnamed": true, "redefines": ["P::Base::bs"]}),
        json!({"op": "add_element", "owner": "P::Van", "text": "part :>> Base::bs;"}),
    ] {
        match intent::apply_op(&f, &op) {
            // Refused: as a capture, or because Van does not inherit `bs`.
            Err(e) => assert!(e.message.contains("named P::bs and would name P::Van::$") || e.message.contains("does not specialize"), "{op}: {}", e.message),
            Ok(a) => assert_eq!(target_of(&a.files, "P::Van::$1", "bs"), Some("P::bs".into()), "{op} re-pointed the reference"),
        }
    }
    // In Truck the reference named the inherited feature; it follows the redefinition, which stands for that feature.
    let mut g = files(text);
    apply(&mut g, json!({"op": "create_element", "owner": "P::Truck", "kind": "part", "unnamed": true, "redefines": ["P::Base::bs"]}));
    let now = target_of(&g, "P::Truck::$1", "bs").unwrap();
    assert!(now.starts_with("P::Truck::$"), "{now}");
}

/// N6. KerML 8.2.3.5.3 and 8.2.3.5.4: the names of a namespace are those it owns, imports and inherits, and a name is
/// looked up in the nearest namespace first. So a new general type brings its members in between a definition's own
/// names and the outer names. Every way to add one is compared reference by reference.
#[test]
fn r18_n6_a_specialization_that_brings_in_a_nearer_name_is_refused() {
    let f = files(CAP2);
    let before = targets(&f);
    assert_eq!(target_of(&f, "P::Car::$3", "s"), Some("P::s".into()));
    for op in [
        json!({"op": "create_relationship", "kind": "specialize", "source": "P::Car", "target": "P::Base2"}),
        json!({"op": "add_specialization", "target": "P::Car", "general": "P::Base2"}),
        json!({"op": "update_element", "ref": "P::Car", "specializes": ["P::Base2"]}),
        json!({"op": "replace", "target": "P::Truck", "text": "part def Truck :> Base, Base2 {\n        part t1 : Engine;\n        satisfy r by s;\n    }"}),
        // A member that a definition below inherits.
        json!({"op": "create_element", "owner": "P::Base", "kind": "part", "name": "s"}),
        json!({"op": "update_element", "ref": "P::Base::bs", "rename": "s"}),
    ] {
        let msg = refused(&f, op.clone());
        assert!(msg.starts_with("refused: this edit would change what") && (msg.contains("would name P::Base2::") || msg.contains("would name P::Base::s")), "{op}: {msg}");
    }
    assert_eq!(targets(&f), before);
    // A general type whose members no reference below uses is written.
    let mut g = files(CAP2);
    apply(&mut g, json!({"op": "create_relationship", "kind": "specialize", "source": "P::Car", "target": "P::Base"}));
    assert_eq!(target_of(&g, "P::Car::$3", "s"), Some("P::s".into()));
    // And an inherited name wins over an outer one, as the clause says.
    let h = files("package P {\n    part bs;\n    requirement r;\n    part def Base {\n        part bs;\n    }\n    part def Truck :> Base {\n        satisfy r by bs;\n    }\n}\n");
    assert_eq!(target_of(&h, "P::Truck::$1", "bs"), Some("P::Base::bs".into()));
}

/// N7. Two wildcard imports that give the same name: the first one written wins. Swapping them re-points the reference.
/// The old check compared the imports of a namespace as a set.
#[test]
fn r18_n7_a_reorder_that_swaps_two_imports_is_refused_when_a_reference_moves() {
    let text = "package P {\n    package LibA {\n        part def Engine;\n    }\n    package LibB {\n        part def Engine;\n        part def Wheel;\n    }\n    part def Car {\n        private import LibA::*;\n        private import LibB::*;\n        part engine : Engine;\n    }\n    part def Van {\n        private import LibA::*;\n        private import LibB::*;\n        part wheel : Wheel;\n    }\n}\n";
    let f = files(text);
    assert_eq!(target_of(&f, "P::Car::engine", "Engine"), Some("P::LibA::Engine".into()));
    let msg = refused(&f, json!({"op": "reorder_members", "owner": "P::Car", "order": ["P::Car::$2", "P::Car::$1", "P::Car::engine"]}));
    assert!(msg.contains("'Engine' in P::Car::engine named P::LibA::Engine and would name P::LibB::Engine"), "{msg}");
    // Where no reference depends on the order, the same reorder is written.
    let mut g = files(text);
    apply(&mut g, json!({"op": "reorder_members", "owner": "P::Van", "order": ["P::Van::$2", "P::Van::$1", "P::Van::wheel"]}));
    assert_eq!(target_of(&g, "P::Van::wheel", "Wheel"), Some("P::LibB::Wheel".into()));
}

/// What the complete comparison must still let through: the changes an edit asks for.
#[test]
fn r18_edits_that_change_only_what_they_ask_for_are_written() {
    // Another type for a feature changes what a chain through it reaches.
    let mut g = files("package P {\n    part def A { attribute m = 1; }\n    part def B { attribute m = 2; }\n    part def Car {\n        part e : A;\n        attribute x = e.m;\n    }\n}\n");
    apply(&mut g, json!({"op": "update_element", "ref": "P::Car::e", "typed_by": "P::B"}));
    assert_eq!(target_of(&g, "P::Car::x", "e.m"), Some("P::B::m".into()));
    // A moved element keeps the references to it and to its members, however they are written.
    let mut h = files("package P {\n    package Lib {\n        part def User {\n            part u;\n        }\n    }\n    part def Car {\n        part a : User;\n        part b :> a.u;\n    }\n    part c : User;\n    part def User;\n    package Q {\n    }\n}\n");
    apply(&mut h, json!({"op": "update_element", "ref": "P::Lib::User", "owner": "P::Q"}));
    assert_eq!(target_of(&h, "P::c", "User"), Some("P::User".into()));
    // A reference that waited for its element resolves; a reference whose element is deleted with it is gone.
    let mut k = files("package P {\n    part def Car {\n        part e : Later;\n        part f : Gone;\n    }\n    part def Gone;\n}\n");
    apply(&mut k, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Later"}));
    apply(&mut k, json!({"op": "delete_element", "ref": "P::Gone", "cascade": true}));
    // A named redefinition and a new member with a name of its own change no other reference.
    let mut r = files("package P {\n    part def Base {\n        part bs;\n    }\n    part def Truck :> Base {\n        part x :> bs;\n    }\n}\n");
    apply(&mut r, json!({"op": "create_element", "owner": "P::Truck", "kind": "part", "name": "bs2", "redefines": ["P::Base::bs"]}));
    apply(&mut r, json!({"op": "create_element", "owner": "P::Truck", "kind": "part", "name": "y"}));
    assert_eq!(target_of(&r, "P::Truck::x", "bs"), Some("P::Base::bs".into()));
}

// ------------------------------------------------------------------ item 3: multiplicity text is parsed

/// N8. The text of a multiplicity was filtered by character, so `1 2`, `x y`, `def` and `1.5.2` were written between
/// the brackets. It is parsed now: one bound, or two with `..`; a bound is a whole number, a name, or `*` for the upper.
#[test]
fn r18_n8_a_multiplicity_is_parsed() {
    let text = "package P {\n    attribute n = 4;\n    package Q { attribute k = 2; }\n    part def Car {\n        part engine;\n    }\n}\n";
    for bad in ["1 2", "x y", "def", "part def X", "x.y", "a,b", "1.5.2", "1.5", "0x1F", "not", "*..1", "1..", "..2", "1..2..3", "1e3", "n::", "[1] [2]", "1]; part z; part q[1", "a b", "\"s\"", "1 /* c */"] {
        for op in [
            json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "k", "multiplicity": bad}),
            json!({"op": "update_element", "ref": "P::Car::engine", "multiplicity": bad}),
        ] {
            let msg = refused(&files(text), op.clone());
            assert!(msg.contains("multiplicity") && msg.contains("is not valid"), "{op}: {msg}");
        }
    }
    for (good, written) in [("1", "[1]"), ("0..*", "[0..*]"), ("*", "[*]"), ("1..n", "[1..n]"), ("n", "[n]"), ("[2]", "[2]"), (" 0 .. 1 ", "[0..1]"), ("Q::k", "[Q::k]"), ("0..Q::k", "[0..Q::k]"), ("'a b'", "['a b']")] {
        let mut f = files(text);
        apply(&mut f, json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "k", "multiplicity": good}));
        assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains(&format!("part k{written};")) || sysml_core::agent::mint::strip_ids(&f[0].text).contains(&format!("part k {written};")), "{good}:\n{}", f[0].text);
        let v = gate(&f, "official");
        assert!(v["findings"].as_array().unwrap().is_empty(), "{good}: {v}");
        apply(&mut f, json!({"op": "update_element", "ref": "P::Car::engine", "multiplicity": good}));
        assert!(gate(&f, "syntax")["passed"] == true && gate(&f, "official")["findings"].as_array().unwrap().is_empty(), "{good}:\n{}", f[0].text);
    }
}

// ------------------------------------------------------------------ item 4: limits that hid a part of the model

// ------------------------------------------------------------------ item 6: tool gaps found while building the Cameo probes

const GAPS: &str = "package P {\n    part def Sys;\n    attribute def Real;\n    state def Modes;\n    allocation def A {\n        end part l;\n        end part p;\n    }\n    part lg;\n    part ph;\n    requirement def R {\n        subject s : Sys;\n        attribute lim : Real;\n    }\n    part car : Sys;\n}\n";

fn fact<'a>(facts: &'a Value, r: &str) -> &'a Value {
    facts["elements"].as_array().unwrap().iter().find(|e| e["ref"] == r).unwrap_or_else(|| panic!("no element {r} in the facts"))
}

/// `assume constraint` and `require constraint` in a requirement, `exhibit state` in a part, and the two ends of an
/// allocation usage: each has a write path now, reads back in export_facts, and is valid under the official grammar.
#[test]
fn r18_gaps_assumed_and_required_constraints_exhibit_state_and_allocation_ends() {
    let mut f = files(GAPS);
    apply(&mut f, json!({"op": "create_element", "owner": "P::R", "kind": "assume constraint", "name": "pre", "body": "lim > 0"}));
    apply(&mut f, json!({"op": "create_element", "owner": "P::R", "kind": "require constraint", "body": "lim < 10"}));
    apply(&mut f, json!({"op": "create_element", "owner": "P::car", "kind": "exhibit state", "name": "modes", "typed_by": "P::Modes"}));
    apply(&mut f, json!({"op": "create_element", "owner": "P", "kind": "allocation", "name": "a1", "typed_by": "P::A", "source": "P::lg", "target": "P::ph"}));
    let text = &sysml_core::agent::mint::strip_ids(&f[0].text);
    for line in ["assume constraint pre {", "require constraint {", "exhibit state modes : P::Modes;", "allocation a1 : P::A allocate lg to ph;"] {
        assert!(text.contains(line), "no `{line}` in:\n{text}");
    }
    for g in ["syntax", "official", "semantic"] {
        let v = gate(&f, g);
        assert!(v["findings"].as_array().unwrap().is_empty(), "{g}: {v}");
    }
    let facts = sysml_core::agent::export_facts(&model(&f), None).unwrap();
    assert_eq!(fact(&facts, "P::R::pre")["kind"], "assume constraint");
    let body = fact(&facts, "P::R::pre::$1");
    assert_eq!((body["value"].as_str(), body["names"][0]["ref"].as_str()), (Some("lim > 0"), Some("P::R::lim")));
    assert_eq!(fact(&facts, "P::R::$4")["kind"], "require constraint");
    assert_eq!(fact(&facts, "P::R::$4::$1")["value"], "lim < 10");
    // Created in the part car, the exhibit stays where it was asked for.
    let ex = fact(&facts, "P::car::modes");
    assert_eq!((ex["kind"].as_str(), ex["typed_by"][0].as_str()), (Some("exhibit state"), Some("P::Modes")));
    let al = fact(&facts, "P::a1");
    assert_eq!((al["ends"]["source"]["ref"].as_str(), al["ends"]["target"]["ref"].as_str()), (Some("P::lg"), Some("P::ph")));
    assert!(facts["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "allocate" && e["source"] == "P::lg" && e["target"] == "P::ph"));
    // A name in the body is a reference: a rename takes it along.
    apply(&mut f, json!({"op": "update_element", "ref": "P::R::lim", "rename": "limit"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("limit > 0") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("limit < 10"), "{}", f[0].text);
    // What does not fit is refused with the reason.
    let g = files(GAPS);
    assert!(refused(&g, json!({"op": "create_element", "owner": "P::car", "kind": "assume constraint", "name": "c", "body": "1 > 0"})).contains("is a member of a requirement"));
    assert!(refused(&g, json!({"op": "create_element", "owner": "P", "kind": "assume constraint def", "name": "c"})).contains("no definition form"));
    assert!(refused(&g, json!({"op": "create_element", "owner": "P", "kind": "exhibit state def", "name": "c"})).contains("no definition form"));
    assert!(refused(&g, json!({"op": "create_element", "owner": "P", "kind": "allocation", "name": "a2", "source": "P::lg"})).contains("takes both ends or none"));
    assert!(refused(&g, json!({"op": "create_element", "owner": "P", "kind": "allocation", "name": "a2", "source": "P::Sys", "target": "P::ph"})).contains("is a definition"));
    assert!(refused(&g, json!({"op": "create_element", "owner": "P::R", "kind": "require constraint", "body": "lim < 10 } part evil; constraint {"})).contains("not a plain expression"));
}

/// A port of a part connected to a port of a part nested in it (a delegation): both ports face the same way, and the
/// connections gate called that an error. Two sibling ports of the same direction are still an error.
#[test]
fn r18_gaps_a_delegation_joins_ports_of_the_same_direction() {
    let base = "package P {\n    item def Fuel;\n    port def FuelIn {\n        in item fuel : Fuel;\n    }\n    part def Engine {\n        port fin : FuelIn;\n    }\n    part def Tank {\n        port fout : ~FuelIn;\n    }\n    part def Car {\n        port fill : FuelIn;\n        part engine : Engine;\n        part spare : Engine;\n        part tank : Tank;\n    }\n    part car : Car;\n}\n";
    let rules_of = |f: &[FileIn]| -> Vec<String> { gate(f, "connections")["findings"].as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap().to_string()).collect() };
    // Boundary port to inner port, same direction: written, and the gate passes.
    let mut f = files(base);
    apply(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "P::Car::fill", "target": "P::Car::engine::fin"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("connect fill to engine.fin;"), "{}", f[0].text);
    assert_eq!(rules_of(&f), Vec::<String>::new());
    assert!(gate(&f, "official")["findings"].as_array().unwrap().is_empty());
    // The same from outside the part: `car.fill` and `car.engine.fin`.
    let outside = base.replace("    part car : Car;\n", "    part car : Car;\n    connect car.fill to car.engine.fin;\n");
    assert_eq!(rules_of(&files(&outside)), Vec::<String>::new());
    // Two sibling parts, both ports receive: still the error.
    let siblings = base.replace("        part tank : Tank;\n", "        part tank : Tank;\n        connect engine.fin to spare.fin;\n");
    assert_eq!(rules_of(&files(&siblings)), vec!["same-direction-ports".to_string()]);
    // Siblings with a conjugated port: no finding, as before.
    let fine = base.replace("        part tank : Tank;\n", "        part tank : Tank;\n        connect tank.fout to engine.fin;\n");
    assert_eq!(rules_of(&files(&fine)), Vec::<String>::new());
}

// ------------------------------------------------------------------ gaps found by the Drone and ANGARS rebuilds

/// `@Tag about a, b;`: what an annotation is about was not in the facts, and `create_element` refused `about` for
/// metadata. It is written, read back, and two annotations of one type about different elements are two annotations.
#[test]
fn r18_gap_an_annotation_says_what_it_is_about() {
    let text = "package P {\n    metadata def Continuous;\n    part def Car {\n        part a;\n        part b;\n    }\n    @Continuous about Car::a;\n}\n";
    let facts = sysml_core::agent::export_facts(&model(&files(text)), None).unwrap();
    assert_eq!(fact(&facts, "P::$3")["about"], json!(["P::Car::a"]));
    let mut f = files(text);
    let info = apply(&mut f, json!({"op": "create_element", "owner": "P", "kind": "metadata", "typed_by": "P::Continuous", "about": ["P::Car::b", "P::Car"]}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("@P::Continuous about P::Car::b, P::Car;"), "{}", f[0].text);
    let facts = sysml_core::agent::export_facts(&model(&f), None).unwrap();
    assert_eq!(fact(&facts, info["ref"].as_str().unwrap())["about"], json!(["P::Car::b", "P::Car"]));
    for g in ["syntax", "official", "semantic"] {
        assert!(gate(&f, g)["findings"].as_array().unwrap().is_empty(), "{g}");
    }
    // The same type about the same element is a duplicate; about nothing, or about another element, is not.
    assert!(refused(&f, json!({"op": "create_element", "owner": "P", "kind": "metadata", "typed_by": "P::Continuous", "about": "P::Car::a"})).contains("already carries"));
    apply(&mut f, json!({"op": "create_element", "owner": "P", "kind": "metadata", "typed_by": "P::Continuous"}));
    // A name that does not exist is refused; a rename takes the `about` along; a delete counts it as a user.
    assert!(refused(&f, json!({"op": "create_element", "owner": "P", "kind": "metadata", "typed_by": "P::Continuous", "about": "P::Car::zz"})).contains("no element named"));
    assert!(refused(&f, json!({"op": "create_element", "owner": "P", "kind": "metadata", "name": "m", "typed_by": "P::Continuous", "about": "P::Car::a"})).contains("'about' does not apply"));
    assert!(refused(&f, json!({"op": "delete_element", "ref": "P::Car::b"})).contains("still refer"));
    apply(&mut f, json!({"op": "update_element", "ref": "P::Car::b", "rename": "bee"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("about P::Car::bee, P::Car;"), "{}", f[0].text);
}

/// An allocate from `start` took its other end only as a full qualified name. With `owner` it is read from the owner
/// outward, as in an allocate between two real elements.
#[test]
fn r18_gap_an_allocate_from_start_reads_a_short_end_from_its_owner() {
    let text = "package M {\n    package UC {\n        action def Fly {\n            action a1;\n            action partition2;\n        }\n    }\n    package LA {\n        part def Ctx {\n            part operator;\n        }\n    }\n}\n";
    let owner = "M::UC::Fly::partition2";
    // With a real action as the source the short target resolved before; it must be the same with `start`.
    let mut f = files(text);
    apply(&mut f, json!({"op": "create_relationship", "kind": "allocate", "owner": owner, "source": "M::UC::Fly::a1", "target": "LA::Ctx::operator", "keep_text": true}));
    let mut f = files(text);
    let info = apply(&mut f, json!({"op": "create_relationship", "kind": "allocate", "owner": owner, "source": "start", "target": "LA::Ctx::operator", "keep_text": true}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("allocate start to LA::Ctx::operator;"), "{}", f[0].text);
    assert_eq!(target_of(&f, info["ref"].as_str().unwrap(), "LA::Ctx::operator"), Some("M::LA::Ctx::operator".into()));
    assert!(gate(&f, "official")["findings"].as_array().unwrap().is_empty());
    // Without keep_text the qualified name is written; `done` as the target works the same way.
    let mut g = files(text);
    apply(&mut g, json!({"op": "create_relationship", "kind": "allocate", "owner": owner, "source": "LA::Ctx::operator", "target": "done"}));
    assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains("allocate M::LA::Ctx::operator to done;"), "{}", g[0].text);
    // A name that is found nowhere is still refused.
    assert!(refused(&files(text), json!({"op": "create_relationship", "kind": "allocate", "owner": owner, "source": "start", "target": "LA::Ctx::nobody"})).contains("no element named"));
}

/// An import whose target does not resolve (`SI::*`, a library that is not loaded) counted as the same as any new
/// import with the same wildcard, so the new one was refused as a duplicate. The targets are compared. A target written
/// as the owner reaches it resolves. And the new import goes after the imports the owner has, not last in its body.
#[test]
fn r18_gap_imports_compare_their_targets_and_stand_with_the_imports() {
    let text = "package A {\n    private import SI::*;\n    package Units {\n        package Len {\n            part def Meter;\n        }\n        package Mass {\n            part def Gram;\n        }\n    }\n    part def Rod;\n}\n";
    let mut f = files(text);
    apply(&mut f, json!({"op": "create_element", "owner": "A", "kind": "import", "target": "A::Units::Len::*", "visibility": "private"}));
    // Owner-relative target.
    apply(&mut f, json!({"op": "create_element", "owner": "A", "kind": "import", "target": "Units::Mass::*", "visibility": "private"}));
    let lines: Vec<&str> = f[0].text.lines().map(str::trim).collect();
    assert_eq!(&lines[..5], &["package A {", "private import SI::*;", "private import A::Units::Len::*;", "private import A::Units::Mass::*;", "package Units {"], "{}", f[0].text);
    // A real duplicate is still one: the same resolved target, and the same library target.
    assert!(refused(&f, json!({"op": "create_element", "owner": "A", "kind": "import", "target": "A::Units::Len::*"})).contains("already imports"));
    assert!(refused(&f, json!({"op": "create_element", "owner": "A", "kind": "import", "target": "SI::*"})).contains("already imports"));
    apply(&mut f, json!({"op": "create_element", "owner": "A", "kind": "import", "target": "ISQ::*"}));
    // No import yet: first in the body. An explicit place wins.
    let mut g = files("package B {\n    part def Rod;\n    package Lib {\n        part def X;\n    }\n}\n");
    apply(&mut g, json!({"op": "create_element", "owner": "B", "kind": "import", "target": "B::Lib::*"}));
    assert_eq!(g[0].text.lines().nth(1).map(str::trim), Some("private import B::Lib::*;"), "{}", g[0].text);
    apply(&mut g, json!({"op": "create_element", "owner": "B", "kind": "import", "target": "B::Lib::X", "position": "last"}));
    assert!(g[0].text.trim_end().ends_with("private import B::Lib::X;\n}"), "{}", g[0].text);
    for h in [&f, &g] {
        assert!(gate(h, "official")["findings"].as_array().unwrap().is_empty() && gate(h, "syntax")["passed"] == true);
    }
}

/// A created import was written last in its owner's body. It goes after the owner's imports, or first in the body when
/// the owner has none. (No other import stands in these owners, so the duplicate check plays no part.)
#[test]
fn r18_gap_a_created_import_stands_with_the_imports() {
    let mut g = files("package B {\n    part def Rod;\n    package Lib {\n        part def X;\n        part def Y;\n    }\n}\n");
    apply(&mut g, json!({"op": "create_element", "owner": "B", "kind": "import", "target": "B::Lib::X"}));
    assert_eq!(g[0].text.lines().nth(1).map(str::trim), Some("private import B::Lib::X;"), "{}", g[0].text);
    apply(&mut g, json!({"op": "create_element", "owner": "B", "kind": "import", "target": "B::Lib::Y"}));
    assert_eq!(g[0].text.lines().nth(2).map(str::trim), Some("private import B::Lib::Y;"), "{}", g[0].text);
    assert_eq!(g[0].text.lines().nth(3).map(str::trim), Some("part def Rod;"), "{}", g[0].text);
}
