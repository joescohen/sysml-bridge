//! Tools round 10: what depends on an element (`get_impact`), what a write changed in model terms (`change_report`),
//! deletes that say what they leave dangling, and interface ends the context can reach.

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{change, connect, impact, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

const BASE: &str = "package P {\n    port def Pwr;\n    port def Data;\n    item def Volt;\n    part def Sub {\n        port a : Pwr;\n    }\n    part def Psu {\n        port pwrout : Pwr;\n        port aux : Data;\n        action regulate;\n    }\n    part def Psu2 :> Psu;\n    part def Load {\n        port inp : ~Pwr;\n        part sub : Sub;\n    }\n    part def Sys {\n        part psu : Psu;\n        part load : Load;\n        connection feed connect psu.pwrout to load.inp;\n        flow of Volt from psu.pwrout to load.inp;\n    }\n    part sys : Sys;\n    action def Fn;\n    requirement r1;\n    requirement r2;\n    satisfy r1 by Psu;\n    allocate Fn to Psu;\n    verification def V1 {\n        objective {\n            verify r1;\n        }\n    }\n}\n";

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn apply(f: &[FileIn], op: Value) -> (Vec<FileIn>, Value) {
    let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    (a.files, a.info)
}

fn report(before: &[FileIn], after: &[FileIn]) -> Value {
    let none = BTreeMap::new();
    change::change_report(before, after, &none, &none, false)
}

fn impact_of(f: &[FileIn], name: &str, depth: usize, limit: usize) -> Value {
    impact::get_impact(&model(f), &BTreeMap::new(), name, depth, limit).unwrap_or_else(|e| panic!("get_impact {name}: {}", e.message))
}

fn refs(rows: &Value) -> Vec<String> {
    rows.as_array().unwrap().iter().map(|r| r["ref"].as_str().unwrap_or("").to_string()).collect()
}

#[test]
fn r10_impact_groups_everything_that_depends_on_a_block() {
    let f = files(BASE);
    let r = impact_of(&f, "P::Psu", 1, 100);
    let c = &r["counts"];
    for (k, n) in [("usages", 1), ("ports", 2), ("port_connections", 1), ("port_flows", 1), ("satisfies", 1), ("verifications", 1), ("allocations", 1), ("specializations", 1), ("behaviour_owned", 1), ("behaviour_allocated", 1)] {
        assert_eq!(c[k], n, "count {k}: {c}");
    }
    assert_eq!(refs(&r["usages"]), ["P::Sys::psu"]);
    // A port row carries the connection, the far end and the part that owns the far end.
    let out = r["ports"].as_array().unwrap().iter().find(|p| p["name"] == "pwrout").unwrap();
    assert_eq!(out["connected"], true);
    assert_eq!(out["connections"][0]["ref"], "P::Sys::feed");
    assert_eq!(out["connections"][0]["far_end"]["port"], "P::Load::inp");
    assert_eq!(out["connections"][0]["far_end"]["owner"], "P::Sys::load");
    assert_eq!(out["flows"][0]["of"][0], "P::Volt");
    let aux = r["ports"].as_array().unwrap().iter().find(|p| p["name"] == "aux").unwrap();
    assert_eq!(aux["connected"], false);
    assert_eq!(r["traceability"]["satisfies"][0]["target"], "P::r1");
    assert_eq!(r["traceability"]["verifications"][0]["source"], "P::V1");
    assert_eq!(r["traceability"]["allocations"][0]["source"], "P::Fn");
    assert_eq!(refs(&r["specializations"]), ["P::Psu2"]);
    assert_eq!(refs(&r["behaviour"]["owned"]), ["P::Psu::regulate"]);
    assert_eq!(refs(&r["behaviour"]["allocated"]), ["P::Fn"]);
    assert_eq!(r["truncated"], false);
    assert_eq!(r["depth"], 1);
}

#[test]
fn r10_impact_from_the_other_ends() {
    let f = files(BASE);
    let req = impact_of(&f, "P::r1", 1, 100);
    assert_eq!(req["counts"]["satisfied_by"], 1);
    assert_eq!(req["counts"]["verifications"], 1);
    assert_eq!(req["traceability"]["satisfied_by"][0]["source"], "P::Psu");
    let port = impact_of(&f, "P::Psu::pwrout", 1, 100);
    assert_eq!(port["counts"]["port_connections"], 1);
    assert_eq!(port["counts"]["port_flows"], 1);
    assert_eq!(port["definition"][0]["ref"], "P::Pwr");
    // A connection names the flows it carries.
    let conn = impact_of(&f, "P::Sys::feed", 1, 100);
    assert_eq!(conn["ends"][0]["port"], "P::Psu::pwrout");
    assert_eq!(conn["ends"][1]["owner"], "P::Sys::load");
    assert_eq!(conn["counts"]["carries"], 1);
    assert_eq!(conn["carries"][0]["of"][0], "P::Volt");
    let flow = impact_of(&f, conn["carries"][0]["ref"].as_str().unwrap(), 1, 100);
    assert_eq!(flow["carried_by"]["ref"], "P::Sys::feed");
    assert!(impact::get_impact(&model(&f), &BTreeMap::new(), "P::Nope", 1, 100).is_err());
}

#[test]
fn r10_change_report_says_which_fact_of_a_port_changed() {
    let before = files(BASE);
    let (after, _) = apply(&before, json!({"op": "replace", "target": "P::Psu::pwrout", "text": "port pwrout : Data;"}));
    let r = report(&before, &after);
    assert_eq!(r["changed"], true);
    assert_eq!(r["summary"]["ports"]["changed"], 1);
    let row = &r["ports"]["changed"][0];
    assert_eq!(row["changed"], json!(["type"]));
    assert_eq!(row["facts"]["type"]["before"], json!(["P::Pwr"]));
    assert_eq!(row["facts"]["type"]["after"], json!(["P::Data"]));
    // The connection on that port now joins two port types: a new finding of the connection check.
    let new = r["gates"]["new_errors"].as_array().unwrap();
    assert!(new.iter().any(|e| e["rule"] == "port-type-mismatch" && e["element"] == "P::Sys::feed"), "{new:?}");
    assert_eq!(r["summary"]["gates"]["connection_findings"]["before"], 0);
    assert!(change::summary_text(&r).contains("ports +0 -0 ~1"), "{}", change::summary_text(&r));
}

#[test]
fn r10_change_report_of_no_change_and_of_a_rename() {
    let before = files(BASE);
    let same = report(&before, &before);
    assert_eq!(same["changed"], false);
    // A rename is one changed element with its previous name; the statements that refer to it did not change.
    let (after, _) = apply(&before, json!({"op": "rename", "target": "P::Psu", "new_name": "Power"}));
    let r = report(&before, &after);
    assert_eq!(r["summary"]["elements"], json!({"added": 0, "changed": 1, "removed": 0}));
    let row = &r["elements"]["changed"][0];
    assert_eq!(row["previous_ref"], "P::Psu");
    assert_eq!(row["ref"], "P::Power");
    assert_eq!(row["facts"]["name"], json!({"before": "Psu", "after": "Power"}));
    assert_eq!(r["summary"]["traceability"]["removed"], 0);
    assert_eq!(r["summary"]["connections"]["re_ended"], 0);
    assert_eq!(r["summary"]["dangling"]["total"], 0);
}

#[test]
fn r10_a_rename_that_also_changes_the_element_is_paired_by_the_rename_log() {
    // Renamed and given a new port in one go: the content no longer matches, so without the log it is removed + added.
    let before = files(BASE);
    let text = BASE.replace("part def Psu {", "part def Power {\n        port extra : Data;").replace("psu : Psu", "psu : Power").replace("Psu2 :> Psu", "Psu2 :> Power").replace("by Psu;", "by Power;").replace("to Psu;", "to Power;");
    let guessed = report(&before, &files(&text));
    assert_eq!(guessed["elements"]["changed"].as_array().unwrap().iter().filter(|r| r["previous_ref"] == "P::Psu").count(), 0);
    let mut after = files(&text);
    after.push(FileIn { path: "renames.jsonl".into(), text: "{\"kind\":\"rename\",\"from\":\"P::Psu\",\"to\":\"P::Power\"}\n".into() });
    let r = report(&before, &after);
    let row = r["elements"]["changed"].as_array().unwrap().iter().find(|r| r["previous_ref"] == "P::Psu").expect("the log pairs the rename");
    assert_eq!(row["ref"], "P::Power");
    assert_eq!(row["facts"]["name"], json!({"before": "Psu", "after": "Power"}));
    assert!(r["elements"]["removed"].as_array().map(|x| x.iter().all(|e| e["ref"] != "P::Psu")).unwrap_or(true));
    // The port inside followed its owner: P::Psu::pwrout is P::Power::pwrout, not removed.
    assert!(r["elements"]["removed"].as_array().map(|x| x.iter().all(|e| e["ref"] != "P::Psu::pwrout")).unwrap_or(true));
}

#[test]
fn r10_change_report_shows_a_connection_moved_to_another_port() {
    let before = files(BASE);
    let (after, _) = apply(&before, json!({"op": "replace", "target": "P::Sys::feed", "text": "connection feed connect psu.aux to load.inp;"}));
    let r = report(&before, &after);
    assert_eq!(r["summary"]["connections"]["re_ended"], 1);
    let row = &r["connections"]["re_ended"][0];
    assert_eq!(row["ref"], "P::Sys::feed");
    assert_eq!(row["facts"]["source"]["before"]["resolves_to"], "P::Psu::pwrout");
    assert_eq!(row["facts"]["source"]["after"]["resolves_to"], "P::Psu::aux");
    // The flow still runs between the old ports, where no connection is left.
    let flow = &r["dangling"]["flows"][0];
    assert_eq!(flow["problem"], "flow-without-connection");
    assert_eq!(flow["was_on"], "P::Sys::feed");
    assert_eq!(flow["connection_removed"], false);
}

#[test]
fn r10_deleting_a_connection_reports_the_flow_it_leaves_and_cascade_removes_it() {
    let before = files(BASE);
    let (after, info) = apply(&before, json!({"op": "delete_element", "ref": "P::Sys::feed"}));
    assert!(after[0].text.contains("flow of Volt"), "the default delete keeps the flow");
    assert_eq!(info["cascade_would_remove"][0]["kind"], "flow");
    assert_eq!(info["warnings"][0]["rule"], "dangling-after-delete");
    let r = report(&before, &after);
    assert_eq!(r["summary"]["connections"]["removed"], 1);
    assert_eq!(r["summary"]["dangling"]["flows"], 1);
    let flow = &r["dangling"]["flows"][0];
    assert_eq!(flow["problem"], "flow-without-connection");
    assert_eq!(flow["connection_removed"], true);
    assert_eq!(flow["was_on"], "P::Sys::feed");
    assert!(change::summary_text(&r).contains("DANGLING"));

    let (after, info) = apply(&before, json!({"op": "delete_element", "ref": "P::Sys::feed", "cascade": true}));
    assert!(!after[0].text.contains("flow of Volt") && !after[0].text.contains("connection feed"));
    assert_eq!(info["cascaded"][0]["kind"], "flow");
    assert_eq!(info["deleted"].as_array().unwrap().len(), 2);
    let r = report(&before, &after);
    assert_eq!(r["summary"]["flows"]["removed"], 1);
    assert_eq!(r["summary"]["dangling"]["flows"], 0);
    // Both ports had one connection and now have none.
    assert_eq!(r["summary"]["dangling"]["ports_unconnected"], 2);
    assert!(model(&after).diagnostics.iter().all(|d| d.severity != sysml_core::Severity::Error));
}

#[test]
fn r10_deleting_a_referenced_block_lists_what_cascade_would_remove() {
    let before = files(BASE);
    let e = match intent::apply_op(&before, &json!({"op": "delete_element", "ref": "P::Psu"})) {
        Ok(_) => panic!("a referenced definition must not be deleted without cascade"),
        Err(e) => e,
    };
    assert!(e.message.contains("cascade: true"), "{}", e.message);
    // As in Cameo, the part typed by the block stays and loses its type; the connection to its port goes.
    assert_eq!(e.data["cascade_count"], 5);
    let kinds: Vec<&str> = e.data["cascade_would_remove"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["part def", "connection", "flow", "satisfy", "allocation"]);
    assert_eq!(e.data["cascade_would_untype"][0]["ref"], "P::Sys::psu");
    assert_eq!(e.data["referrers"].as_array().unwrap().len(), 6);

    let (after, info) = apply(&before, json!({"op": "delete_element", "ref": "P::Psu", "cascade": true}));
    assert_eq!(info["cascaded"].as_array().unwrap().len(), 5);
    assert!(model(&after).find("P::Sys::psu").is_some(), "{}", after[0].text);
    assert!(model(&after).diagnostics.iter().all(|d| d.severity != sysml_core::Severity::Error), "{}", after[0].text);
    let r = report(&before, &after);
    assert_eq!(r["summary"]["dangling"]["connections"], 0);
    assert_eq!(r["summary"]["dangling"]["flows"], 0);
    assert_eq!(r["summary"]["connections"]["removed"], 1);
    assert_eq!(r["summary"]["traceability"]["removed"], 2);
    // The requirement lost its only satisfy: coverage state changed and the gate has one more error.
    assert_eq!(r["traceability"]["coverage_changes"][0]["ref"], "P::r1");
    assert_eq!(r["traceability"]["coverage_changes"][0]["change"], "satisfied -> unsatisfied");
    assert_eq!(r["summary"]["coverage"]["before"]["satisfied"], 1);
    assert_eq!(r["summary"]["coverage"]["after"]["satisfied"], 0);
    assert!(r["gates"]["new_errors"].as_array().unwrap().iter().any(|g| g["rule"] == "requirement-not-satisfied" && g["element"] == "P::r1"));
    assert_eq!(r["dangling"]["ports_unconnected"][0]["port"], "P::Load::inp");
}

#[test]
fn r10_a_raw_delete_of_a_port_shows_the_ends_that_no_longer_resolve() {
    let before = files(BASE);
    let (after, _) = apply(&before, json!({"op": "delete", "target": "P::Psu::pwrout"}));
    let r = report(&before, &after);
    assert_eq!(r["summary"]["ports"]["removed"], 1);
    assert_eq!(r["summary"]["dangling"]["connections"], 1);
    assert_eq!(r["summary"]["dangling"]["flows"], 1);
    let c = &r["dangling"]["connections"][0];
    assert_eq!(c["problem"], "lost-end");
    assert_eq!(c["ref"], "P::Sys::feed");
    assert_eq!(c["end"], "source");
    assert_eq!(c["was"], "P::Psu::pwrout");
    assert!(r["gates"]["new_errors"].as_array().unwrap().iter().any(|g| g["rule"] == "unresolved-reference"));
}

#[test]
fn r10_the_summary_is_bounded_and_full_is_not() {
    let before = files("package Q {\n}\n");
    let mut text = String::from("package Q {\n");
    for i in 0..40 {
        text.push_str(&format!("    part def D{i};\n"));
    }
    text.push_str("}\n");
    let after = files(&text);
    let none = BTreeMap::new();
    let short = change::change_report(&before, &after, &none, &none, false);
    assert_eq!(short["summary"]["elements"]["added"], 40, "the summary counts everything");
    assert_eq!(short["elements"]["added"].as_array().unwrap().len(), change::SUMMARY_ROWS);
    assert_eq!(short["truncated"], true);
    assert!(short["truncated_lists"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("elements")));
    let full = change::change_report(&before, &after, &none, &none, true);
    assert_eq!(full["elements"]["added"].as_array().unwrap().len(), 40);
    assert_eq!(full["truncated"], false);
    assert_eq!(full["mode"], "full");
}

#[test]
fn r10_add_interface_writes_the_chain_the_context_reaches() {
    let before = files(BASE);
    let (after, _) = apply(&before, json!({"op": "add_interface", "context": "P::Sys", "from": "P::Sys::load::sub", "to": "P::Sys::psu", "item": "Heat"}));
    let text = &after[0].text;
    // The sub inside load is reached through load's border (abstraction rule A2): Load gets the port and a delegation.
    assert!(text.contains("connect load.heatOut to psu.heatIn;"), "{text}");
    assert!(text.contains("from load.heatOut.heat to psu.heatIn.heat;"), "{text}");
    assert!(text.contains("connect heatOut to sub.heatOut;"), "{text}");
    assert!(!text.contains("Load::sub."), "an end through the definition is not reachable: {text}");
    let m = model(&after);
    assert!(m.find("P::Load::heatOut").is_some_and(|p| p.kind == "port"), "{text}");
    assert!(m.find("P::Sub::heatOut").is_some_and(|p| p.kind == "port"), "{text}");
    let c = connect::check_connections(&m, "P").unwrap();
    assert_eq!(c["passed"], true, "{c}");
    assert_eq!(c["warnings"], 0, "{c}");
    // The nested part named through its definition is refused, and nothing is written.
    let e = match intent::apply_op(&before, &json!({"op": "add_interface", "context": "P::Sys", "from": "P::Load::sub", "to": "P::Sys::psu", "item": "Heat"})) {
        Ok(_) => panic!("an unreachable end must be refused"),
        Err(e) => e,
    };
    assert!(e.message.contains("cannot reach"), "{}", e.message);
}

#[test]
fn r10_check_connections_reports_an_end_the_context_cannot_reach() {
    let text = BASE.replace("        flow of Volt from psu.pwrout to load.inp;\n", "        flow of Volt from psu.pwrout to load.inp;\n        connection far connect P::Load::sub.a to psu.pwrout;\n");
    let f = files(&text);
    let m = model(&f);
    let c = connect::check_connections(&m, "P::Sys").unwrap();
    let hit: Vec<&Value> = c["findings"].as_array().unwrap().iter().filter(|x| x["rule"] == "unreachable-ends").collect();
    assert_eq!(hit.len(), 1, "{c}");
    assert_eq!(hit[0]["element"], "P::Sys::far");
    assert_eq!(hit[0]["severity"], "warning");
    assert!(c["warnings"].as_u64().unwrap() >= 1);
    // A reachable connection of the same context has no such finding.
    assert!(!c["findings"].as_array().unwrap().iter().any(|x| x["rule"] == "unreachable-ends" && x["element"] == "P::Sys::feed"));
}

#[test]
fn r10_a_flow_is_carried_by_the_connection_between_the_same_ports() {
    let f = files(BASE);
    let m = model(&f);
    let carriers = connect::flow_carriers(&m);
    assert_eq!(carriers.len(), 1);
    let (flow, conn) = carriers.iter().next().unwrap();
    assert_eq!(m.elements[*flow].kind, "flow");
    assert_eq!(m.elements[*conn].qualified_name, "P::Sys::feed");
}

#[test]
fn r10_line_and_column_agree_with_a_plain_count() {
    let f = files(BASE);
    let m = model(&f);
    let text = &m.files[0].text;
    for offset in (0..=text.len()).step_by(7) {
        let before = &text[..offset];
        let plain = (before.bytes().filter(|b| *b == b'\n').count() + 1, before.rfind('\n').map(|i| offset - i).unwrap_or(offset + 1));
        assert_eq!(m.line_col(0, offset), plain, "offset {offset}");
    }
}

#[test]
fn a_package_lists_what_it_depends_on_outside_itself() {
    let text = "package Lib {\n    part def Battery;\n    port def Pwr;\n}\npackage Car {\n    part def Body {\n        port p : Lib::Pwr;\n    }\n    part def Ev :> Body {\n        part pack : Lib::Battery;\n        part spare : Lib::Battery;\n    }\n    part body : Body;\n}\npackage Other {\n    part car : Car::Ev;\n}\n";
    let v = impact_of(&files(text), "Car", 1, 100);
    let d = &v["depends_on"];
    let pairs: Vec<(String, String, String)> = d["rows"].as_array().unwrap().iter()
        .map(|r| (r["from"].as_str().unwrap().into(), r["to"].as_str().unwrap().into(), r["role"].as_str().unwrap().into())).collect();
    // Out of the package: the port and both parts typed by Lib; inside it (Ev :> Body, body : Body) is not a dependency.
    assert_eq!(d["count"], 3, "{d}");
    assert!(pairs.contains(&("Car::Body::p".into(), "Lib::Pwr".into(), "typed by".into())), "{pairs:?}");
    assert!(pairs.iter().all(|(_, to, _)| to.starts_with("Lib::")), "{pairs:?}");
    assert_eq!(d["by_package"], json!([{ "package": "Lib", "count": 3 }]));
    assert_eq!(d["truncated"], false);
    // What depends on Car from outside is not what Car depends on.
    assert!(pairs.iter().all(|(from, _, _)| from.starts_with("Car::")));
    let capped = impact_of(&files(text), "Car", 1, 1);
    assert_eq!(capped["depends_on"]["rows"].as_array().unwrap().len(), 1);
    assert_eq!(capped["depends_on"]["count"], 3);
    assert_eq!(capped["depends_on"]["truncated"], true);
    // A part is not a package: no depends_on.
    assert!(impact_of(&files(text), "Car::Body", 1, 100).get("depends_on").is_none());
}
