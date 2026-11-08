//! Tools round 6: faults a tools-only rebuild of an untuned project met.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, Severity};

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn errors(m: &Model) -> Vec<String> {
    m.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
}

fn run(f: &mut Vec<FileIn>, op: Value) -> Value {
    let applied = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    *f = applied.files;
    assert!(errors(&model(f)).is_empty(), "errors after {op}: {:?}\n{}", errors(&model(f)), f[0].text);
    applied.info
}

fn try_op(f: &[FileIn], op: Value) -> Result<Value, String> {
    intent::apply_op(f, &op).map(|a| a.info).map_err(|e| e.message)
}

fn fails(f: &[FileIn], op: Value) -> String {
    try_op(f, op.clone()).err().unwrap_or_else(|| panic!("op {op} should have failed"))
}

fn facts(f: &[FileIn]) -> Value {
    agent::export_facts(&model(f), None).unwrap()
}

// ---- item 1: the kind words export_facts prints are accepted as they are printed ----------------------------------

/// Element kinds export_facts prints for the ten reference workspaces, plus the relationship kinds of its edges.
/// Kind names only, no project content.
const PRINTED_KINDS: &[&str] = &[
    "package", "metadata def", "requirement", "part def", "metadata", "part", "attribute", "connection def", "alias", "constraint",
    "constraint def", "port", "subject", "action def", "action", "parameter", "fork node", "join node", "succession", "flow",
    "allocation", "include use case", "dependency", "comment", "connection", "port def", "item", "satisfy", "item def",
    "attribute def", "enum def", "enum", "use case def", "feature", "state def", "state", "do action", "entry action",
    "exit action", "transition", "occurrence", "decide node", "binding", "use case", "actor", "occurrence def", "message",
    "merge node", "refine",
    // edge kinds
    "allocate", "connect", "bind", "derive", "trace", "verify", "specialization", "redefinition",
];

const FIX: &str = "package S {
    metadata def Md0;
    part def Car { attribute mass : ScalarValues::Real; part pp; part pp2; port po; port po2; }
    part def Car2 :> Car;
    enum def EDf;
    use case def UCf;
    requirement r0;
    requirement r1;
    verification def VD;
    part def Car3;
    part def Car4 :> Car { part pp5; }
    action def ADf { action ac0; action ac1; }
    occurrence def ODf { occurrence o1; occurrence o2; }
    state def SDf { state s0; state s1; }
}
";

/// Params that complete a call for the printed kind word; the word itself goes in `kind`.
fn params(word: &str) -> Vec<Value> {
    let p = |v: Value| v;
    match word {
        "package" => vec![p(json!({"name": "Pk", "owner": "S"}))],
        "metadata def" | "part def" | "connection def" | "constraint def" | "action def" | "port def" | "item def" | "attribute def" | "enum def" | "use case def" | "state def" | "occurrence def" => {
            vec![p(json!({"name": "NewDef", "owner": "S"}))]
        }
        "requirement" | "constraint" | "occurrence" | "use case" => vec![p(json!({"name": "nw", "owner": "S"}))],
        "metadata" => vec![p(json!({"typed_by": "S::Md0", "owner": "S::Car"}))],
        "part" | "attribute" | "port" | "item" => vec![p(json!({"name": "nw", "owner": "S::Car"}))],
        "alias" => vec![p(json!({"name": "al", "target": "S::Car", "owner": "S"}))],
        "subject" => vec![p(json!({"name": "su", "typed_by": "S::Car", "owner": "S::UCf"}))],
        "parameter" => vec![p(json!({"name": "nw", "owner": "S::ADf", "direction": "in", "typed_by": "ScalarValues::Real"}))],
        "action" | "fork node" | "join node" | "decide node" | "merge node" => vec![p(json!({"name": "nw", "owner": "S::ADf"}))],
        "succession" => vec![p(json!({"source": "start", "target": "S::ADf::ac0", "owner": "S::ADf"}))],
        "flow" => vec![p(json!({"source": "S::Car::po", "target": "S::Car::po2", "owner": "S::Car"}))],
        "allocation" | "allocate" | "connection" | "connect" | "binding" | "bind" => {
            vec![p(json!({"source": "S::Car::pp", "target": "S::Car::pp2", "owner": "S::Car"}))]
        }
        "include use case" | "actor" => vec![p(json!({"typed_by": "S::UCf", "owner": "S::UCf"}))],
        "dependency" | "trace" => vec![p(json!({"source": "S::Car", "target": "S::Car2"}))],
        "refine" | "derive" => vec![p(json!({"source": "S::r1", "target": "S::r0"}))],
        "comment" => vec![p(json!({"body": "text", "about": "S::Car", "owner": "S"}))],
        "satisfy" => vec![p(json!({"source": "S::Car::pp", "target": "S::r0"}))],
        "verify" => vec![p(json!({"source": "S::VD", "target": "S::r0"}))],
        "enum" => vec![p(json!({"name": "ev", "owner": "S::EDf"}))],
        "feature" => vec![p(json!({"unnamed": true, "redefines": "mass", "value": "5", "owner": "S::Car2"}))],
        "state" | "do action" | "entry action" | "exit action" => vec![p(json!({"name": "nw", "owner": "S::SDf"}))],
        "transition" => vec![p(json!({"source": "S::SDf::s0", "target": "S::SDf::s1", "owner": "S::SDf"}))],
        "message" => vec![p(json!({"name": "mm", "source": "S::ODf::o1", "target": "S::ODf::o2", "owner": "S::ODf"}))],
        "specialization" => vec![p(json!({"source": "S::Car3", "target": "S::Car"}))],
        "redefinition" => vec![p(json!({"source": "S::Car4::pp5", "target": "S::Car::pp"}))],
        other => panic!("no params for {other}"),
    }
}

#[test]
fn r6_every_printed_kind_word_is_accepted_as_printed() {
    // A write tool must not call the word unknown. A call may still fail for its own reasons (for example a source that
    // is a definition), so the check is on the kind refusal only.
    // Declaration words go to create_element; relationship words (what an edge or a statement is called) to
    // create_relationship. Each must take the exact word.
    const RELATIONSHIPS: &[&str] = &[
        "allocation", "connection", "binding", "satisfy", "dependency", "refine", "flow", "transition", "succession", "allocate", "connect",
        "bind", "derive", "trace", "verify", "specialization", "redefinition",
    ];
    let mut bad = Vec::new();
    for word in PRINTED_KINDS {
        let tools: &[&str] = if RELATIONSHIPS.contains(word) { &["create_relationship"] } else { &["create_element"] };
        let mut last = String::new();
        let mut accepted = false;
        for tool in tools {
            for mut p in params(word) {
                p["op"] = json!(tool);
                p["kind"] = json!(word);
                match try_op(&files(FIX), p) {
                    Ok(_) => accepted = true,
                    Err(e) => last = e,
                }
            }
        }
        if !accepted {
            bad.push(format!("{word}: {last}"));
        }
    }
    assert!(bad.is_empty(), "kind words refused:\n{}", bad.join("\n"));
}

// ---- item 2: an end that is start or done round-trips ----------------------------------------------------------------

#[test]
fn r6_allocate_from_start_prints_a_ref_create_relationship_takes() {
    let f = files("package S {\n    action def A {\n        action a1;\n        allocate start to a1;\n    }\n    part def P { part pp; }\n}\n");
    let v = facts(&f);
    let al = v["elements"].as_array().unwrap().iter().find(|e| e["kind"] == "allocation").expect("an allocation is listed");
    let src = al["ends"]["source"]["ref"].as_str().unwrap().to_string();
    let tgt = al["ends"]["target"]["ref"].as_str().unwrap().to_string();
    assert_eq!(src, "start", "{al}");
    // Rebuild it in an empty copy of the behavior.
    let g = files("package S {\n    action def A {\n        action a1;\n    }\n}\n");
    let r = try_op(&g, json!({"op": "create_relationship", "kind": "allocate", "source": src, "target": tgt, "owner": "S::A", "allow_duplicate": true}));
    assert!(r.is_ok(), "{r:?} for {src} -> {tgt}");
    // `done` as the target, and a behavior that holds an element named start (an alias hides the marker).
    let d = files("package S {\n    action def A {\n        action a1;\n        alias start for a1;\n        allocate a1 to done;\n        allocate start to a1;\n    }\n}\n");
    let v = facts(&d);
    for al in v["elements"].as_array().unwrap().iter().filter(|e| e["kind"] == "allocation") {
        let (src, tgt) = (al["ends"]["source"]["ref"].as_str().unwrap(), al["ends"]["target"]["ref"].as_str().unwrap());
        let g = files("package S {\n    action def A {\n        action a1;\n        alias start for a1;\n    }\n}\n");
        let r = try_op(&g, json!({"op": "create_relationship", "kind": "allocate", "source": src, "target": tgt, "owner": "S::A"}));
        assert!(r.is_ok(), "{r:?} for {src} -> {tgt}");
    }
    // A succession keeps the bare marker.
    let s = files("package S {\n    action def A {\n        action a1;\n        first start then a1;\n    }\n}\n");
    let v = facts(&s);
    let su = v["elements"].as_array().unwrap().iter().find(|e| e["kind"] == "succession").unwrap();
    assert_eq!(su["ends"]["source"]["ref"], "start");
}

// ---- item 3: a raw add looks only at its own statement --------------------------------------------------------------

#[test]
fn r6_raw_add_is_not_refused_for_old_repeated_statements() {
    let mut f = files("package S {\n    part def Car { part pp; part pp2; }\n    part x;\n    part y;\n}\n");
    run(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "S::x", "target": "S::y", "allow_duplicate": true}));
    run(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "S::x", "target": "S::y", "allow_duplicate": true}));
    // Unrelated text, placed before the repeats so the element numbers of the later ones shift.
    let r = try_op(&f, json!({"op": "add_element", "owner": "S::Car", "text": "end part zzz;"}));
    assert!(r.is_ok(), "{r:?}");
    let r = try_op(&f, json!({"op": "add_element", "owner": "S", "text": "part def First;"}));
    assert!(r.is_ok(), "{r:?}");
    // A statement that repeats one is still refused.
    let e = fails(&f, json!({"op": "add_element", "owner": "S", "text": "allocate x to y;"}));
    assert!(e.contains("allow_duplicate: true"), "{e}");
}

// ---- item 7: a refused repeat names allow_duplicate -----------------------------------------------------------------

#[test]
fn r6_every_refused_repeat_names_allow_duplicate() {
    let mut f = files("package S {\n    part def Car { part pp; part pp2; port po; port po2; }\n    part def Car2 { part c1 : Car; part c2 : Car; }\n    requirement r0;\n    requirement r1;\n    action def A { action a1; action a2; }\n    state def St { state s0; state s1; }\n    part x; part y;\n}\n");
    let calls = vec![
        json!({"kind": "allocate", "source": "S::x", "target": "S::y"}),
        json!({"kind": "satisfy", "source": "S::x", "target": "S::r0"}),
        json!({"kind": "dependency", "source": "S::x", "target": "S::y"}),
        json!({"kind": "refine", "source": "S::r1", "target": "S::r0"}),
        json!({"kind": "trace", "source": "S::x", "target": "S::y"}),
        json!({"kind": "derive", "source": "S::r1", "target": "S::r0"}),
        json!({"kind": "connect", "source": "S::Car::pp", "target": "S::Car::pp2", "owner": "S::Car"}),
        json!({"kind": "flow", "source": "S::Car::po", "target": "S::Car::po2", "owner": "S::Car"}),
        json!({"kind": "succession", "source": "S::A::a1", "target": "S::A::a2"}),
        json!({"kind": "transition", "source": "S::St::s0", "target": "S::St::s1"}),
    ];
    for c in calls {
        let mut op = c.clone();
        op["op"] = json!("create_relationship");
        run(&mut f, op.clone());
        let e = fails(&f, op);
        assert!(e.contains("allow_duplicate: true"), "{c}: {e}");
    }
    let mut op = json!({"op": "create_element", "kind": "bind", "source": "S::Car::pp", "target": "S::Car::pp2", "owner": "S::Car"});
    run(&mut f, op.clone());
    let e = fails(&f, op.take());
    assert!(e.contains("allow_duplicate: true"), "{e}");
}

// ---- item 8: typing by a usage ---------------------------------------------------------------------------------------

#[test]
fn r6_typing_by_a_usage_is_refused_plainly() {
    let f = files("package S {\n    part def Eng;\n    part e1 : Eng;\n    requirement r1;\n    part holder;\n}\n");
    let e = fails(&f, json!({"op": "create_element", "kind": "part", "name": "p", "owner": "S::holder", "typed_by": "S::e1"}));
    assert!(e.contains("SysML v2 does not allow typing by a usage") && e.contains("S::Eng"), "{e}");
    let e = fails(&f, json!({"op": "create_element", "kind": "part", "name": "p", "owner": "S::holder", "typed_by": "S::r1"}));
    assert!(e.contains("SysML v2 does not allow typing by a usage") && e.contains("no definition"), "{e}");
}

// ---- item 9: the batch limit -----------------------------------------------------------------------------------------

#[test]
fn r6_batch_limit_refusal_says_the_limit_and_that_generated_edits_count() {
    let f = files("package S {\n}\n");
    let n = intent::MAX_EDITS + 1;
    let edits: Vec<Value> = (0..n).map(|i| json!({"op": "create_element", "owner": "S", "kind": "part def", "name": format!("D{i}")})).collect();
    let e = intent::apply_batch(&f, &edits).err().unwrap().1.message;
    assert!(e.contains(&intent::MAX_EDITS.to_string()) && e.contains("generated") && e.contains(&n.to_string()), "{e}");
}

// ---- item 5: source_ids ------------------------------------------------------------------------------------------------

#[test]
fn r6_source_ids_gives_one_element_several_ids() {
    let f = files("package S {\n    part def Eng;\n}\n");
    let a = intent::apply_op(&f, &json!({"op": "create_element", "kind": "part", "name": "p", "owner": "S", "typed_by": "S::Eng", "source_ids": ["id_one", "id_two", "id_three"]})).unwrap();
    assert_eq!(a.info["source_id"], "id_one");
    assert_eq!(a.info["source_ids"], json!(["id_one", "id_two", "id_three"]));
    let assigned: Vec<String> = a.events.iter().map(|e| format!("{e:?}")).collect();
    assert_eq!(assigned.iter().filter(|e| e.contains("S::p")).count(), 3, "{assigned:?}");
    // Mixed with source_id, and bad input.
    assert!(try_op(&f, json!({"op": "create_element", "kind": "part", "name": "q", "owner": "S", "source_id": "a", "source_ids": ["b"]})).is_ok());
    assert!(fails(&f, json!({"op": "create_element", "kind": "part", "name": "q", "owner": "S", "source_ids": []})).contains("source_ids"));
    assert!(fails(&f, json!({"op": "create_element", "kind": "part", "name": "q", "owner": "S", "source_ids": [3]})).contains("source_ids"));
    assert!(fails(&f, json!({"op": "create_element", "kind": "part", "name": "q", "owner": "S", "source_ids": ["has space"]})).contains("not valid"));
}

// ---- item 11: packages that refer to each other move together ---------------------------------------------------------

#[test]
fn r6_packages_that_refer_to_each_other_move_in_one_batch() {
    let f = files("package A { part def X; part y : B::Z; }\npackage B { part def Z; part w : A::X; }\npackage Root;\n");
    // One move alone is refused: B still says A::X.
    let e = fails(&f, json!({"op": "update_element", "ref": "A", "owner": "Root"}));
    assert!(e.contains("unresolved"), "{e}");
    let batch = intent::apply_batch(&f, &[json!({"op": "update_element", "ref": "A", "owner": "Root"}), json!({"op": "update_element", "ref": "B", "owner": "Root"})]);
    let b = batch.unwrap_or_else(|e| panic!("batch refused: {}", e.1.message));
    let m = model(&b.files);
    assert!(errors(&m).is_empty(), "{:?}", errors(&m));
    assert!(m.find("Root::A::X").is_some() && m.find("Root::B::Z").is_some());
    assert!(!m.diagnostics.iter().any(|d| d.message.starts_with("cannot resolve")), "{:?}", m.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    // A batch that really leaves a name unresolved is still refused, as a whole.
    let bad = intent::apply_batch(&f, &[json!({"op": "update_element", "ref": "A", "owner": "Root"})]);
    assert!(bad.err().unwrap().1.message.contains("unresolved"));
}

// ---- item 12: a name is quoted in a view label only when it needs quotes --------------------------------------------------
