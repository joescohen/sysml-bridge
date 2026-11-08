//! Tools round 7: gaps a tools-only rebuild of an untuned project met (prefix words, repeated annotations,
//! forward references in a batch, ends through inherited features, replies).

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
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

// ---- item 1: every prefix word export_facts prints is written by create_element and update_element ---------------------

/// Prefix words that export_facts prints for the ten reference workspaces (abstract, end, event, parallel, private,
/// constant, ref; in, out and inout as direction) and the others a model may carry. Words only, no project content.
const WORDS: &[&str] = &["public", "private", "protected", "abstract", "variation", "individual", "constant", "derived", "ref", "end", "event", "ordered", "nonunique", "parallel"];

const PREFIXED: &str = "package S {
    constraint def C;
    part def Base;
    attribute def AT;
    abstract part def AD;
    variation part def VD;
    private port def PD;
    protected item def ID;
    public attribute def PA;
    individual part def IND;
    part holder {
        private constraint c1 : C;
        private port v;
        constant item ro : Base;
        ref part rp : Base;
        derived attribute dv : AT;
        attribute ord : AT [0..*] ordered nonunique;
        in port ip;
        out item oi : Base;
        inout port io;
        end part ep : Base;
        event occurrence ev;
        private ref part pr : Base;
    }
    state def SD {
        state st parallel {
            state inner;
        }
    }
}
";

/// What a rebuild passes to create_element for a printed element: the printed words, not a table of its own.
fn call_for(e: &Value) -> Value {
    let mut c = json!({"op": "create_element", "kind": e["kind"], "name": e["name"]});
    if let Some(o) = e["owner"].as_str() {
        c["owner"] = json!(o);
    }
    for k in ["typed_by", "multiplicity", "direction"] {
        if !e[k].is_null() {
            c[k] = e[k].clone();
        }
    }
    let words: Vec<&Value> = e["prefixes"].as_array().into_iter().flatten().filter(|p| !p.as_str().unwrap().starts_with('#')).collect();
    if !words.is_empty() {
        c["modifiers"] = json!(words);
    }
    c
}

fn shape(v: &Value) -> BTreeMap<String, (Vec<String>, Option<String>)> {
    v["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let mut p: Vec<String> = e["prefixes"].as_array().into_iter().flatten().map(|x| x.as_str().unwrap().to_string()).collect();
            p.sort();
            (e["ref"].as_str().unwrap().to_string(), (p, e["direction"].as_str().map(str::to_string)))
        })
        .collect()
}

#[test]
fn r7_every_printed_prefix_word_is_written_as_printed() {
    let src = files(PREFIXED);
    assert!(errors(&model(&src)).is_empty(), "{:?}", errors(&model(&src)));
    let printed = facts(&src);
    // Every word of the list occurs in the fixture (the test would prove nothing otherwise).
    let seen: Vec<String> = printed["elements"].as_array().unwrap().iter().flat_map(|e| e["prefixes"].as_array().cloned().unwrap_or_default()).map(|p| p.as_str().unwrap().to_string()).collect();
    for w in WORDS {
        assert!(seen.iter().any(|s| s == w), "the fixture lacks the word {w}: {seen:?}");
    }
    // Rebuild every printed element with create_element alone, in the order they are printed.
    let mut built = files("");
    for e in printed["elements"].as_array().unwrap() {
        run(&mut built, call_for(e));
    }
    let (a, b) = (shape(&printed), shape(&facts(&built)));
    assert_eq!(a, b, "prefixes differ after the rebuild:\n{}", built[0].text);
}

#[test]
fn r7_update_element_sets_and_clears_modifiers_and_direction() {
    let mut f = files("package S {\n    metadata def Tag;\n    part def A;\n    part x : A;\n    part y : A [2];\n    #Tag part z : A;\n    part def H {\n        part w : A;\n    }\n}\n");
    let info = run(&mut f, json!({"op": "update_element", "ref": "S::x", "modifiers": ["ref", "private"]}));
    assert!(f[0].text.contains("private ref part x : A;"), "{}", f[0].text);
    assert_eq!(info["changed"], json!(["modifiers"]));
    run(&mut f, json!({"op": "update_element", "ref": "S::y", "modifiers": ["nonunique", "ordered"]}));
    assert!(f[0].text.contains("part y : A [2] ordered nonunique;"), "{}", f[0].text);
    // A direction goes on a feature of a definition: a package owns no parameter (owner-directed-feature).
    run(&mut f, json!({"op": "update_element", "ref": "S::H::w", "direction": "in", "modifiers": ["protected"]}));
    assert!(f[0].text.contains("protected in part w : A;"), "{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "S::x", "modifiers": ["protected"]}));
    // A tag stays where it is when the words change, and the words stay when the tags change.
    run(&mut f, json!({"op": "update_element", "ref": "S::z", "modifiers": ["private"]}));
    assert!(f[0].text.contains("private #Tag part z : A;"), "{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "S::z", "metadata": []}));
    assert!(f[0].text.contains("private part z : A;"), "{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "S::z", "metadata": ["Tag"]}));
    assert!(f[0].text.contains("private #Tag part z : A;"), "{}", f[0].text);
    // Empty values remove.
    run(&mut f, json!({"op": "update_element", "ref": "S::x", "modifiers": [], "direction": ""}));
    assert!(f[0].text.contains("\n    part x : A;"), "{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "S::y", "modifiers": []}));
    assert!(f[0].text.contains("part y : A [2];"), "{}", f[0].text);
    // The same call twice changes nothing the second time.
    let before = f[0].text.clone();
    let again = run(&mut f, json!({"op": "update_element", "ref": "S::x", "modifiers": []}));
    assert_eq!(f[0].text, before);
    assert_eq!(again["changed"], json!([]));
    // Refusals say what to do.
    let e = fails(&f, json!({"op": "update_element", "ref": "S::x", "modifiers": ["bogus"]}));
    assert!(e.contains("unknown modifier \"bogus\"") && e.contains("private") && e.contains("nonunique"), "{e}");
    let e = fails(&f, json!({"op": "update_element", "ref": "S::x", "modifiers": ["private", "public"]}));
    assert!(e.contains("one visibility"), "{e}");
    let e = fails(&f, json!({"op": "update_element", "ref": "S::A", "modifiers": ["ref"]}));
    assert!(e.contains("does not fit part def"), "{e}");
    let e = fails(&f, json!({"op": "update_element", "ref": "S::x", "direction": "sideways"}));
    assert!(e.contains("direction must be"), "{e}");
}

// ---- item 2: a repeated annotation --------------------------------------------------------------------------------

#[test]
fn r7_a_repeated_annotation_is_written_with_allow_duplicate() {
    let mut f = files("package S {\n    metadata def X { attribute v : ScalarValues::Integer; }\n    part a;\n}\n");
    let make = |slots: Value| json!({"op": "create_element", "kind": "metadata", "owner": "S::a", "typed_by": "S::X", "slots": slots});
    run(&mut f, make(json!([{"name": "v", "value": "1"}])));
    let e = fails(&f, make(json!([{"name": "v", "value": "2"}])));
    assert!(e.contains("allow_duplicate: true") && !e.contains("if_absent"), "{e}");
    let mut again = make(json!([{"name": "v", "value": "2"}]));
    again["allow_duplicate"] = json!(true);
    run(&mut f, again.clone());
    let m = model(&f);
    let n = m.children(m.find("S::a").unwrap().id).filter(|c| c.kind == "metadata").count();
    assert_eq!(n, 2, "{}", f[0].text);
    again["if_absent"] = json!(true);
    assert!(fails(&f, again).contains("contradict"));
}

// ---- item 3: a batch orders its own forward references --------------------------------------------------------------

#[test]
fn r7_a_batch_orders_forward_references_including_a_short_redefines() {
    let f = files("package S {\n    part def A;\n    part def B :> A { part q; }\n    part holder { part inner; }\n    part other;\n}\n");
    let edit = |v: Value| v;
    let cases: Vec<(&str, Vec<Value>)> = vec![
        ("redefines by short name", vec![
            edit(json!({"op": "create_element", "kind": "part", "name": "u", "owner": "S::B", "redefines": "late"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "late", "owner": "S::A"})),
        ]),
        ("redefines by qualified name", vec![
            edit(json!({"op": "create_element", "kind": "part", "name": "u", "owner": "S::B", "redefines": "S::A::late"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "late", "owner": "S::A"})),
        ]),
        ("update redefines", vec![
            edit(json!({"op": "update_element", "ref": "S::B::q", "redefines": "late"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "late", "owner": "S::A"})),
        ]),
        ("references", vec![
            edit(json!({"op": "create_element", "kind": "part", "name": "u", "owner": "S", "references": "S::holder::late"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "late", "owner": "S::holder"})),
        ]),
        ("specializes", vec![
            edit(json!({"op": "create_element", "kind": "part def", "name": "C", "owner": "S", "specializes": "S::D"})),
            edit(json!({"op": "create_element", "kind": "part def", "name": "D", "owner": "S"})),
        ]),
        ("subsets", vec![
            edit(json!({"op": "create_element", "kind": "part", "name": "u", "owner": "S", "subsets": "S::holder::late"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "late", "owner": "S::holder"})),
        ]),
        ("typed_by", vec![
            edit(json!({"op": "create_element", "kind": "part", "name": "u", "owner": "S", "typed_by": "S::Z"})),
            edit(json!({"op": "create_element", "kind": "part def", "name": "Z", "owner": "S"})),
        ]),
        ("about", vec![
            edit(json!({"op": "create_element", "kind": "comment", "owner": "S", "body": "x", "about": "S::Late"})),
            edit(json!({"op": "create_element", "kind": "part def", "name": "Late", "owner": "S"})),
        ]),
        ("alias target", vec![
            edit(json!({"op": "create_element", "kind": "alias", "owner": "S", "name": "al", "target": "S::Late"})),
            edit(json!({"op": "create_element", "kind": "part def", "name": "Late", "owner": "S"})),
        ]),
        ("bind source and target", vec![
            edit(json!({"op": "create_element", "kind": "bind", "owner": "S::holder", "source": "S::holder::l1", "target": "S::holder::l2"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "l1", "owner": "S::holder"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "l2", "owner": "S::holder"})),
        ]),
        ("connect source and target", vec![
            edit(json!({"op": "create_relationship", "kind": "connect", "owner": "S::holder", "source": "S::holder::l1", "target": "S::holder::l2"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "l1", "owner": "S::holder"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "l2", "owner": "S::holder"})),
        ]),
        ("a feature of a nested member", vec![
            edit(json!({"op": "create_element", "kind": "part", "name": "u", "owner": "S", "specializes": "S::holder::late::deep"})),
            edit(json!({"op": "create_element", "kind": "part", "name": "late", "owner": "S::holder", "features": [{"kind": "part", "name": "deep"}]})),
        ]),
    ];
    for (label, edits) in cases {
        let b = intent::apply_batch(&f, &edits).unwrap_or_else(|(i, e)| panic!("{label}: edit #{} failed: {}", i + 1, e.message));
        assert!(errors(&model(&b.files)).is_empty(), "{label}: {:?}\n{}", errors(&model(&b.files)), b.files[0].text);
        assert!(!b.moved.is_empty(), "{label}: the creator should have moved before the edit that needs it");
    }
}

// ---- item 4: an end through an inherited feature draws -----------------------------------------------------------------

// ---- item 5: replies ----------------------------------------------------------------------------------------------------

#[test]
fn r7_a_qualified_name_with_quotes_is_not_quoted_twice() {
    let mut f = files("package 'My Pkg' {\n    metadata def X;\n    part 'A b';\n}\n");
    run(&mut f, json!({"op": "create_element", "kind": "metadata", "owner": "'My Pkg'::'A b'", "typed_by": "'My Pkg'::X"}));
    let e = fails(&f, json!({"op": "create_element", "kind": "metadata", "owner": "'My Pkg'::'A b'", "typed_by": "'My Pkg'::X"}));
    assert!(e.starts_with("'My Pkg'::'A b' already carries"), "{e}");
    assert!(!e.contains("''"), "{e}");
    assert_eq!(intent::undouble_quotes("rename '' clears the name"), "rename '' clears the name");
    assert_eq!(intent::undouble_quotes("''Name'' already"), "'Name' already");
    assert_eq!(intent::undouble_quotes("see ''A'::'B C'::D'."), "see 'A'::'B C'::D.");
}

#[test]
fn r7_a_create_with_source_ids_reports_every_id() {
    let f = files("package S {\n    part def A;\n}\n");
    let info = try_op(&f, json!({"op": "create_element", "kind": "part", "name": "x", "owner": "S", "typed_by": "S::A", "source_ids": ["_a", "_b", "_c"]})).unwrap();
    assert_eq!(info["source_ids"], json!(["_a", "_b", "_c"]));
    assert_eq!(info["source_id"], "_a");
}

#[test]
fn r7_a_batch_that_fails_for_two_causes_groups_them() {
    let f = files("package S {\n    part def A;\n    part dup;\n}\n");
    let edits = vec![
        json!({"op": "create_element", "kind": "part", "name": "a1", "owner": "S", "typed_by": "S::Missing1"}),
        json!({"op": "create_element", "kind": "part", "name": "a2", "owner": "S", "typed_by": "S::Missing2"}),
        json!({"op": "create_element", "kind": "part", "name": "ok", "owner": "S", "typed_by": "S::A"}),
        json!({"op": "create_element", "kind": "part", "name": "dup", "owner": "S"}),
        json!({"op": "create_element", "kind": "part", "name": "a3", "owner": "S", "typed_by": "S::Missing3"}),
    ];
    let (i, e) = intent::apply_batch(&f, &edits).unwrap_err();
    assert_eq!(i, 0);
    assert!(e.message.contains("2 different causes"), "{}", e.message);
    assert!(e.message.contains("#2") && e.message.contains("#5") && e.message.contains("(2) #4"), "{}", e.message);
    assert!(!e.message.contains("fail too when this one is left out (complete list): "), "{}", e.message);
    let causes = e.data["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 2);
    assert_eq!(causes[0]["edits"], json!([1, 2, 5]));
    assert_eq!(causes[1]["edits"], json!([4]));
    // One cause only: the message says so.
    let (_, e) = intent::apply_batch(&f, &edits[..2]).unwrap_err();
    assert!(e.message.contains("same cause") && e.message.contains("#2"), "{}", e.message);
}

// ---- item 6: comment doc, bind ------------------------------------------------------------------------------------------

#[test]
fn r7_a_comment_takes_doc_as_body_and_bind_is_a_listed_kind() {
    let mut f = files("package S {\n    part def A;\n}\n");
    run(&mut f, json!({"op": "create_element", "kind": "comment", "owner": "S", "doc": "hello", "about": "S::A"}));
    assert!(f[0].text.contains("comment about S::A /* hello */"), "{}", f[0].text);
    let printed = facts(&f);
    let c = printed["elements"].as_array().unwrap().iter().find(|e| e["kind"] == "comment").unwrap();
    assert_eq!(c["doc"], "hello");
    let e = fails(&f, json!({"op": "create_element", "kind": "comment", "owner": "S", "doc": "a", "body": "b"}));
    assert!(e.contains("same thing"), "{e}");
    let e = fails(&f, json!({"op": "create_element", "kind": "bogus", "owner": "S", "name": "n"}));
    assert!(e.contains("bind"), "{e}");
}
