//! Tools round 14: injection guards that read quotes as the lexer does, a value that cannot join the text beside it, control
//! characters kept out of the files, and a cap on the pairs one statement states.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, graph, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::edit::{self, EditOp};
use sysml_core::Model;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn fails(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(a) => panic!("op {op} should have failed; it wrote:\n{}", a.files[0].text),
        Err(e) => e.message,
    }
}

const ACTIONS: &str = "package P {\n    action def Act {\n        action s1;\n        action s2;\n        action s3;\n    }\n    state def St {\n        state a;\n        state b;\n    }\n}\n";

// ------------------------------------------------------------------ A1: one reading of quotes

#[test]
fn r14_a_guard_cannot_open_another_clause_behind_an_escaped_quote() {
    let f = files(ACTIONS);
    let rel = |extra: Value| {
        let mut op = json!({"op": "create_relationship", "kind": "succession", "source": "P::Act::s1", "target": "P::Act::s2"});
        for (k, v) in extra.as_object().unwrap() {
            op[k] = v.clone();
        }
        op
    };
    let msg = fails(&f, rel(json!({"guard": "\"\\\"\" then P::Act::s3"})));
    assert!(msg.contains("reserved word"), "{msg}");
    let tr = |extra: Value| {
        let mut op = json!({"op": "create_relationship", "kind": "transition", "source": "P::St::a", "target": "P::St::b"});
        for (k, v) in extra.as_object().unwrap() {
            op[k] = v.clone();
        }
        op
    };
    for (k, v) in [("guard", "\"\\\"\" do evil"), ("effect", "\"\\\"\" then evil"), ("trigger", "when \"\\\"\" if y"), ("trigger", "after \"\\\"\" if y")] {
        let msg = fails(&f, tr(json!({ k: v })));
        assert!(msg.contains("reserved word"), "{k} {v}: {msg}");
    }
}

#[test]
fn r14_a_guard_may_hold_the_clause_words_inside_quotes_and_names() {
    let mut f = files(ACTIONS);
    for guard in ["\"it's if then\" == s", "a_if_b > 1", "level > 3"] {
        let r = intent::apply_op(&f, &json!({"op": "create_relationship", "kind": "succession", "source": "P::Act::s1", "target": "P::Act::s2", "guard": guard, "allow_duplicate": true}));
        let r = r.unwrap_or_else(|e| panic!("{guard}: {}", e.message));
        f = r.files;
        let text = &f[0].text;
        assert!(text.contains(&format!("first s1 if {guard} then s2;")) || text.contains(&format!("if {guard} then")), "{text}");
    }
    // Every succession is a succession with its two ends.
    let m = model(&f);
    assert_eq!(m.elements.iter().filter(|e| e.kind == "succession").count(), 3);
}

// ------------------------------------------------------------------ A2: a value does not join the text beside it

const NOTE: &str = "package P {\n    attribute x = 1//* c */;\n    attribute y = 2;\n}\n";

#[test]
fn r14_a_value_that_would_open_a_note_is_refused() {
    let f = files(NOTE);
    for v in ["5 /", "/ 5", "*5", "5/"] {
        let msg = fails(&f, json!({"op": "update_element", "ref": "P::x", "value": v}));
        assert!(msg.contains("not a plain expression"), "{v}: {msg}");
    }
    // The ordinary value keeps the note beside it apart from the value, and y stays.
    let r = intent::apply_op(&f, &json!({"op": "update_element", "ref": "P::x", "value": "5"})).unwrap();
    assert!(r.files[0].text.contains("x = 5 //* c */;"), "{}", r.files[0].text);
    let m = model(&r.files);
    assert!(m.find("P::y").is_some() && m.find("P::x").unwrap().value.as_deref() == Some("5"));
}

#[test]
fn r14_the_edit_writer_keeps_a_space_between_new_text_and_a_slash_beside_it() {
    // The raw edit does not go through the expression guard: the writer itself must not make `///* c */`.
    let m = model(&files(NOTE));
    let changes = edit::apply(&m, &EditOp::SetValue { target: "P::x".into(), value: "5 /".into() }).unwrap();
    let after = model(&[FileIn { path: changes[0].path.clone(), text: changes[0].text.clone() }]);
    assert!(after.find("P::y").is_some(), "the value swallowed y:\n{}", changes[0].text);
    assert_eq!(after.elements.len(), m.elements.len(), "{}", changes[0].text);
}

// ------------------------------------------------------------------ A3: no control character in a file

#[test]
fn r14_control_characters_do_not_reach_the_file() {
    let f = files("package P {\n    attribute x;\n    part a;\n    part b;\n}\n");
    for v in ["\"a\\\u{0}b\"", "\"a\\\u{7}b\"", "\"a\u{7}b\""] {
        let msg = fails(&f, json!({"op": "update_element", "ref": "P::x", "value": v}));
        assert!(msg.contains("control character"), "{v:?}: {msg}");
    }
    let msg = fails(&f, json!({"op": "create_element", "owner": "P", "kind": "part", "name": "q\u{7}r"}));
    assert!(msg.contains("control character"), "{msg}");
    let msg = fails(&f, json!({"op": "create_element", "owner": "P", "kind": "part", "name": "d", "doc": "a\u{0}b"}));
    assert!(msg.contains("control character"), "{msg}");
    let msg = fails(&f, json!({"op": "update_element", "ref": "P::x", "doc": "a\u{1b}b"}));
    assert!(msg.contains("control character"), "{msg}");
    // Raw text is held to the same rule.
    let msg = fails(&f, json!({"op": "add_element", "owner": "P", "text": "part c; // \u{7}"}));
    assert!(msg.contains("control character"), "{msg}");
    // Documentation keeps its line breaks and tabs.
    let r = intent::apply_op(&f, &json!({"op": "create_element", "owner": "P", "kind": "part", "name": "d", "doc": "line one\nline two\tend"})).unwrap();
    assert!(r.files[0].text.contains("line two\tend"));
}

// ------------------------------------------------------------------ A4: the pairs one statement states are capped

#[test]
fn r14_a_statement_within_the_cap_is_not_marked_cut() {
    let m = model(&files("package P {\n    part a; part b; part c; part d;\n    dependency from a, b to c, d;\n}\n"));
    let (stmts, cuts) = graph::statements_cut(&m);
    assert_eq!(stmts.len(), 4);
    assert!(cuts.is_empty());
    let facts = agent::export_facts(&m, None).unwrap();
    assert!(facts.get("pairs_truncated").is_none());
}

#[test]
fn r14_a_repeated_statement_with_allow_duplicate_is_checked_by_what_it_wrote() {
    let mut f = files("package P {\n    part a;\n    part b;\n    dependency from a to b;\n}\n");
    let before = graph::statements(&model(&f)).len();
    for _ in 0..2 {
        let r = intent::apply_op(&f, &json!({"op": "create_relationship", "kind": "dependency", "source": "P::a", "target": "P::b", "allow_duplicate": true})).unwrap();
        f = r.files;
    }
    assert_eq!(graph::statements(&model(&f)).len(), before + 2);
}

// ------------------------------------------------------------------ C1: a chain end whose member is not declared yet

#[test]
fn r14_a_chain_end_with_an_undeclared_port_names_what_is_missing() {
    let f = files("package P {\n    port def Sig;\n    part def Sat {\n        port p0 : Sig;\n        port q0 : Sig;\n    }\n    part def Sys {\n        part gps : Sat;\n    }\n    part def Other { part x; }\n}\n");
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "connect", "source": "P::Sys::gps::p1", "target": "P::Sys::gps::q0"}));
    assert!(msg.contains("resolves") && msg.contains("no member \"p1\" yet"), "{msg}");
    assert!(msg.contains("P::Sat::p0"), "{msg}");
    assert!(!msg.contains("Did you mean") && !msg.contains("P::Other"), "{msg}");
}

// ------------------------------------------------------------------ B: a batch of creates shares analyses

const BUILD: &str = "package P {\n    part def Sat {\n        port p0;\n    }\n    package Q {\n        part x;\n    }\n    package R;\n    metadata def Tag {\n        attribute level;\n    }\n    part def Sys {\n        part gps : Sat;\n    }\n}\n";

/// A batch that mixes what a build sends: creates in different owners, several in one owner, a package made and then
/// filled, an annotation with slots stacked on one owner, a comment about an element made earlier, a repeat that the
/// one-by-one path refuses, and an edit that is not a create.
fn build_batch() -> Vec<Value> {
    let mut e: Vec<Value> = Vec::new();
    for i in 0..6 {
        e.push(json!({"op": "create_element", "owner": "P::Q", "kind": "part", "name": format!("q{i}"), "source_id": format!("id-q{i}")}));
        e.push(json!({"op": "create_element", "owner": "P::Sat", "kind": "port", "name": format!("s{i}")}));
    }
    e.push(json!({"op": "create_element", "owner": "P", "kind": "package", "name": "Fresh"}));
    e.push(json!({"op": "create_element", "owner": "P::Fresh", "kind": "part def", "name": "Inner"}));
    e.push(json!({"op": "create_element", "owner": "P::Fresh::Inner", "kind": "attribute", "name": "w", "multiplicity": "2"}));
    e.push(json!({"op": "create_element", "owner": "P::R", "kind": "part", "name": "r0"}));
    e.push(json!({"op": "create_element", "owner": "P::R", "kind": "part", "name": "r1"}));
    for i in 0..4 {
        e.push(json!({"op": "create_element", "owner": "P::Sys", "kind": "metadata", "typed_by": "P::Tag", "allow_duplicate": true, "slots": [{"name": "level", "value": format!("{i}"), "source_id": format!("slot{i}")}]}));
    }
    e.push(json!({"op": "create_element", "owner": "P::Q", "kind": "comment", "name": "note", "body": "made after q0", "about": ["P::Q::q0"]}));
    e.push(json!({"op": "create_element", "owner": "P::Sys", "kind": "part", "name": "later", "typed_by": "P::Fresh::Inner"}));
    e.push(json!({"op": "update_element", "ref": "P::Q::q1", "doc": "a barrier between creates"}));
    for i in 0..4 {
        e.push(json!({"op": "create_element", "owner": "P::Q", "kind": "part", "name": format!("t{i}")}));
    }
    e
}

fn batch_with(on: bool, f: &[FileIn], edits: &[Value]) -> Result<intent::Batch, (usize, agent::Fail)> {
    intent::set_waves(Some(on));
    let r = intent::apply_batch(f, edits);
    intent::set_waves(None);
    r
}

#[test]
fn r14_b_a_batch_of_creates_gives_the_same_result_with_fewer_analyses() {
    let f = files(BUILD);
    let edits = build_batch();
    let one = intent::analyses_run();
    let slow = batch_with(false, &f, &edits).expect("one by one");
    let slow_runs = intent::analyses_run() - one;
    let one = intent::analyses_run();
    let fast = batch_with(true, &f, &edits).expect("in waves");
    let fast_runs = intent::analyses_run() - one;
    assert_eq!(fast.files[0].text, slow.files[0].text, "the text differs");
    assert_eq!(fast.results, slow.results, "the replies differ");
    assert_eq!(format!("{:?}", fast.events), format!("{:?}", slow.events), "the id events differ");
    assert_eq!(fast.moved, slow.moved);
    assert_eq!(fast.source_id_moves, slow.source_id_moves);
    assert!(slow_runs >= edits.len(), "the one-by-one path should parse at least once per edit, did {slow_runs}");
    assert!(fast_runs * 2 <= slow_runs, "waves should halve the analyses: {fast_runs} against {slow_runs}");
}

#[test]
fn r14_b_edits_that_could_see_each_other_keep_their_order() {
    let f = files(BUILD);
    // The second names the first as its type; the third repeats a name of the first; the fourth is a repeat without allow_duplicate.
    let edits = vec![
        json!({"op": "create_element", "owner": "P::Q", "kind": "part def", "name": "D1"}),
        json!({"op": "create_element", "owner": "P::R", "kind": "part", "name": "uses", "typed_by": "P::Q::D1"}),
        json!({"op": "create_element", "owner": "P::Sys", "kind": "part", "name": "other"}),
        json!({"op": "create_element", "owner": "P::Sys", "kind": "metadata", "typed_by": "P::Tag", "allow_duplicate": true}),
        json!({"op": "create_element", "owner": "P::Sys", "kind": "metadata", "typed_by": "P::Tag", "allow_duplicate": true}),
        json!({"op": "create_element", "owner": "P::Q", "kind": "part", "name": "plain"}),
        json!({"op": "create_element", "owner": "P::R", "kind": "part", "name": "plain"}),
    ];
    let slow = batch_with(false, &f, &edits).expect("one by one");
    let fast = batch_with(true, &f, &edits).expect("in waves");
    assert_eq!(fast.files[0].text, slow.files[0].text);
    assert_eq!(fast.results, slow.results);
}

#[test]
fn r14_b_a_failing_edit_gives_the_same_refusal_in_both_paths() {
    let f = files(BUILD);
    let mut edits = build_batch();
    // The same name twice in one owner: the one-by-one path refuses the second, and a wave must not let it through.
    edits.insert(5, json!({"op": "create_element", "owner": "P::Q", "kind": "part", "name": "q4"}));
    let slow = batch_with(false, &f, &edits).expect_err("one by one");
    let fast = batch_with(true, &f, &edits).expect_err("in waves");
    assert_eq!(fast.0, slow.0);
    assert_eq!(fast.1.message, slow.1.message);
    // And an owner that does not exist.
    let mut edits = build_batch();
    edits.insert(3, json!({"op": "create_element", "owner": "P::Nowhere", "kind": "part", "name": "z"}));
    let slow = batch_with(false, &f, &edits).expect_err("one by one");
    let fast = batch_with(true, &f, &edits).expect_err("in waves");
    assert_eq!((fast.0, fast.1.message), (slow.0, slow.1.message));
}
