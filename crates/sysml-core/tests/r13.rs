//! Tools round 13: gaps found by the first rebuild of Discovery One. A trace count that does not depend on how an end is
//! written, slot values with an apostrophe in a double-quoted string, library names in `specializes`, and ends read the way
//! the parser reads them from the owner outward.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, graph, intent};
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

// ---------------------------------------------------------------- 1. counts do not depend on how an end is written

fn trace_src(long: bool) -> String {
    let (a, b) = if long { ("P::Funcs::Nav::step1", "P::Phys::Sys::box") } else { ("Funcs::Nav::step1", "Phys::Sys::box") };
    let (c, d) = if long { ("P::Funcs::Nav::step2", "P::Phys::Sys::box") } else { ("final", "Phys::Sys::box") };
    let (e, f) = if long { ("P::Funcs::Nav::step1", "P::Phys::Sys::Inner::deep") } else { ("Funcs::Nav::step1", "Inner::deep") };
    format!(
        "package P {{\n  package Funcs {{\n    action def Nav {{ action step1; action step2; }}\n  }}\n  alias final for Funcs::Nav::step2;\n  package Phys {{\n    part def Sys {{ part box; part other; part def Inner {{ part deep; }} allocate {e} to {f}; }}\n  }}\n  package Uses {{\n    allocate {a} to {b};\n    allocate {c} to {d};\n  }}\n}}\n"
    )
}

#[test]
fn r13_a_path_that_only_qualifies_a_name_is_no_feature_chain() {
    let m = model(&files(&trace_src(true)));
    let stmts = graph::statements(&m);
    let s = stmts.iter().find(|s| s.kind == "allocate").unwrap();
    assert!(graph::through(&m, s.via, &s.source).is_empty());
    assert!(graph::through(&m, s.via, &s.target).is_empty());
    // A chain through usages that do not own the end is a chain.
    let m = model(&files("package P { part def E { part power; } part def S { part engine : E; } part sys : S; requirement def R; requirement r : R; satisfy r by sys.engine.power; }"));
    let s = graph::statements(&m).into_iter().find(|s| s.kind == "satisfy").unwrap();
    let names: Vec<String> = graph::through(&m, s.via, &s.source).into_iter().map(|i| m.elements[i].qualified_name.clone()).collect();
    assert!(names.contains(&"P::sys".to_string()) && names.contains(&"P::S::engine".to_string()), "{names:?}");
}

// ---------------------------------------------------------------- 2. slot values

const SLOT_BASE: &str = "package P {\n    metadata def Note { attribute s; attribute t; }\n    part def T;\n    part x : T { attribute lvl; }\n}\n";

fn slot_op(value: &str) -> Value {
    json!({"op": "create_element", "kind": "metadata", "owner": "P::x", "typed_by": "P::Note", "slots": [{"name": "s", "value": value}]})
}

#[test]
fn r13_an_apostrophe_inside_a_double_quoted_string_is_text() {
    for v in ["\"Crew's vital needs (air, water) are met\"", "\"it's\"", "\"a 'quoted' word\"", "\"back\\\\slash \\\" and it's\""] {
        let mut f = files(SLOT_BASE);
        run(&mut f, slot_op(v));
        assert!(f[0].text.contains(&format!("s = {v};")), "{}", f[0].text);
        // update_element value takes the same text.
        let mut g = files(SLOT_BASE);
        run(&mut g, json!({"op": "update_element", "ref": "P::x::lvl", "value": v}));
    }
}

#[test]
fn r13_a_value_still_cannot_close_the_statement_or_open_another() {
    let f = files(SLOT_BASE);
    for v in [
        "1; part y",
        "\"a\" ; part y",
        "\"it's\"; part y",
        "\"a\" } part y {",
        "\"open",
        "'open",
        "\"a'b",
        "1 // c",
        "\"a\" /* c */",
        "\"x\\\"; part y; \"",
        "\"unterminated\\",
        "\"line\nbreak\"",
        "(1",
        "1)",
        "[1",
    ] {
        let e = fails(&f, slot_op(v));
        assert!(e.contains("not a plain expression") || e.contains("is not"), "{v:?}: {e}");
    }
    let e = fails(&f, json!({"op": "update_element", "ref": "P::x::lvl", "value": "\"it's\"; part y"}));
    assert!(e.contains("plain expression"), "{e}");
    // A nested statement did not slip into the file.
    assert!(!f[0].text.contains("part y"));
}

// ---------------------------------------------------------------- 3. library names in specializes

const LIB_BASE: &str = "package P {\n    attribute def Level;\n    attribute def Other;\n    part def Thing;\n    part t : Thing;\n}\n";

#[test]
fn r13_library_names_work_in_all_three_ways_to_specialize() {
    let mut a = files(LIB_BASE);
    run(&mut a, json!({"op": "create_element", "kind": "attribute def", "owner": "P", "name": "L1", "specializes": "ScalarValues::Real"}));
    let mut b = files(LIB_BASE);
    run(&mut b, json!({"op": "update_element", "ref": "P::Level", "specializes": "ScalarValues::Real"}));
    let mut c = files(LIB_BASE);
    let info = run(&mut c, json!({"op": "create_relationship", "kind": "specialize", "source": "P::Level", "target": "ScalarValues::Real"}));
    assert_eq!(info["statement"], "P::Level :> ScalarValues::Real", "{info}");
    for f in [&a, &b, &c] {
        assert!(f[0].text.contains(":> ScalarValues::Real"), "{}", f[0].text);
    }
    // Twice is a no-op for update_element, and a refusal (or a no-op with if_absent) for create_relationship.
    run(&mut b, json!({"op": "update_element", "ref": "P::Level", "specializes": "ScalarValues::Real"}));
    assert_eq!(b[0].text.matches("ScalarValues::Real").count(), 1);
    let e = fails(&c, json!({"op": "create_relationship", "kind": "specialize", "source": "P::Level", "target": "ScalarValues::Real"}));
    assert!(e.contains("already exists"), "{e}");
    let info = run(&mut c, json!({"op": "create_relationship", "kind": "specialize", "source": "P::Level", "target": "ScalarValues::Real", "if_absent": true}));
    assert_eq!(info["existed"], true);
    // It can be removed again.
    run(&mut b, json!({"op": "update_element", "ref": "P::Level", "remove_specializes": "ScalarValues::Real"}));
    assert!(!b[0].text.contains("ScalarValues"), "{}", b[0].text);
    // A usage does not specialize a library definition, and an unknown name is still refused.
    let e = fails(&c, json!({"op": "create_relationship", "kind": "specialize", "source": "P::t", "target": "ScalarValues::Real"}));
    assert!(e.contains("definition"), "{e}");
    let e = fails(&c, json!({"op": "create_relationship", "kind": "specialize", "source": "P::Other", "target": "Nope::Real"}));
    assert!(e.contains("no element named"), "{e}");
}

// ---------------------------------------------------------------- 4. ends read from the owner outward, keep_text

const END_BASE: &str = "package P {\n  package Funcs {\n    action def Nav { action step1; action step2; }\n  }\n  alias final for Funcs::Nav::step2;\n  package Phys {\n    part def Sys { part box; part other; }\n  }\n  package Uses {\n  }\n}\n";

fn alloc(source: &str, target: &str) -> Value {
    json!({"op": "create_relationship", "kind": "allocate", "owner": "P::Uses", "source": source, "target": target})
}

#[test]
fn r13_a_short_end_is_read_from_the_owner_outward_and_written_in_full() {
    let mut f = files(END_BASE);
    let info = run(&mut f, alloc("Funcs::Nav::step1", "Phys::Sys::box"));
    assert!(f[0].text.contains("allocate P::Funcs::Nav::step1 to P::Phys::Sys::box;"), "{}", f[0].text);
    assert_eq!(info["source"], "P::Funcs::Nav::step1");
    // An alias of the scope stands for what it names.
    let mut g = files(END_BASE);
    run(&mut g, alloc("final", "Phys::Sys::other"));
    assert!(g[0].text.contains("allocate P::Funcs::Nav::step2 to P::Phys::Sys::other;"), "{}", g[0].text);
    // satisfy and dependency read their ends the same way.
    let mut h = files("package P {\n  package Reqs { requirement def R; requirement r : R; }\n  package Sys { part def S; part s : S; }\n  package Uses {}\n}\n");
    run(&mut h, json!({"op": "create_relationship", "kind": "satisfy", "owner": "P::Uses", "source": "Sys::s", "target": "Reqs::r", "allow_definitions": false}));
    run(&mut h, json!({"op": "create_relationship", "kind": "dependency", "owner": "P::Uses", "source": "Sys::s", "target": "Reqs::r"}));
    assert!(h[0].text.contains("satisfy P::Reqs::r by P::Sys::s;") && h[0].text.contains("dependency from P::Sys::s to P::Reqs::r;"), "{}", h[0].text);
    // Without an owner there is nothing to read from, and the refusal says so.
    let e = fails(&files(END_BASE), json!({"op": "create_relationship", "kind": "allocate", "source": "Funcs::Nav::step1", "target": "Phys::Sys::box"}));
    assert!(e.contains("no element named"), "{e}");
    let e = fails(&files(END_BASE), alloc("Funcs::Nav::nope", "Phys::Sys::box"));
    assert!(e.contains("looked up from 'P::Uses' outward"), "{e}");
}

#[test]
fn r13_the_batch_tool_takes_keep_text_too() {
    let f = files(END_BASE);
    let mut op = alloc("final", "Phys::Sys::box");
    op["keep_text"] = json!(true);
    op["op"] = json!("create_relationship");
    let b = intent::apply_batch(&f, &[op]).unwrap_or_else(|e| panic!("{}", e.1.message));
    assert!(b.files[0].text.contains("allocate final to Phys::Sys::box;"));
    let _ = agent::project_state(&model(&b.files));
}
