//! Tools round 9: one name rule for every relationship kind and for raw text, and connectors whose ends the owner
//! cannot reach (the reference states them in a definition, with ends through another usage).

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, Severity};

const BASE: &str = "package P {\n    part def Node;\n    part def Sys {\n        part a : Node;\n        part b : Node;\n        attribute m;\n        attribute n;\n    }\n    package Vals {\n        part t : Sys;\n    }\n    action def Act {\n        action a1;\n        action a2;\n    }\n    state def St {\n        state s1;\n        state s2;\n    }\n}\n";

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

fn rel(kind: &str, source: &str, target: &str, name: Option<&str>) -> Value {
    let mut o = json!({"op": "create_relationship", "kind": kind, "source": source, "target": target});
    if let Some(n) = name {
        o["name"] = json!(n);
    }
    o
}

/// One rule for every kind: a differing name (or only one of the two named) is a new statement; the same name, or
/// no name on both, is a repeat that needs allow_duplicate.
fn rule(f: &mut Vec<FileIn>, make: &dyn Fn(Option<&str>) -> Value) {
    run(f, make(Some("n1")));
    run(f, make(Some("n2")));
    run(f, make(None));
    let m = fails(f, make(Some("n1")));
    assert!(m.contains("already"), "same name: {m}");
    assert!(fails(f, make(None)).contains("allow_duplicate"), "unnamed twice");
    let mut again = make(None);
    again["allow_duplicate"] = json!(true);
    run(f, again);
}

#[test]
fn r9_a_different_name_is_a_different_connect() {
    let mut f = files(BASE);
    rule(&mut f, &|n| rel("connect", "P::Sys::a", "P::Sys::b", n));
}

#[test]
fn r9_a_different_name_is_a_different_bind_succession_and_transition() {
    let mut f = files(BASE);
    rule(&mut f, &|n| {
        let mut o = rel("bind", "P::Sys::m", "P::Sys::n", n);
        o["owner"] = json!("P::Sys");
        o
    });
    rule(&mut f, &|n| rel("succession", "P::Act::a1", "P::Act::a2", n));
    rule(&mut f, &|n| rel("transition", "P::St::s1", "P::St::s2", n));
}

#[test]
fn r9_raw_text_follows_the_same_name_rule() {
    let f = files(BASE);
    let add = |text: &str| json!({"op": "add_element", "owner": "P::Sys", "text": text});
    // Two names, one name and none: all new statements.
    for text in ["connection x1 connect a to b;\nconnection x2 connect a to b;", "connection x1 connect a to b;\nconnect a to b;", "binding k1 bind m = n;\nbinding k2 bind m = n;"] {
        intent::apply_op(&f, &add(text)).unwrap_or_else(|e| panic!("{text}: {}", e.message));
    }
    // The same name twice is a name clash; two unnamed statements are a repeat.
    assert!(intent::apply_op(&f, &add("connect a to b;\nconnect a to b;")).unwrap_err().message.contains("allow_duplicate"));
    let mut allowed = add("connect a to b;\nconnect a to b;");
    allowed["allow_duplicate"] = json!(true);
    intent::apply_op(&f, &allowed).unwrap_or_else(|e| panic!("{}", e.message));
    // A named statement against an existing one with another name.
    let mut g = files(BASE);
    run(&mut g, add("connection x1 connect a to b;"));
    intent::apply_op(&g, &add("connection x2 connect a to b;")).unwrap_or_else(|e| panic!("{}", e.message));
    intent::apply_op(&g, &add("connect a to b;")).unwrap_or_else(|e| panic!("{}", e.message));
}

fn unreachable_op(name: &str) -> Value {
    json!({"op": "create_relationship", "kind": "connect", "name": name, "owner": "P::Sys", "source": "P::Vals::t::a", "target": "P::Vals::t::b"})
}

#[test]
fn r9_ends_the_owner_cannot_reach_are_refused_and_the_refusal_names_the_flag() {
    let f = files(BASE);
    let msg = fails(&f, unreachable_op("c1"));
    assert!(msg.contains("cannot reach") && msg.contains("allow_unreachable_ends: true"), "{msg}");
    let mut other = unreachable_op("c1");
    other["kind"] = json!("flow");
    other["allow_unreachable_ends"] = json!(true);
    assert!(fails(&f, other).contains("applies only to connect"));
}

#[test]
fn r9_allow_unreachable_ends_writes_the_ends_as_given_and_warns() {
    let mut f = files(BASE);
    let mut op = unreachable_op("c1");
    op["allow_unreachable_ends"] = json!(true);
    let info = run(&mut f, op);
    assert!(f[0].text.contains("connection c1 connect Vals::t.a to Vals::t.b;"), "{}", f[0].text);
    assert_eq!(info["warnings"][0]["rule"], "unreachable-ends", "{info}");
    assert!(info["warnings"][0]["message"].as_str().unwrap().contains("will not draw"), "{info}");
    // A reachable connector carries no warning.
    let mut ok = rel("connect", "P::Sys::a", "P::Sys::b", Some("c2"));
    ok["allow_unreachable_ends"] = json!(true);
    assert!(run(&mut f, ok).get("warnings").is_none());
}

#[test]
fn r9_export_facts_end_refs_and_text_round_trip_with_the_flag() {
    let mut f = files(BASE);
    // The shape of the reference: a connector in a definition whose ends are stated through a usage of it.
    f[0].text = f[0].text.replace("attribute n;\n", "attribute n;\n        connection c0 connect Vals::t.a to Vals::t.b;\n");
    assert!(errors(&model(&f)).is_empty(), "{:?}", errors(&model(&f)));
    let facts = agent::export_facts(&model(&f), None).unwrap();
    let c0 = facts["elements"].as_array().unwrap().iter().find(|e| e["kind"] == "connection").unwrap().clone();
    let (sref, tref) = (c0["ends"]["source"]["ref"].as_str().unwrap().to_string(), c0["ends"]["target"]["ref"].as_str().unwrap().to_string());
    let (stext, ttext) = (c0["ends"]["source"]["text"].as_str().unwrap().to_string(), c0["ends"]["target"]["text"].as_str().unwrap().to_string());
    assert_eq!((stext.as_str(), ttext.as_str()), ("Vals::t.a", "Vals::t.b"));
    // Same ends under another name: ref as printed, and text as printed.
    for (n, (s, t)) in [("r1", (&sref, &tref)), ("r2", (&stext, &ttext))] {
        let mut g = f.clone();
        let mut op = unreachable_op(n);
        op["source"] = json!(s);
        op["target"] = json!(t);
        op["allow_unreachable_ends"] = json!(true);
        run(&mut g, op);
        assert!(g[0].text.contains(&format!("connection {n} connect Vals::t.a to Vals::t.b;")), "{}", g[0].text);
    }
}
