//! Tools round 11: member order. `before` / `after` / `position` on the tools that create a member,
//! `reorder_members`, and the `index` that `export_facts` prints. Every test here fails on the round 10
//! build: the arguments are unknown there, `reorder_members` does not exist and facts carry no `index`.

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{self, ids, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, Severity};

const BASE: &str = "package P {\n    part def A;\n    part def B;\n    part def C;\n    part def Sys {\n        part a : A;\n        part b : B;\n        part c : C;\n    }\n    requirement r1;\n    requirement r2;\n}\n";

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

/// The members of `owner` in the order the text states them: a name, or `<kind>` for an unnamed member.
fn members(f: &[FileIn], owner: &str) -> Vec<String> {
    let m = model(f);
    let o = m.find(owner).unwrap_or_else(|| panic!("no {owner}"));
    o.children.iter().map(|c| if m.elements[*c].implied_name { None } else { m.elements[*c].name.clone() }.unwrap_or_else(|| format!("<{}>", m.elements[*c].kind))).collect()
}

fn lines_sorted(text: &str) -> Vec<String> {
    let mut v: Vec<String> = text.lines().map(str::to_string).collect();
    v.sort();
    v
}

#[test]
fn r11_create_element_takes_a_place_among_the_members() {
    for (place, want) in [
        (json!({ "position": "first" }), ["n", "a", "b", "c"]),
        (json!({ "position": "last" }), ["a", "b", "c", "n"]),
        (json!({ "before": "P::Sys::b" }), ["a", "n", "b", "c"]),
        (json!({ "after": "P::Sys::b" }), ["a", "b", "n", "c"]),
        (json!({ "before": "P::Sys::a" }), ["n", "a", "b", "c"]),
        (json!({ "after": "P::Sys::c" }), ["a", "b", "c", "n"]),
    ] {
        let mut f = files(BASE);
        let mut op = json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "typed_by": "P::A" });
        op.as_object_mut().unwrap().extend(place.as_object().unwrap().clone());
        let info = run(&mut f, op);
        assert_eq!(members(&f, "P::Sys"), want, "{place}");
        assert_eq!(info["ref"], "P::Sys::n");
        // The only new text is the new line.
        let mut expect = lines_sorted(BASE);
        expect.push("        part n : P::A;".into());
        expect.sort();
        assert_eq!(lines_sorted(&sysml_core::agent::mint::strip_ids(&f[0].text)), expect, "{place}");
    }
}

#[test]
fn r11_default_place_is_the_end_and_is_unchanged() {
    let mut a = files(BASE);
    let mut b = files(BASE);
    run(&mut a, json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "typed_by": "P::A" }));
    run(&mut b, json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "typed_by": "P::A", "position": "last" }));
    assert_eq!(a[0].text, b[0].text);
    assert_eq!(members(&a, "P::Sys"), ["a", "b", "c", "n"]);
}

#[test]
fn r11_an_unnamed_member_gets_the_ref_of_its_new_place() {
    let mut f = files(BASE);
    let info = run(&mut f, json!({ "op": "create_element", "owner": "P::Sys", "kind": "comment", "body": "note", "before": "P::Sys::b" }));
    assert_eq!(info["ref"], "P::Sys::$2");
    assert_eq!(info["created"][0]["ref"], "P::Sys::$2");
    assert_eq!(members(&f, "P::Sys"), ["a", "<comment>", "b", "c"]);
    // The ref in the reply names the comment, not the member that took its old number.
    assert_eq!(model(&f).find("P::Sys::$2").unwrap().kind, "comment");
    assert_eq!(info["placement"]["index"], 1);
}

#[test]
fn r11_create_relationship_takes_a_place() {
    let mut f = files(BASE);
    let info = run(&mut f, json!({ "op": "create_relationship", "kind": "satisfy", "source": "P::Sys", "target": "P::r1", "owner": "P", "before": "P::r1", "allow_definitions": true }));
    assert_eq!(members(&f, "P"), ["A", "B", "C", "Sys", "<satisfy>", "r1", "r2"]);
    assert_eq!(info["ref"], "P::$5");
    let info = run(&mut f, json!({ "op": "create_relationship", "kind": "connect", "source": "P::Sys::a", "target": "P::Sys::b", "owner": "P::Sys", "after": "P::Sys::a" }));
    assert_eq!(members(&f, "P::Sys"), ["a", "<connection>", "b", "c"]);
    assert_eq!(info["ref"], "P::Sys::$2");
    // A named allocation at the top of its owner.
    run(&mut f, json!({ "op": "create_relationship", "kind": "allocate", "source": "P::Sys", "target": "P::A", "owner": "P", "position": "first", "allow_definitions": true }));
    assert_eq!(members(&f, "P")[0], "<allocation>");
}

#[test]
fn r11_add_interface_places_the_connection_and_its_flow_together() {
    let base = "package P {\n    item def Volt;\n    part def X;\n    part def Y;\n    part def Sys {\n        part x : X;\n        part y : Y;\n        part z;\n    }\n}\n";
    let mut f = files(base);
    let info = run(&mut f, json!({ "op": "add_interface", "context": "P::Sys", "from": "P::Sys::x", "to": "P::Sys::y", "item": "P::Volt", "before": "P::Sys::y" }));
    let names = members(&f, "P::Sys");
    let at = names.iter().position(|n| n == "y").unwrap();
    assert_eq!(&names[at - 2..at], ["x_to_y", "<flow>"], "{names:?}");
    assert_eq!(names[0], "x");
    assert_eq!(info["connection"], "P::Sys::x_to_y");
    assert!(info["placement"]["owner"] == "P::Sys");
    // The same call at the front.
    let mut g = files(base);
    run(&mut g, json!({ "op": "add_interface", "context": "P::Sys", "from": "P::Sys::x", "to": "P::Sys::y", "item": "P::Volt", "position": "first" }));
    assert_eq!(&members(&g, "P::Sys")[..2], ["x_to_y", "<flow>"]);
}

#[test]
fn r11_raw_add_element_takes_a_place() {
    let mut f = files(BASE);
    let info = run(&mut f, json!({ "op": "add_element", "owner": "P::Sys", "text": "part n : P::A;", "before": "P::Sys::b" }));
    assert_eq!(members(&f, "P::Sys"), ["a", "n", "b", "c"]);
    assert_eq!(info["ref"], "P::Sys::n");
    // Two statements in one text stay together and in order.
    run(&mut f, json!({ "op": "add_element", "owner": "P::Sys", "text": "part m : P::A;\npart o : P::A;", "after": "P::Sys::a" }));
    assert_eq!(members(&f, "P::Sys"), ["a", "m", "o", "n", "b", "c"]);
}

#[test]
fn r11_a_place_must_be_a_member_of_the_owner_and_the_refusal_says_which() {
    let f = files(BASE);
    for (key, tool) in [("before", "create_element"), ("after", "create_element")] {
        let m = fails(&f, json!({ "op": tool, "owner": "P::Sys", "kind": "part", "name": "n", "typed_by": "P::A", key: "P::B" }));
        assert!(m.contains("not a member of 'P::Sys'") && m.contains("has 3 members"), "{m}");
        assert!(m.contains("a member of 'P'"), "{m}");
    }
    let m = fails(&f, json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "typed_by": "P::A", "before": "P::Sys::nope" }));
    assert!(m.contains("not a member of 'P::Sys'") && m.contains("no such element"), "{m}");
    let m = fails(&f, json!({ "op": "create_relationship", "kind": "connect", "source": "P::Sys::a", "target": "P::Sys::b", "owner": "P::Sys", "after": "P::r1" }));
    assert!(m.contains("not a member of 'P::Sys'"), "{m}");
    let m = fails(&f, json!({ "op": "add_element", "owner": "P::Sys", "text": "part n;", "before": "P::A" }));
    assert!(m.contains("has 3 members"), "{m}");
    // Nothing else is accepted.
    let m = fails(&f, json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "before": "P::Sys::a", "after": "P::Sys::b" }));
    assert!(m.contains("one of before, after and position"), "{m}");
    let m = fails(&f, json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "position": "middle" }));
    assert!(m.contains("'first' or 'last'"), "{m}");
    let m = fails(&f, json!({ "op": "create_element", "kind": "package", "name": "Q", "position": "first" }));
    assert!(m.contains("top-level"), "{m}");
    let m = fails(&f, json!({ "op": "update_element", "ref": "P::A", "doc": "x", "before": "P::B" }));
    assert!(m.contains("apply to create_element"), "{m}");
}

#[test]
fn r11_an_existing_element_is_not_moved_by_if_absent() {
    let mut f = files(BASE);
    let before = f[0].text.clone();
    let info = run(&mut f, json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "c", "typed_by": "P::C", "if_absent": true, "position": "first" }));
    assert_eq!(info["existed"], true);
    assert_eq!(f[0].text, before);
}

#[test]
fn r11_reorder_moves_statements_and_nothing_else() {
    let mut f = files(BASE);
    let info = run(&mut f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::c", "P::Sys::a", "P::Sys::b"] }));
    assert_eq!(members(&f, "P::Sys"), ["c", "a", "b"]);
    assert_eq!(info["moved"], 3);
    assert_eq!(info["members"], 3);
    assert_eq!(lines_sorted(&f[0].text), lines_sorted(BASE));
    assert!(f[0].text.starts_with("package P {\n    part def A;\n"));
    // The way back restores the exact bytes.
    run(&mut f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::a", "P::Sys::b", "P::Sys::c"] }));
    assert_eq!(f[0].text, BASE);
}

#[test]
fn r11_members_not_listed_keep_their_order_after_the_listed_ones() {
    let mut f = files(BASE);
    let info = run(&mut f, json!({ "op": "reorder_members", "owner": "P", "order": ["P::r2", "P::C"] }));
    assert_eq!(members(&f, "P"), ["r2", "C", "A", "B", "Sys", "r1"]);
    assert_eq!(info["listed"], 2);
    assert_eq!(info["unlisted"], 4);
    // A bare member name of the owner is accepted.
    let mut g = files(BASE);
    run(&mut g, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["c"] }));
    assert_eq!(members(&g, "P::Sys"), ["c", "a", "b"]);
}

#[test]
fn r11_an_order_that_is_already_true_writes_nothing() {
    let f = files(BASE);
    let a = intent::apply_op(&f, &json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::a", "P::Sys::b", "P::Sys::c"] })).unwrap();
    assert_eq!(a.files[0].text, BASE);
    assert_eq!(a.info["moved"], 0);
    let b = intent::apply_op(&f, &json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::a"] })).unwrap();
    assert_eq!(b.files[0].text, BASE);
}

const NOTES: &str = "package P {\n    part def A;\n    // note for B\n    // second line\n    part def B; // trailing B\n\n    // not attached: blank line before C\n\n    part def C {\n        doc /* c doc */\n        part x : A;\n    }\n    /* block comment member */\n    requirement r1;\n    satisfy r1 by A;\n}\n";

#[test]
fn r11_notes_and_comments_move_with_their_member() {
    let mut f = files(NOTES);
    let ids_before = members(&f, "P");
    assert_eq!(ids_before, ["A", "B", "C", "<comment>", "r1", "<satisfy>"]);
    run(&mut f, json!({ "op": "reorder_members", "owner": "P", "order": ["P::B", "P::C", "P::A"] }));
    let t = &f[0].text;
    // The two note lines and the trailing note stay with B.
    assert!(t.contains("    // note for B\n    // second line\n    part def B; // trailing B\n"), "{t}");
    // B is now first, so its notes come right after the opening brace.
    assert!(t.starts_with("package P {\n    // note for B\n    // second line\n    part def B; // trailing B\n"), "{t}");
    // The note behind a blank line stays where it was, in its slot, and C moves without it.
    assert!(t.contains("    // not attached: blank line before C\n"), "{t}");
    assert_eq!(members(&f, "P"), ["B", "C", "A", "<comment>", "r1", "<satisfy>"]);
    // Same lines, only moved.
    assert_eq!(lines_sorted(t), lines_sorted(NOTES));
    // The comment is a member of its own and can be moved too.
    run(&mut f, json!({ "op": "reorder_members", "owner": "P", "order": ["P::$4"] }));
    assert_eq!(members(&f, "P")[0], "<comment>");
    // The doc inside C moved with C.
    assert!(model(&f).find("P::C").unwrap().doc.as_deref() == Some("c doc"));
}

#[test]
fn r11_a_trailing_note_never_swallows_the_next_statement_on_a_single_line_body() {
    let mut f = files("package P {\n    part def S { part a; // about a\n    }\n}\n");
    run(&mut f, json!({ "op": "reorder_members", "owner": "P::S", "order": ["P::S::a"] }));
    let mut g = files("package P {\n    part def S { part a; part b; // about b\n    }\n}\n");
    run(&mut g, json!({ "op": "reorder_members", "owner": "P::S", "order": ["P::S::b", "P::S::a"] }));
    assert_eq!(members(&g, "P::S"), ["b", "a"]);
    let t = &g[0].text;
    assert!(t.contains("part b; // about b\n"), "{t}");
}

const KINDS: &str = "package P {\n    part def A;\n    part def B;\n    requirement r1;\n    action def Act {\n        action s1;\n        action s2;\n        first s1 then s2;\n    }\n    state def St {\n        state p;\n        state q;\n        transition t1 first p then q;\n    }\n    part def Sys {\n        part a : A;\n        part b : B;\n        connection cc connect a to b;\n        flow of A from a to b;\n    }\n    import ScalarValues::*;\n    satisfy r1 by Sys;\n    allocate Act to Sys;\n    metadata def Tag;\n    @Tag;\n    comment about A /* a note */\n    alias Al for A;\n}\n";

#[test]
fn r11_every_statement_kind_can_be_moved() {
    for owner in ["P", "P::Act", "P::St", "P::Sys"] {
        let mut f = files(KINDS);
        let before = members(&f, owner);
        let m = model(&f);
        let o = m.find(owner).unwrap();
        // Reverse the whole owner by refs, whatever the kinds are.
        let order: Vec<Value> = o.children.iter().rev().map(|c| json!(m.elements[*c].qualified_name)).collect();
        let info = run(&mut f, json!({ "op": "reorder_members", "owner": owner, "order": order }));
        let mut want = before.clone();
        want.reverse();
        assert_eq!(members(&f, owner), want, "{owner}");
        assert_eq!(info["members"], before.len());
        assert_eq!(lines_sorted(&f[0].text), lines_sorted(KINDS), "{owner}");
    }
    // The kinds present in P: package members of eight statement kinds.
    let m = model(&files(KINDS));
    let kinds: Vec<String> = m.find("P").unwrap().children.iter().map(|c| m.elements[*c].kind.clone()).collect();
    for k in ["satisfy", "allocation", "import", "metadata", "comment", "alias"] {
        assert!(kinds.iter().any(|x| x == k), "{k} missing from {kinds:?}");
    }
    let sys: Vec<String> = m.find("P::Sys").unwrap().children.iter().map(|c| m.elements[*c].kind.clone()).collect();
    assert!(sys.contains(&"connection".to_string()) && sys.contains(&"flow".to_string()), "{sys:?}");
}

#[test]
fn r11_unnamed_statements_are_addressed_by_their_dollar_ref_and_the_reply_warns() {
    let mut f = files(KINDS);
    let before = members(&f, "P");
    let n = before.len();
    let satisfy = format!("P::${}", before.iter().position(|k| k == "<satisfy>").unwrap() + 1);
    let info = run(&mut f, json!({ "op": "reorder_members", "owner": "P", "order": [satisfy] }));
    assert_eq!(members(&f, "P")[0], "<satisfy>");
    assert_eq!(info["moved"].as_u64().unwrap() as usize, before.iter().position(|k| k == "<satisfy>").unwrap() + 1);
    assert_eq!(info["warnings"][0]["rule"], "positional-refs-renumbered");
    assert!(info["renumbered_count"].as_u64().unwrap() > 0);
    // The renumbering table says where each unnamed ref went.
    let rows = info["renumbered"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["from"] == satisfy.as_str() && r["to"] == "P::$1"), "{rows:?}");
    assert_eq!(members(&f, "P").len(), n);
    // A reorder of named members only renumbers nothing and does not warn.
    let mut g = files(BASE);
    let info = run(&mut g, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::c"] }));
    assert!(info.get("warnings").is_none() && info.get("renumbered").is_none(), "{info}");
}

#[test]
fn r11_reorder_refusals_name_the_owner_and_its_member_count() {
    let f = files(BASE);
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::A"] }));
    assert!(m.contains("not a member of 'P::Sys'") && m.contains("has 3 members") && m.contains("a member of 'P'"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::zzz"] }));
    assert!(m.contains("no member \"P::Sys::zzz\"") && m.contains("has 3 members"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::a", "P::Sys::a"] }));
    assert!(m.contains("listed twice"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": [] }));
    assert!(m.contains("order is empty"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Sys" }));
    assert!(m.contains("missing argument 'order'"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "order": ["P::A"] }));
    assert!(m.contains("missing argument 'owner'"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::A", "order": ["P::Sys::a"] }));
    assert!(m.contains("not a member of 'P::A'") && m.contains("has 0 members"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Nope", "order": ["P::A"] }));
    assert!(m.contains("no element named"), "{m}");
    let m = fails(&f, json!({ "op": "reorder_members", "owner": "P::Sys", "order": [3] }));
    assert!(m.contains("must be a ref"), "{m}");
}

#[test]
fn r11_a_batch_places_and_reorders_in_one_step_and_follows_source_ids() {
    let mut map = BTreeMap::new();
    map.insert("id-a".to_string(), "P::Sys::a".to_string());
    map.insert("id-c".to_string(), "P::Sys::c".to_string());
    let edits = vec![
        json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "n", "typed_by": "P::A", "after": "source_id:id-a", "source_id": "id-n" }),
        json!({ "op": "create_element", "owner": "P::Sys", "kind": "part", "name": "m", "typed_by": "P::A", "position": "first", "source_id": "id-m" }),
        json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["source_id:id-c", "source_id:id-n"] }),
    ];
    let b = intent::apply_batch_ids(&files(BASE), &edits, &map).unwrap_or_else(|e| panic!("{}", e.1.message));
    assert_eq!(members(&b.files, "P::Sys"), ["c", "n", "m", "a", "b"]);
    assert_eq!(b.results.len(), 3);
    assert_eq!(b.results[2]["moved"].as_u64().unwrap() > 0, true);
    assert!(errors(&model(&b.files)).is_empty());
    // A failing edit names its number and nothing is written.
    let bad = vec![edits[0].clone(), json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::A"] })];
    let e = intent::apply_batch_ids(&files(BASE), &bad, &map).unwrap_err();
    assert_eq!(e.0, 1);
    assert!(e.1.message.contains("not a member of 'P::Sys'"), "{}", e.1.message);
}

#[test]
fn r11_source_ids_of_unnamed_statements_follow_a_reorder() {
    let f = files(KINDS);
    let m = model(&f);
    let p = m.find("P").unwrap();
    let pos = |k: &str| p.children.iter().position(|c| m.elements[*c].kind == k).unwrap();
    let (sat, imp) = (pos("satisfy"), pos("import"));
    let mut map = BTreeMap::new();
    map.insert("id-sat".to_string(), format!("P::${}", sat + 1));
    map.insert("id-imp".to_string(), format!("P::${}", imp + 1));
    map.insert("id-b".to_string(), "P::B".to_string());
    let edits = vec![json!({ "op": "reorder_members", "owner": "P", "order": ["source_id:id-sat", "source_id:id-imp"] })];
    let b = intent::apply_batch_ids(&f, &edits, &map).unwrap_or_else(|e| panic!("{}", e.1.message));
    let mut after = map.clone();
    ids::apply_events(&mut after, &b.events).unwrap();
    assert_eq!(after["id-sat"], "P::$1");
    assert_eq!(after["id-imp"], "P::$2");
    assert_eq!(after["id-b"], "P::B");
    // The id now names the same statement.
    let m2 = model(&b.files);
    assert_eq!(m2.find(&after["id-sat"]).unwrap().kind, "satisfy");
    assert_eq!(m2.find(&after["id-imp"]).unwrap().kind, "import");
    // A positional ref written for the text before the batch names that statement, also after an earlier edit renumbered it.
    let stale = vec![edits[0].clone(), json!({ "op": "reorder_members", "owner": "P", "order": [format!("P::${}", sat + 1)] })];
    let b = intent::apply_batch_ids(&f, &stale, &map).unwrap_or_else(|e| panic!("{}", e.1.message));
    assert_eq!(model(&b.files).find("P::$1").unwrap().kind, "satisfy");
    // A positional ref whose statement an earlier edit deleted is refused.
    let gone = vec![json!({ "op": "delete_element", "ref": format!("P::${}", sat + 1) }), json!({ "op": "set_source_id", "ref": format!("P::${}", sat + 1), "source_id": "x" })];
    let e = intent::apply_batch_ids(&f, &gone, &map).unwrap_err();
    assert!(e.1.message.contains("positional ref"), "{}", e.1.message);
}

#[test]
fn r11_export_facts_gives_every_element_and_edge_an_index() {
    let f = files(KINDS);
    let m = model(&f);
    let facts = agent::export_facts(&m, None).unwrap();
    let els = facts["elements"].as_array().unwrap();
    let index_of = |r: &str| els.iter().find(|e| e["ref"] == r).unwrap()["index"].as_u64().unwrap() as usize;
    for e in els {
        assert!(e["index"].is_u64(), "{e}");
        assert!(e["line"].is_u64(), "line is kept: {e}");
    }
    // Sorted by index, the members of an owner come in the order of the text, a doc statement counted in the gaps.
    for owner in ["P", "P::Sys", "P::Act", "P::St", "P::St"] {
        let mut kids: Vec<(usize, u64)> = els.iter().filter(|e| e["owner"] == owner).map(|e| (e["index"].as_u64().unwrap() as usize, e["line"].as_u64().unwrap())).collect();
        kids.sort();
        assert!(kids.windows(2).all(|w| w[0].1 <= w[1].1), "{owner}: {kids:?}");
    }
    assert_eq!(index_of("P::A"), 0);
    assert_eq!(index_of("P::B"), 1);
    assert_eq!(index_of("P::Sys::b"), 1);
    let o = m.find("P").unwrap();
    for (i, c) in o.children.iter().enumerate() {
        assert_eq!(index_of(&m.elements[*c].qualified_name), i);
    }
    // An edge carries the index of the statement that declares it.
    let edge = facts["edges"].as_array().unwrap().iter().find(|e| e["kind"] == "satisfy").unwrap();
    assert_eq!(edge["index"].as_u64().unwrap() as usize, index_of(edge["declared_at"]["element"].as_str().unwrap()));
    assert_eq!(edge["declared_at"]["index"], edge["index"]);
    assert!(edge["declared_at"]["line"].is_u64());
    for e in facts["edges"].as_array().unwrap() {
        assert!(e["index"].is_u64(), "{e}");
    }
    // After a reorder the facts say the new order.
    let mut g = f.clone();
    run(&mut g, json!({ "op": "reorder_members", "owner": "P::Sys", "order": ["P::Sys::b", "P::Sys::a"] }));
    let facts = agent::export_facts(&model(&g), None).unwrap();
    let els = facts["elements"].as_array().unwrap();
    let at = |r: &str| els.iter().find(|e| e["ref"] == r).unwrap()["index"].as_u64().unwrap();
    assert_eq!((at("P::Sys::b"), at("P::Sys::a")), (0, 1));
}

#[test]
fn r11_crlf_text_and_multibyte_names_move_byte_for_byte() {
    let text = "package P {\r\n    part def S {\r\n        // Größe note\r\n        part 'größe';\r\n        part 'λ x'; // trailing λ\r\n        part c;\r\n    }\r\n}\r\n";
    let mut f = files(text);
    run(&mut f, json!({ "op": "reorder_members", "owner": "P::S", "order": ["P::S::c", "P::S::'λ x'"] }));
    assert_eq!(members(&f, "P::S"), ["c", "λ x", "größe"]);
    let t = &f[0].text;
    assert!(t.contains("        part 'λ x'; // trailing λ\r\n"), "{t:?}");
    assert!(t.contains("        // Größe note\r\n        part 'größe';\r\n"), "{t:?}");
    assert!(!t.contains("\n\n") && t.matches("\r\n").count() == text.matches("\r\n").count());
    let mut a: Vec<&str> = t.split("\r\n").collect();
    let mut b: Vec<&str> = text.split("\r\n").collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    run(&mut f, json!({ "op": "reorder_members", "owner": "P::S", "order": ["P::S::'größe'", "P::S::'λ x'", "P::S::c"] }));
    assert_eq!(f[0].text, text);
}
