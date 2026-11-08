//! One element, one representation (agent/duplicates.rs): every write path refuses a second representation of an element
//! the model has, in one kind of view (a second IBD of the same structure) and across kinds (the activity, the IBD, the
//! BDD, the requirements), and the semantic gate finds the ones a model already holds.

use serde_json::{json, Value};
use sysml_core::agent::{intent, rules};
use sysml_core::api::FileIn;
use sysml_core::model::Model;

fn run(f: &mut Vec<FileIn>, op: Value) {
    let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("{op}: {}", e.message));
    *f = a.files;
}

/// The rule a write was refused by, or None when it was written.
fn refused_by(f: &[FileIn], op: Value) -> Option<String> {
    match intent::apply_op(f, &op) {
        Ok(_) => None,
        Err(e) => Some(e.data["rule"].as_str().unwrap_or(&e.message).to_string()),
    }
}

fn semantic(m: &Model) -> Vec<String> {
    rules::run_gates(m, &["semantic"], None, None, &[]).findings.iter().map(|f| f["rule"].as_str().unwrap().to_string()).collect()
}

/// A satellite built through the tools: the payload bus is a part of the satellite (BDD, IBD), performs the route step
/// of the mission (activity) and satisfies a requirement (requirements view).
fn satellite() -> Vec<FileIn> {
    let mut f = vec![FileIn { path: "sat.sysml".into(), text: "package Sat {\n}\n".into() }];
    for (k, n) in [("part def", "PayloadBus"), ("part def", "Camera"), ("part def", "Satellite"), ("item def", "Image"), ("action def", "Mission"), ("action def", "Route"), ("package", "Ground")] {
        run(&mut f, json!({"op": "create_element", "owner": "Sat", "kind": k, "name": n}));
    }
    run(&mut f, json!({"op": "create_element", "owner": "Sat::PayloadBus", "kind": "attribute", "name": "rate"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::PayloadBus", "kind": "attribute", "name": "width"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Satellite", "kind": "part", "name": "camera", "typed_by": "Sat::Camera"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Satellite", "kind": "part", "name": "payloadBus", "typed_by": "Sat::PayloadBus"}));
    run(&mut f, json!({"op": "add_interface", "context": "Sat::Satellite", "from": "Sat::Satellite::camera", "to": "Sat::Satellite::payloadBus", "item": "Sat::Image"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Mission", "kind": "action", "name": "capture"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Mission", "kind": "action", "name": "route", "typed_by": "Sat::Route"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "perform", "source": "Sat::Satellite::payloadBus", "target": "Sat::Mission::route"}));
    run(&mut f, json!({"op": "create_element", "owner": "Sat", "kind": "requirement", "name": "routeImages", "doc": "The payload bus shall route every image to the downlink."}));
    run(&mut f, json!({"op": "create_relationship", "kind": "satisfy", "source": "Sat::Satellite::payloadBus", "target": "Sat::routeImages"}));
    f
}

#[test]
fn the_satellite_has_no_duplicate() {
    let f = satellite();
    let m = intent::model_of(&f);
    let found: Vec<String> = semantic(&m).into_iter().filter(|r| r.starts_with("duplicate") || r.starts_with("copied")).collect();
    assert!(found.is_empty(), "{found:?}\n{}", f[0].text);
}

/// IBD to IBD: a second internal block diagram of the satellite, or of another assembly, must reuse the payload bus.
#[test]
fn a_second_ibd_reuses_the_payload_bus() {
    let f = satellite();
    // The same concept defined again elsewhere, under the same name or another spelling of it.
    for name in ["PayloadBus", "Payload_Bus", "payloadbus"] {
        assert_eq!(refused_by(&f, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": name})).as_deref(), Some("duplicate-definition"), "{name}");
    }
    // The same part again in the satellite under another spelling.
    assert_eq!(refused_by(&f, json!({"op": "create_element", "owner": "Sat::Satellite", "kind": "part", "name": "payload_bus", "typed_by": "Sat::PayloadBus"})).as_deref(), Some("duplicate-usage"));
    // A copy of the definition under another name: made empty, then given the same members; the edit that completes the
    // copy is refused.
    let mut g = f.clone();
    run(&mut g, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "Harness"}));
    run(&mut g, json!({"op": "create_element", "owner": "Sat::Ground::Harness", "kind": "attribute", "name": "rate"}));
    run(&mut g, json!({"op": "create_element", "owner": "Sat::Ground::Harness", "kind": "attribute", "name": "width"}));
    let copy = json!({"op": "create_element", "owner": "Sat::Ground::Harness", "kind": "port", "name": "imageIn", "typed_by": "~Sat::ImagePort"});
    assert_eq!(refused_by(&g, copy).as_deref(), Some("copied-definition"), "{}", g[0].text);
    // A rename into the payload bus's name.
    assert_eq!(refused_by(&f, json!({"op": "update_element", "ref": "Sat::Camera", "rename": "PAYLOADBUS"})).as_deref(), Some("duplicate-definition"));
    // What a second IBD may do: show the payload bus as it is, in another assembly typed by the one definition.
    let mut h = f.clone();
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "TestRig"}));
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground::TestRig", "kind": "part", "name": "payloadBus", "typed_by": "Sat::PayloadBus"}));
    // A definition that specializes the payload bus is a kind of it, not a copy.
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "PayloadBusMk2"}));
    run(&mut h, json!({"op": "add_specialization", "target": "Sat::Ground::PayloadBusMk2", "general": "Sat::PayloadBus"}));
}

/// A curated IBD: a block copied with fewer ports so one diagram leaves the others out. The diagram draws the one block
/// and hides the ports instead.
#[test]
fn a_curated_ibd_hides_ports_instead_of_copying_the_block() {
    let mut f = satellite();
    for n in ["PowerPort", "DataPort"] {
        run(&mut f, json!({"op": "create_element", "owner": "Sat", "kind": "port def", "name": n}));
    }
    for (n, t) in [("power", "Sat::PowerPort"), ("data", "Sat::DataPort")] {
        run(&mut f, json!({"op": "create_element", "owner": "Sat::PayloadBus", "kind": "port", "name": n, "typed_by": t}));
    }
    // The functional view's copy: the image and power ports only (not data), made one port at a time. A name that says
    // it is a view is refused at the first port; another at the second.
    for (name, view) in [("PayloadBusFunctional", true), ("PayloadBus_IBD", true), ("FunctionalPayloadBusView", true), ("PayloadBusPower", false)] {
        let mut g = f.clone();
        run(&mut g, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": name}));
        let q = format!("Sat::Ground::{name}");
        let first = json!({"op": "create_element", "owner": q, "kind": "port", "name": "imageIn", "typed_by": "~Sat::ImagePort"});
        if view {
            assert_eq!(refused_by(&g, first).as_deref(), Some("view-copy"), "{name}");
            continue;
        }
        run(&mut g, first);
        assert_eq!(refused_by(&g, json!({"op": "create_element", "owner": q, "kind": "port", "name": "power", "typed_by": "Sat::PowerPort"})).as_deref(), Some("view-copy"), "{name}");
        // As raw text in one go, too.
        let raw = format!("package Spare {{ part def {name} {{ port imageIn : ~Sat::ImagePort; port power : Sat::PowerPort; }} }}\n");
        let mut w = f.clone();
        w.push(FileIn { path: "spare.sysml".into(), text: raw });
        let e = sysml_core::agent::duplicates::refuse_new(&intent::model_of(&f), &intent::model_of(&w)).expect_err(name);
        assert_eq!(e.data["rule"], "view-copy", "{name}: {}", e.message);
        assert!(semantic(&intent::model_of(&w)).contains(&"view-copy".to_string()), "{name}");
    }
    // A name with words of its own is another thing: a data bus with two of the payload bus's ports.
    let mut d = f.clone();
    run(&mut d, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "DataBus"}));
    run(&mut d, json!({"op": "create_element", "owner": "Sat::Ground::DataBus", "kind": "port", "name": "imageIn", "typed_by": "~Sat::ImagePort"}));
    run(&mut d, json!({"op": "create_element", "owner": "Sat::Ground::DataBus", "kind": "port", "name": "power", "typed_by": "Sat::PowerPort"}));
    // Records of attributes alone are not a diagram's copy: two verification records of one test, one per requirement
    // (t9ak: `VerTp01Pwr` beside `VerTp01`).
    d[0].text = sysml_core::agent::mint::strip_ids(&d[0].text).replacen("package Ground {", "package Ground {\n        attribute def Text;\n        verification def VerTp01 { attribute method : Text = \"Test\"; attribute procedure : Text = \"TP-01\"; attribute status : Text = \"open\"; }\n        verification def VerTp01Pwr { attribute method : Text = \"Inspection\"; attribute procedure : Text = \"TP-01\"; attribute status : Text = \"open\"; }\n        verification def VerTp02 { attribute method : Text = \"Test\"; attribute procedure : Text = \"TP-02\"; attribute status : Text = \"open\"; }", 1);
    assert!(d[0].text.contains("VerTp01Pwr"), "{}", d[0].text);
    let found = semantic(&intent::model_of(&d));
    assert!(!found.contains(&"view-copy".to_string()) && !found.contains(&"copied-definition".to_string()), "{found:?}");
    // Nor do records of one shape with other values look alike (VerTp01, VerTp02); a name one word apart still asks.
    let m = intent::model_of(&d);
    let similar: Vec<String> = rules::run_gates(&m, &["semantic"], None, None, &[]).findings.iter().filter(|f| f["rule"] == "similar-element").map(|f| f["message"].as_str().unwrap().to_string()).collect();
    assert!(similar.iter().all(|s| !s.contains("VerTp02")), "{similar:?}");
    // Another part that happens to have two of the same ports is not a copy.
    let mut h = f.clone();
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "Modem"}));
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground::Modem", "kind": "port", "name": "power", "typed_by": "Sat::PowerPort"}));
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground::Modem", "kind": "port", "name": "data", "typed_by": "Sat::DataPort"}));
    // Nor is a kind of the payload bus.
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "PayloadBusLite"}));
    run(&mut h, json!({"op": "add_specialization", "target": "Sat::Ground::PayloadBusLite", "general": "Sat::PayloadBus"}));
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground::PayloadBusLite", "kind": "port", "name": "imageIn", "typed_by": "~Sat::ImagePort"}));
    run(&mut h, json!({"op": "create_element", "owner": "Sat::Ground::PayloadBusLite", "kind": "port", "name": "power", "typed_by": "Sat::PowerPort"}));
    assert!(!semantic(&intent::model_of(&h)).contains(&"view-copy".to_string()));
}

/// Across kinds: the activity, the IBD, the BDD and the requirements view must show the one payload bus, the one route
/// step and the one requirement.
#[test]
fn the_activity_and_the_requirements_reuse_their_elements() {
    let f = satellite();
    // A second usage of the route function made only to say the payload bus performs it.
    assert_eq!(refused_by(&f, json!({"op": "create_element", "owner": "Sat::PayloadBus", "kind": "perform action", "name": "route", "typed_by": "Sat::Route"})).as_deref(), Some("copied-usage"));
    // The requirement written again with the same text.
    assert_eq!(refused_by(&f, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "requirement", "name": "routing", "doc": "The payload bus shall route every image to the downlink."})).as_deref(), Some("duplicate-requirement"));
}

/// A batch takes the same guard: a duplicate anywhere in it refuses the whole batch.
#[test]
fn a_batch_with_a_duplicate_is_refused() {
    let f = satellite();
    let edits = vec![
        json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "Antenna"}),
        json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "Payload Bus"}),
    ];
    match intent::apply_batch(&f, &edits) {
        Ok(b) => panic!("written:\n{}", b.files[0].text),
        Err((i, e)) => assert_eq!((i, e.data["rule"].as_str()), (1, Some("duplicate-definition")), "{}", e.message),
    }
}

/// A duplicate already in the model is reported, not refused, so the model can still be edited and the duplicate fixed.
#[test]
fn a_model_that_has_a_duplicate_can_still_be_edited_and_fixed() {
    let mut f = satellite();
    f[0].text = f[0].text.replacen("package Sat {", "package Sat {\n    package Old { part def PayloadBus; }", 1);
    let m = intent::model_of(&f);
    assert!(semantic(&m).contains(&"duplicate-definition".to_string()));
    run(&mut f, json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "Antenna"}));
    run(&mut f, json!({"op": "delete_element", "ref": "Sat::Old::PayloadBus"}));
    assert!(!semantic(&intent::model_of(&f)).contains(&"duplicate-definition".to_string()));
}

/// A client's edits (`apply_edit`, text into an owner) take the same guard.
#[test]
fn an_applied_edit_cannot_add_a_second_payload_bus() {
    use sysml_core::edit::EditOp;
    let f = satellite();
    let add = |owner: &str, text: &str| sysml_core::api::apply_edit(f.clone(), &EditOp::AddElement { owner: Some(owner.into()), file: None, text: text.into() });
    for (owner, text) in [("Sat::Ground", "part def Payload_Bus;"), ("Sat::Satellite", "part payload_bus : Sat::PayloadBus;"), ("Sat::PayloadBus", "perform action route : Sat::Route;")] {
        let e = add(owner, text).expect_err(text);
        assert!(e.starts_with("refused:"), "{text}: {e}");
    }
    add("Sat::Ground", "part def Antenna;").expect("a new concept is written");
}

/// One intent under another name or owner: written (it may be two things), and the reply asks whether to use the
/// existing element; the gate lists it until it is merged or told apart.
#[test]
fn a_look_alike_is_answered_with_the_element_to_reuse() {
    let f = satellite();
    let a = intent::apply_op(&f, &json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "PayloadDataBus"})).expect("written");
    let ll = &a.info["looks_like"][0];
    assert_eq!(ll["looks_like"][0], "Sat::PayloadBus", "{}", a.info);
    assert!(ll["question"].as_str().unwrap().ends_with("use the existing element?"));
    assert!(semantic(&intent::model_of(&a.files)).contains(&"similar-element".to_string()));
    // Two requirements that say one thing in other words.
    let b = intent::apply_op(&f, &json!({"op": "create_element", "owner": "Sat::Ground", "kind": "requirement", "name": "imageRouting", "doc": "Every image shall be routed by the payload bus to the downlink."})).expect("written");
    assert_eq!(b.info["looks_like"][0]["looks_like"][0], "Sat::routeImages", "{}", b.info);
    // Something new is not asked about.
    let c = intent::apply_op(&f, &json!({"op": "create_element", "owner": "Sat::Ground", "kind": "part def", "name": "Antenna"})).expect("written");
    assert!(c.info.get("looks_like").is_none());
}

/// Records that say they are two are not asked about (the trial exports, 2026-10-21): a system requirement and the
/// subsystem requirement derived from it declare other ids; a signal and its forwarded copy on another link carry other
/// members. One body under two names still asks.
#[test]
fn records_told_apart_are_not_look_alikes() {
    let text = "package K {\n    attribute def Real;\n    requirement def <'PWR-02'> BatteryMonitor { doc /* The battery monitor shall report the state of charge every second. */ }\n    requirement def <'CPS-BMS-01'> BatteryMonitorReport { doc /* The battery monitor shall report the state of charge every second to the computer. */ }\n    item def MISSION_PLAN { attribute waypoints : Real; attribute uhfFrame : Real; }\n    item def MISSION_PLAN_FWD { attribute waypoints : Real; attribute ethernetFrame : Real; }\n    item def REMOTE_ESTOP { attribute value : Real { doc /* kind: UHF data link */ } }\n    item def REMOTE_ESTOP_FWD { attribute value : Real { doc /* kind: Ethernet */ } }\n    part def <'mission-computer'> MissionComputer { port power : Real; port data : Real; }\n    part def <'autonomy-mission-computer'> AutonomyMissionComputer { port power : Real; port data : Real; }\n    item def ESTOP_ACTIVE { attribute latched : Real; attribute source : Real; }\n    item def ESTOP_ACTIVE_2 { attribute latched : Real; attribute source : Real; }\n}\n";
    let f = vec![FileIn { path: "k.sysml".into(), text: text.into() }];
    let m = intent::model_of(&f);
    let similar: Vec<String> = rules::run_gates(&m, &["semantic"], None, None, &[]).findings.iter().filter(|f| f["rule"] == "similar-element").map(|f| f["message"].as_str().unwrap().to_string()).collect();
    assert!(similar.iter().all(|s| !s.contains("BatteryMonitor") && !s.contains("MISSION_PLAN") && !s.contains("REMOTE_ESTOP")), "{similar:?}");
    assert!(similar.iter().any(|s| s.contains("ESTOP_ACTIVE")), "{similar:?}");
    // Other ids tell two components apart too, unless one name only adds a domain (the_trial_export_verdicts_hold).
    assert!(similar.iter().all(|s| !s.contains("MissionComputer")), "{similar:?}");
}

/// Pairs judged by hand on trial exports, in the shapes an exporter writes: each real one asks, each false one does not.
#[test]
fn the_trial_export_verdicts_hold() {
    let files = vec![
        FileIn { path: "autonomy.sysml".into(), text: "package Autonomy {\n    package Requirements {\n        requirement def <'req-aut-02'> ReqAut02 { doc /* The mission computer shall replan the route within 2 s of a new mission plan. */ }\n        requirement def <'req-cps-msc-01'> ReqCpsMsc01 { doc /* The mission computer shall replan the route within 2 s of receiving a mission plan. */ }\n    }\n    package Structure {\n        part def <'autonomy-mission-computer'> AutonomyMissionComputer { doc /* Runs autonomy and route planning. */ port plan : ~Communications::Interfaces::InterfaceMissionPlanFwdPort; port power : Power::Interfaces::PowerPort; }\n    }\n}\n".into() },
        FileIn { path: "system.sysml".into(), text: "package System {\n    package Structure {\n        part def <'mission-computer'> MissionComputer { doc /* External endpoint of the safety chain. */ port estop : Safety::Interfaces::InterfaceEstopActiveSafetyPort; }\n        part def <'estop-chain-controller'> EStopChainController { doc /* External endpoint. */ port estop : Safety::Interfaces::InterfaceEstopActiveSafetyPort; }\n    }\n}\n".into() },
        FileIn { path: "safety.sysml".into(), text: "package Safety {\n    package Interfaces {\n        item def <'estop-active-2'> ESTOP_ACTIVE_2 { attribute value : Power::Interfaces::Real { doc /* kind: hardwired loop */ } }\n        item def <'remote-estop-fwd'> REMOTE_ESTOP_FWD { attribute value : Power::Interfaces::Real { doc /* kind: Ethernet */ } }\n        port def <'interface-estop-active-safety-port'> InterfaceEstopActiveSafetyPort { in item estop : ESTOP_ACTIVE_2; }\n        interface def <'interface-estop-active-safety'> InterfaceEstopActiveSafety { end a : InterfaceEstopActiveSafetyPort; end b : ~InterfaceEstopActiveSafetyPort; }\n        port def <'interface-remote-estop-forward-port'> InterfaceRemoteEstopForwardPort { out item estop : REMOTE_ESTOP_FWD; }\n        interface def <'interface-remote-estop-forward'> InterfaceRemoteEstopForward { end a : InterfaceRemoteEstopForwardPort; end b : ~InterfaceRemoteEstopForwardPort; }\n    }\n    package Structure {\n        part def <'safety-e-stop-chain-controller'> SafetyEStopChainController { doc /* Opens the motor contactor when any e-stop input opens. */ port estop : Propulsion::Interfaces::InterfaceEstopActivePort; port remote : ~InterfaceRemoteEstopForwardPort; }\n    }\n}\n".into() },
        FileIn { path: "propulsion.sysml".into(), text: "package Propulsion {\n    package Interfaces {\n        item def <'estop-active'> ESTOP_ACTIVE { attribute value : Power::Interfaces::Real { doc /* kind: hardwired loop */ } }\n        port def <'interface-estop-active-port'> InterfaceEstopActivePort { out item estop : ESTOP_ACTIVE; }\n        interface def <'interface-estop-active'> InterfaceEstopActive { end a : InterfaceEstopActivePort; end b : ~InterfaceEstopActivePort; }\n    }\n    package Requirements {\n        requirement def <'req-pro-03'> ReqPro03 { doc /* The power control unit shall limit the thrust command to the rated value. */ }\n        requirement def <'req-cps-pcu-01'> ReqCpsPcu01 { doc /* The power control unit shall limit each thrust command to the rated value. */ }\n    }\n}\n".into() },
        FileIn { path: "communications.sysml".into(), text: "package Communications {\n    package Interfaces {\n        item def <'mission-plan'> MISSION_PLAN { attribute value : Power::Interfaces::Real { doc /* kind: UHF data link */ } }\n        item def <'mission-plan-fwd'> MISSION_PLAN_FWD { attribute value : Power::Interfaces::Real { doc /* kind: Ethernet */ } }\n        item def <'remote-estop'> REMOTE_ESTOP { attribute value : Power::Interfaces::Real { doc /* kind: UHF data link */ } }\n        port def <'interface-mission-plan-port'> InterfaceMissionPlanPort { out item plan : MISSION_PLAN; }\n        port def <'interface-mission-plan-fwd-port'> InterfaceMissionPlanFwdPort { out item plan : MISSION_PLAN_FWD; }\n        interface def <'interface-mission-plan'> InterfaceMissionPlan { end a : InterfaceMissionPlanPort; end b : ~InterfaceMissionPlanPort; }\n        interface def <'interface-mission-plan-fwd'> InterfaceMissionPlanFwd { end a : InterfaceMissionPlanFwdPort; end b : ~InterfaceMissionPlanFwdPort; }\n        port def <'interface-remote-estop-port'> InterfaceRemoteEstopPort { out item estop : REMOTE_ESTOP; }\n        interface def <'interface-remote-estop'> InterfaceRemoteEstop { end a : InterfaceRemoteEstopPort; end b : ~InterfaceRemoteEstopPort; }\n    }\n    package Requirements {\n        requirement def <'req-com-01'> ReqCom01 { doc /* The radio shall carry telemetry to the ground station at 1 Hz or faster. */ }\n        requirement def <'req-cps-rdo-01'> ReqCpsRdo01 { doc /* The radio shall carry the telemetry to the ground station at 1 Hz or faster. */ }\n    }\n}\n".into() },
        FileIn { path: "power.sysml".into(), text: "package Power {\n    package Interfaces {\n        attribute def Real;\n        port def PowerPort;\n    }\n}\n".into() },
    ];
    let m = intent::model_of(&files);
    let findings = rules::run_gates(&m, &["semantic"], None, None, &[]).findings;
    let pairs: Vec<(String, String)> = findings
        .iter()
        .filter(|f| f["rule"] == "similar-element")
        .map(|f| (f["element"].as_str().or(f["ref"].as_str()).unwrap_or("").to_string(), f["message"].as_str().unwrap_or("").to_string()))
        .collect();
    let asks = |a: &str, b: &str| pairs.iter().any(|(e, msg)| (e.ends_with(a) || msg.contains(&format!("'{a}'")) || msg.contains(&format!("::{a}'"))) && (msg.contains(&format!("::{b}'")) || e.ends_with(b) && msg.contains(a)));
    for (a, b) in [("AutonomyMissionComputer", "MissionComputer"), ("SafetyEStopChainController", "EStopChainController"), ("ESTOP_ACTIVE_2", "ESTOP_ACTIVE"), ("InterfaceEstopActiveSafety", "InterfaceEstopActive"), ("InterfaceEstopActiveSafetyPort", "InterfaceEstopActivePort")] {
        assert!(asks(a, b) || asks(b, a), "real pair not asked: {a} ~ {b}\n{pairs:#?}");
    }
    for (a, b) in [("ReqCpsMsc01", "ReqAut02"), ("ReqCpsPcu01", "ReqPro03"), ("ReqCpsRdo01", "ReqCom01"), ("MISSION_PLAN_FWD", "MISSION_PLAN"), ("REMOTE_ESTOP_FWD", "REMOTE_ESTOP"), ("InterfaceMissionPlanFwd", "InterfaceMissionPlan"), ("InterfaceMissionPlanFwdPort", "InterfaceMissionPlanPort"), ("InterfaceRemoteEstopForward", "InterfaceRemoteEstop"), ("InterfaceRemoteEstopForwardPort", "InterfaceRemoteEstopPort")] {
        assert!(!asks(a, b) && !asks(b, a), "false pair asked: {a} ~ {b}\n{pairs:#?}");
    }
}


/// A copy already in the model (made before view-copy was refused) folds back into its original in one edit: the usages
/// it typed are typed by the original, connections and all; it is deleted; and the reply says which ports to hide where
/// it was drawn.
#[test]
fn a_view_copy_already_in_the_model_folds_back_into_its_original() {
    let mut f = satellite();
    for n in ["PowerPort", "DataPort"] {
        run(&mut f, json!({"op": "create_element", "owner": "Sat", "kind": "port def", "name": n}));
    }
    for (n, t) in [("power", "Sat::PowerPort"), ("data", "Sat::DataPort")] {
        run(&mut f, json!({"op": "create_element", "owner": "Sat::PayloadBus", "kind": "port", "name": n, "typed_by": t}));
    }
    // The work agent's copy, written as text (the guards would refuse it now), and a functional context that uses it.
    let bus = intent::model_of(&f).find("Sat::PayloadBus").unwrap().children.len();
    let mut g = f.clone();
    g.push(FileIn {
        path: "view.sysml".into(),
        text: "package View {\n    part def PayloadBusFunctional {\n        attribute rate;\n        attribute width;\n        port imageIn : ~Sat::ImagePort;\n        port power : Sat::PowerPort;\n    }\n    part def FunctionalContext {\n        part bus : PayloadBusFunctional;\n        part camera : Sat::Camera;\n        connect camera.imageOut to bus.imageIn;\n    }\n}\n".into(),
    });
    assert!(semantic(&intent::model_of(&g)).contains(&"view-copy".to_string()));
    // Refused while the copy holds something the original lacks, and nothing is written.
    let mut extra = g.clone();
    extra[1].text = extra[1].text.replace("port power : Sat::PowerPort;", "port power : Sat::PowerPort;\n        attribute mass;");
    let e = intent::apply_op(&extra, &json!({"op": "fold_copy", "copy": "View::PayloadBusFunctional", "into": "Sat::PayloadBus"})).map(|_| ()).expect_err("own member");
    assert!(e.message.contains("attribute mass"), "{}", e.message);
    // Refused while something other than a typed usage names it.
    let mut named = g.clone();
    named[1].text = named[1].text.replace("    part def FunctionalContext {", "    part def Special :> PayloadBusFunctional;\n    part def FunctionalContext {");
    let e = intent::apply_op(&named, &json!({"op": "fold_copy", "copy": "View::PayloadBusFunctional", "into": "Sat::PayloadBus"})).map(|_| ()).expect_err("specialized");
    assert!(e.message.contains("View::Special"), "{}", e.message);
    // Folded.
    let a = intent::apply_op(&g, &json!({"op": "fold_copy", "copy": "View::PayloadBusFunctional", "into": "Sat::PayloadBus"})).unwrap_or_else(|e| panic!("{}", e.message));
    let m = intent::model_of(&a.files);
    assert!(m.find("View::PayloadBusFunctional").is_none(), "{}", a.files[1].text);
    let usage = m.find("View::FunctionalContext::bus").unwrap();
    assert_eq!(usage.refs_of(sysml_core::model::RefRole::Type).filter_map(|r| r.resolved()).map(|d| m.elements[d].qualified_name.clone()).collect::<Vec<_>>(), vec!["Sat::PayloadBus".to_string()]);
    assert_eq!(m.find("Sat::PayloadBus").unwrap().children.len(), bus, "the original is unchanged");
    assert_eq!(a.info["retyped"], json!(["View::FunctionalContext::bus"]));
    assert_eq!(a.info["hidden_ports"], json!(["Sat::PayloadBus::data"]));
    let found = semantic(&m);
    assert!(!found.iter().any(|r| r == "view-copy" || r.starts_with("copied") || r.starts_with("duplicate")), "{found:?}");
    assert!(a.files[1].text.contains("connect camera.imageOut to bus.imageIn"), "{}", a.files[1].text);
}
