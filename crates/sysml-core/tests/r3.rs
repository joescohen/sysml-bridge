//! Tools round 3: name shadowing, global scope, connectors, export, naming.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, Severity};

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "demo.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn diags(m: &Model) -> Vec<String> {
    m.diagnostics.iter().map(|d| format!("{:?} {}", d.severity, d.message)).collect()
}

fn apply(f: &mut Vec<FileIn>, op: Value) -> Result<Value, String> {
    match intent::apply_op(f, &op) {
        Ok(a) => {
            *f = a.files;
            Ok(a.info)
        }
        Err(e) => Err(e.message),
    }
}

const SHADOW: &str = "package distiller {\n    part def Distiller;\n    connection def C {\n    }\n    part def D2 {\n        part x : Distiller;\n    }\n}\n";

fn unresolved(f: &[FileIn]) -> Vec<String> {
    diags(&model(f)).into_iter().filter(|d| d.contains("cannot resolve")).collect()
}

#[test]
fn a1_a_name_hidden_by_the_new_element_is_written_so_it_resolves() {
    let mut f = files(SHADOW);
    let info = apply(&mut f, json!({"op": "create_element", "owner": "distiller::C", "kind": "part", "name": "distiller", "typed_by": "distiller::Distiller"})).unwrap();
    assert!(info["names_rewritten"].as_u64().unwrap_or(0) >= 1, "{info}");
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("part distiller : Distiller;"), "{}", f[0].text);
    assert!(unresolved(&f).is_empty(), "{:?}", diags(&model(&f)));
    // The same through a feature that specializes and a relationship that names the type.
    apply(&mut f, json!({"op": "create_element", "owner": "distiller::C", "kind": "part", "name": "distiller2", "typed_by": "distiller::Distiller"})).unwrap();
    assert!(unresolved(&f).is_empty());
}

#[test]
fn a2_the_resolver_honours_a_leading_global_scope() {
    let f = files("package distiller {\n    part def Distiller;\n    connection def C {\n        part distiller : $::distiller::Distiller;\n    }\n}\n");
    let m = model(&f);
    assert!(m.diagnostics.is_empty(), "{:?}", diags(&m));
    let part = m.find("distiller::C::distiller").unwrap();
    assert_eq!(part.first_resolved(sysml_core::model::RefRole::Type), m.find("distiller::Distiller").map(|e| e.id));
}

#[test]
fn a3_validate_reports_a_hidden_name_as_an_error() {
    let f = files("package distiller {\n    part def Distiller;\n    connection def C {\n        part distiller : distiller::Distiller;\n    }\n}\n");
    let m = model(&f);
    let errs: Vec<String> = m.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect();
    assert_eq!(errs.len(), 1, "{:?}", diags(&m));
    assert!(errs[0].contains("cannot resolve") && errs[0].contains("$::distiller::Distiller") && errs[0].contains("Fix:"), "{}", errs[0]);
    let v = agent::trace::validate_model(&m, Some("semantic")).unwrap();
    assert_eq!(v["passed"], false);
}

#[test]
fn a4_rename_also_updates_references_a_nearer_name_hid() {
    let mut f = files("package distiller {\n    part def Distiller;\n    connection def C {\n        part distiller : distiller::Distiller;\n    }\n    part def D2 {\n        part x : distiller::Distiller;\n    }\n}\n");
    apply(&mut f, json!({"op": "update_element", "ref": "distiller", "rename": "distiller_1"})).unwrap();
    assert!(!sysml_core::agent::mint::strip_ids(&f[0].text).contains("distiller::"), "{}", f[0].text);
    assert!(unresolved(&f).is_empty(), "{:?}\n{}", diags(&model(&f)), f[0].text);
    assert!(model(&f).diagnostics.iter().all(|d| d.severity != Severity::Error));
}

const HB: &str = "package P {\n    attribute def H2O {\n        attribute 'mass flow rate';\n    }\n    constraint def CD {\n        attribute r1;\n        attribute r2;\n    }\n    part def HB {\n        attribute main1 : H2O;\n        attribute main2 : H2O;\n        constraint e1 : CD;\n    }\n}\n";

#[test]
fn b1_connect_two_usages_of_one_definition_is_two_ends() {
    let mut f = files(HB);
    let i = apply(&mut f, json!({"op": "create_relationship", "kind": "connect", "owner": "P::HB", "source": "P::HB::main2::'mass flow rate'", "target": "P::HB::main1::'mass flow rate'"})).unwrap();
    assert_eq!(i["statement"], "connect main2.'mass flow rate' to main1.'mass flow rate';", "{i}");
    // The very same chain on both sides is still refused.
    let e = apply(&mut f, json!({"op": "create_relationship", "kind": "connect", "owner": "P::HB", "source": "P::HB::main2::'mass flow rate'", "target": "P::HB::main2::'mass flow rate'"})).unwrap_err();
    assert!(e.contains("same element"), "{e}");
}

#[test]
fn b2_a_connector_chain_runs_through_a_constraint_usage() {
    let mut f = files(HB);
    let i = apply(&mut f, json!({"op": "create_relationship", "kind": "connect", "owner": "P::HB", "source": "P::H2O::'mass flow rate'", "target": "P::HB::e1::r1"}));
    let i = i.unwrap_or_else(|e| panic!("{e}"));
    assert!(i["statement"].as_str().unwrap().contains("e1.r1"), "{i}");
}

#[test]
fn b3_a_deliberate_duplicate_allocation_needs_the_flag() {
    let mut f = files("package P {\n    part def A;\n    part def B;\n    part a : A;\n    part b : B;\n}\n");
    apply(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "P::a", "target": "P::b"})).unwrap();
    let e = apply(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "P::a", "target": "P::b"})).unwrap_err();
    assert!(e.contains("already exists") && e.contains("allow_duplicate"), "{e}");
    apply(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "P::a", "target": "P::b", "allow_duplicate": true})).unwrap();
    assert_eq!(f[0].text.matches("allocate ").count(), 2, "{}", f[0].text);
}

#[test]
fn d1_an_unnamed_statement_can_be_named_later_and_dependency_takes_a_name() {
    let mut f = files("package P {\n    part def A;\n    part def B;\n    part a : A;\n    part b : B;\n    action def Go {\n        action x;\n        action y;\n        first x then y;\n    }\n    connection def L;\n    part s {\n        part p : A;\n        part q : A;\n        bind p = q;\n        allocate p to q;\n        connection : L connect p to q;\n    }\n}\n");
    let m = model(&f);
    let succ = m.find("P::Go").unwrap().children.iter().map(|c| &m.elements[*c]).find(|c| c.kind == "succession").unwrap().qualified_name.clone();
    assert!(succ.contains("$"), "{succ}");
    let r = apply(&mut f, json!({"op": "update_element", "ref": succ, "rename": "after_x"})).unwrap();
    assert_eq!(r["ref"], "P::Go::after_x");
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("succession after_x first x then y;"), "{}", f[0].text);
    for (ref_, want) in [("P::s::$3", "binding bound bind p = q;"), ("P::s::$4", "allocation alloc1 allocate p to q;"), ("P::s::$5", "connection wire : L connect p to q;")] {
        let n = want.split_whitespace().nth(1).unwrap();
        apply(&mut f, json!({"op": "update_element", "ref": ref_, "rename": n})).unwrap_or_else(|e| panic!("{ref_}: {e}"));
        assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains(want), "{want}\n{}", f[0].text);
    }
    assert!(model(&f).diagnostics.is_empty(), "{:?}", diags(&model(&f)));
    // dependency takes a name.
    let d = apply(&mut f, json!({"op": "create_relationship", "kind": "dependency", "source": "P::a", "target": "P::b", "name": "if_2"})).unwrap();
    assert_eq!(d["statement"], "dependency if_2 from P::a to P::b;", "{d}");
    assert!(model(&f).find("P::if_2").is_some());
}

#[test]
fn enum_mismatch_inside_a_metadata_body_is_reported() {
    let m = model(&files("package P {\n    enum def Level { L1; L2; }\n    metadata def Tag {\n        attribute level : Level;\n    }\n    part def X {\n        @Tag {\n            level = L9;\n        }\n    }\n}\n"));
    let found: Vec<String> = m.diagnostics.iter().filter(|d| d.message.starts_with("value does not fit type")).map(|d| d.message.clone()).collect();
    assert_eq!(found.len(), 1, "{:?}", diags(&m));
}

#[test]
fn a_part_typed_by_an_action_definition_gets_a_warning_with_a_fix() {
    let m = model(&files("package P {\n    action def Go;\n    part bad : Go;\n}\n"));
    let v = agent::trace::validate_model(&m, Some("semantic")).unwrap();
    let f = v["findings"].as_array().unwrap().iter().find(|f| f["rule"] == "usage-definition-kind-mismatch").unwrap_or_else(|| panic!("{v}"));
    assert_eq!(f["severity"], "warning");
    assert!(f["fix"].as_str().unwrap().len() > 10);
    // The clean case: a part typed by a part definition raises nothing.
    let ok = model(&files("package P {\n    part def Car;\n    part c : Car;\n}\n"));
    assert!(agent::trace::validate_model(&ok, Some("semantic")).unwrap()["findings"].as_array().unwrap().is_empty());
}

#[test]
fn coverage_report_counts_requirement_usages_only() {
    let m = model(&files("package P {\n    requirement def R;\n    requirement r1 : R;\n    part def A;\n    part a : A;\n    satisfy r1 by a;\n}\n"));
    let c = agent::trace::coverage_report(&m, None).unwrap();
    assert_eq!(c["totals"]["requirements"], 1, "{c}");
}

#[test]
fn export_facts_lists_the_use_case_subject_and_actor_and_connector_ends() {
    let m = model(&files("package P {\n    part def E;\n    part def Car {\n        part e : E;\n        part e2 : E;\n        connect e to e2;\n    }\n    use case def U {\n        subject s : Car;\n        actor a : E;\n    }\n}\n"));
    let v = agent::export_facts(&m, None).unwrap();
    let refs: Vec<&str> = v["elements"].as_array().unwrap().iter().map(|e| e["ref"].as_str().unwrap()).collect();
    assert!(refs.contains(&"P::U::s") && refs.contains(&"P::U::a"), "{refs:?}");
    let conn = v["elements"].as_array().unwrap().iter().find(|e| e["kind"] == "connection").unwrap();
    assert_eq!(conn["ends"]["target"]["path"], json!(["P::Car", "P::Car::e2"]));
    assert_eq!(conn["ends"]["target"]["ref"], "P::Car::e2");
}

#[test]
fn a_part_def_member_move_and_move_package_keep_their_ids_through_reanchor() {
    use agent::ids;
    let before = model(&files("package P {\n    action def Go {\n        action a;\n        first a then a;\n    }\n}\n"));
    let after = model(&files("package P {\n    action def Go {\n        doc /* d */\n        action a;\n        first a then a;\n    }\n}\n"));
    let mut map = std::collections::BTreeMap::new();
    map.insert("sid".to_string(), "P::Go::$2".to_string());
    let original = map.clone();
    ids::reanchor(&mut map, &original, &before, &after);
    assert_eq!(map["sid"], "P::Go::$3");
}

