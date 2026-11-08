//! Tests for the agent interface logic. Every rule has a seeded defect that
//! must fail and a clean case that must pass.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, connect, graph, intent, plan, trace};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, RefRole, Severity};

const BASE: &str = r#"package Demo {
    part def Vehicle {
        part engine : Engine;
        part trans : Transmission;
    }
    part def Engine;
    part def Transmission;
    part vehicle1 : Vehicle;
    package Reqs {
        requirement speed {
            doc /* Reach 100 km/h. */
        }
        requirement braking;
    }
    verification def CheckSpeed;
    part def Tester;
}
"#;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "demo.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn errors(m: &Model) -> Vec<String> {
    m.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
}

/// Apply an op and demand a model with no syntax errors and no unresolved names.
fn run(f: &mut Vec<FileIn>, op: Value) -> Value {
    let applied = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    *f = applied.files;
    if let Some(d) = std::env::var_os("R15_OUT") {
        let n = std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0);
        let _ = std::fs::write(std::path::Path::new(&d).join(format!("a{n}.sysml")), f.iter().map(|x| x.text.as_str()).collect::<Vec<_>>().join("\n"));
    }
    let m = model(f);
    assert!(errors(&m).is_empty(), "errors after {op}: {:?}\n{}", errors(&m), f[0].text);
    let unresolved: Vec<_> = m.diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).map(|d| d.message.clone()).collect();
    assert!(unresolved.is_empty(), "unresolved after {op}: {unresolved:?}\n{}", f[0].text);
    applied.info
}

fn fails(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(_) => panic!("op {op} should have failed"),
        Err(e) => e.message,
    }
}

fn rules(v: &Value) -> Vec<String> {
    v["findings"].as_array().unwrap().iter().map(|f| f["rule"].as_str().unwrap().to_string()).collect()
}

// ---- reads ---------------------------------------------------------------------

#[test]
fn revision_is_stable_and_sensitive() {
    assert_eq!(agent::revision_of("a"), agent::revision_of("a"));
    assert_ne!(agent::revision_of("a"), agent::revision_of("b"));
    let a = vec![FileIn { path: "a".into(), text: "x".into() }, FileIn { path: "b".into(), text: "y".into() }];
    let mut b = a.clone();
    b.reverse();
    assert_eq!(agent::project_revision(&a), agent::project_revision(&b));
    b[0].text.push(' ');
    assert_ne!(agent::project_revision(&a), agent::project_revision(&b));
}

#[test]
fn project_state_counts_kinds_and_diagnostics() {
    let f = files(BASE);
    let s = agent::project_state(&model(&f));
    assert_eq!(s["counts_by_kind"]["part def"], 4);
    assert_eq!(s["counts_by_kind"]["requirement"], 2);
    assert_eq!(s["diagnostics"]["errors"], 0);
    assert_eq!(s["files"][0]["path"], "demo.sysml");
    assert_eq!(s["revision"], agent::project_revision(&f));
    let bad = agent::project_state(&model(&files("part def X {")));
    assert!(bad["diagnostics"]["errors"].as_u64().unwrap() >= 1);
}

#[test]
fn query_elements_filters() {
    let m = model(&files(BASE));
    let q = |kind, name, owner, typed_by| {
        agent::query_elements(&m, &agent::Query { kind, name, owner, typed_by, limit: 50 }).unwrap()
    };
    assert_eq!(q(Some("requirement"), None, None, None)["total"], 2);
    assert_eq!(q(None, Some("ENG"), None, None)["total"], 2); // Engine, engine
    assert_eq!(q(None, None, Some("Demo::Vehicle"), None)["total"], 2);
    let typed = q(None, None, None, Some("Demo::Engine"));
    assert_eq!(typed["elements"][0]["ref"], "Demo::Vehicle::engine");
    assert_eq!(q(Some("part def"), None, None, None)["total"], 4);
    let limited = agent::query_elements(&m, &agent::Query { kind: None, name: None, owner: None, typed_by: None, limit: 2 }).unwrap();
    assert_eq!(limited["returned"], 2);
    assert_eq!(limited["truncated"], true);
    assert!(agent::query_elements(&m, &agent::Query { kind: None, name: None, owner: Some("Nope"), typed_by: None, limit: 5 }).is_err());
}

#[test]
fn referenced_by_is_uncapped_and_filterable() {
    let mut text = String::from("package P { part def Wheel; part def Car {\n");
    for i in 0..80 {
        text.push_str(&format!("part w{i} : Wheel;\n"));
    }
    text.push_str("} requirement r; }\n");
    let m = model(&files(&text));
    let all = agent::element_details(&m, "P::Wheel", None).unwrap();
    assert_eq!(all["referencedBy"].as_array().unwrap().len(), 80);
    let some = agent::element_details(&m, "P::Wheel", Some("requirement")).unwrap();
    assert_eq!(some["referencedBy"].as_array().unwrap().len(), 0);
    assert_eq!(some["referencedByTotal"], 80);
    assert!(agent::element_details(&m, "P::Whel", None).unwrap_err().message.contains("Did you mean: P::Wheel"));
}

#[test]
fn query_relationships_directions_kinds_depth() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    let m = model(&f);
    let speed = m.find("Demo::Reqs::speed").unwrap().id;
    let inn = graph::query(&m, speed, Some("satisfy"), "in", 1).unwrap();
    assert_eq!(inn["count"], 1);
    assert_eq!(inn["edges"][0]["source"], "Demo::vehicle1");
    assert_eq!(graph::query(&m, speed, Some("satisfy"), "out", 1).unwrap()["count"], 0);
    let v = m.find("Demo::vehicle1").unwrap().id;
    assert_eq!(graph::query(&m, v, Some("typing"), "out", 1).unwrap()["edges"][0]["target"], "Demo::Vehicle");
    // A specialization chain Turbo :> Motor :> Engine is two hops in from Engine.
    let chain = model(&files("package C { part def Engine; part def Motor :> Engine; part def Turbo :> Motor; }"));
    let e = chain.find("C::Engine").unwrap().id;
    assert_eq!(graph::query(&chain, e, Some("specialize"), "in", 1).unwrap()["count"], 1);
    assert_eq!(graph::query(&chain, e, Some("specialize"), "in", 2).unwrap()["count"], 2);
    assert!(graph::query(&m, v, None, "sideways", 1).is_err());
}

// ---- create / update / delete ---------------------------------------------------

#[test]
fn create_element_generates_canonical_text() {
    let mut f = files(BASE);
    let info = run(
        &mut f,
        json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "Wheel", "doc": "A wheel.",
               "features": [{"kind": "attribute", "name": "radius", "typed_by": "Real"}, {"kind": "port", "name": "hub"}]}),
    );
    assert_eq!(info["ref"], "Demo::Wheel");
    assert_eq!(info["created"].as_array().unwrap().len(), 3);
    run(&mut f, json!({"op": "create_element", "owner": "Demo::Vehicle", "kind": "part", "name": "wheels", "typed_by": "Demo::Wheel", "multiplicity": "[4]"}));
    let m = model(&f);
    let w = m.find("Demo::Vehicle::wheels").unwrap();
    assert_eq!(w.multiplicity.as_deref(), Some("4"));
    assert_eq!(w.first_resolved(sysml_core::RefRole::Type), m.find("Demo::Wheel").map(|e| e.id));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("part wheels : Demo::Wheel [4];"), "{}", f[0].text);
}

#[test]
fn create_element_refuses_bad_input_with_next_step() {
    let f = files(BASE);
    assert!(fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "Engine"})).contains("already has a member"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "gizmo", "name": "X"})).contains("unknown kind"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Nope", "kind": "part", "name": "X"})).contains("query_elements"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "x", "typed_by": "Demo::vehicle1"})).contains("must be a definition"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "X", "typed_by": "Demo::Engine"})).contains("specialize"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "x", "typed_by": "Demo::Missing"})).contains("no element named"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Demo", "name": "x"})).contains("missing argument 'kind'"));
}

#[test]
fn create_element_quotes_odd_names_and_top_level() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_element", "owner": "Demo", "kind": "requirement", "name": "Top speed", "doc": "x"}));
    assert!(model(&f).find("Demo::'Top speed'").is_some());
    run(&mut f, json!({"op": "create_element", "kind": "package", "name": "Other", "file": "other.sysml"}));
    assert!(f.iter().any(|x| x.path == "other.sysml"));
}

#[test]
fn update_element_changes_name_type_multiplicity_doc() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    let info = run(&mut f, json!({"op": "update_element", "ref": "Demo::vehicle1", "rename": "car1"}));
    assert_eq!(info["ref"], "Demo::car1");
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("satisfy Demo::Reqs::speed by Demo::car1;") || sysml_core::agent::mint::strip_ids(&f[0].text).contains("by Demo::car1"), "{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "multiplicity": "2"}));
    assert_eq!(model(&f).find("Demo::Vehicle::engine").unwrap().multiplicity.as_deref(), Some("2"));
    run(&mut f, json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "multiplicity": "1..3"}));
    assert_eq!(model(&f).find("Demo::Vehicle::engine").unwrap().multiplicity.as_deref(), Some("1..3"));
    run(&mut f, json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "multiplicity": ""}));
    assert_eq!(model(&f).find("Demo::Vehicle::engine").unwrap().multiplicity, None);
    run(&mut f, json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "Motor"}));
    run(&mut f, json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "typed_by": "Demo::Motor"}));
    let m = model(&f);
    assert_eq!(m.find("Demo::Vehicle::engine").unwrap().first_resolved(sysml_core::RefRole::Type), m.find("Demo::Motor").map(|e| e.id));
    run(&mut f, json!({"op": "update_element", "ref": "Demo::Reqs::braking", "doc": "Stop in 40 m."}));
    assert_eq!(model(&f).find("Demo::Reqs::braking").unwrap().doc.as_deref(), Some("Stop in 40 m."));
    // Untyped usage gains a type.
    run(&mut f, json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "loose"}));
    run(&mut f, json!({"op": "update_element", "ref": "Demo::loose", "typed_by": "Demo::Engine", "multiplicity": "3"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("part loose : Demo::Engine [3];"), "{}", f[0].text);
}

#[test]
fn update_element_rejects_nonsense() {
    let f = files(BASE);
    assert!(fails(&f, json!({"op": "update_element", "ref": "Demo::Engine", "typed_by": "Demo::Vehicle"})).contains("definition"));
    assert!(fails(&f, json!({"op": "update_element", "ref": "Demo::Engine"})).contains("nothing to change"));
    assert!(fails(&f, json!({"op": "update_element", "ref": "Demo::Engine", "rename": "Transmission"})).contains("already has a member"));
}

#[test]
fn delete_element_refuses_when_referenced_unless_cascade() {
    let mut f = files(BASE);
    let msg = fails(&f, json!({"op": "delete_element", "ref": "Demo::Engine"}));
    assert!(msg.contains("Demo::Vehicle::engine"), "{msg}");
    assert!(msg.contains("cascade"));
    let err = intent::apply_op(&f, &json!({"op": "delete_element", "ref": "Demo::Engine"})).unwrap_err();
    assert_eq!(err.data["referrers"][0]["ref"], "Demo::Vehicle::engine");
    // Unreferenced: deletes cleanly.
    run(&mut f, json!({"op": "delete_element", "ref": "Demo::Tester"}));
    assert!(model(&f).find("Demo::Tester").is_none());
    // Cascade: as in Cameo, the part typed by the deleted block stays and loses its type; what is left parses.
    let info = intent::apply_op(&f, &json!({"op": "delete_element", "ref": "Demo::Engine", "cascade": true})).unwrap();
    let m = model(&info.files);
    assert!(errors(&m).is_empty());
    assert!(m.find("Demo::Engine").is_none());
    let engine = m.find("Demo::Vehicle::engine").expect("the typed part stays");
    assert!(engine.first_resolved(RefRole::Type).is_none() && !engine.refs.iter().any(|r| r.role == RefRole::Type), "{:?}", engine.refs);
    assert!(m.find("Demo::Vehicle::trans").is_some());
    assert_eq!(info.info["deleted"].as_array().unwrap().len(), 1);
    assert_eq!(info.info["untyped"][0]["ref"], "Demo::Vehicle::engine");
}

// ---- relationships ----------------------------------------------------------------

fn has_edge(f: &[FileIn], kind: &str, s: &str, t: &str) -> bool {
    let m = model(f);
    graph::edges(&m).iter().any(|(st, a, b)| st.kind == kind && m.elements[*a].qualified_name == s && m.elements[*b].qualified_name == t)
}

#[test]
fn satisfy_needs_usages_and_resolves() {
    let mut f = files(BASE);
    // Seeded defects first: a definition on either side.
    let m1 = fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::Vehicle", "target": "Demo::Reqs::speed"}));
    assert!(m1.contains("is a definition") && m1.contains("create_element"), "{m1}");
    let mut g = f.clone();
    run(&mut g, json!({"op": "create_element", "owner": "Demo", "kind": "requirement def", "name": "RD"}));
    assert!(fails(&g, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::RD"})).contains("is a definition"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::vehicle1"})).contains("same element"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Vehicle::engine"})).contains("requirement usage"));
    // Clean case.
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    assert_eq!(info["statement"], "satisfy Demo::Reqs::speed by Demo::vehicle1;");
    assert!(has_edge(&f, "satisfy", "Demo::vehicle1", "Demo::Reqs::speed"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"})).contains("already exists"));
}

#[test]
fn verify_lives_in_a_verification_objective() {
    let mut f = files(BASE);
    // Seeded defect: a part is not a verification case.
    let msg = fails(&f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    assert!(msg.contains("only legal inside the objective") && msg.contains("verification def"), "{msg}");
    run(&mut f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::CheckSpeed", "target": "Demo::Reqs::speed"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("objective {") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("verify Demo::Reqs::speed;"), "{}", f[0].text);
    assert!(has_edge(&f, "verify", "Demo::CheckSpeed", "Demo::Reqs::speed"));
    // The second verify reuses the objective.
    run(&mut f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::CheckSpeed", "target": "Demo::Reqs::braking"}));
    assert_eq!(f[0].text.matches("objective").count(), 1, "{}", f[0].text);
    assert!(has_edge(&f, "verify", "Demo::CheckSpeed", "Demo::Reqs::braking"));
    // And the semantic gate accepts it.
    let v = trace::validate_model(&model(&f), Some("semantic")).unwrap();
    assert_eq!(v["passed"], true, "{v}");
}

#[test]
fn allocate_derive_refine_dependency_specialize() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "Demo::Reqs::speed", "target": "Demo::vehicle1"}));
    assert!(has_edge(&f, "allocate", "Demo::Reqs::speed", "Demo::vehicle1"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "allocate", "source": "Demo::Reqs::braking", "target": "Demo::Engine"})).contains("is a definition"));
    run(&mut f, json!({"op": "create_relationship", "kind": "derive", "source": "Demo::Reqs::braking", "target": "Demo::Reqs::speed"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("#SysMLv1::DeriveReqt dependency from Demo::Reqs::braking to Demo::Reqs::speed;"), "{}", f[0].text);
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("metadata def DeriveReqt;"));
    assert!(has_edge(&f, "derive", "Demo::Reqs::braking", "Demo::Reqs::speed"));
    run(&mut f, json!({"op": "create_element", "owner": "Demo::Reqs", "kind": "requirement", "name": "third"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "derive", "source": "Demo::Reqs::third", "target": "Demo::Reqs::speed"}));
    assert_eq!(f[0].text.matches("package SysMLv1").count(), 1);
    run(&mut f, json!({"op": "create_relationship", "kind": "refine", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    assert!(has_edge(&f, "refine", "Demo::vehicle1", "Demo::Reqs::speed"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "derive", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"})).contains("must be a requirement"));
    run(&mut f, json!({"op": "create_relationship", "kind": "dependency", "source": "Demo::Engine", "target": "Demo::Transmission"}));
    assert!(has_edge(&f, "dependency", "Demo::Engine", "Demo::Transmission"));
    // Specialize: ok, mixed kinds refused, cycle refused.
    run(&mut f, json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "Motor"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "specialize", "source": "Demo::Motor", "target": "Demo::Engine"}));
    assert!(has_edge(&f, "specialization", "Demo::Motor", "Demo::Engine"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "specialize", "source": "Demo::Engine", "target": "Demo::Motor"})).contains("cycle"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "specialize", "source": "Demo::vehicle1", "target": "Demo::Engine"})).contains("both"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "friends", "source": "Demo::Motor", "target": "Demo::Engine"})).contains("unknown relationship kind"));
}

// ---- coverage and gates ----------------------------------------------------------

#[test]
fn coverage_report_finds_gaps_then_closes_them() {
    let mut f = files(BASE);
    let r = trace::coverage_report(&model(&f), None).unwrap();
    assert_eq!(r["totals"]["requirements"], 2);
    assert_eq!(r["totals"]["fully_covered"], 0);
    assert_eq!(r["requirements"][0]["gaps"], json!(["not satisfied", "not verified"]));
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::CheckSpeed", "target": "Demo::Reqs::speed"}));
    let r = trace::coverage_report(&model(&f), Some("Demo::Reqs")).unwrap();
    assert_eq!(r["totals"]["satisfied"], 1);
    assert_eq!(r["totals"]["percent_covered"], 50.0);
    let speed = r["requirements"].as_array().unwrap().iter().find(|x| x["ref"] == "Demo::Reqs::speed").unwrap();
    assert_eq!(speed["satisfied_by"], json!(["Demo::vehicle1"]));
    assert_eq!(speed["verified_by"], json!(["Demo::CheckSpeed"]));
    assert_eq!(speed["gaps"], json!([]));
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::braking"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::CheckSpeed", "target": "Demo::Reqs::braking"}));
    let r = trace::coverage_report(&model(&f), None).unwrap();
    assert_eq!(r["totals"]["percent_covered"], 100.0);
    assert!(trace::coverage_report(&model(&f), Some("Nope")).is_err());
}

#[test]
fn traceability_walks_multiple_hops_both_ways() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "derive", "source": "Demo::Reqs::braking", "target": "Demo::Reqs::speed"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::CheckSpeed", "target": "Demo::Reqs::braking"}));
    let m = model(&f);
    let one = trace::traceability(&m, "Demo::Reqs::speed", 1).unwrap();
    assert_eq!(one["edge_count"], 2); // satisfy + derive
    let two = trace::traceability(&m, "Demo::Reqs::speed", 2).unwrap();
    assert_eq!(two["edge_count"], 3); // + verify of braking
    assert!(two["nodes"].as_array().unwrap().iter().any(|n| n["ref"] == "Demo::CheckSpeed" && n["depth"] == 2));
    assert!(trace::traceability(&m, "Demo::Nope", 2).is_err());
}

#[test]
fn semantic_gate_catches_each_seeded_defect() {
    let bad = r#"package B {
    part def D;
    part u : D;
    requirement r;
    requirement rd1;
    requirement def RD;
    satisfy r by D;
    satisfy RD by u;
    allocate D to u;
    part def Holder {
        verify r;
    }
    connection c connect D to D;
    part q : Missing;
}"#;
    let m = model(&files(bad));
    let v = trace::validate_model(&m, Some("semantic")).unwrap();
    let r = rules(&v);
    assert!(r.iter().filter(|x| *x == "trace-operand-definition").count() >= 2, "{r:?}");
    // `satisfy RD by u;`: the satisfied requirement is a def. An error, since Cameo 2026x refuses the file.
    assert!(r.contains(&"requirement-operand-definition".to_string()), "{r:?}");
    assert!(r.contains(&"verify-outside-objective".to_string()), "{r:?}");
    assert!(r.contains(&"connection-between-definitions".to_string()), "{r:?}");
    assert!(r.contains(&"unresolved-reference".to_string()), "{r:?}");
    assert_eq!(v["passed"], false);
    assert!(v["findings"][0]["fix"].as_str().unwrap().len() > 10);
    // Clean model passes.
    let ok = trace::validate_model(&model(&files(BASE)), Some("semantic")).unwrap();
    assert_eq!(ok["passed"], true);
    assert!(trace::validate_model(&model(&files(BASE)), Some("magic")).is_err());
}

#[test]
fn syntax_gate() {
    let bad = trace::validate_model(&model(&files("package X { part def }")), Some("syntax")).unwrap();
    assert_eq!(bad["passed"], false);
    assert_eq!(bad["findings"][0]["rule"], "syntax");
    assert_eq!(trace::validate_model(&model(&files(BASE)), Some("syntax")).unwrap()["passed"], true);
}

#[test]
fn traceability_gate_needs_satisfy_verify_and_no_dangling_end() {
    let mut f = files(BASE);
    let v = trace::validate_model(&model(&f), Some("traceability")).unwrap();
    assert_eq!(v["passed"], false);
    let r = rules(&v);
    assert_eq!(r.iter().filter(|x| *x == "requirement-not-satisfied").count(), 2);
    assert_eq!(r.iter().filter(|x| *x == "requirement-not-verified").count(), 2);
    for req in ["speed", "braking"] {
        let t = format!("Demo::Reqs::{req}");
        run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": t}));
        run(&mut f, json!({"op": "create_relationship", "kind": "verify", "source": "Demo::CheckSpeed", "target": t}));
    }
    let v = trace::validate_model(&model(&f), Some("traceability")).unwrap();
    assert_eq!(v["passed"], true, "{v}");
    // Seeded dangling end: the satisfier does not exist.
    let mut dangling = f.clone();
    dangling[0].text = dangling[0].text.replace("by Demo::vehicle1", "by Demo::ghost");
    let v = trace::validate_model(&model(&dangling), Some("traceability")).unwrap();
    assert_eq!(v["passed"], false);
    assert!(rules(&v).contains(&"dangling-trace-end".to_string()), "{v}");
    // All three gates together.
    let all = trace::validate_model(&model(&f), None).unwrap();
    assert_eq!(all["gates"]["syntax"]["passed"], true);
    assert_eq!(all["passed"], true);
}

// ---- connections -------------------------------------------------------------------

fn vehicle_with_interface() -> Vec<FileIn> {
    let mut f = files(BASE);
    run(
        &mut f,
        json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Vehicle::engine", "to": "Demo::Vehicle::trans", "item": "Torque"}),
    );
    f
}

#[test]
fn add_interface_creates_everything_and_it_all_resolves() {
    let mut f = files(BASE);
    let info = run(
        &mut f,
        json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Vehicle::engine", "to": "Demo::Vehicle::trans", "item": "Torque"}),
    );
    let whats: Vec<&str> = info["created"].as_array().unwrap().iter().map(|c| c["what"].as_str().unwrap()).collect();
    for w in ["item def", "port def", "port def feature", "connection", "flow"] {
        assert!(whats.contains(&w), "{whats:?}");
    }
    assert_eq!(whats.iter().filter(|w| **w == "port").count(), 2);
    let t = &f[0].text;
    // The new definitions carry ids, as create_element gives them (`item def <e...> Torque;`).
    assert!(t.contains("> Torque;") && t.contains("item def <"), "{t}");
    assert!(t.contains("> TorquePort {"), "{t}");
    assert!(t.contains("port torqueOut : Demo::TorquePort;") && t.contains("port torqueIn : ~Demo::TorquePort;"), "{t}");
    assert!(t.contains("connect engine.torqueOut to trans.torqueIn;"), "{t}");
    assert!(t.contains("flow of Demo::Torque from engine.torqueOut.torque to trans.torqueIn.torque;"), "{t}");
    let m = model(&f);
    let c = check(&m, "Demo");
    assert_eq!(c["passed"], true, "{c}");
    assert!(has_edge(&f, "connect", "Demo::Engine::torqueOut", "Demo::Transmission::torqueIn"));
    assert!(has_edge(&f, "flow", "Demo::TorquePort::torque", "Demo::TorquePort::torque"));
}

fn check(m: &Model, scope: &str) -> Value {
    connect::check_connections(m, scope).unwrap()
}

#[test]
fn add_interface_is_idempotent_for_parts_and_refuses_definitions() {
    let mut f = vehicle_with_interface();
    // Second item between the same parts reuses the existing definitions and adds fresh ports.
    let info = run(
        &mut f,
        json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Vehicle::engine", "to": "Demo::Vehicle::trans", "item": "Heat",
               "names": {"connection": "heatPath"}}),
    );
    assert!(info["created"].as_array().unwrap().iter().any(|c| c["ref"].as_str().unwrap().ends_with("heatPath")));
    // Same names again: the connection already exists.
    let msg = fails(&f, json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Vehicle::engine", "to": "Demo::Vehicle::trans", "item": "Heat", "names": {"connection": "heatPath"}}));
    assert!(msg.contains("already has a member named 'heatPath'"), "{msg}");
    let msg = fails(&f, json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Engine", "to": "Demo::Transmission", "item": "Torque"}));
    assert!(msg.contains("definition") && msg.contains("Pass the part usage"), "{msg}");
    assert!(fails(&f, json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Vehicle::engine", "to": "Demo::Vehicle::engine", "item": "T"})).contains("same part"));
}

#[test]
fn unconnected_ports_are_found_and_cleared() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_element", "owner": "Demo::Engine", "kind": "port", "name": "spare"}));
    let m = model(&f);
    let r = connect::find_unconnected_ports(&m, "Demo").unwrap();
    assert!(r["unconnected"].as_array().unwrap().iter().any(|p| p["port"] == "Demo::Engine::spare" && p["part"] == "Demo::Vehicle::engine"), "{r}");
    // Wiring the port clears it.
    let mut g = files(BASE);
    run(&mut g, json!({"op": "add_interface", "context": "Demo::Vehicle", "from": "Demo::Vehicle::engine", "to": "Demo::Vehicle::trans", "item": "Torque"}));
    let r = connect::find_unconnected_ports(&model(&g), "Demo").unwrap();
    assert_eq!(r["unconnected_count"], 0, "{r}");
    assert_eq!(r["ports_checked"].as_u64().unwrap() >= 2, true);
}

#[test]
fn check_connections_catches_each_seeded_defect() {
    // Clean first.
    assert_eq!(check(&model(&vehicle_with_interface()), "Demo")["passed"], true);
    // Same direction: both ports non-conjugated.
    let mut f = vehicle_with_interface();
    f[0].text = f[0].text.replace("~Demo::TorquePort", "Demo::TorquePort");
    let r = rules(&check(&model(&f), "Demo"));
    assert!(!r.is_empty() && r.iter().all(|x| x == "same-direction-ports"), "{r:?}");
    // Mismatched types.
    let mut f = vehicle_with_interface();
    f[0].text = f[0].text.replace("part def Tester;", "part def Tester;\n    port def Other { in item x : Demo::Torque; }");
    f[0].text = f[0].text.replace("port torqueIn : ~Demo::TorquePort;", "port torqueIn : ~Demo::Other;");
    let r = rules(&check(&model(&f), "Demo"));
    assert!(r.contains(&"port-type-mismatch".to_string()), "{r:?}");
    // Connection between definitions.
    let f = files("package Z { part def A; part def B; part def C { connection k connect A to B; } }");
    let r = rules(&check(&model(&f), "Z"));
    assert_eq!(r.iter().filter(|x| *x == "connection-between-definitions").count(), 2, "{r:?}");
}

// ---- export, view stats, plan -----------------------------------------------------

#[test]
fn export_whole_and_subtree() {
    let m = model(&files(BASE));
    let all = agent::export_sysml(&m, None).unwrap();
    assert!(all["text"].as_str().unwrap().contains("// ---- file: demo.sysml ----"));
    let sub = agent::export_sysml(&m, Some("Demo::Vehicle")).unwrap();
    let text = sub["text"].as_str().unwrap();
    assert!(text.starts_with("part def Vehicle {") && text.contains("    part engine : Engine;"), "{text}");
    assert_eq!(sub["element_count"], 3);
    assert_eq!(sub["external_references"], json!(["Demo::Engine", "Demo::Transmission"]));
    // The exported subtree parses by itself; only the names it takes from outside stay open.
    assert!(errors(&model(&files(text))).is_empty());
    assert!(agent::export_sysml(&m, Some("Nope")).is_err());
}

#[test]
fn plan_reports_diff_and_new_errors_with_the_parse_fault_first() {
    let before = files(BASE);
    let ok = plan::plan(&before, vec![plan::Proposed { path: "demo.sysml".into(), text: Some(BASE.replace("part def Tester;", "part def Tester2;")) }]);
    assert!(ok.changed() && ok.new_errors.is_empty() && ok.parse_faults.is_empty());
    assert!(ok.diff().contains("-    part def Tester;\n+    part def Tester2;"), "{}", ok.diff());
    assert_ne!(ok.revision_before, ok.revision_after);
    let bad = plan::plan(&before, vec![plan::Proposed { path: "demo.sysml".into(), text: Some(BASE.replace("part def Tester;", "part def Tester {")) }]);
    // A plan never refuses: it reports the new errors, and the parse fault leads every list.
    assert!(bad.errors_after > bad.errors_before);
    assert!(!bad.new_errors.is_empty());
    assert!(!bad.parse_faults.is_empty(), "{:?}", bad.new_errors);
    assert_eq!(bad.new_errors[0].message, bad.parse_faults[0].message);
    assert_eq!(bad.touched_diagnostics[0].message, bad.parse_faults[0].message);
    // No change, no delta.
    let same = plan::plan(&before, vec![plan::Proposed { path: "demo.sysml".into(), text: Some(BASE.into()) }]);
    assert!(!same.changed());
    // Create and delete.
    let c = plan::plan(&before, vec![plan::Proposed { path: "new.sysml".into(), text: Some("part def N;\n".into()) }]);
    assert_eq!(c.deltas[0].change, "created");
    let d = plan::plan(&before, vec![plan::Proposed { path: "demo.sysml".into(), text: None }]);
    assert_eq!(d.deltas[0].change, "deleted");
}

#[test]
fn raw_ops_and_bad_ops_through_apply_op() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "set_doc", "target": "Demo::Engine", "doc": "Makes torque."}));
    run(&mut f, json!({"op": "rename", "target": "Demo::Tester", "new_name": "Rig"}));
    assert!(model(&f).find("Demo::Rig").is_some());
    assert!(fails(&f, json!({"op": "teleport"})).contains("unknown edit"));
    assert!(fails(&f, json!({"nothing": 1})).contains("needs an 'op'"));
    assert!(fails(&f, json!({"op": "rename", "target": "Demo::Nope", "new_name": "x"})).contains("no element named"));
}

// ---- richer create_element (A, B) ------------------------------------------------

const SYS: &str = r#"package Sys {
    part def Engine {
        attribute power : Real;
        part inner : Gear;
    }
    part def Gear {
        attribute teeth : Integer;
    }
    part def Base {
        attribute baseAttr : Real;
    }
    part def Motor :> Base;
    item def Fuel;
    connection def Link;
    action def Burn;
    metadata def Approved;
    state def Machine {
        state off;
        state on;
    }
    part def Car {
        part engine : Engine;
        part gear : Gear;
        action run : Burn;
    }
    part car : Car {
        part motor : Motor;
    }
    requirement def Req;
}
"#;

fn text_of(f: &[FileIn]) -> String {
    f.iter().map(|x| x.text.clone()).collect::<Vec<_>>().join("\n")
}

#[test]
fn create_element_takes_specializes_value_short_name_multiplicity_doc() {
    let mut f = files(SYS);
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "part def", "name": "Turbo", "specializes": ["Sys::Engine", "Sys::Base"], "doc": "Fast."}));
    let m = model(&f);
    let turbo = m.find("Sys::Turbo").unwrap();
    assert_eq!(turbo.refs_of(sysml_core::RefRole::Specializes).count(), 2);
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("part def Turbo :> Sys::Engine, Sys::Base {"), "{}", text_of(&f));
    run(&mut f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "attribute", "name": "mass", "typed_by": "Real", "value": "5", "multiplicity": "[1..2]"}));
    let m = model(&f);
    assert_eq!(m.find("Sys::Car::mass").unwrap().value.as_deref(), Some("5"));
    assert_eq!(m.find("Sys::Car::mass").unwrap().multiplicity.as_deref(), Some("1..2"));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("attribute mass : Real [1..2] = 5;"), "{}", text_of(&f));
    // A unit in brackets is a plain expression.
    let applied = intent::apply_op(&f, &json!({"op": "create_element", "owner": "Sys::Car", "kind": "attribute", "name": "volts", "typed_by": "Real", "value": "12 [V]"})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&applied.files)).contains("attribute volts : Real = 12 [V];"));
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "requirement", "name": "Top speed", "short_name": "R-1", "doc": "Go fast."}));
    let m = model(&f);
    assert_eq!(m.find("Sys::'Top speed'").unwrap().short_name.as_deref(), Some("R-1"));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("requirement <'R-1'> 'Top speed' {"), "{}", text_of(&f));
    // A usage subsets another usage.
    run(&mut f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "part", "name": "engine2", "typed_by": "Sys::Engine", "specializes": "Sys::Car::engine"}));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("part engine2 : Sys::Engine :> Sys::Car::engine;"), "{}", text_of(&f));
    run(&mut f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "port", "name": "fuelIn", "direction": "in"}));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("in port fuelIn;"));
    // Nested features take the same arguments.
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "part def", "name": "Rig", "features": [{"kind": "attribute", "name": "k", "typed_by": "Real", "value": "2", "multiplicity": "3"}]}));
    assert_eq!(model(&f).find("Sys::Rig::k").unwrap().value.as_deref(), Some("2"));
}

#[test]
fn create_element_redefines_an_inherited_feature() {
    let mut f = files(SYS);
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "part def", "name": "Turbo", "specializes": "Sys::Engine"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sys::Turbo", "kind": "attribute", "name": "power", "redefines": "power", "value": "900"}));
    let m = model(&f);
    let p = m.find("Sys::Turbo::power").unwrap();
    assert_eq!(p.first_resolved(sysml_core::RefRole::Redefines), m.find("Sys::Engine::power").map(|e| e.id));
    assert_eq!(p.value.as_deref(), Some("900"));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("attribute power :>> Sys::Engine::power = 900;"), "{}", text_of(&f));
    // The owner must specialize the type that declares the feature.
    let e = fails(&f, json!({"op": "create_element", "owner": "Sys::Gear", "kind": "attribute", "name": "x", "redefines": "Sys::Engine::power"}));
    assert!(e.contains("Fix: create_relationship {kind: 'specialize', source: 'Sys::Gear', target: 'Sys::Engine'}"), "{e}");
    let e = fails(&f, json!({"op": "create_element", "kind": "attribute", "name": "x", "redefines": "Sys::Engine::power"}));
    assert!(e.contains("needs an owner"), "{e}");
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys", "kind": "attribute def", "name": "x", "redefines": "a"})).contains("does not redefine"));
}

#[test]
fn create_element_validates_new_arguments() {
    let f = files(SYS);
    let mk = |extra: Value| {
        let mut op = json!({"op": "create_element", "owner": "Sys::Car", "kind": "attribute", "name": "z", "typed_by": "Real"});
        for (k, v) in extra.as_object().unwrap() {
            op[k] = v.clone();
        }
        fails(&f, op)
    };
    for bad in ["5; part evil", "1 } part evil {", "5 // x", "5 /* x */", "5\npart evil", "(5", "'a", "5 $ 6", "5 # 6"] {
        let e = mk(json!({"value": bad}));
        assert!(e.contains("not a plain expression") && e.contains("Fix:"), "{bad}: {e}");
    }
    for bad in ["4]; part evil [", "1..*;", "2\n3", "{3}", ""] {
        let op = json!({"op": "create_element", "owner": "Sys::Car", "kind": "part", "name": "z", "multiplicity": bad});
        if bad.is_empty() {
            assert!(intent::apply_op(&f, &op).is_ok(), "empty multiplicity means none");
            continue;
        }
        let e = fails(&f, op);
        assert!(e.contains("multiplicity") && e.contains("Fix:"), "{bad}: {e}");
    }
    assert!(mk(json!({"short_name": "a'b"})).contains("short_name"));
    assert!(mk(json!({"specializes": "Sys::Nope"})).contains("no element named"));
    assert!(mk(json!({"specializes": "Sys::Engine"})).contains("a usage specializes usages"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys", "kind": "part def", "name": "X", "specializes": "Sys::car"})).contains("a definition specializes definitions"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys", "kind": "part def", "name": "X", "value": "5"})).contains("has no value"));
    assert!(mk(json!({"specializes": 5})).contains("name or a list of names"));
    assert!(mk(json!({"direction": "sideways"})).contains("direction must be"));
}

#[test]
fn metadata_without_a_name_is_an_annotation() {
    let mut f = files(SYS);
    let info = run(&mut f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "metadata", "typed_by": "Sys::Approved"}));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("@Sys::Approved;"), "{}", text_of(&f));
    let m = model(&f);
    let made = m.find(info["ref"].as_str().unwrap()).unwrap();
    assert_eq!(made.kind, "metadata");
    assert!(made.name.is_none());
    assert_eq!(made.first_resolved(sysml_core::RefRole::Type), m.find("Sys::Approved").map(|e| e.id));
    // The same annotation twice is refused; if_absent makes it a no-op.
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "metadata", "typed_by": "Sys::Approved"})).contains("already carries @"));
    let again = intent::apply_op(&f, &json!({"op": "create_element", "owner": "Sys::Car", "kind": "metadata", "typed_by": "Sys::Approved", "if_absent": true})).unwrap();
    assert_eq!(again.info["existed"], true);
    assert_eq!(again.files[0].text, f[0].text);
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "metadata"})).contains("needs typed_by"));
    assert!(fails(&f, json!({"op": "create_element", "kind": "metadata", "typed_by": "Sys::Approved"})).contains("pass owner"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "metadata", "typed_by": "Sys::Approved", "doc": "x"})).contains("takes no 'doc'"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "Sys", "kind": "metadata def"})).contains("missing argument 'name'"));
    // A named metadata usage is still an ordinary usage.
    run(&mut f, json!({"op": "create_element", "owner": "Sys::Car", "kind": "metadata", "name": "tag", "typed_by": "Sys::Approved"}));
    assert!(model(&f).find("Sys::Car::tag").is_some());
}

// ---- new relationship kinds (C) -----------------------------------------------------

#[test]
fn connect_joins_two_usages_without_ports() {
    let mut f = files(SYS);
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Car::engine", "target": "Sys::Car::gear"}));
    assert_eq!(info["statement"], "connect engine to gear;");
    assert_eq!(info["declared_in"], "Sys::Car");
    let named = run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Car::gear", "target": "Sys::Car::engine", "name": "back", "typed_by": "Sys::Link"}));
    assert_eq!(named["statement"], "connection back : Sys::Link connect gear to engine;");
    let m = model(&f);
    let c = m.find("Sys::Car::back").unwrap();
    assert_eq!(c.first_resolved(sysml_core::RefRole::Type), m.find("Sys::Link").map(|e| e.id));
    let typed = run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::car::motor", "target": "Sys::Car::gear", "typed_by": "Sys::Link", "owner": "Sys::car"}));
    assert_eq!(typed["statement"], "connection : Sys::Link connect motor to gear;");
    let e = fails(&f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Engine", "target": "Sys::Gear"}));
    assert!(e.contains("joins usages") && e.contains("Fix:"), "{e}");
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Car::engine", "target": "Sys::Car::gear"})).contains("already exists"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Car::engine", "target": "Sys::car::motor", "typed_by": "Sys::Engine"})).contains("connection definition"));
    // The nearest shared owner here is a package: the caller must say where it goes.
    let e = fails(&f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Car::engine", "target": "Sys::car::motor"}));
    assert!(e.contains("pass owner"), "{e}");
    run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::Car::engine", "target": "Sys::car::motor", "owner": "Sys::car"}));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Sys::car", "target": "Sys::Req", "name": "x"})).contains("does not apply"));
}

#[test]
fn flow_names_the_item_and_refuses_definitions() {
    let mut f = files(SYS);
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "flow", "source": "Sys::Car::engine", "target": "Sys::Car::gear", "of": "Sys::Fuel"}));
    assert_eq!(info["statement"], "flow of Sys::Fuel from engine to gear;");
    let m = model(&f);
    assert!(graph::statements(&m).iter().any(|s| s.kind == "flow"));
    // Another item between the same ends is a different flow; the same one is not.
    run(&mut f, json!({"op": "create_relationship", "kind": "flow", "source": "Sys::Car::engine", "target": "Sys::Car::gear", "name": "plain"}));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "flow", "source": "Sys::Car::engine", "target": "Sys::Car::gear", "of": "Sys::Fuel"})).contains("already exists"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "flow", "source": "Sys::Engine", "target": "Sys::Car::gear"})).contains("joins usages"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "flow", "source": "Sys::Car::engine", "target": "Sys::Car::gear", "of": "Sys::Car::engine", "name": "z"})).contains("must be a definition"));
}

#[test]
fn perform_links_a_part_to_an_action() {
    let mut f = files(SYS);
    // Car holds a usage of Burn (run): performing the definition would add a second usage of one function. Car performs
    // run instead (agent/duplicates.rs, copied-usage).
    let copy = fails(&f, json!({"op": "create_relationship", "kind": "perform", "source": "Sys::Car", "target": "Sys::Burn"}));
    assert!(copy.contains("Sys::Car::run"), "{copy}");
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "action def", "name": "Ignite"}));
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "perform", "source": "Sys::Car", "target": "Sys::Ignite"}));
    assert_eq!(info["statement"], "perform action ignite : Sys::Ignite;");
    assert_eq!(info["declared_in"], "Sys::Car");
    let again = fails(&f, json!({"op": "create_relationship", "kind": "perform", "source": "Sys::Car", "target": "Sys::Ignite"}));
    assert!(again.contains("already exists"), "{again}");
    let note = intent::apply_op(&f, &json!({"op": "create_relationship", "kind": "perform", "source": "Sys::Car", "target": "Sys::Ignite", "if_absent": true})).unwrap();
    assert_eq!(note.info["existed"], true);
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "perform", "source": "Sys::car", "target": "Sys::Car::run"}));
    assert_eq!(info["statement"], "perform Sys::Car::run;");
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "perform", "source": "Sys::Car", "target": "Sys::Engine"})).contains("needs an action"));
}

#[test]
fn transition_takes_trigger_guard_and_effect() {
    let mut f = files(SYS);
    let info = run(
        &mut f,
        json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::off", "target": "Sys::Machine::on", "trigger": "after 30 [s]", "guard": "ready", "effect": "start"}),
    );
    assert_eq!(info["statement"], "transition first off accept after 30 [s] if ready do start then on;");
    assert_eq!(info["declared_in"], "Sys::Machine");
    let m = model(&f);
    let t = m.elements.iter().find(|e| e.kind == "transition").unwrap();
    assert_eq!(t.props.get("trigger").map(|s| s.trim()), Some("after 30 [s]"));
    assert_eq!(t.props.get("guard").map(|s| s.trim()), Some("ready"));
    assert!(graph::statements(&m).iter().any(|s| s.kind == "transition" && s.source.id == m.find("Sys::Machine::off").map(|e| e.id)));
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::on", "target": "Sys::Machine::off", "name": "back", "trigger": "when level > 5"}));
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("transition back first on accept when level > 5 then off;"), "{}", text_of(&f));
    // The same ends with another trigger is a different transition; the same trigger is a duplicate.
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::off", "target": "Sys::Machine::on", "trigger": "when go"}));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::off", "target": "Sys::Machine::on", "trigger": "after 30 [s]", "guard": "ready", "effect": "start"})).contains("already exists"));
    // A signal reference must resolve.
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::off", "target": "Sys::Machine::on", "trigger": "Sys::Fuel", "name": "t3"}));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::off", "target": "Sys::Machine::on", "trigger": "Sys::Nope", "name": "t4"})).contains("not 'after N [unit]'"));
    for bad in ["after 5; part evil", "when x then y", "after 5 // c"] {
        let e = fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine::off", "target": "Sys::Machine::on", "trigger": bad, "name": "t5"}));
        assert!(e.contains("Fix:"), "{bad}: {e}");
    }
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Machine", "target": "Sys::Machine::on"})).contains("joins usages"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "Sys::Car::engine", "target": "Sys::Car::gear"})).contains("joins states"));
}

#[test]
fn trace_and_redefine_are_plain_statements() {
    let mut f = files(SYS);
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "trace", "source": "Sys::Car::engine", "target": "Sys::Req"}));
    assert_eq!(info["statement"], "#SysMLv1::Trace dependency from Sys::Car::engine to Sys::Req;");
    let m = model(&f);
    assert!(graph::statements(&m).iter().any(|s| s.kind == "trace"));
    assert!(m.find("Sys::SysMLv1::Trace").is_some());
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "trace", "source": "Sys::Car::engine", "target": "Sys::Req"})).contains("already exists"));
    // redefine: Turbo.power redefines Engine.power, inherited through the specialization.
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "part def", "name": "Turbo", "specializes": "Sys::Engine"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sys::Turbo", "kind": "attribute", "name": "power", "typed_by": "Real"}));
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "redefine", "source": "Sys::Turbo::power", "target": "Sys::Engine::power"}));
    assert_eq!(info["statement"], "Sys::Turbo::power :>> Sys::Engine::power");
    assert!(sysml_core::agent::mint::strip_ids(&text_of(&f)).contains("attribute power : Real :>> Sys::Engine::power;"), "{}", text_of(&f));
    let m = model(&f);
    assert_eq!(m.find("Sys::Turbo::power").unwrap().first_resolved(sysml_core::RefRole::Redefines), m.find("Sys::Engine::power").map(|e| e.id));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "redefine", "source": "Sys::Turbo::power", "target": "Sys::Engine::power"})).contains("already exists"));
    let e = fails(&f, json!({"op": "create_relationship", "kind": "redefine", "source": "Sys::Gear::teeth", "target": "Sys::Engine::power"}));
    assert!(e.contains("Fix: create_relationship {kind: 'specialize', source: 'Sys::Gear', target: 'Sys::Engine'}"), "{e}");
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "redefine", "source": "Sys::Turbo", "target": "Sys::Engine"})).contains("needs a usage"));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "teleport", "source": "Sys::Turbo", "target": "Sys::Engine"})).contains("redefine, connect, flow, succession flow, perform, transition or succession"));
}

// ---- idempotency (D) --------------------------------------------------------------------

#[test]
fn raw_add_element_refuses_an_existing_name() {
    let f = files(BASE);
    let e = fails(&f, json!({"op": "add_element", "owner": "Demo", "text": "part def Engine;"}));
    assert!(e.contains("'Demo' already has a member named 'Engine'") && e.contains("Demo::Engine"), "{e}");
    assert!(fails(&f, json!({"op": "add_element", "owner": "Demo", "text": "part Engine : Demo::Engine;"})).contains("already has a member"));
    assert!(fails(&f, json!({"op": "add_element", "file": "demo.sysml", "text": "package Demo { }"})).contains("already has a member"));
    let mut g = files(BASE);
    run(&mut g, json!({"op": "add_element", "owner": "Demo", "text": "part def Fresh;"}));
    // A package of the same name in another file is a split package, not a duplicate.
    run(&mut g, json!({"op": "add_element", "file": "other.sysml", "text": "package Demo { part def Elsewhere; }"}));
    // Unnamed statements never clash by name. A repeated relationship is refused as create_relationship refuses it,
    // unless the caller says the source model has parallel statements.
    run(&mut g, json!({"op": "add_element", "owner": "Demo", "text": "dependency from Engine to Fresh;"}));
    let e = fails(&g, json!({"op": "add_element", "owner": "Demo", "text": "dependency from Engine to Fresh;"}));
    assert!(e.contains("already exists") && e.contains("allow_duplicate"), "{e}");
    run(&mut g, json!({"op": "add_element", "owner": "Demo", "text": "dependency from Engine to Fresh;", "allow_duplicate": true}));
}

#[test]
fn if_absent_returns_the_existing_element_and_writes_nothing() {
    let f = files(BASE);
    let a = intent::apply_op(&f, &json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "Engine", "if_absent": true})).unwrap();
    assert_eq!(a.info["existed"], true);
    assert_eq!(a.info["ref"], "Demo::Engine");
    assert_eq!(a.files[0].text, f[0].text);
    // The kind must match.
    let e = fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "port", "name": "Engine", "if_absent": true}));
    assert!(e.contains("it is a part def, not a port"), "{e}");
    // A new name is created and not marked as existing.
    let mut g = files(BASE);
    let info = run(&mut g, json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "Fresh", "if_absent": true}));
    assert!(info.get("existed").is_none());
    // Relationships.
    let mut g = files(BASE);
    run(&mut g, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    let a = intent::apply_op(&g, &json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed", "if_absent": true})).unwrap();
    assert_eq!(a.info["existed"], true);
    assert_eq!(a.info["declared_at"]["element"].as_str().is_some(), true);
    assert_eq!(a.files[0].text, g[0].text);
    let b = fails(&g, json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::speed"}));
    assert!(b.contains("already exists") && b.contains("if_absent"), "{b}");
    run(&mut g, json!({"op": "create_relationship", "kind": "specialize", "source": "Demo::Engine", "target": "Demo::Transmission"}));
    let c = intent::apply_op(&g, &json!({"op": "create_relationship", "kind": "specialize", "source": "Demo::Engine", "target": "Demo::Transmission", "if_absent": true})).unwrap();
    assert_eq!(c.info["existed"], true);
    assert_eq!(c.files[0].text, g[0].text);
}

// ---- forward references in a batch (E) ------------------------------------------------

#[test]
fn a_batch_may_use_an_element_a_later_edit_creates() {
    let edits = vec![
        json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Reqs::late"}),
        json!({"op": "create_element", "owner": "Demo::Reqs", "kind": "requirement", "name": "late"}),
        json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "spare", "typed_by": "Demo::NewDef"}),
        json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "NewDef"}),
    ];
    let b = intent::apply_batch(&files(BASE), &edits).unwrap();
    assert_eq!(b.moved, vec![json!({"edit": 2, "before": 1}), json!({"edit": 4, "before": 3})]);
    assert_eq!(b.results.len(), 4);
    // Results stay in the order the caller wrote the edits.
    assert_eq!(b.results[0]["op"], "create_relationship");
    assert_eq!(b.results[1]["ref"], "Demo::Reqs::late");
    assert_eq!(b.results[3]["ref"], "Demo::NewDef");
    let m = model(&b.files);
    assert!(errors(&m).is_empty());
    assert!(m.find("Demo::spare").is_some());
    // A batch that is already in order is not touched.
    let ordered = intent::apply_batch(&files(BASE), &[edits[1].clone(), edits[0].clone()]).unwrap();
    assert!(ordered.moved.is_empty());
}

#[test]
fn a_batch_that_cannot_be_ordered_says_which_edit_to_move() {
    let edits = vec![
        json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "A", "specializes": "Demo::B"}),
        json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "B", "specializes": "Demo::A"}),
    ];
    let (i, f) = intent::apply_batch(&files(BASE), &edits).unwrap_err();
    assert!(f.message.contains("created by edit #") && f.message.contains("move #"), "{}", f.message);
    assert!(i < 2);
    // A name nothing creates keeps the plain error and the index of the failing edit.
    let edits = vec![json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "C"}), json!({"op": "create_relationship", "kind": "satisfy", "source": "Demo::vehicle1", "target": "Demo::Nope"})];
    let (i, f) = intent::apply_batch(&files(BASE), &edits).unwrap_err();
    assert_eq!(i, 1);
    assert!(f.message.contains("no element named \"Demo::Nope\"") && !f.message.contains("move #"), "{}", f.message);
}

// ---- feature chains through typing (F) ----------------------------------------------

#[test]
fn locate_follows_a_chain_through_typing_and_specialization() {
    let m = model(&files(SYS));
    let l = |n: &str| agent::locate(&m, n).map(|e| e.qualified_name.clone());
    assert_eq!(l("Sys::car::motor::baseAttr").unwrap(), "Sys::Base::baseAttr");
    assert_eq!(l("Sys::Car::engine::power").unwrap(), "Sys::Engine::power");
    assert_eq!(l("Sys::Car::engine::inner::teeth").unwrap(), "Sys::Gear::teeth");
    // An exact name still wins.
    assert_eq!(l("Sys::Car::engine").unwrap(), "Sys::Car::engine");
    assert_eq!(l("Sys::Engine::power").unwrap(), "Sys::Engine::power");
    assert!(l("Sys::Car::engine::nothing").unwrap_err().message.contains("no element named"));
    // Specialization cycles do not hang the lookup.
    let cyc = model(&files("package C { part def A :> B { part a; } part def B :> A; part x : A; }"));
    assert!(agent::locate(&cyc, "C::x::missing").is_err());
    assert_eq!(agent::locate(&cyc, "C::x::a").unwrap().qualified_name, "C::A::a");
    let r = agent::locate_ref(&m, "Sys::Car::engine::power").unwrap();
    assert_eq!(r.text, "Sys::Car::engine::power");
    assert_eq!(r.anchor.qualified_name, "Sys::Car::engine");
}

#[test]
fn trace_statements_can_name_a_chain() {
    let mut f = files(SYS);
    run(&mut f, json!({"op": "create_element", "owner": "Sys", "kind": "requirement", "name": "Power"}));
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Sys::car::motor::baseAttr", "target": "Sys::Power"}));
    assert_eq!(info["statement"], "satisfy Sys::Power by Sys::car::motor::baseAttr;");
    // Cameo puts a new satisfy in the client's nearest package, not in the part.
    assert_eq!(info["declared_in"], "Sys");
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "Sys::Car::engine::power", "target": "Sys::car::motor::baseAttr"}));
    assert_eq!(info["statement"], "allocate Sys::Car::engine::power to Sys::car::motor::baseAttr;");
    let m = model(&f);
    let sat = graph::statements(&m).into_iter().find(|s| s.kind == "satisfy").unwrap();
    assert_eq!(sat.source.id, m.find("Sys::Base::baseAttr").map(|e| e.id));
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "Sys::car::engine", "target": "Sys::car::motor::baseAttr", "owner": "Sys::car"}));
    // The ends are written relative to the owner, so a view can draw them.
    assert_eq!(info["statement"], "connect engine to motor.baseAttr;");
}

// ---- messages (G) and names (H) ------------------------------------------------------------

#[test]
fn missing_arguments_name_the_argument_and_say_what_it_is() {
    let f = files(BASE);
    let e = fails(&f, json!({"op": "create_element", "owner": "Demo", "name": "x"}));
    assert!(e.starts_with("missing argument 'kind': ") && e.contains("part def"), "{e}");
    let e = fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part"}));
    assert!(e.starts_with("missing argument 'name': the element name"), "{e}");
    let e = fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "target": "Demo::Reqs::speed"}));
    assert!(e.starts_with("missing argument 'source': the qualified name"), "{e}");
    let e = fails(&f, json!({"op": "create_relationship", "source": "Demo::Engine", "target": "Demo::Reqs::speed"}));
    assert!(e.contains("missing argument 'kind'") && e.contains("trace, redefine, connect, flow, succession flow, perform, transition or succession"), "{e}");
    let e = fails(&f, json!({"op": "update_element", "rename": "x"}));
    assert!(e.starts_with("missing argument 'ref': the qualified name"), "{e}");
    let e = fails(&f, json!({"op": "rename", "target": "Demo::Engine"}));
    assert!(e.starts_with("missing argument 'new_name': "), "{e}");
    let e = fails(&f, json!({"op": "add_element", "owner": "Demo"}));
    assert!(e.starts_with("missing argument 'text': "), "{e}");
    let e = fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": 7, "name": "x"}));
    assert!(e.starts_with("argument 'kind' must be a string: "), "{e}");
    let e = fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "  "}));
    assert!(e.starts_with("argument 'name' is empty: "), "{e}");
    let e = fails(&f, json!({"op": "add_interface", "context": "Demo", "from": "Demo::vehicle1"}));
    assert!(e.starts_with("missing argument 'to': "), "{e}");
}

#[test]
fn a_definition_and_a_usage_with_one_name_raise_the_duplicate_warning() {
    let mut m = model(&files("package P { part def Engine; part Engine; }"));
    agent::add_namespace_checks(&mut m);
    let dups: Vec<_> = m.diagnostics.iter().filter(|d| d.message.starts_with("duplicate name 'Engine'")).collect();
    assert_eq!(dups.len(), 1);
    // The same package declared twice (or in two files) shares one namespace.
    let two = vec![
        FileIn { path: "a.sysml".into(), text: "package P { part def Engine; }".into() },
        FileIn { path: "b.sysml".into(), text: "package P { part Engine : P::Engine; part Other; }".into() },
    ];
    let mut m = api::to_model(two.clone());
    assert!(m.diagnostics.iter().all(|d| !d.message.starts_with("duplicate")), "parser alone cannot see it");
    agent::add_namespace_checks(&mut m);
    let d: Vec<_> = m.diagnostics.iter().filter(|d| d.message.starts_with("duplicate name 'Engine'")).collect();
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("a.sysml:1"), "{}", d[0].message);
    // validate_model reports it under the duplicate-name rule.
    let v = trace::validate_model(&m, Some("semantic")).unwrap();
    assert!(rules(&v).contains(&"duplicate-name".to_string()), "{v}");
    // Split packages alone are not duplicates.
    let mut ok = api::to_model(vec![
        FileIn { path: "a.sysml".into(), text: "package P { part def A; }".into() },
        FileIn { path: "b.sysml".into(), text: "package P { part def B; }".into() },
    ]);
    agent::add_namespace_checks(&mut ok);
    assert!(ok.diagnostics.iter().all(|d| !d.message.starts_with("duplicate")));
}

#[test]
fn a_new_warning_is_not_hidden_by_old_ones() {
    // 60 old unresolved names fill the file; the write adds one duplicate at the end.
    let mut text = String::from("package P {\n");
    for i in 0..60 {
        text.push_str(&format!("    part p{i} : Missing{i};\n"));
    }
    text.push_str("    part def Engine;\n}\n");
    let before = files(&text);
    let after = text.replace("    part def Engine;\n", "    part def Engine;\n    part Engine;\n");
    let p = plan::plan(&before, vec![plan::Proposed { path: "demo.sysml".into(), text: Some(after) }]);
    assert_eq!(p.new_warnings.len(), 1, "{:?}", p.new_warnings);
    assert!(p.new_warnings[0].message.starts_with("duplicate name 'Engine'"));
    assert!(p.touched_diagnostics[0].message.starts_with("duplicate name 'Engine'"), "{:?}", p.touched_diagnostics[0]);
}

#[test]
fn path_is_an_alias_of_file() {
    let mut f = files(BASE);
    run(&mut f, json!({"op": "create_element", "kind": "package", "name": "Other", "path": "other.sysml"}));
    assert!(f.iter().any(|x| x.path == "other.sysml"));
    run(&mut f, json!({"op": "add_element", "path": "third.sysml", "text": "package Third;"}));
    assert!(f.iter().any(|x| x.path == "third.sysml"));
    let e = fails(&f, json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "q", "path": "elsewhere.sysml"}));
    assert!(e.contains("lives in 'demo.sysml'"), "{e}");
}

// ---- round 2: names that nothing reported ---------------------------------------------

fn unresolved(text: &str) -> Vec<String> {
    let m = model(&files(text));
    m.diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).map(|d| d.message.clone()).collect()
}

#[test]
fn metadata_prefix_and_annotation_names_must_resolve() {
    let bad = unresolved("package P { metadata def Tag; #P::NoSuchThing part x; @NoSuch; part y { @Nope2; } }");
    assert_eq!(bad.len(), 3, "{bad:?}");
    assert!(bad.iter().any(|m| m.contains("NoSuchThing")) && bad.iter().any(|m| m.contains("'NoSuch'")) && bad.iter().any(|m| m.contains("Nope2")));
    assert!(unresolved("package P { metadata def Tag; #Tag part x; @Tag; #P::Tag part z; }").is_empty());
}

#[test]
fn value_expression_names_must_resolve_and_literals_do_not_raise() {
    let bad = unresolved("package P { attribute def A; part def Q { attribute a : A; attribute x = Nope::Missing; attribute y = a.zz; } }");
    assert_eq!(bad.len(), 2, "{bad:?}");
    let ok = unresolved(
        "package P { enum def L { L3; L4; } part def Q { attribute a : Real = 5 [s]; attribute n = 2.0e3; attribute b = true; attribute s = \"hi\"; \
         attribute l : L = L3; attribute m = (a + 1) * 2 - sum(a); attribute o = a.size(); attribute k : Real = 3 [kg]; attribute v = null; } }",
    );
    assert!(ok.is_empty(), "{ok:?}");
    // The right-hand name is a model element, so rename and delete must see it.
    let f = files("package P { attribute base = 1; attribute copy = base + 1; }");
    let m = model(&f);
    let base = m.find("P::base").unwrap();
    assert_eq!(agent::referrers(&m, base.id, false).len(), 1);
    let renamed = intent::apply_op(&f, &json!({"op": "update_element", "ref": "P::base", "rename": "root"})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&renamed.files[0].text).contains("= root + 1"), "{}", renamed.files[0].text);
    assert!(fails(&f, json!({"op": "delete_element", "ref": "P::base"})).contains("P::copy"));
}

#[test]
fn bare_assignments_resolve_against_the_owner_type() {
    let head = "package P { metadata def M { attribute 'detection time'; attribute level; } ";
    let bad = unresolved(&format!("{head} part x {{ @M {{ detection_time = 2.0; level = 3; }} }} }}"));
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].contains("detection_time"));
    assert!(unresolved(&format!("{head} part x {{ @M {{ level = 3; }} }} }}")).is_empty());
    // An owner with no types, or a library type, is not checked.
    assert!(unresolved("package P { part x { y = 3; } attribute z : Real { w = 1; } }").is_empty());
    // Renaming the feature rewrites the assignment.
    let f = files(&format!("{head} part x {{ @M {{ level = 3; }} }} }}"));
    let r = intent::apply_op(&f, &json!({"op": "update_element", "ref": "P::M::level", "rename": "tier"})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&r.files[0].text).contains("tier = 3"), "{}", r.files[0].text);
    assert!(fails(&f, json!({"op": "delete_element", "ref": "P::M::level"})).contains("refused"));
}

#[test]
fn metadata_prefix_is_a_referrer() {
    let f = files("package P { metadata def Tag; #Tag part x; }");
    assert!(fails(&f, json!({"op": "delete_element", "ref": "P::Tag"})).contains("P::x"));
    let r = intent::apply_op(&f, &json!({"op": "update_element", "ref": "P::Tag", "rename": "Mark"})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&r.files[0].text).contains("#Mark part x"), "{}", r.files[0].text);
}

#[test]
fn quoted_names_may_be_written_unquoted_everywhere() {
    let f = files("package '4.0 Risk Analysis' { package 'Risk Elements' { part def H; } }\n");
    let m = model(&f);
    assert!(m.find("4.0 Risk Analysis::Risk Elements::H").is_some());
    assert!(m.find("'4.0 Risk Analysis'::'Risk Elements'::H").is_some());
    assert!(m.find("4.0 Risk Analysis::Nope").is_none());
    assert!(agent::locate(&m, "4.0 Risk Analysis").is_ok());
    let r = intent::apply_op(&f, &json!({"op": "add_element", "owner": "4.0 Risk Analysis", "text": "part def X;"})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&r.files[0].text).contains("part def X;"));
    assert!(api::outline(&m, Some("4.0 Risk Analysis"), 3).unwrap().contains("Risk Elements"));
}

#[test]
fn allocate_between_definitions_and_the_require_option() {
    let f = files("package P { part def A; part def B; part a : A; requirement r; }");
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "allocate", "source": "P::A", "target": "P::B"})).contains("allow_definitions: true"));
    let mut g = f.clone();
    let info = run(&mut g, json!({"op": "create_relationship", "kind": "allocate", "source": "P::A", "target": "P::B", "allow_definitions": true}));
    assert_eq!(info["warnings"][0]["rule"], "trace-operand-definition");
    assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains("allocate P::A to P::B;"));
    let v = trace::validate_model(&model(&g), Some("semantic")).unwrap();
    assert!(rules(&v).contains(&"trace-operand-definition".to_string()));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "verify", "source": "P::A", "target": "P::r", "allow_definitions": true})).contains("only to allocate and satisfy"));
    // Packages stay refused.
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "allocate", "source": "P", "target": "P::B", "allow_definitions": true})).contains("package"));
    let mut h = f.clone();
    run(&mut h, json!({"op": "create_relationship", "kind": "satisfy", "source": "P::a", "target": "P::r"}));
    let m = model(&h);
    let req = |r: Option<&[String]>| trace::validate_model_with(&m, Some("traceability"), r).unwrap();
    assert_eq!(req(None)["passed"], false);
    assert_eq!(req(Some(&["satisfy".to_string()]))["passed"], true);
    assert_eq!(req(Some(&["satisfy".to_string()]))["required"], json!(["satisfy"]));
    assert!(trace::validate_model_with(&m, None, Some(&["x".to_string()])).is_err());
}

#[test]
fn enum_typed_values_are_checked() {
    let head = "package P { enum def Level { L1; L2; L3; } part def Q { attribute level : Level; } ";
    let bad = |body: &str| -> Vec<String> {
        model(&files(&format!("{head}{body} }}"))).diagnostics.iter().filter(|d| d.message.starts_with("value does not fit type")).map(|d| d.message.clone()).collect()
    };
    assert_eq!(bad("part q : Q { attribute :>> level = \"L3\"; }").len(), 1);
    assert_eq!(bad("attribute x : Level = \"L3\";").len(), 1);
    assert_eq!(bad("attribute x : Level = L9;").len(), 1);
    assert_eq!(bad("attribute x : Level = 3;").len(), 1);
    let m = bad("attribute x : Level = L9;");
    assert!(m[0].contains("L1, L2, L3"), "{m:?}");
    // An unknown enum literal is one finding, not also an unresolved name.
    let all = model(&files(&format!("{head}attribute x : Level = L9; }}")));
    assert!(!all.diagnostics.iter().any(|d| d.message.starts_with("cannot resolve")), "{:?}", all.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    assert!(bad("attribute x : Level = L2; attribute y : Level = Level::L3; part q : Q { attribute :>> level = L1; } attribute z : Level = x;").is_empty());
    assert!(bad("attribute s : String = \"hi\"; attribute r : Real = 2.5;").is_empty());
}

#[test]
fn export_facts_lists_unnamed_members() {
    let m = model(&files("package F { part def Base { attribute mass : Real; } part def Sub :> Base { attribute :>> mass = 2.0; } part a; part b; connect a to b; }"));
    let v = agent::export_facts(&m, Some("F")).unwrap();
    let els = v["elements"].as_array().unwrap();
    assert!(els.iter().any(|e| e["unnamed"] == true && e["name"] == "mass" && e["ref"].as_str().unwrap().contains("$")));
    assert!(els.iter().any(|e| e["unnamed"] == true && e["kind"] == "connection"));
    assert!(els.iter().all(|e| e["unnamed"] != true || e["ref"].as_str().unwrap().contains("$")));
    assert!(v["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "redefinition"));
}

#[test]
fn source_id_events_keep_the_map_right() {
    use agent::ids::{self, IdEvent};
    let mut map: std::collections::BTreeMap<String, String> = [("a", "P::X"), ("b", "P::X::y"), ("c", "P::$3"), ("d", "P::$5::z"), ("e", "P::K")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    ids::apply_events(&mut map, &[IdEvent::Rename { from: "P::X".into(), to: "P::W".into() }]).unwrap();
    assert_eq!((map["a"].as_str(), map["b"].as_str()), ("P::W", "P::W::y"));
    ids::apply_events(&mut map, &[IdEvent::Delete { qn: "P::K".into(), owner: Some("P".into()), index: 1 }]).unwrap();
    assert!(!map.contains_key("e"));
    // Members after the deleted one (position 2, so `$3` and later) move up.
    assert_eq!((map["c"].as_str(), map["d"].as_str()), ("P::$2", "P::$4::z"));
    assert!(ids::apply_events(&mut map, &[IdEvent::Assign { id: "a".into(), qn: "P::other".into() }]).is_err());
    assert!(ids::apply_events(&mut map, &[IdEvent::Assign { id: "a".into(), qn: "P::W".into() }]).is_ok());
    assert!(ids::valid_source_id("_x.1-2").is_ok());
    for bad in ["", "a b", "\u{e9}", &"x".repeat(257)] {
        assert!(ids::valid_source_id(bad).is_err(), "{bad}");
    }
}

// ---- resource bounds ---------------------------------------------------------------

#[test]
fn oversized_batches_are_refused_before_any_edit_runs() {
    let f = files(BASE);
    let edits: Vec<Value> = (0..intent::MAX_EDITS + 1).map(|i| json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": format!("p{i}")})).collect();
    let t = std::time::Instant::now();
    let (i, e) = intent::apply_batch(&f, &edits).unwrap_err();
    assert_eq!(i, 0);
    assert!(e.message.contains(&format!("between 1 and {}", intent::MAX_EDITS)), "{}", e.message);
    let mut feats = json!({"kind": "part", "name": "leaf"});
    for _ in 0..30 {
        feats = json!({"kind": "part", "name": "n", "features": [feats]});
    }
    let deep = vec![json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "D", "features": [feats]})];
    assert!(intent::apply_batch(&f, &deep).unwrap_err().1.message.contains("features[]"));
    let long: Vec<String> = (0..100).map(|i| format!("Demo::B{i}")).collect();
    let lists = vec![json!({"op": "create_element", "owner": "Demo", "kind": "part def", "name": "D", "specializes": long})];
    assert!(intent::apply_batch(&f, &lists).unwrap_err().1.message.contains("at most 50"));
    assert!(agent::locate_ref(&model(&f), &"a".repeat(5000)).is_err());
    assert!(t.elapsed().as_secs_f64() < 2.0, "took {:?}", t.elapsed());
    // The refusals above are cheap; this one is a real apply, so it sits outside the timed part.
    // One edit has no size limit of its own: a big ICD package is one edit (no arbitrary caps).
    let body: String = (0..20_000).map(|i| format!("    part def Part{i};\n")).collect();
    let huge = vec![json!({"op": "add_element", "owner": "Demo", "text": format!("package Huge {{\n{body}}}")})];
    assert!(huge[0].to_string().len() > 256 << 10);
    assert!(intent::apply_batch(&f, &huge).is_ok());
}

#[test]
fn a_plan_with_thousands_of_diagnostics_stays_fast() {
    // 20000 new errors: matching each against every other one is the quadratic case.
    let text = "}\n".repeat(20_000);
    let before = files("package P { }\n");
    let t = std::time::Instant::now();
    let p = plan::plan(&before, vec![plan::Proposed { path: "demo.sysml".into(), text: Some(text) }]);
    assert!(p.errors_after >= 20_000);
    assert!(p.new_errors.len() <= 200 && p.touched_diagnostics.len() <= 50);
    assert!(p.errors_after > p.errors_before && !p.parse_faults.is_empty() && p.parse_faults.len() <= 50);
    assert!(t.elapsed().as_secs_f64() < 5.0, "plan took {:?}", t.elapsed());
}
