//! Tools round 15: the rule filters, `list_rules` and `check_all`.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::rules::{self, Selection};
use sysml_core::agent::{trace};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

fn model(text: &str) -> Model {
    api::to_model(vec![FileIn { path: "m.sysml".into(), text: text.into() }])
}

const BAD: &str = "package P {\n    part def A { part x : Missing; action op; }\n    allocate A to A;\n}\n";

fn sel<'a>(rules: Option<Vec<&str>>, groups: Option<Vec<&str>>) -> Selection<'a> {
    Selection { gates: None, rules: rules.map(|r| r.into_iter().map(String::from).collect()), groups: groups.map(|r| r.into_iter().map(String::from).collect()), require: None }
}

#[test]
fn list_rules_has_every_rule_of_every_gate() {
    let all = rules::list_rules(None, None).unwrap();
    assert_eq!(all["count"].as_u64().unwrap() as usize, rules::registry().len());
    let r = all["rules"].as_array().unwrap();
    assert!(r.iter().all(|x| ["id", "gate", "severity", "group", "meaning"].iter().all(|k| x[k].as_str().map(|s| !s.is_empty()).unwrap_or(false))));
    let gates: std::collections::BTreeSet<&str> = r.iter().map(|x| x["gate"].as_str().unwrap()).collect();
    assert_eq!(gates.into_iter().collect::<Vec<_>>(), ["connections", "coverage", "official", "semantic", "spec", "syntax", "traceability"]);
    assert!(rules::list_rules(Some("nope"), None).is_err());
}

#[test]
fn check_all_runs_every_gate_and_fails_on_an_error() {
    let m = model(BAD);
    let v = rules::check_all(&m, None, None).unwrap();
    assert_eq!(v["gates"].as_object().unwrap().len(), 7);
    assert_eq!(v["passed"], false);
    let text = rules::report_text(&v, 1);
    assert!(text.starts_with("FAILED"), "{text}");
    // Only the named gates.
    let one = rules::check_all(&m, Some(&["syntax".to_string()]), None).unwrap();
    assert_eq!(one["gates"].as_object().unwrap().len(), 1);
    assert_eq!(one["passed"], true);
    assert!(rules::check_all(&m, Some(&["nope".to_string()]), None).is_err());
    assert!(rules::check_all(&m, Some(&[]), None).is_err());
}

// ---- part S: security ------------------------------------------------------------------------

mod sec {
    use super::*;
    use sysml_core::agent::intent;

    pub const CAR: &str = "package P { part def Engine; requirement def R1; part def Car { attribute x = 1; attribute y = 2; part engine : Engine; doc /* The car. */ } }\n";

    pub fn files(t: &str) -> Vec<FileIn> {
        vec![FileIn { path: "m.sysml".into(), text: t.into() }]
    }

    /// The edit must be refused; if it is not, the saved text is shown.
    pub fn refused(t: &str, op: Value) -> String {
        match intent::apply_op(&files(t), &op) {
            Err(e) => e.message,
            Ok(a) => panic!("op {op} was saved:\n{}", a.files[0].text),
        }
    }

    pub fn ok(t: &str, op: Value) -> String {
        match intent::apply_op(&files(t), &op) {
            Ok(a) => {
                // R15_OUT=<dir>: keep what the write tools produced, for the official-grammar check.
                if let Some(d) = std::env::var_os("R15_OUT") {
                    let n = std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0);
                    let _ = std::fs::write(std::path::Path::new(&d).join(format!("w{n}.sysml")), &a.files[0].text);
                }
                a.files[0].text.clone()
            }
            Err(e) => panic!("op {op} failed: {}", e.message),
        }
    }
}

#[test]
fn s1_a_library_typed_by_cannot_carry_statements() {
    let evil = [
        "Real::q; attribute y = 666; part engine : P::Car; doc /* gone",
        "Real::x { satisfy P::R1 by P::Car; }",
        "Real::x { part evil : P::Engine; }",
        "Real::x :> P::Car::x = 5",
    ];
    for t in evil {
        sec::refused(sec::CAR, json!({"op": "update_element", "ref": "P::Car::x", "typed_by": t}));
        sec::refused(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "part", "name": "p3", "typed_by": t}));
    }
    sec::refused(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "part", "name": "p3", "typed_by": ["Real::x { part evil : P::Engine; }"]}));
    sec::refused(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "D", "specializes": "Real::q, P::Car"}));
    sec::refused(sec::CAR, json!({"op": "update_element", "ref": "P::Car::x", "specializes": "Real::q; attribute y = 666"}));
}

#[test]
fn s1_a_library_name_is_still_accepted_and_written_canonically() {
    let t = sec::ok(sec::CAR, json!({"op": "update_element", "ref": "P::Car::x", "typed_by": "ScalarValues::Real"}));
    assert!(t.contains("attribute x : ScalarValues::Real"), "{t}");
}

#[test]
fn s1_imports_and_relationships_take_only_qualified_names() {
    sec::refused(sec::CAR, json!({"op": "create_import", "owner": "P", "target": "Real::x; part evil : P::Engine"}));
    sec::refused(sec::CAR, json!({"op": "create_relationship", "kind": "specialize", "source": "P::Car", "target": "Real::q; part evil : P::Engine"}));
}

#[test]
fn s3_add_specialization_general_is_a_qualified_name() {
    let msg = sec::refused(sec::CAR, json!({"op": "add_specialization", "target": "P::Car::x", "general": "P::Car::y; attribute y = 666; part engine : P::Car; doc /* gone"}));
    assert!(msg.contains("general"), "{msg}");
}

#[test]
fn s5_hidden_and_direction_characters_are_refused_in_names_and_values() {
    for c in ['\u{2028}', '\u{2029}', '\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}', '\u{200B}', '\u{200D}', '\u{FEFF}', '\r'] {
        sec::refused(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "part", "name": format!("a{c}b")}));
        sec::refused(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "attribute", "name": "z", "value": format!("1{c}2")}));
        sec::refused(sec::CAR, json!({"op": "update_element", "ref": "P::Car::x", "rename": format!("x{c}y")}));
        sec::refused(sec::CAR, json!({"op": "update_element", "ref": "P::Car::x", "value": format!("2{c}3")}));
        sec::refused(sec::CAR, json!({"op": "set_doc", "target": "P::Car", "text": format!("no{c}te")}));
    }
}

#[test]
fn s7_no_member_under_an_alias_and_no_short_name_clash() {
    let t = "package P { part def A; alias AA for A; part def B { part s; } }\n";
    let m = sec::refused(t, json!({"op": "create_element", "owner": "P::AA", "kind": "part", "name": "q"}));
    assert!(m.contains("alias") && m.contains("Fix"), "{m}");
    let m = sec::refused(t, json!({"op": "create_element", "owner": "P::B", "kind": "part", "name": "t", "short_name": "s"}));
    assert!(m.contains("'s'") && m.contains("Fix"), "{m}");
}

#[test]
fn s3_a_moved_package_does_not_hide_a_stray_element() {
    // A move may declare the new owner again; a stray element beside it still counts.
    let t = "package A { package Q { part def X; } }\npackage B { }\n";
    sec::ok(t, json!({"op": "update_element", "ref": "A::Q", "owner": "B"}));
}

#[test]
fn s3_create_element_holds_only_the_named_elements() {
    let m = sec::refused(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "part", "name": "p3", "typed_by": "Real::x { part evil : P::Engine; }"}));
    assert!(m.contains("did not list") || m.contains("qualified name"), "{m}");
}

#[test]
fn s6b_the_member_tables_use_the_keyed_hasher() {
    let src = include_str!("../src/resolve.rs");
    assert!(!src.contains("BuildHasherDefault") && !src.contains("0xcbf2_9ce4"), "resolve.rs must not use an unkeyed hash");
}

fn many_pairs(copies: usize) -> String {
    let from: Vec<String> = (0..31).map(|i| format!("a{i}")).collect();
    let to: Vec<String> = (0..31).map(|i| format!("b{i}")).collect();
    let mut t = String::from("package P {\n");
    for n in from.iter().chain(to.iter()) {
        t.push_str(&format!("  part {n};\n"));
    }
    for _ in 0..copies {
        t.push_str(&format!("  dependency from {} to {};\n", from.join(", "), to.join(", ")));
    }
    t.push_str("  requirement r1;\n  part s;\n}\n");
    t
}

#[test]
fn s6a_a_model_past_the_pair_limit_is_not_reported_as_passed() {
    let t = many_pairs(1041); // 961 pairs a statement: past graph::MAX_PAIRS_TOTAL
    let m = model(&t);
    let (_, cuts) = sysml_core::agent::graph::statements_cut(&m);
    assert!(cuts.iter().any(|c| c.by_total), "the fixture must pass the total limit");
    let v = trace::validate_model_sel(&m, None, &sel(None, None)).unwrap();
    assert_eq!(v["passed"], false);
    assert!(v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "not-fully-checked"), "{v}");
    let c = sysml_core::agent::connect::check_connections(&m, "P").unwrap();
    assert!(c["findings"].as_array().is_some());
    let all = rules::check_all(&m, None, None).unwrap();
    assert_eq!(all["passed"], false);
}

#[test]
fn s6a_a_duplicate_check_on_a_cut_model_refuses_instead_of_passing() {
    let t = many_pairs(1041); // 961 pairs a statement: past graph::MAX_PAIRS_TOTAL
    let m = sec::refused(&t, json!({"op": "create_relationship", "kind": "satisfy", "source": "P::s", "target": "P::r1"}));
    assert!(m.contains("not checked"), "{m}");
}

#[test]
fn s2_a_statement_with_fifty_thousand_repeated_ends_is_read_in_linear_time() {
    let ends = vec!["x.y"; 50_000].join(", ");
    let t = format!("package P {{ part x {{ part y; }} part b;\n dependency from {ends} to P::b;\n dependency from {} to P::b;\n}}\n", vec!["a"; 50_000].join(", "));
    let m = model(&t);
    let start = std::time::Instant::now();
    let (stmts, cuts) = sysml_core::agent::graph::statements_cut(&m);
    assert!(start.elapsed().as_secs_f64() < 2.0, "took {:?}", start.elapsed());
    assert!(stmts.len() <= 2 * sysml_core::agent::graph::MAX_PAIRS_PER_STATEMENT);
    assert!(cuts.iter().all(|c| c.sources >= 50_000), "the cut records the real number of ends: {cuts:?}");
}

// ---- grammar: G1, G3 ---------------------------------------------------------------------------

#[test]
fn g1_the_writer_emits_private_import_and_the_gate_rejects_a_bare_one() {
    let t = sec::ok(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "import", "target": "ScalarValues::*"}));
    assert!(t.contains("private import ScalarValues::*;"), "{t}");
    let t = sec::ok(sec::CAR, json!({"op": "create_element", "owner": "P", "kind": "import", "target": "ScalarValues::*", "visibility": "public"}));
    assert!(t.contains("public import ScalarValues::*;"), "{t}");
    let v = trace::validate_model_sel(&model("package P { import ScalarValues::*; }\n"), Some("syntax"), &sel(None, None)).unwrap();
    assert_eq!(v["passed"], false);
    assert_eq!(v["findings"][0]["rule"], "syntax-import-visibility");
    let ok = trace::validate_model_sel(&model("package P { private import ScalarValues::*; public import ISQ::*; }\n"), Some("syntax"), &sel(None, None)).unwrap();
    assert_eq!(ok["passed"], true, "{ok}");
}

#[test]
fn g3_the_syntax_gate_rejects_each_known_bad_form() {
    let cases = [
        ("package P { part def A; part x : A [1..]; }", "syntax-multiplicity"),
        ("package P { part def A; part x : A [1, 2]; }", "syntax-multiplicity"),
        ("package P { part def A; part x : A [..5]; }", "syntax-multiplicity"),
        ("package P { part def A; part x : A [1][2]; }", "syntax-multiplicity"),
        ("package P { attribute x = ; }", "syntax-missing-operand"),
        ("package P { attribute x = 1 + ; }", "syntax-missing-operand"),
        ("package P { attribute x = 1.; }", "syntax-real-literal"),
        ("package P { def A; }", "syntax-definition-kind"),
        ("package P { use def U; }", "syntax-definition-kind"),
        ("package P { private private part x; }", "syntax-repeated-modifier"),
        ("package P { part def A; alias B; }", "syntax-alias-target"),
        ("package P { action a { attribute v; assign v = 1; } }", "syntax-assign-operator"),
        ("package P { action a { accept; } }", "syntax-accept-payload"),
        ("package P { enum def E { a; part b; } }", "syntax-enum-member"),
        ("package P { state def S { state a; transition; } }", "syntax-empty-transition"),
    ];
    for (text, rule) in cases {
        let v = trace::validate_model_sel(&model(text), Some("syntax"), &sel(None, None)).unwrap();
        assert!(v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == rule), "{text}: {v}");
    }
}

#[test]
fn g3_valid_forms_stay_clean() {
    let good = "package P {\n  private import ScalarValues::*;\n  part def A { attribute r : ScalarValues::Real = 1.5; attribute n = 12 [V]; part x : A [0..*]; part y : A [*]; part z : A [3]; }\n  alias B for A;\n  action a { attribute v; assign v := 1; accept after 5 [s]; }\n  enum def E { a; enum b; }\n  state def S { state a; state b; transition t first a then b; }\n  use case def U;\n  abstract part def Q;\n  doc /* a ; = [1..] 1. */\n}\n";
    let v = trace::validate_model_sel(&model(good), Some("syntax"), &sel(None, None)).unwrap();
    let hits: Vec<_> = v["findings"].as_array().unwrap().iter().filter(|f| f["rule"].as_str().unwrap().starts_with("syntax-")).collect();
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn g3_non_ascii_text_in_comments_notes_names_and_strings_does_not_panic() {
    let texts = [
        "package P { /* größe é ✓ */ part x; }\n",
        "package P { // Übergröße ✓\n part y; }\n",
        "package P { part 'größe ✓'; attribute s = \"é ✓ \\\" ü\"; }\n",
        "package P { part x; } /* unclosed é ✓",
        "package P { attribute s = \"unclosed é ✓",
        "package P { part 'unclosed é ✓",
        "package P { part x; } // é",
        "/* ✓",
        "§ ✓ é",
    ];
    for t in texts {
        let _ = trace::validate_model_sel(&model(t), Some("syntax"), &sel(None, None)).unwrap();
    }
}
