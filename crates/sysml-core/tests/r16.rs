//! Tools round 16: the official-grammar gate, the check on every saved file, and the four write gaps.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::rules::{self, OfficialVerdict};
use sysml_core::agent::{self, graph, intent, trace};
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

const BAD_FILE: &str = "package P {\n    part def A;\n    part x : A [1..];\n}\n";
const GOOD_FILE: &str = "package P {\n    part def A;\n    part x : A;\n}\n";

fn sel<'a>() -> rules::Selection<'a> {
    rules::Selection { gates: None, rules: None, groups: None, require: None }
}

// ------------------------------------------------------------------ 1: the official gate

#[test]
fn r16_the_official_gate_names_file_line_column_token_and_what_was_allowed() {
    let m = model(&[FileIn { path: "ok.sysml".into(), text: GOOD_FILE.into() }, FileIn { path: "bad.sysml".into(), text: BAD_FILE.into() }]);
    let v = trace::validate_model_sel(&m, Some("official"), &sel()).unwrap();
    assert_eq!(v["passed"], false);
    let f = v["findings"].as_array().unwrap();
    assert_eq!(f.len(), 1, "{v}");
    assert_eq!((f[0]["gate"].as_str(), f[0]["rule"].as_str(), f[0]["severity"].as_str()), (Some("official"), Some("official"), Some("error")));
    assert_eq!((f[0]["file"].as_str(), f[0]["line"].as_u64(), f[0]["column"].as_u64(), f[0]["found"].as_str()), (Some("bad.sysml"), Some(3), Some(20), Some("]")));
    assert!(!f[0]["allowed"].as_array().unwrap().is_empty() && f[0]["allowed"].as_array().unwrap().len() <= 13);
    assert!(f[0]["message"].as_str().unwrap().contains("bad.sysml:3:20"), "{}", f[0]["message"]);
    // A good model passes.
    let ok = trace::validate_model_sel(&model(&files(GOOD_FILE)), Some("official"), &sel()).unwrap();
    assert_eq!(ok["passed"], true);
}

#[test]
fn r16_the_official_gate_is_opt_in_for_validate_model_and_on_in_check_all() {
    let m = model(&files(BAD_FILE));
    let default = trace::validate_model(&m, None).unwrap();
    assert!(default["gates"].as_object().unwrap().get("official").is_none());
    let all = rules::check_all(&m, None, None).unwrap();
    assert_eq!(all["gates"]["official"]["passed"], false);
    assert_eq!(all["passed"], false);
    assert!(all["by_rule"].as_array().unwrap().iter().any(|r| r["rule"] == "official" && r["count"] == 1));
    // A rule filter pulls the gate in by its rule id.
    let by_rule = trace::validate_model_sel(&m, None, &rules::Selection { gates: None, rules: Some(vec!["official".into()]), groups: None, require: None }).unwrap();
    assert_eq!(by_rule["findings"].as_array().unwrap().len(), 1);
    assert!(rules::list_rules(Some("official"), None).unwrap()["count"] == 1);
}

#[test]
fn r16_a_file_the_checker_cannot_read_whole_gets_a_note_not_an_error() {
    let m = model(&files(GOOD_FILE));
    let f = rules::official_findings_with(&m, &|_| (OfficialVerdict::NotChecked("a text the recogniser reads in more than linear time".into()), 0));
    assert_eq!(f.len(), 1);
    assert_eq!((f[0]["rule"].as_str(), f[0]["severity"].as_str(), f[0]["gate"].as_str()), (Some(graph::CUT_RULE), Some("warning"), Some("official")));
    assert!(f[0]["message"].as_str().unwrap().starts_with("not checked:"), "{}", f[0]["message"]);
    // The real checker with a tiny limit says the same.
    let tiny = sysml_conform::Limits { max_tokens: 2, ..sysml_conform::Limits::default() };
    let checker = sysml_conform::Checker::new();
    let got = checker.check_str(GOOD_FILE, &tiny).verdict;
    assert!(matches!(got, sysml_conform::Verdict::NotChecked(_)));
    // A file of any size is read: there is no size limit.
    let big = format!("package Big {{\n{}}}\n", (0..20_000).map(|i| format!("  part def P{i};\n")).collect::<String>());
    assert!(big.len() > 300_000);
    assert_eq!(checker.check_str(&big, &sysml_conform::Limits::default()).verdict, sysml_conform::Verdict::Valid);
}

#[test]
fn r16_a_build_without_the_checker_says_so_in_a_note() {
    // The registered checker is global, so the no-checker reply is read through the same function with a checker that cannot run.
    let m = model(&files(GOOD_FILE));
    let f = rules::official_findings_with(&m, &|_| (OfficialVerdict::NotChecked("no checker".into()), 0));
    assert_eq!(f[0]["severity"], "warning");
}

#[test]
fn r16_every_bad_file_of_the_conformance_set_fails_the_official_and_the_syntax_gate() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sysml-conform/tests/bad");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "sysml").unwrap_or(false)).collect();
    names.sort();
    assert_eq!(names.len(), 46);
    let mut problems = Vec::new();
    for p in names {
        let text = std::fs::read_to_string(&p).unwrap();
        let m = api::to_model(files(&text));
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let o = trace::validate_model_sel(&m, Some("official"), &sel()).unwrap();
        if o["passed"] != false {
            problems.push(format!("{name}: official gate passed"));
        }
        let s = trace::validate_model_sel(&m, Some("syntax"), &sel()).unwrap();
        // The engine reads a bare name that starts with a digit (`5out`) and every write quotes it, so
        // only the official gate (the file as it stands) refuses that one.
        let lenient = name == "46-name-starts-with-digit.sysml";
        if (s["passed"] != false) != lenient {
            problems.push(format!("{name}: syntax gate {}", if lenient { "refused a digit-first name" } else { "passed" }));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

// ------------------------------------------------------------------ 2: every saved file is valid

#[test]
fn r16_the_write_observer_runs_and_refuses_text_the_grammar_rejects() {
    let before = common::OBSERVED.load(std::sync::atomic::Ordering::SeqCst);
    let f = files(GOOD_FILE);
    intent::apply_op(&f, &json!({"op": "create_element", "kind": "part def", "name": "B", "owner": "P"})).unwrap();
    assert!(common::OBSERVED.load(std::sync::atomic::Ordering::SeqCst) > before);
    // The observer fails on a bad result of an intent op, and lets a raw op through.
    let bad = files(BAD_FILE);
    let op = json!({"op": "create_element"});
    let caught = std::panic::catch_unwind(|| common::observe(&[&op], &files(GOOD_FILE), &files(BAD_FILE)));
    assert!(caught.is_err());
    let raw = json!({"op": "replace"});
    common::observe(&[&raw], &files(GOOD_FILE), &bad);
}

// ------------------------------------------------------------------ 3: the four gaps

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

fn text(f: &[FileIn]) -> String {
    sysml_core::agent::mint::strip_ids(&f[0].text)
}

fn gate(f: &[FileIn], g: &str) -> Value {
    trace::validate_model_sel(&model(f), Some(g), &sel()).unwrap()
}

fn rules_hit(v: &Value) -> Vec<String> {
    v["findings"].as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap().to_string()).collect()
}

const NET: &str = "package P {\n    item def Signal;\n    port def SigPort { out item sig : Signal; }\n    part def Source { port o : SigPort; }\n    part def Sink { port i : ~SigPort; }\n    interface def Link;\n    part def Sys {\n        part src : Source;\n        part snk : Sink;\n    }\n}\n";

fn ends_of(f: &[FileIn], name: &str) -> (String, String) {
    let facts = agent::export_facts(&model(f), None).unwrap();
    let e = facts["elements"].as_array().unwrap().iter().find(|e| e["ref"] == name).unwrap_or_else(|| panic!("{name} not listed"));
    (e["ends"]["source"]["ref"].as_str().unwrap().to_string(), e["ends"]["target"]["ref"].as_str().unwrap().to_string())
}

#[test]
fn r16_a_an_interface_usage_is_written_with_its_ends() {
    let mut f = files(NET);
    let info = apply(&mut f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "lnk", "typed_by": "P::Link", "source": "P::Sys::src::o", "target": "P::Sys::snk::i"}));
    assert_eq!(info["ref"], "P::Sys::lnk");
    assert!(text(&f).contains("interface lnk : P::Link connect src.o to snk.i;"), "{}", text(&f));
    // Read back: the usage keeps its type and has two ends, and the connection views and checks see it.
    assert_eq!(ends_of(&f, "P::Sys::lnk"), ("P::Sys::src::o".to_string(), "P::Sys::snk::i".to_string()));
    let m = model(&f);
    assert_eq!(m.find("P::Sys::lnk").unwrap().kind, "interface");
    assert_eq!(connect_errors(&f), 0);
    let facts = agent::export_facts(&m, None).unwrap();
    assert!(facts["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "connect" && e["declared_at"]["element"] == "P::Sys::lnk"), "{facts}");
    // Without a type, and without ends, as before.
    apply(&mut f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "bare", "source": "P::Sys::snk::i", "target": "P::Sys::src::o"}));
    assert!(text(&f).contains("interface bare connect snk.i to src.o;"));
    apply(&mut f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "empty", "typed_by": "P::Link"}));
    assert!(text(&f).contains("interface empty : P::Link;"));
}

fn connect_errors(f: &[FileIn]) -> u64 {
    gate(f, "connections")["errors"].as_u64().unwrap()
}

#[test]
fn r16_a_the_ends_of_an_interface_usage_are_checked() {
    let f = files(NET);
    let m = refused(&f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "x", "source": "P::Source", "target": "P::Sys::snk::i"}));
    assert!(m.contains("definition"), "{m}");
    let m = refused(&f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "x", "source": "P::Sys::src::o"}));
    assert!(m.contains("both ends or none"), "{m}");
    let m = refused(&f, json!({"op": "create_element", "kind": "interface", "name": "x", "source": "P::Sys::src::o", "target": "P::Sys::snk::i"}));
    assert!(m.contains("owner"), "{m}");
    let m = refused(&f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "x", "source": "P::Sys::src::o", "target": "P::Sys::nope"}));
    assert!(m.contains("nope"), "{m}");
    // Another kind does not take source and target.
    let m = refused(&f, json!({"op": "create_element", "owner": "P::Sys", "kind": "part", "name": "x", "source": "P::Sys::src::o", "target": "P::Sys::snk::i"}));
    assert!(m.contains("does not apply"), "{m}");
    // Ends cannot be used to smuggle text.
    let m = refused(&f, json!({"op": "create_element", "owner": "P::Sys", "kind": "interface", "name": "x", "source": "P::Sys::src::o; part evil", "target": "P::Sys::snk::i"}));
    assert!(!m.is_empty());
}

const RIG: &str = "package P {\n    calc def Double { in attribute x : ScalarValues::Real; }\n    constraint def Limit { in attribute v : ScalarValues::Real; }\n    part def Rig {\n        attribute a : ScalarValues::Real = 2;\n        attribute total : ScalarValues::Real = a * 2;\n    }\n}\n";

fn expression_of(f: &[FileIn], owner: &str) -> Option<String> {
    let m = model(f);
    let o = m.find(owner)?;
    let v = m.children(o.id).find(|c| c.kind == "expression").and_then(|c| c.value.clone());
    v
}

#[test]
fn r16_b_a_constraint_or_calc_usage_has_an_expression_body() {
    let mut f = files(RIG);
    apply(&mut f, json!({"op": "create_element", "owner": "P::Rig", "kind": "constraint", "name": "lim", "typed_by": "P::Limit", "body": "total < 100"}));
    assert!(text(&f).contains("constraint lim : P::Limit {\n"), "{}", text(&f));
    assert_eq!(expression_of(&f, "P::Rig::lim").as_deref(), Some("total < 100"));
    apply(&mut f, json!({"op": "create_element", "owner": "P::Rig", "kind": "calc", "name": "dbl", "typed_by": "P::Double", "body": "a * 2"}));
    assert_eq!(expression_of(&f, "P::Rig::dbl").as_deref(), Some("a * 2"));
    // A definition takes the body too; it comes after the parameters.
    apply(&mut f, json!({"op": "create_element", "owner": "P", "kind": "constraint def", "name": "Pos", "features": [{"kind": "parameter", "name": "y", "typed_by": "ScalarValues::Real", "direction": "in"}], "body": "y > 0"}));
    assert_eq!(expression_of(&f, "P::Pos").as_deref(), Some("y > 0"));
    // With a doc, the doc comes first and the expression last.
    apply(&mut f, json!({"op": "create_element", "owner": "P::Rig", "kind": "constraint", "name": "lim2", "doc": "Stay below the cap.", "body": "total <= 5"}));
    let t = text(&f);
    assert!(t.find("Stay below the cap.").unwrap() < t.find("total <= 5").unwrap());
    // The gates stay clean: nothing unresolved, and no syntax error.
    assert!(gate(&f, "syntax")["passed"] == true && gate(&f, "semantic")["passed"] == true, "{}", gate(&f, "semantic"));
}

#[test]
fn r16_b_the_body_of_a_constraint_is_one_plain_expression() {
    let f = files(RIG);
    for bad in ["a; part evil", "a } part evil {", "a /* x */", "", "a\nb;"] {
        let m = refused(&f, json!({"op": "create_element", "owner": "P::Rig", "kind": "constraint", "name": "lim", "body": bad}));
        assert!(m.contains("body") || m.contains("empty"), "{bad}: {m}");
    }
    // Only a constraint or calc takes it.
    let m = refused(&f, json!({"op": "create_element", "owner": "P::Rig", "kind": "part", "name": "p", "body": "x"}));
    assert!(m.contains("does not apply"), "{m}");
}

const REQS: &str = "package P {\n    part def Sys;\n    part sys : Sys;\n    requirement <'R-1'> Fast { doc /* Fast. */ }\n    requirement <'R-2'> Safe { doc /* Safe. */ }\n    requirement <'R-3'> Child { doc /* Child. */ }\n    use case def Goal;\n}\n";

#[test]
fn r16_c_derive_and_refine_carry_the_sysmlv1_library_and_it_is_made_when_missing() {
    let mut f = files(REQS);
    assert!(!text(&f).contains("SysMLv1"));
    let info = apply(&mut f, json!({"op": "create_relationship", "kind": "derive", "source": "P::Child", "target": "P::Fast"}));
    assert_eq!(info["status"], "created");
    assert!(text(&f).contains("#SysMLv1::DeriveReqt dependency from P::Child to P::Fast;"), "{}", text(&f));
    assert!(text(&f).contains("package SysMLv1") && text(&f).contains("metadata def DeriveReqt;"), "{}", text(&f));
    // Refine: a use case refines a requirement; the library gains Refine and keeps DeriveReqt.
    apply(&mut f, json!({"op": "create_relationship", "kind": "refine", "source": "P::Goal", "target": "P::Safe"}));
    assert!(text(&f).contains("#SysMLv1::Refine dependency from P::Goal to P::Safe;"), "{}", text(&f));
    assert_eq!(text(&f).matches("package SysMLv1").count(), 1, "{}", text(&f));
    assert!(text(&f).contains("metadata def DeriveReqt;") && text(&f).contains("metadata def Refine;"));
    // Read back as the edge kinds, and the model resolves.
    let facts = agent::export_facts(&model(&f), None).unwrap();
    let edges: Vec<(String, String, String)> = facts["edges"].as_array().unwrap().iter().map(|e| (e["kind"].as_str().unwrap().into(), e["source"].as_str().unwrap().into(), e["target"].as_str().unwrap().into())).collect();
    assert!(edges.contains(&("derive".into(), "P::Child".into(), "P::Fast".into())), "{edges:?}");
    assert!(edges.contains(&("refine".into(), "P::Goal".into(), "P::Safe".into())), "{edges:?}");
    assert_eq!(gate(&f, "semantic")["errors"], 0, "{}", gate(&f, "semantic"));
    let cov = agent::trace::coverage_report(&model(&f), None).unwrap();
    let fast = cov["requirements"].as_array().unwrap().iter().find(|r| r["ref"] == "P::Fast").unwrap();
    assert_eq!(fast["derives"], json!(["P::Child"]));
    // Writing the same derive again is refused rather than doubled.
    let m = refused(&f, json!({"op": "create_relationship", "kind": "derive", "source": "P::Child", "target": "P::Fast"}));
    assert!(m.contains("already"), "{m}");
}

#[test]
fn r16_c_a_derive_in_a_nested_package_finds_the_library_it_made() {
    let mut f = files("package Outer {\n    package Inner {\n        requirement a { doc /* A. */ }\n        requirement b { doc /* B. */ }\n    }\n}\n");
    apply(&mut f, json!({"op": "create_relationship", "kind": "derive", "source": "Outer::Inner::b", "target": "Outer::Inner::a"}));
    assert_eq!(gate(&f, "semantic")["errors"], 0, "{}", gate(&f, "semantic"));
    apply(&mut f, json!({"op": "create_relationship", "kind": "refine", "source": "Outer::Inner::b", "target": "Outer::Inner::a"}));
    assert_eq!(gate(&f, "semantic")["errors"], 0);
    assert_eq!(text(&f).matches("package SysMLv1").count(), 1);
}

#[test]
fn r16_d_a_unit_without_a_library_import_is_an_unresolved_unit() {
    let f = files("package P {\n    part def Pack {\n        attribute voltage = 400 [V];\n        attribute temp = 9.5 [SI::K];\n    }\n}\n");
    let v = gate(&f, "semantic");
    assert_eq!(rules_hit(&v), ["unresolved-unit"], "{v}");
    let hit = &v["findings"][0];
    assert!(hit["message"].as_str().unwrap().contains("'V'") && hit["fix"].as_str().unwrap().contains("SI::*"), "{hit}");
    assert_eq!(hit["element"], "P::Pack::voltage");
    // An import in scope, a qualified unit, or a model element of that name resolves it.
    for ok in [
        "package P { private import SI::*; part def Pack { attribute v = 400 [V]; } }",
        "package P { part def Pack { private import SI::V; attribute v = 400 [V]; } }",
        "package P { private import USCustomaryUnits::**; part def Pack { attribute v = 4 [ft]; } }",
        "package P { attribute def V; part def Pack { attribute v = 400 [V]; } }",
        "package P { part def Pack { attribute v = 400 [SI::V]; attribute w = 5 [m/s^2]; } }",
    ] {
        // `m/s^2` names two units, and no import is in scope for them.
        let want = if ok.contains("m/s^2") { 2 } else { 0 };
        assert_eq!(gate(&files(ok), "semantic")["findings"].as_array().unwrap().len(), want, "{ok}");
    }
    // An index is not a unit.
    assert_eq!(gate(&files("package P { part def A { attribute xs = ys[1]; attribute z = 3 * a[2]; } part def B; }"), "semantic")["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "unresolved-unit").count(), 0);
}

#[test]
fn r16_d_the_writer_adds_the_unit_import_once() {
    let mut f = files("package P {\n    part def Pack;\n}\n");
    let info = apply(&mut f, json!({"op": "create_element", "owner": "P::Pack", "kind": "attribute", "name": "voltage", "value": "400 [V]"}));
    assert_eq!(info["imports_added"][0]["import"], "SI::*", "{info}");
    assert_eq!(info["imports_added"][0]["in"], "P");
    assert_eq!(text(&f).matches("import SI::*;").count(), 1, "{}", text(&f));
    assert_eq!(gate(&f, "semantic")["errors"], 0);
    // A second unit in the same package adds nothing.
    let info = apply(&mut f, json!({"op": "create_element", "owner": "P::Pack", "kind": "attribute", "name": "maxTemp", "value": "60 [K]"}));
    assert!(info.get("imports_added").is_none(), "{info}");
    assert_eq!(text(&f).matches("import SI::*;").count(), 1);
    // update_element, a features[] member and a US customary unit.
    let info = apply(&mut f, json!({"op": "update_element", "ref": "P::Pack::maxTemp", "value": "70 [K]"}));
    assert!(info.get("imports_added").is_none());
    let info = apply(&mut f, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Rod", "features": [{"kind": "attribute", "name": "len", "value": "3 [ft]"}]}));
    assert_eq!(info["imports_added"][0]["import"], "USCustomaryUnits::*", "{info}");
    assert_eq!(gate(&f, "semantic")["errors"], 0, "{}", gate(&f, "semantic"));
    // The value reads back with its unit.
    let m = model(&f);
    assert_eq!(m.find("P::Pack::voltage").unwrap().value.as_deref(), Some("400 [V]"));
}

#[test]
fn r16_d_a_unit_in_a_batch_and_in_a_transition_trigger_gets_its_import() {
    let mut f = files("package P {\n    state def Run { state a; state b; }\n}\n");
    let edits = vec![
        json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Pack"}),
        json!({"op": "create_element", "owner": "P::Pack", "kind": "attribute", "name": "cap", "value": "75 [kg]"}),
        json!({"op": "create_relationship", "kind": "transition", "source": "P::Run::a", "target": "P::Run::b", "trigger": "after 5 [s]"}),
    ];
    let b = intent::apply_batch(&f, &edits).unwrap_or_else(|(i, e)| panic!("edit {i}: {}", e.message));
    f = b.files;
    common::assert_official(&f, "batch");
    assert_eq!(text(&f).matches("import SI::*;").count(), 1, "{}", text(&f));
    assert_eq!(gate(&f, "semantic")["errors"], 0, "{}", gate(&f, "semantic"));
}

// ------------------------------------------------------------------ 4: a read from cut pairs says so

#[test]
fn r16_a_model_that_was_read_whole_carries_no_notice() {
    let m = model(&files("package P {\n    requirement def RD;\n    part def C { requirement r : RD; part s; satisfy r by s; }\n}\n"));
    for v in [trace::traceability(&m, "P::C::r", 3).unwrap(), graph::query(&m, m.find("P::C::r").unwrap().id, None, "both", 2).unwrap(), trace::coverage_report(&m, None).unwrap(), agent::export_facts(&m, None).unwrap()] {
        assert!(v.get("not_checked").is_none() && v.get("pairs_truncated").is_none());
    }
}
