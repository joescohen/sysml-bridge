//! The ownership rules: every element has one owner its kind allows, every write path
//! refuses an element under an owner that cannot own it, and an element drawn in the structure, interconnection,
//! action and requirements views is one element of the containment tree, the same one in each.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use sysml_core::agent::{intent, rules};
use sysml_core::api::{self, FileIn};
use sysml_core::model::Model;

fn run(f: &mut Vec<FileIn>, op: Value) -> Value {
    let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("{op}: {}", e.message));
    *f = a.files;
    a.info
}

fn refused(f: &[FileIn], op: Value) -> (String, Value) {
    match intent::apply_op(f, &op) {
        Ok(a) => panic!("{op} was written:\n{}", a.files[0].text),
        Err(e) => (e.message, e.data),
    }
}

fn semantic(m: &Model) -> Vec<(String, String)> {
    rules::run_gates(m, &["semantic"], None, None, &[]).findings.iter().map(|f| (f["rule"].as_str().unwrap().to_string(), f["element"].as_str().unwrap_or("").to_string())).collect()
}

/// A satellite built only through the write tools: structure (parts, ports, a connection with an item flow), behavior (an
/// activity whose step the payload bus performs) and requirements (the payload bus satisfies one).
fn satellite() -> Vec<FileIn> {
    let mut f = vec![FileIn { path: "sat.sysml".into(), text: "package Sat {\n}\n".into() }];
    for (k, n) in [("part def", "PayloadBus"), ("part def", "Camera"), ("part def", "Satellite"), ("item def", "Image"), ("action def", "Mission"), ("requirement def", "RouteImages")] {
        run(&mut f, json!({"op": "create_element", "owner": "Sat", "kind": k, "name": n}));
    }
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Satellite", "kind": "part", "name": "camera", "typed_by": "Sat::Camera"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Satellite", "kind": "part", "name": "payloadBus", "typed_by": "Sat::PayloadBus"}));
    run(&mut f, json!({"op": "add_interface", "context": "Sat::Satellite", "from": "Sat::Satellite::camera", "to": "Sat::Satellite::payloadBus", "item": "Sat::Image"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Mission", "kind": "action", "name": "capture"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Mission", "kind": "action", "name": "route"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "succession", "source": "Sat::Mission::capture", "target": "Sat::Mission::route"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "perform", "source": "Sat::Satellite::payloadBus", "target": "Sat::Mission::route"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat", "kind": "requirement", "name": "routeImages", "typed_by": "Sat::RouteImages"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Sat::Satellite::payloadBus", "target": "Sat::routeImages"}));
    f
}

#[test]
fn the_tools_place_every_element_under_its_usual_owner() {
    let m = api::to_model(satellite());
    assert!(semantic(&m).iter().all(|(r, _)| !r.starts_with("owner-")), "{:?}", semantic(&m));
    let owner = |q: &str| m.find(q).and_then(|e| e.owner).map(|o| m.elements[o].qualified_name.clone()).unwrap_or_default();
    // Ports under their blocks (Class::ownedPort), the connector and its item flow under the IBD's block, the control
    // flow under its activity, the perform under the performing part, the satisfy in its client's package.
    assert_eq!(owner("Sat::PayloadBus::imageIn"), "Sat::PayloadBus");
    assert_eq!(owner("Sat::Camera::imageOut"), "Sat::Camera");
    assert_eq!(owner("Sat::Satellite::camera_to_payloadBus"), "Sat::Satellite");
    let kinds_under = |q: &str| -> BTreeSet<String> { m.find(q).unwrap().children.iter().map(|c| m.elements[*c].keyword()).collect() };
    assert!(kinds_under("Sat::Satellite").contains("flow") && !kinds_under("Sat::Satellite").contains("satisfy"));
    assert!(kinds_under("Sat").contains("satisfy"));
    assert!(kinds_under("Sat::Mission").contains("succession"));
    assert!(kinds_under("Sat::Satellite::payloadBus").contains("perform"));
}

#[test]
fn every_write_path_refuses_a_misplaced_element() {
    let f = satellite();
    // create_element: a port in a package.
    let (msg, data) = refused(&f, json!({"op": "create_element", "owner": "Sat", "kind": "port", "name": "stray", "typed_by": "Sat::ImagePort"}));
    assert_eq!(data["rule"], "owner-port", "{msg}");
    assert!(msg.contains("Nothing was written") && msg.contains("block"), "{msg}");
    // Raw text: a connector in the package that reaches its ends through a block.
    let (_, data) = refused(&f, json!({"op": "add_element", "owner": "Sat", "text": "connect Sat::Satellite::camera.imageOut to Sat::Satellite::payloadBus.imageIn;"}));
    assert_eq!(data["rule"], "owner-connector");
    // Raw text: a control flow in a package; a transition in an action def.
    let (_, data) = refused(&f, json!({"op": "add_element", "owner": "Sat", "text": "succession first Sat::Mission::capture then Sat::Mission::route;"}));
    assert_eq!(data["rule"], "owner-behavior-step");
    let (_, data) = refused(&f, json!({"op": "add_element", "owner": "Sat::Mission", "text": "transition first capture then route;"}));
    assert_eq!(data["rule"], "owner-transition");
    // A move: the port out of its block into the package.
    let (msg, data) = refused(&f, json!({"op": "update_element", "ref": "Sat::PayloadBus::imageIn", "owner": "Sat"}));
    assert_eq!(data["rule"], "owner-port", "{msg}");
    // replace: rewriting the block so its port lands outside it.
    let (_, data) = refused(&f, json!({"op": "add_element", "owner": "Sat", "text": "in item loose : Sat::Image;"}));
    assert_eq!(data["rule"], "owner-directed-feature");
    let (_, data) = refused(&f, json!({"op": "add_element", "owner": "Sat", "text": "perform Sat::Mission::route;"}));
    assert_eq!(data["rule"], "owner-perform");
    // A batch refuses the same, and writes nothing.
    let r = intent::apply_batch(&f, &[json!({"op": "create_element", "owner": "Sat", "kind": "part def", "name": "Ok"}), json!({"op": "create_element", "owner": "Sat", "kind": "port", "name": "stray"})]);
    let (i, e) = r.err().expect("the batch must be refused");
    assert_eq!((i, e.data["rule"].as_str()), (1, Some("owner-port")), "{}", e.message);
    // The same port in a block is written.
    let mut g = f.clone();
    run(&mut g, json!({"op": "create_element", "owner": "Sat::Camera", "kind": "port", "name": "power"}));
}

#[test]
fn a_misplacement_the_model_already_has_does_not_block_other_writes() {
    let mut f = vec![FileIn { path: "m.sysml".into(), text: "package P {\n    port def D;\n    port stray : D;\n}\n".into() }];
    assert!(semantic(&api::to_model(f.clone())).iter().any(|(r, _)| r == "owner-port"));
    run(&mut f, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Block"}));
    // Moving the stray port into the block fixes it.
    run(&mut f, json!({"op": "update_element", "ref": "P::stray", "owner": "P::Block"}));
    assert!(semantic(&api::to_model(f.clone())).iter().all(|(r, _)| !r.starts_with("owner-")), "{}", f[0].text);
}

#[test]
fn redefinition_and_conjugation_rules() {
    let rules_of = |t: &str| semantic(&api::to_model(vec![FileIn { path: "m.sysml".into(), text: t.into() }])).into_iter().map(|(r, _)| r).collect::<Vec<_>>();
    // Inherited through specialization or typing: fine.
    assert!(!rules_of("package P { part def A { attribute m; } part def B :> A { attribute :>> m; } }").contains(&"redefines-not-inherited".into()));
    assert!(!rules_of("package P { part def A { attribute m; } part def S { part a : A { attribute :>> m = 1; } } }").contains(&"redefines-not-inherited".into()));
    // Not inherited: a feature of an unrelated block.
    assert!(rules_of("package P { part def A { attribute m; } part def B { attribute m2 :>> A::m; } }").contains(&"redefines-not-inherited".into()));
    // ~ belongs on a port typed by a port def.
    assert!(!rules_of("package P { port def D; part def B { port p : ~D; } }").contains(&"conjugated-type-not-port".into()));
    assert!(rules_of("package P { part def A; part def B { part x : ~A; } }").contains(&"conjugated-type-not-port".into()));
    assert!(rules_of("package P { part def A; part def B { port x : ~A; } }").contains(&"conjugated-type-not-port".into()));
}

/// What the Cameo 2026x box check found: links go in the client's nearest
/// package, deleting a block keeps the parts typed by it, behaviors own no ports, and parts own no requirements.
#[test]
fn the_engine_follows_the_ownership_rules() {
    let mut f = satellite();
    // An allocate and a dependency from a nested part land in the package, and their ends resolve from there.
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "allocate", "source": "Sat::Mission::route", "target": "Sat::Satellite::payloadBus"}));
    assert_eq!(info["declared_in"], "Sat", "{info}");
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "dependency", "source": "Sat::Satellite::camera", "target": "Sat::Satellite::payloadBus"}));
    assert_eq!(info["declared_in"], "Sat", "{info}");
    // An owner given by the caller still wins.
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "dependency", "source": "Sat::Satellite::camera", "target": "Sat::Mission", "owner": "Sat::Satellite"}));
    assert_eq!(info["declared_in"], "Sat::Satellite", "{info}");
    let m = api::to_model(f.clone());
    assert!(m.diagnostics.iter().all(|d| !d.message.starts_with("cannot resolve")), "{:?}", m.diagnostics);
    // No port on an activity or state machine.
    let (_, data) = refused(&f, json!({"op": "create_element", "owner": "Sat::Mission", "kind": "port", "name": "p"}));
    assert_eq!(data["rule"], "owner-port");
    // A requirement inside a part is a warning.
    let rules_of = |t: &str| semantic(&api::to_model(vec![FileIn { path: "m.sysml".into(), text: t.into() }])).into_iter().map(|(r, _)| r).collect::<Vec<_>>();
    assert!(rules_of("package P { part def B { requirement r; } }").contains(&"owner-requirement".into()));
    assert!(!rules_of("package P { requirement r { requirement s; } }").contains(&"owner-requirement".into()));
    // Deleting a block: the part typed by it stays, untyped; the links that name the block go.
    let mut g = vec![FileIn { path: "d.sysml".into(), text: "package D {\n    part def Engine;\n    part def Car {\n        part engine : Engine;\n    }\n    requirement r;\n    satisfy r by Engine;\n}\n".into() }];
    let info = run(&mut g, json!({"op": "delete_element", "ref": "D::Engine", "cascade": true}));
    let m = api::to_model(g.clone());
    assert!(m.find("D::Engine").is_none());
    assert!(m.find("D::Car::engine").is_some(), "{}", g[0].text);
    assert!(!g[0].text.contains("satisfy"), "{}", g[0].text);
    assert_eq!(info["untyped"][0]["ref"], "D::Car::engine", "{info}");
    // A move into the element itself or onto a same-name member is refused, as Cameo refuses the drop.
    let (msg, _) = refused(&f, json!({"op": "update_element", "ref": "Sat::Satellite", "owner": "Sat::Satellite::camera"}));
    assert!(msg.contains("lives inside it"), "{msg}");
}

/// The OMG examples are legal SysML v2: none of them breaks an ownership or relationship rule.
#[test]
fn the_official_examples_break_no_ownership_rule() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sysml-conform/tests/official/examples");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("sysml") {
            continue;
        }
        let m = api::to_model(vec![FileIn { path: p.display().to_string(), text: std::fs::read_to_string(&p).unwrap() }]);
        // owner-requirement is a Cameo warning on legal v2 text: RequirementTest writes the requirement a part satisfies
        // inside the part, where Cameo would keep it in a package and link it with satisfy.
        let tool_only = |r: &str, el: &str| r == "owner-requirement" && el == "RequirementTest::q::r";
        let bad: Vec<_> = semantic(&m).into_iter().filter(|(r, el)| (r.starts_with("owner-") || r == "redefines-not-inherited" || r == "conjugated-type-not-port") && !tool_only(r, el)).collect();
        assert!(bad.is_empty(), "{}: {bad:?}", p.display());
        n += 1;
    }
    assert!(n >= 15, "{n} examples");
}
