//! Tools round 8: gaps a tools-only rebuild of ANGARS met (satisfy by a definition, a state that leaves itself,
//! textual representations, imports, relative triggers, named dependencies, unrecognised text, slot ids).

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

fn fails(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(_) => panic!("op {op} should have failed"),
        Err(e) => e.message,
    }
}

fn facts(f: &[FileIn]) -> Value {
    agent::export_facts(&model(f), None).unwrap()
}

fn element<'a>(facts: &'a Value, kind: &str) -> Vec<&'a Value> {
    facts["elements"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).collect()
}

const BASE: &str = "package P {
    requirement def R1;
    requirement r1 : R1;
    action def Act;
    part def Pt;
    item def Sig;
    state def S1 {
        state a;
        state b;
        item 'Error Recovered' : Sig;
    }
    constraint c1 { }
    package Q { }
}
";

// ---- 1. satisfy by a definition ----------------------------------------------------------------------------------

#[test]
fn r8_satisfy_by_a_definition_needs_allow_definitions_and_is_written_as_given() {
    let mut f = files(BASE);
    let msg = fails(&f, json!({"kind": "satisfy", "source": "P::Act", "target": "P::r1", "op": "create_relationship"}));
    assert!(msg.contains("allow_definitions"), "{msg}");
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "P::Act", "target": "P::r1", "allow_definitions": true}));
    assert_eq!(info["warnings"][0]["rule"], "trace-operand-definition");
    assert!(f[0].text.contains("satisfy P::r1 by P::Act;") || f[0].text.contains("satisfy r1 by Act;"), "{}", f[0].text);
    let fa = facts(&f);
    assert!(fa["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "satisfy" && e["source"] == "P::Act" && e["target"] == "P::r1"), "{fa}");
    // The semantic gate warns and does not error.
    let v = sysml_core::agent::trace::validate_model(&model(&f), Some("semantic")).unwrap();
    assert_eq!(v["passed"], true, "{v}");
    assert!(v["findings"].as_array().unwrap().iter().any(|x| x["rule"] == "trace-operand-definition" && x["severity"] == "warning"));
}

#[test]
fn r8_satisfy_still_refuses_a_definition_requirement_and_a_package() {
    let f = files(BASE);
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "P::Act", "target": "P::R1", "allow_definitions": true}));
    assert!(msg.contains("requirement"), "{msg}");
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "P::Q", "target": "P::r1", "allow_definitions": true}));
    assert!(msg.contains("package"), "{msg}");
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "verify", "source": "P::Act", "target": "P::r1", "allow_definitions": true}));
    assert!(msg.contains("allow_definitions applies only"), "{msg}");
}

// ---- 2. a state that leaves itself -------------------------------------------------------------------------------

#[test]
fn r8_a_transition_may_leave_and_enter_the_same_state() {
    let mut f = files(BASE);
    let call = json!({"op": "create_relationship", "kind": "transition", "source": "P::S1::a", "target": "P::S1::a", "owner": "P::S1"});
    run(&mut f, call.clone());
    assert!(f[0].text.contains("transition first a then a;"), "{}", f[0].text);
    let msg = fails(&f, call.clone());
    assert!(msg.contains("allow_duplicate"), "{msg}");
    let mut again = call.clone();
    again["allow_duplicate"] = json!(true);
    run(&mut f, again);
    let fa = facts(&f);
    let selfs: Vec<_> = element(&fa, "transition").into_iter().filter(|t| t["ends"]["source"]["ref"] == t["ends"]["target"]["ref"]).collect();
    assert_eq!(selfs.len(), 2);
    // Other relationships still refuse a single element at both ends.
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "dependency", "source": "P::Act", "target": "P::Act"}));
    assert!(msg.contains("same element"), "{msg}");
}

// ---- 3. textual representation -----------------------------------------------------------------------------------

#[test]
fn r8_a_textual_representation_is_written_and_printed_with_language_and_body() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_element", "kind": "textual representation", "owner": "P::c1", "language": "StructuredExpression", "body": "true\nTypeTest"}));
    run(&mut f, json!({"op": "create_element", "kind": "textual representation", "owner": "P::c1", "language": "OCL", "body": "x > 1"}));
    assert!(f[0].text.contains("language \"StructuredExpression\" /*\n"), "{}", f[0].text);
    let fa = facts(&f);
    let reps = element(&fa, "textual representation");
    assert_eq!(reps.len(), 2);
    assert_eq!(reps[0]["language"], "StructuredExpression");
    assert_eq!(reps[0]["body"], "true\nTypeTest");
    assert_eq!(reps[1]["body"], "x > 1");
    // What export_facts prints is what create_element takes.
    let mut g = files(BASE);
    for r in &reps {
        run(&mut g, json!({"op": "create_element", "kind": r["kind"], "owner": r["owner"], "language": r["language"], "body": r["body"]}));
    }
    let ga = facts(&g);
    let again: Vec<_> = element(&ga, "textual representation").iter().map(|e| (e["language"].clone(), e["body"].clone())).collect();
    let first: Vec<_> = reps.iter().map(|e| (e["language"].clone(), e["body"].clone())).collect();
    assert_eq!(again, first);
    let msg = fails(&f, json!({"op": "create_element", "kind": "textual representation", "owner": "P::c1", "body": "x"}));
    assert!(msg.contains("language"), "{msg}");
    let msg = fails(&f, json!({"op": "create_element", "kind": "part", "name": "x", "owner": "P", "language": "OCL"}));
    assert!(msg.contains("textual representation"), "{msg}");
}

// ---- 4. imports --------------------------------------------------------------------------------------------------

#[test]
fn r8_an_import_is_written_listed_and_read_back_as_printed() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_element", "kind": "import", "owner": "P::Q", "target": "P::S1::*"}));
    run(&mut f, json!({"op": "create_element", "kind": "import", "owner": "P::Q", "target": "P", "visibility": "private", "recursive": true}));
    run(&mut f, json!({"op": "create_element", "kind": "import", "owner": "P::Q", "target": "P::Sig", "all": true}));
    let top = run(&mut f, json!({"op": "create_element", "kind": "import", "target": "P::Q::*"}));
    assert!(top["ref"].as_str().unwrap().contains('$'), "{top}");
    assert!(f[0].text.contains("import P::S1::*;"), "{}", f[0].text);
    assert!(f[0].text.contains("private import P::**;"), "{}", f[0].text);
    assert!(f[0].text.contains("import all P::Sig;"), "{}", f[0].text);
    let fa = facts(&f);
    let imports = element(&fa, "import");
    assert_eq!(imports.len(), 4);
    let targets: Vec<&str> = imports.iter().map(|e| e["target"].as_str().unwrap()).collect();
    assert_eq!(targets, vec!["P::S1::*", "P::**", "P::Sig", "P::Q::*"]);
    assert_eq!(imports[1]["prefixes"], json!(["private"]));
    assert_eq!(imports[2]["all"], true);
    assert!(fa["not_listed"].as_array().unwrap().iter().all(|n| n["kind"] != "import"), "{}", fa["not_listed"]);
    // The printed form is the argument form.
    let mut g = files(BASE);
    for i in &imports {
        let mut op = json!({"op": "create_element", "kind": "import", "target": i["target"]});
        if i["owner"] != Value::Null && i["owner"] != "" {
            op["owner"] = i["owner"].clone();
        }
        if let Some(p) = i["prefixes"].as_array() {
            op["modifiers"] = json!(p);
        }
        if i["all"] == true {
            op["all"] = json!(true);
        }
        run(&mut g, op);
    }
    let again: Vec<String> = element(&facts(&g), "import").iter().map(|e| e["target"].as_str().unwrap().to_string()).collect();
    assert_eq!(again, targets.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    // A repeat needs allow_duplicate; if_absent returns the first.
    let rep = json!({"op": "create_element", "kind": "import", "owner": "P::Q", "target": "P::S1::*"});
    assert!(fails(&f, rep.clone()).contains("allow_duplicate"));
    let mut ia = rep.clone();
    ia["if_absent"] = json!(true);
    assert_eq!(run(&mut f, ia)["existed"], true);
    let mut ad = rep.clone();
    ad["allow_duplicate"] = json!(true);
    run(&mut f, ad);
    assert!(fails(&f, json!({"op": "create_element", "kind": "import", "owner": "P::Q", "target": "P::Nope::*"})).contains("Nope"));
    assert!(fails(&f, json!({"op": "create_element", "kind": "import", "owner": "P::Q", "target": "P::*", "visibility": "secret"})).contains("visibility"));
}

// ---- 5. a trigger as export_facts prints it -------------------------------------------------------------------------

#[test]
fn r8_a_trigger_is_resolved_from_the_owner_outward_and_written_as_printed() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "P::S1::a", "target": "P::S1::b", "owner": "P::S1", "trigger": "'Error Recovered'"}));
    assert!(f[0].text.contains("accept 'Error Recovered'"), "{}", f[0].text);
    let fa = facts(&f);
    assert_eq!(element(&fa, "transition")[0]["trigger"], "'Error Recovered'");
    // A name from further out, and the qualified form, are accepted too.
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "P::S1::b", "target": "P::S1::a", "owner": "P::S1", "trigger": "Sig"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "P::S1::a", "target": "P::S1::b", "owner": "P::S1", "trigger": "P::Sig"}));
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "P::S1::a", "target": "P::S1::b", "owner": "P::S1", "trigger": "Nothing Here", "allow_duplicate": true}));
    assert!(msg.contains("trigger"), "{msg}");
    // `start` as the source goes through the other writer and takes the same triggers.
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "start", "target": "P::S1::a", "owner": "P::S1", "trigger": "'Error Recovered'"}));
}

// ---- 6. named dependencies -----------------------------------------------------------------------------------------

#[test]
fn r8_dependencies_with_the_same_ends_and_different_names_are_different_statements() {
    let mut f = files(BASE);
    let dep = |name: Option<&str>| {
        let mut o = json!({"op": "create_relationship", "kind": "dependency", "source": "P::Act", "target": "P::Pt"});
        if let Some(n) = name {
            o["name"] = json!(n);
        }
        o
    };
    run(&mut f, dep(Some("d1")));
    run(&mut f, dep(Some("d2")));
    run(&mut f, dep(None));
    let msg = fails(&f, dep(Some("d1")));
    assert!(msg.contains("name") || msg.contains("already"), "{msg}");
    let msg = fails(&f, dep(None));
    assert!(msg.contains("allow_duplicate"), "{msg}");
    let mut again = dep(None);
    again["allow_duplicate"] = json!(true);
    run(&mut f, again);
}

#[test]
fn r8_raw_text_with_two_named_dependencies_is_not_a_repeat() {
    let f = files(BASE);
    let op = json!({"op": "add_element", "owner": "P", "text": "dependency d1 from Act to Pt;\ndependency d2 from Act to Pt;"});
    intent::apply_op(&f, &op).unwrap_or_else(|e| panic!("{}", e.message));
}

// ---- 7. text the parser does not recognise ------------------------------------------------------------------------

#[test]
fn r8_text_the_parser_does_not_recognise_fails_the_edit() {
    let f = files(BASE);
    let msg = fails(&f, json!({"op": "add_element", "owner": "P", "text": "bogus nonsense here ;;"}));
    assert!(msg.contains("not recognized as SysML v2"), "{msg}");
    // Nothing was written.
    assert_eq!(f[0].text, BASE);
    // Recognised text still goes in.
    intent::apply_op(&f, &json!({"op": "add_element", "owner": "P", "text": "part ok;"})).unwrap();
}

// ---- 8. slot source ids --------------------------------------------------------------------------------------------

#[test]
fn r8_metadata_slots_record_their_source_ids_in_the_same_call() {
    let text = "package P { metadata def Tag { attribute k; attribute m; } part def Pt; }";
    let mut f = files(text);
    let info = run(&mut f, json!({"op": "create_element", "kind": "metadata", "owner": "P::Pt", "typed_by": "P::Tag", "slots": [{"name": "k", "value": "1", "source_id": "id-k"}, {"name": "m", "value": "2", "source_ids": ["id-m1", "id-m2"]}]}));
    let ids = info["feature_ids"].as_array().unwrap();
    assert_eq!(ids.len(), 3, "{info}");
    assert!(ids[0]["ref"].as_str().unwrap().ends_with("::k"), "{info}");
    assert_eq!(ids[0]["source_id"], "id-k");
}
