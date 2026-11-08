//! Tools round 17: the rest of the third security review. Each test fails without its fix.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::rules;
use sysml_core::agent::{self, intent, trace};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, RefRole};

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
    common::assert_official(f, &op.to_string());
    a.info
}

fn refused(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(a) => panic!("op {op} should have failed; it wrote:\n{}", a.files[0].text),
        Err(e) => e.message,
    }
}

/// The references of one element that come from clause text: (text, what it resolves to).
fn clause_refs(m: &Model, qn: &str) -> Vec<(String, Option<String>)> {
    let e = m.find(qn).unwrap_or_else(|| panic!("no element {qn}"));
    e.refs.iter().filter(|r| matches!(r.role, RefRole::Expr | RefRole::Call)).map(|r| (r.text.clone(), r.resolved().map(|t| m.elements[t].qualified_name.clone()))).collect()
}

// ------------------------------------------------------------------ item 1: names in clause text are references

const SM: &str = "package P {\n    item def Go;\n    part def Ctl {\n        attribute level = 1;\n        attribute limit = 5;\n        port cmd;\n        action reset;\n        state def Run {\n            state idle;\n            state busy;\n            transition t1 first idle accept Go via cmd if level > limit and busy.x == 0 do reset then busy;\n            transition t2 first busy accept after 5 [s] if not (level < 2) do send Go() via cmd then idle;\n            transition t3 first busy accept g : Go if g.v > level do assign level := g.v + 1 then idle;\n        }\n    }\n}\n";

/// The parser records each name of a trigger, a guard and an effect as a reference, and the resolver resolves it. The name
/// a trigger declares (`g`) names the payload, so a chain that starts with it is no reference; a unit is none either.
#[test]
fn r17_1_names_in_trigger_guard_and_effect_are_references_that_resolve() {
    let m = model(&files(SM));
    let some = |s: &str| Some(s.to_string());
    assert_eq!(
        clause_refs(&m, "P::Ctl::Run::t1"),
        [("Go".to_string(), some("P::Go")), ("cmd".into(), some("P::Ctl::cmd")), ("level".into(), some("P::Ctl::level")), ("limit".into(), some("P::Ctl::limit")), ("busy.x".into(), None), ("reset".into(), some("P::Ctl::reset"))]
    );
    assert_eq!(clause_refs(&m, "P::Ctl::Run::t2"), [("level".to_string(), some("P::Ctl::level")), ("Go".into(), some("P::Go")), ("cmd".into(), some("P::Ctl::cmd"))]);
    assert_eq!(clause_refs(&m, "P::Ctl::Run::t3"), [("Go".to_string(), some("P::Go")), ("level".into(), some("P::Ctl::level")), ("level".into(), some("P::Ctl::level"))]);
    // The guard of a succession and the body of a constraint are read the same way.
    let m = model(&files("package Q { attribute a = 1; action def A { action x; action y; first x if a > 0 then y; } constraint def C { in total; total < a } }"));
    let all: Vec<(String, Option<String>)> = m.elements.iter().flat_map(|e| e.refs.iter().filter(|r| r.role == RefRole::Expr).map(|r| (r.text.clone(), r.resolved().map(|t| m.elements[t].qualified_name.clone())))).collect();
    assert_eq!(all, [("a".to_string(), some("Q::a")), ("total".into(), some("Q::C::total")), ("a".into(), some("Q::a"))]);
}

/// A name in a guard, a trigger or an effect that names nothing is a finding of the semantic gate. Before, the text was
/// not read and the gate passed.
#[test]
fn r17_1_an_unresolved_name_in_a_clause_is_a_semantic_finding() {
    for (clause, name) in [("if nope > 1", "nope"), ("accept Missing", "Missing"), ("do gone", "gone"), ("if level > 1 and other.y == 2", "other")] {
        let text = format!("package P {{ part def Ctl {{ attribute level = 1; state def Run {{ state a; state b; transition first a {clause} then b; }} }} }}");
        let v = gate(&files(&text), "semantic");
        let hits: Vec<&Value> = v["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "unresolved-reference").collect();
        assert_eq!(hits.len(), 1, "{clause}: {:?}", v["findings"]);
        assert!(hits[0]["message"].as_str().unwrap().contains(&format!("'{name}'")), "{}", hits[0]["message"]);
        assert_eq!(v["passed"], false);
    }
    // The clean model passes: nothing in it is reported.
    let v = gate(&files(&SM.replace(" and busy.x == 0", "")), "semantic");
    assert_eq!(v["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "unresolved-reference").count(), 0, "{:?}", v["findings"]);
}

/// A rename rewrites the name inside the clauses, as it does every other reference. Before, the guard kept the old name and
/// pointed at nothing.
#[test]
fn r17_1_a_rename_reaches_the_names_inside_clauses() {
    let mut f = files(SM);
    apply(&mut f, json!({"op": "update_element", "ref": "P::Ctl::level", "rename": "depth"}));
    apply(&mut f, json!({"op": "update_element", "ref": "P::Go", "rename": "Start Now"}));
    apply(&mut f, json!({"op": "update_element", "ref": "P::Ctl::cmd", "rename": "cmdIn"}));
    apply(&mut f, json!({"op": "update_element", "ref": "P::Ctl::reset", "rename": "clear"}));
    let t = &f[0].text;
    assert!(t.contains("transition t1 first idle accept 'Start Now' via cmdIn if depth > limit and busy.x == 0 do clear then busy;"), "{t}");
    assert!(t.contains("transition t2 first busy accept after 5 [s] if not (depth < 2) do send 'Start Now'() via cmdIn then idle;"), "{t}");
    assert!(t.contains("transition t3 first busy accept g : 'Start Now' if g.v > depth do assign depth := g.v + 1 then idle;"), "{t}");
    assert!(!t.contains("level"), "{t}");
}

/// A delete counts the clauses that name the element: it is refused, and the reply lists the transition.
#[test]
fn r17_1_a_delete_counts_the_clauses_that_name_the_element() {
    let f = files(SM);
    for (target, holder) in [("P::Ctl::limit", "P::Ctl::Run::t1"), ("P::Ctl::reset", "P::Ctl::Run::t1"), ("P::Ctl::cmd", "P::Ctl::Run::t1"), ("P::Go", "P::Ctl::Run::t1")] {
        let e = intent::apply_op(&f, &json!({"op": "delete_element", "ref": target})).err().unwrap_or_else(|| panic!("the delete of {target} must be refused"));
        assert!(e.message.starts_with("refused:"), "{}", e.message);
        let who: Vec<&str> = e.data["referrers"].as_array().unwrap().iter().filter_map(|r| r["ref"].as_str()).collect();
        assert!(who.contains(&holder), "{target}: {who:?}");
    }
}

/// `export_facts` lists what each clause names, and `get_impact` lists the transition among what refers to the element.
#[test]
fn r17_1_export_facts_and_get_impact_report_the_names_of_a_clause() {
    let m = model(&files(SM));
    let facts = agent::export_facts(&m, None).unwrap();
    let t1 = facts["elements"].as_array().unwrap().iter().find(|e| e["ref"] == "P::Ctl::Run::t1").unwrap();
    assert_eq!(t1["names"], json!([{"text": "Go", "ref": "P::Go"}, {"text": "cmd", "ref": "P::Ctl::cmd"}, {"text": "level", "ref": "P::Ctl::level"}, {"text": "limit", "ref": "P::Ctl::limit"}, {"text": "busy.x", "ref": null}, {"text": "reset", "ref": "P::Ctl::reset"}]));
    let imp = agent::impact::get_impact(&m, &Default::default(), "P::Ctl::limit", 1, 50).unwrap();
    let other: Vec<&str> = imp["other_references"].as_array().unwrap().iter().filter_map(|r| r["ref"].as_str()).collect();
    assert_eq!(other, ["P::Ctl::Run::t1"], "{}", imp["other_references"]);
}

/// The names of a value are read by the expression grammar too: the function of a call and a name in brackets are
/// references when they resolve and no finding when they do not; a named argument and a unit are no names.
#[test]
fn r17_1_a_value_is_read_by_the_expression_grammar() {
    let m = model(&files("package P { calc def area { in w; in h; } attribute xs = (1, 2); attribute i = 1; attribute a = area(w = i, h = 2) + xs->size() + xs[i] + 3 [mm]; }"));
    assert_eq!(clause_refs(&m, "P::a"), [("area".to_string(), Some("P::area".to_string())), ("i".into(), Some("P::i".into())), ("xs".into(), Some("P::xs".into())), ("size".into(), None), ("xs".into(), Some("P::xs".into())), ("i".into(), Some("P::i".into()))]);
    assert_eq!(gate(&files(&m.files[0].text), "semantic")["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "unresolved-reference").count(), 0);
    let mut f = files(&m.files[0].text);
    apply(&mut f, json!({"op": "update_element", "ref": "P::area", "rename": "surface"}));
    apply(&mut f, json!({"op": "update_element", "ref": "P::i", "rename": "k"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("attribute a = surface(w = k, h = 2) + xs->size() + xs[k] + 3 [mm];"), "{}", f[0].text);
}

// ------------------------------------------------------------------ item 2: `accept ... via <port>`

const VIA: &str = "package P {\n    item def Go;\n    part def Ctl {\n        port cmd;\n        state def Run {\n            state idle;\n            state busy;\n        }\n    }\n}\n";

/// A trigger may name the port the signal comes in through. Before, the tool refused the text.
#[test]
fn r17_2_a_trigger_takes_via_a_port() {
    let mut f = files(VIA);
    apply(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "P::Ctl::Run::idle", "target": "P::Ctl::Run::busy", "name": "go", "trigger": "Go via cmd"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("transition go first idle accept Go via cmd then busy;"), "{}", f[0].text);
    let m = model(&f);
    assert_eq!(clause_refs(&m, "P::Ctl::Run::go"), [("Go".to_string(), Some("P::Go".to_string())), ("cmd".into(), Some("P::Ctl::cmd".into()))]);
    // What export_facts prints for the trigger is taken back as printed: the same call changes nothing.
    let facts = agent::export_facts(&m, None).unwrap();
    assert_eq!(facts["elements"].as_array().unwrap().iter().find(|e| e["ref"] == "P::Ctl::Run::go").unwrap()["trigger"], "Go via cmd");
    let before = f[0].text.clone();
    apply(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "P::Ctl::Run::idle", "target": "P::Ctl::Run::busy", "name": "go", "trigger": "Go via cmd", "if_absent": true}));
    assert_eq!(f[0].text, before);
}

/// The port must be a name that resolves, and nothing else rides in after `via`.
#[test]
fn r17_2_a_port_that_does_not_resolve_and_text_that_is_no_port_are_refused() {
    let f = files(VIA);
    let op = |t: &str| json!({"op": "create_relationship", "kind": "transition", "source": "P::Ctl::Run::idle", "target": "P::Ctl::Run::busy", "trigger": t});
    assert!(refused(&f, op("Go via nope")).contains("does not resolve"));
    assert!(refused(&f, op("Missing via cmd")).contains("Missing"));
    for bad in ["Go via", "Go via cmd then busy; part evil", "Go via cmd { part evil; }", "Go via 5", "Go via cmd via cmd", "via cmd", "Go via action evil"] {
        refused(&f, op(bad));
    }
}

// ------------------------------------------------------------------ item 3: any cut is "not checked"; a write that needs no list is written

/// One statement of 101 by 101 ends states 10201 pairs; 10000 are read. Nothing reaches the limit for the model.
fn one_statement_cut() -> String {
    let names: String = (0..101).map(|i| format!("    part fa{i};\n    part fb{i};\n")).collect();
    let list = |p: &str| (0..101).map(|i| format!("{p}{i}")).collect::<Vec<_>>().join(", ");
    format!("package P {{\n{names}    dependency from {} to {};\n    requirement def RD;\n    part def C {{\n        requirement r : RD;\n        part s;\n        part t;\n        satisfy r by s;\n    }}\n}}\n", list("fa"), list("fb"))
}

/// 1041 statements of 31 by 31 ends: over the limit for the model, so the last statements are not read at all.
fn model_cut() -> String {
    let names: String = (0..31).map(|i| format!("    part fa{i};\n    part fb{i};\n")).collect();
    let list = |p: &str| (0..31).map(|i| format!("{p}{i}")).collect::<Vec<_>>().join(", ");
    let dep: String = (0..1041).map(|_| format!("    dependency from {} to {};\n", list("fa"), list("fb"))).collect();
    format!("package P {{\n{names}{dep}    part def C {{\n        part s;\n        part t;\n    }}\n}}\n")
}

/// F8. A statement cut by the limit for one statement makes every gate that reads the pairs say "not checked" and fail.
/// Before, only one gate said so; the others passed on the part they had read.
#[test]
fn r17_3_one_cut_statement_makes_every_gate_that_reads_pairs_not_checked() {
    let f = files(&one_statement_cut());
    let (_, cuts) = agent::graph::statements_cut(&model(&f));
    assert!(cuts.len() == 1 && !cuts[0].by_total, "the fixture cuts one statement and stays under the limit for the model");
    for g in ["semantic", "traceability", "connections", "coverage"] {
        let v = gate(&f, g);
        let cut: Vec<&Value> = v["findings"].as_array().unwrap().iter().filter(|x| x["rule"] == "not-fully-checked").collect();
        assert_eq!(cut.len(), 1, "{g}: {:?}", v["findings"].as_array().unwrap().iter().map(|x| x["rule"].clone()).collect::<Vec<_>>());
        assert!(cut[0]["message"].as_str().unwrap().starts_with("not checked:"), "{g}: {}", cut[0]["message"]);
        assert_eq!(cut[0]["severity"], "error", "{g}");
        assert_eq!(v["passed"], false, "{g}");
    }
    // The syntax gate reads no pairs and is not touched.
    assert_eq!(gate(&f, "syntax")["findings"].as_array().unwrap().iter().filter(|x| x["rule"] == "not-fully-checked").count(), 0);
    let all = rules::check_all(&model(&f), None, None).unwrap();
    for g in ["semantic", "traceability", "connections", "coverage"] {
        assert_eq!(all["gates"][g]["passed"], false, "check_all {g}");
    }
}

/// F8. The repeat check of a relationship write reads the pairs, so on a cut model it says "not checked" and writes
/// nothing; `allow_duplicate` asks for no check and is written. Before, the check passed on the first 1000 pairs.
#[test]
fn r17_3_a_repeat_check_on_one_cut_statement_is_refused_as_not_checked() {
    let mut f = files(&one_statement_cut());
    // fa39 to fb39 is pair 1600 of the statement: stated, and past what is read.
    let op = json!({"op": "create_relationship", "kind": "dependency", "source": "P::fa39", "target": "P::fb39"});
    let why = refused(&f, op.clone());
    assert!(why.starts_with("not checked:") && why.ends_with("Nothing was written."), "{why}");
    let mut with = op.clone();
    with["allow_duplicate"] = json!(true);
    apply(&mut f, with);
}

/// F9. A write that adds no relationship statement needs no list of them: it is written on a cut model. Before, raw text
/// (`edit_model add_element`) was refused whatever it held.
#[test]
fn r17_3_a_write_that_adds_no_statement_is_written_on_a_cut_model() {
    for text in [model_cut(), one_statement_cut()] {
        let mut f = files(&text);
        apply(&mut f, json!({"op": "add_element", "owner": "P::C", "text": "part extra;"}));
        apply(&mut f, json!({"op": "add_element", "owner": "P", "text": "part def D { attribute n = 1; }"}));
        apply(&mut f, json!({"op": "create_element", "owner": "P::C", "kind": "part", "name": "u"}));
        apply(&mut f, json!({"op": "update_element", "ref": "P::C::s", "rename": "s2"}));
        apply(&mut f, json!({"op": "delete_element", "ref": "P::C::t"}));
        assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("part extra;") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("part def D") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("part u;") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("part s2;"), "a write was lost");
        // Text that adds a statement still needs the check, and is refused.
        let why = refused(&f, json!({"op": "add_element", "owner": "P", "text": "dependency from fa0 to fb0;"}));
        assert!(why.starts_with("not checked:"), "{why}");
    }
}

// ------------------------------------------------------------------ item 4: the work of the gates grows in step with the model

/// F6, the 22 second case. Many lines that each name a missing tag (`#Trace dependency from a to b;`) give one finding
/// each, and the semantic gate found the element of each finding by reading every diagnostic and every element. It now
/// reads them once.
#[test]
fn r17_4_the_semantic_gate_finds_the_element_of_each_finding_in_step_with_the_model() {
    let text = |k: usize| format!("package P {{\n    part a;\n    part b;\n{}}}\n", "    #Trace dependency from a to b;\n".repeat(k));
    let steps = |k: usize| {
        let m = model(&files(&text(k)));
        trace::take_work_steps();
        let v = trace::validate_model_sel(&m, Some("semantic"), &sel()).unwrap();
        assert_eq!(v["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "unresolved-reference").count(), k);
        // Each finding names its statement and its line.
        let f = v["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "unresolved-reference").nth(k - 1).unwrap().clone();
        assert_eq!((f["line"].as_u64(), f["element"].as_str().map(|e| e.starts_with("P::$"))), (Some(k as u64 + 3), Some(true)), "{f}");
        trace::take_work_steps()
    };
    let (one, two) = (steps(400), steps(800));
    assert!(one > 0 && two * 10 <= one * 22, "{one} steps for 400 lines, {two} for 800: not in step");
}

// ------------------------------------------------------------------ item 5: hidden characters by Unicode category, one test, counted each by itself

/// The characters the review got through: format characters, default-ignorables, a private-use and an unassigned code point.
const HIDDEN: &[char] = &[
    '\u{200E}', '\u{200F}', '\u{061C}', '\u{2060}', '\u{2064}', '\u{206A}', '\u{206F}', '\u{00AD}', '\u{034F}', '\u{180E}', '\u{FE00}', '\u{FE0F}', '\u{E0001}', '\u{E0020}', '\u{E007F}',
    '\u{FFF9}', '\u{FFFB}', '\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}', '\u{FFFE}', '\u{FFFF}', '\u{E000}', '\u{F8FF}', '\u{0378}', '\u{2028}', '\u{202E}', '\u{200B}', '\u{FEFF}',
];
/// Spaces that are not the plain space.
const SPACES: &[char] = &['\u{00A0}', '\u{1680}', '\u{2003}', '\u{202F}', '\u{205F}', '\u{3000}'];

const PLAIN: &str = "package P {\n    part def A {\n        attribute m = 1;\n    }\n    part a : A;\n}\n";

/// F4. One test decides for every field: a character of the categories Cf, Zl, Zp, Co, Cn or a default-ignorable one is
/// refused in a name, a short name, a value, documentation and a quoted name inside an expression. Before, a list of 13
/// code points was refused and the rest was written.
#[test]
fn r17_5_hidden_characters_are_refused_by_category_in_every_field() {
    let f = files(PLAIN);
    let mut written: Vec<String> = Vec::new();
    for c in HIDDEN {
        let s = |t: &str| t.replace('#', &c.to_string());
        let ops = [
            json!({"op": "create_element", "owner": "P", "kind": "part def", "name": s("B#C")}),
            json!({"op": "update_element", "ref": "P::A", "rename": s("A#")}),
            json!({"op": "update_element", "ref": "P::A", "short_name": s("R#1")}),
            json!({"op": "update_element", "ref": "P::A::m", "value": s("\"x#y\"")}),
            json!({"op": "update_element", "ref": "P::A::m", "value": s("'q#r' + 1")}),
            json!({"op": "update_element", "ref": "P::A", "doc": s("line one#\nline two")}),
            json!({"op": "create_element", "owner": "P", "kind": "part", "name": "b", "typed_by": s("P::'A#'")}),
            json!({"op": "rename", "target": "P::A", "new_name": s("A#")}),
            json!({"op": "set_doc", "target": "P::A", "doc": s("d#")}),
            json!({"op": "add_element", "owner": "P", "text": s("part def 'Z#';")}),
        ];
        for op in ops {
            if let Ok(a) = intent::apply_op(&f, &op) {
                if sysml_core::agent::mint::strip_ids(&a.files[0].text).contains(*c) {
                    written.push(format!("U+{:04X} by {}", *c as u32, op["op"].as_str().unwrap().to_string() + " " + &op.to_string().chars().take(60).collect::<String>()));
                }
            }
        }
    }
    assert!(written.is_empty(), "{} writes put a hidden character into the file: {:?}", written.len(), written.iter().take(12).collect::<Vec<_>>());
}

/// F4. A name is one line and its only space is the plain one: a no-break space in a name is another name that looks the
/// same. In a value or documentation such a space is text and is kept. Letters of any script and symbols stay allowed.
#[test]
fn r17_5_a_name_takes_no_space_but_the_plain_one_and_text_keeps_it() {
    let f = files(PLAIN);
    for c in SPACES {
        let s = |t: &str| t.replace('#', &c.to_string());
        for op in [
            json!({"op": "create_element", "owner": "P", "kind": "part def", "name": s("B#C")}),
            json!({"op": "update_element", "ref": "P::A", "rename": s("A#B")}),
            json!({"op": "update_element", "ref": "P::A", "short_name": s("R#1")}),
            json!({"op": "create_element", "owner": "P", "kind": "part", "name": "b", "typed_by": s("P::'A#B'")}),
            json!({"op": "update_element", "ref": "P::A::m", "value": s("'q#r' + 1")}),
        ] {
            refused(&f, op);
        }
        let mut g = f.clone();
        apply(&mut g, json!({"op": "update_element", "ref": "P::A", "doc": s("10#kg")}));
        apply(&mut g, json!({"op": "update_element", "ref": "P::A::m", "value": s("\"10#kg\"")}));
        assert_eq!(g[0].text.matches(*c).count(), 2);
    }
    let mut g = f.clone();
    for name in ["Größe", "部品", "Moteur à gaz", "Δv", "T°C", "A — B", "naïve 😀"] {
        apply(&mut g, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": name}));
        assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains(name), "{name}");
    }
}

/// F5. The gate on what a write adds counts each hidden character by itself. Before, it compared two totals, so text that
/// dropped a zero-width space and gained a right-to-left override passed.
#[test]
fn r17_5_a_new_hidden_character_does_not_ride_in_on_one_that_was_there() {
    use sysml_core::edit;
    assert_eq!(edit::gained_bad_char("a\u{200B}b", "a\u{202E}b"), Some('\u{202E}'));
    assert_eq!(edit::gained_bad_char("a\u{200B}b", "a\u{200B}b\u{200B}"), Some('\u{200B}'));
    assert_eq!(edit::gained_bad_char("a\u{200B}b\u{202E}", "\u{202E}x\u{200B}"), None);
    assert_eq!(edit::gained_bad_char("a\rb", "a\r\nb"), None);
    assert_eq!(edit::gained_bad_char("a\r\nb", "a\rb"), Some('\r'));
    // Through a write: the file holds a zero-width space in a note; raw text that swaps it for an override is refused.
    let f = files("package P {\n    part def A {\n        doc /* old\u{200B} */\n    }\n}\n");
    let why = refused(&f, json!({"op": "replace", "target": "P::A", "text": "part def A {\n    doc /* new\u{202E} */\n}"}));
    assert!(why.contains("U+202E"), "{why}");
    // Text that keeps the one it had is written.
    let mut g = f.clone();
    apply(&mut g, json!({"op": "replace", "target": "P::A", "text": "part def A {\n    doc /* still\u{200B} */\n}"}));
}

// ------------------------------------------------------------------ item 6: F10 to F14

const NS: &str = "package P {\n    part def A;\n    alias AA for A;\n    part a;\n    part b;\n    part <s1> c;\n    package Q {\n        part s1;\n        part <b2> d;\n    }\n}\n";

/// F10. A member cannot go under an alias and a short name cannot be the name or the short name of a member beside it, by
/// whatever tool the write comes. Before, only `create_element` refused; the other paths wrote it.
#[test]
fn r17_6_f10_alias_members_and_short_name_clashes_are_refused_on_every_write_path() {
    let f = files(NS);
    let under_alias = [
        json!({"op": "add_element", "owner": "P::AA", "text": "part z;"}),
        json!({"op": "create_relationship", "kind": "dependency", "source": "P::a", "target": "P::b", "owner": "P::AA"}),
        json!({"op": "update_element", "ref": "P::a", "owner": "P::AA"}),
        json!({"op": "create_element", "owner": "P::AA", "kind": "part", "name": "z"}),
    ];
    for op in under_alias {
        let why = refused(&f, op.clone());
        assert!(why.contains("alias"), "{op}: {why}");
    }
    let clash = [
        json!({"op": "update_element", "ref": "P::b", "short_name": "a"}),
        json!({"op": "update_element", "ref": "P::b", "short_name": "s1"}),
        json!({"op": "update_element", "ref": "P::b", "rename": "s1"}),
        json!({"op": "add_element", "owner": "P", "text": "part <a> e;"}),
        json!({"op": "add_element", "owner": "P", "text": "part s1;"}),
        json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "D", "features": [{"kind": "attribute", "name": "x"}, {"kind": "attribute", "name": "y", "short_name": "x"}]}),
        json!({"op": "update_element", "ref": "P::Q::s1", "owner": "P"}),
        json!({"op": "update_element", "ref": "P::c", "owner": "P::Q"}),
        json!({"op": "create_element", "owner": "P", "kind": "part", "name": "e", "short_name": "b"}),
    ];
    let mut wrote: Vec<String> = Vec::new();
    for op in clash {
        if let Ok(a) = intent::apply_op(&f, &op) {
            wrote.push(format!("{op} wrote:\n{}", a.files[0].text));
        }
    }
    assert!(wrote.is_empty(), "{} clashing writes were saved:\n{}", wrote.len(), wrote.join("\n"));
    // A batch is checked too.
    assert!(intent::apply_batch(&f, &[json!({"op": "create_element", "owner": "P", "kind": "part", "name": "n1"}), json!({"op": "update_element", "ref": "P::b", "short_name": "a"})]).is_err());
    // What is no clash is written: a short name of its own, and one equal to the element's own name.
    let mut g = f.clone();
    apply(&mut g, json!({"op": "update_element", "ref": "P::b", "short_name": "B-1"}));
    apply(&mut g, json!({"op": "add_element", "owner": "P", "text": "part <k> k;"}));
    apply(&mut g, json!({"op": "update_element", "ref": "P::Q::d", "owner": "P"}));
}

/// F10. A fault the model already had is not the edit's: other writes still go through.
#[test]
fn r17_6_f10_a_clash_that_was_there_does_not_block_other_writes() {
    let mut f = files("package P {\n    part def A;\n    alias AA for A { part old; }\n    part a;\n    part <a> b;\n}\n");
    // The fixture is no valid SysML (an alias with a member), so the writes are not sent through the official check.
    for op in [json!({"op": "create_element", "owner": "P", "kind": "part", "name": "fresh"}), json!({"op": "update_element", "ref": "P::fresh", "doc": "ok"})] {
        f = intent::apply_op(&f, &op).unwrap_or_else(|e| panic!("{op}: {}", e.message)).files;
    }
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("part fresh") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("ok"));
    assert!(refused(&f, json!({"op": "add_element", "owner": "P::AA", "text": "part more;"})).contains("alias"));
}

/// F11. `~` conjugates the type of a port. A general type takes none: before, `add_specialization` wrote `:> ~B`.
#[test]
fn r17_6_f11_a_general_type_takes_no_conjugation_mark() {
    let f = files("package P {\n    part def A;\n    part def B;\n    port def PD;\n    part x : A;\n}\n");
    let mut wrote: Vec<String> = Vec::new();
    for op in [
        json!({"op": "add_specialization", "target": "P::A", "general": "~P::B"}),
        json!({"op": "add_specialization", "target": "P::A", "general": " ~ B"}),
        json!({"op": "update_element", "ref": "P::A", "specializes": ["~P::B"]}),
        json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "C", "specializes": "~P::B"}),
        json!({"op": "create_relationship", "kind": "specialize", "source": "P::A", "target": "~P::B"}),
    ] {
        if let Ok(a) = intent::apply_op(&f, &op) {
            if sysml_core::agent::mint::strip_ids(&a.files[0].text).contains('~') {
                wrote.push(format!("{op} wrote:\n{}", a.files[0].text));
            }
        }
    }
    assert!(wrote.is_empty(), "{}", wrote.join("\n"));
    assert!(refused(&f, json!({"op": "add_specialization", "target": "P::A", "general": "~P::B"})).contains("'~'"));
    // The plain name is written, and a port still takes a conjugated type.
    let mut g = f.clone();
    apply(&mut g, json!({"op": "add_specialization", "target": "P::A", "general": "P::B"}));
    apply(&mut g, json!({"op": "create_element", "owner": "P::A", "kind": "port", "name": "p", "typed_by": "~P::PD"}));
    assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains("part def A :> P::B") && sysml_core::agent::mint::strip_ids(&g[0].text).contains("port p : ~P::PD;"), "{}", g[0].text);
}

/// F12. A name that holds a line break (written `'a\nb'` in the text) is written back with the escape. Before, the writer
/// put a real line break inside the quotes.
#[test]
fn r17_6_f12_a_line_break_in_a_name_is_written_as_its_escape() {
    assert_eq!(sysml_core::quote_name("a\nb"), "'a\\nb'");
    assert_eq!(sysml_core::quote_name("a\tb"), "'a\\tb'");
    assert_eq!(sysml_core::canonical_name("P::'a\\nb'").as_deref(), Some("P::'a\\nb'"));
    let mut f = files("package P {\n    part def 'a\\nb';\n    package Q;\n    part x : 'a\\nb';\n}\n");
    let m = model(&f);
    let def = m.elements.iter().find(|e| e.is_def).unwrap();
    assert_eq!((def.name.as_deref(), def.qualified_name.as_str()), (Some("a\nb"), "P::'a\\nb'"));
    assert!(m.find("P::'a\\nb'").is_some(), "the printed name finds the element");
    let lines = f[0].text.lines().count();
    // A new part typed by it: the tool writes the name of the element it found.
    apply(&mut f, json!({"op": "create_element", "owner": "P::Q", "kind": "part", "name": "y", "typed_by": "P::'a\\nb'"}));
    assert!(!sysml_core::agent::mint::strip_ids(&f[0].text).contains("a\nb"), "a real line break was written into a name:\n{}", f[0].text);
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("part y : P::'a\\nb';"), "{}", f[0].text);
    assert_eq!(f[0].text.lines().count(), lines + 2, "{}", f[0].text);
    let m = model(&f);
    let y = m.find("P::Q::y").unwrap();
    assert_eq!(y.first_resolved(RefRole::Type).map(|t| m.elements[t].qualified_name.clone()).as_deref(), Some("P::'a\\nb'"));
}

/// F14. A slot of an annotation sets a feature of the metadata definition. A slot the definition does not declare is
/// refused when the definition and its general types are in the model. Before, it was written and reported "0 errors".
#[test]
fn r17_6_f14_a_slot_must_be_a_feature_of_the_metadata_definition() {
    let f = files("package P {\n    metadata def Base {\n        attribute owner;\n    }\n    metadata def Review :> Base {\n        attribute level;\n    }\n    metadata def Lib :> Metadata::MetadataItem;\n    part def A;\n}\n");
    let op = |ty: &str, slot: &str| json!({"op": "create_element", "owner": "P::A", "kind": "metadata", "typed_by": ty, "slots": [{"name": slot, "value": "1"}]});
    let why = refused(&f, op("P::Review", "nope"));
    assert!(why.contains("\"nope\"") && why.contains("level") && why.contains("owner"), "{why}");
    refused(&f, op("P::Base", "level"));
    // Its own feature and an inherited one are written; a definition that specializes a library type is not checked.
    for (ty, slot) in [("P::Review", "level"), ("P::Review", "owner"), ("P::Lib", "anything")] {
        let mut g = f.clone();
        apply(&mut g, op(ty, slot));
        assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains(&format!("{slot} = 1;")), "{}", g[0].text);
    }
}

// ------------------------------------------------------------------ item 7: a name of the nearest namespace wins

fn named(m: &Model, qn: &str, role: RefRole) -> Option<String> {
    let e = m.find(qn).unwrap_or_else(|| panic!("no element {qn}"));
    e.refs.iter().find(|r| r.role == role).and_then(|r| r.resolved()).map(|t| m.elements[t].qualified_name.clone())
}

const NEAR: &str = "package Lib {\n    part def X;\n    part def Base {\n        attribute lim = 1;\n    }\n}\npackage Out {\n    part def X;\n    attribute lim = 9;\n    package In {\n        private import Lib::*;\n        part p : X;\n        part def D {\n            part q : X;\n        }\n        part def E :> Base {\n            attribute twice = lim;\n        }\n    }\n    package Plain {\n        part r : X;\n    }\n}\n";

/// KerML 8.2.3.5.4 (Full Resolution): a name is looked up in the nearest namespace first, and the names of a namespace are
/// those it owns, those it imports and those it inherits (8.2.3.5.3); only when it has none is the namespace around it
/// asked. So the `X` that `In` imports is nearer than the `X` that `Out` declares, and the `lim` that `E` inherits is
/// nearer than the `lim` of `Out`. Before, the name declared outside won: it was found before the import resolved.
#[test]
fn r17_7_an_imported_or_inherited_name_of_a_nearer_namespace_wins_over_one_declared_outside() {
    let m = model(&files(NEAR));
    let some = |s: &str| Some(s.to_string());
    assert_eq!(named(&m, "Out::In::p", RefRole::Type), some("Lib::X"));
    assert_eq!(named(&m, "Out::In::D::q", RefRole::Type), some("Lib::X"));
    assert_eq!(named(&m, "Out::In::E::twice", RefRole::Expr), some("Lib::Base::lim"));
    assert_eq!(named(&m, "Out::Plain::r", RefRole::Type), some("Out::X"));
    assert!(m.diagnostics.is_empty(), "{:?}", m.diagnostics);
}

/// The capture check agrees: an import that would bring a nearer `X` to references that name the outer one is refused,
/// and one that brings in no name in use is written.
#[test]
fn r17_7_an_import_that_would_hide_an_outer_name_in_use_is_refused() {
    let f = files(&NEAR.replace("        private import Lib::*;\n", ""));
    let m = model(&f);
    assert_eq!(named(&m, "Out::In::p", RefRole::Type), Some("Out::X".to_string()));
    let why = refused(&f, json!({"op": "create_element", "owner": "Out::In", "kind": "import", "target": "Lib::*"}));
    assert!(why.contains("would change what") && why.contains("Lib::X"), "{why}");
    let why = refused(&f, json!({"op": "add_element", "owner": "Out::In", "text": "private import Lib::*;"}));
    assert!(why.contains("would change what"), "{why}");
    let mut g = f.clone();
    apply(&mut g, json!({"op": "create_element", "owner": "Out::Plain", "kind": "import", "target": "Lib::Base"}));
}

// ------------------------------------------------------------------ item 8: the unit check reads in step with the model

/// The unit check read every member of a namespace once for each unit written in it, read the rest of a text once for
/// each bracket in it, and compared each unit of a text with every one before it. Four shapes, each twice the size: the
/// reads at most double (a little slack), and the findings are the same as before.
#[test]
fn r17_8_the_unit_check_reads_in_step_with_the_model() {
    use sysml_core::agent::units;
    let shapes: [(&str, fn(usize) -> String); 3] = [
        ("many units in one package", |k| format!("package P {{\n{}}}\n", (0..k).map(|i| format!("    attribute a{i} = 1 [zz];\n")).collect::<String>())),
        ("many units and many top-level elements", |k| format!("{}package P {{\n{}}}\n", (0..k).map(|i| format!("part t{i};\n")).collect::<String>(), (0..k).map(|i| format!("    attribute a{i} = 1 [zz];\n")).collect::<String>())),
        ("many units in one bracket", |k| format!("package P {{\n    attribute a = 1 [{}];\n}}\n", (0..k).map(|i| format!("u{i}")).collect::<Vec<_>>().join("*"))),
    ];
    let mut slow = Vec::new();
    for (what, text) in shapes {
        let steps = |k: usize| {
            let m = model(&files(&text(k)));
            units::take_work_steps();
            let n = units::gaps(&m).len();
            (units::take_work_steps(), n)
        };
        let ((one, n1), (two, n2)) = (steps(400), steps(800));
        assert!(one > 0 && (n1 == n2 || n2 == 2 * n1), "{what}: {one} steps, {n1} and {n2} findings");
        if two * 10 > one * 22 {
            slow.push(format!("{what}: {one} steps for 400, {two} for 800"));
        }
    }
    // One text, read by itself: a bracket that is never closed, and one that follows no number.
    for (what, part) in [("brackets that are never closed", "1 ["), ("brackets after no number", "x [")] {
        let steps = |k: usize| {
            units::take_work_steps();
            assert!(units::unit_names(&part.repeat(k)).is_empty());
            units::take_work_steps()
        };
        let (one, two) = (steps(400), steps(800));
        if one == 0 || two * 10 > one * 22 {
            slow.push(format!("{what}: {one} steps for 400, {two} for 800"));
        }
    }
    assert!(slow.is_empty(), "not in step:\n{}", slow.join("\n"));
    // What resolves and what does not. `USCustomaryUnits::*` in `P` gives its own units only (it imports SI privately), so
    // `m` and the made-up `zz` stay gaps there; `SI::*` gives no `ft`.
    let m = model(&files("private import SI::V;\npackage P {\n    private import USCustomaryUnits::*;\n    attribute k = 1;\n    part def A {\n        attribute a = 1 [V];\n        attribute b = 2 [ft];\n        attribute c = 3 [m];\n        attribute d = 4 [SI::m];\n        attribute e = 5 [k*zz];\n        attribute f = xs[1];\n    }\n}\npackage Q {\n    private import SI::*;\n    attribute a = 1 [m/s^2];\n    attribute b = 2 [ft];\n}\npackage R {\n    attribute a = 1 [V];\n    attribute b = 1 [m];\n    attribute c = 1 [k*zz];\n}\n"));
    let got: Vec<(String, String)> = units::gaps(&m).into_iter().map(|g| (m.elements[g.element].qualified_name.clone(), g.unit)).collect();
    let want: Vec<(String, String)> = [("P::A::c", "m"), ("P::A::e", "zz"), ("Q::b", "ft"), ("R::b", "m"), ("R::c", "zz")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(got, want);
}

/// Cameo 2026x (Magic Systems of Systems Architect SysML v2 Community, build 2026.0.0-126) could not resolve `[SI::kWh]`: the
/// official SI library has no kWh, mV or ms. The unit check reads the names the library declares, so `import SI::*` no longer
/// covers every name, a qualified `SI::kWh` is a gap, and the writer adds no import that cannot help.
#[test]
fn r17_units_not_in_the_si_library_are_gaps() {
    use sysml_core::agent::units;
    let m = model(&files("package P {\n    private import SI::*;\n    attribute a = 1 [V];\n    attribute b = 2 [kWh];\n    attribute c = 3 [SI::kWh];\n    attribute d = 4 [SI::kW*SI::h];\n    attribute e = 5 [ms];\n    attribute f = 6 [kW*h];\n}\n"));
    let got: Vec<(String, String)> = units::gaps(&m).into_iter().map(|g| (m.elements[g.element].qualified_name.clone(), g.unit)).collect();
    let want: Vec<(String, String)> = [("P::b", "kWh"), ("P::c", "SI::kWh"), ("P::e", "ms")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(got, want);
    assert!(units::declares("SI", "kW") && !units::declares("SI", "kWh") && units::declares("USCustomaryUnits", "ft"));
    assert_eq!(units::library_for("ft"), "USCustomaryUnits::*");
    assert_eq!(units::library_for("kWh"), units::DEFAULT_IMPORT);
}
