//! Element ids in the model text (agent/mint.rs): a tool gives every element it creates an id (a SysML v2 short name),
//! a rename leaves it alone, a reference written by it follows a change of it, two elements never share one, and
//! `mint_ids` gives a model written before ids existed one id per element.

use serde_json::{json, Value};
use sysml_core::agent::mint::{is_minted_id, strip_ids};
use sysml_core::agent::{intent, rules};
use sysml_core::api::FileIn;
use sysml_core::model::Model;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn run(f: &mut Vec<FileIn>, op: Value) -> Value {
    let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("{op}: {}", e.message));
    *f = a.files;
    a.info
}

fn model(f: &[FileIn]) -> std::sync::Arc<Model> {
    intent::model_of(f)
}

fn id_of(f: &[FileIn], qn: &str) -> Option<String> {
    model(f).find(qn).and_then(|e| e.short_name.clone())
}

fn rules_found(f: &[FileIn]) -> Vec<String> {
    rules::run_gates(&model(f), &["semantic"], None, None, &[]).findings.iter().map(|x| x["rule"].as_str().unwrap().to_string()).collect()
}

const LEGACY: &str = "package P {\n    part def Pump { port out1 : Flow; }\n    port def Flow;\n    requirement <'R-1'> flowRate;\n    part def Plant { part pump : Pump; }\n    satisfy flowRate by Plant;\n}\n";

#[test]
fn a_created_element_and_its_features_get_ids_and_the_reply_names_the_id() {
    let mut f = files("package P {\n}\n");
    let info = run(&mut f, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Analyser", "features": [{"kind": "port", "name": "resultOut"}, {"kind": "attribute", "name": "rate"}]}));
    let id = id_of(&f, "P::Analyser").expect("an id");
    assert!(is_minted_id(&id), "{id}");
    assert_eq!(info["id"], json!(id));
    assert!(f[0].text.contains(&format!("part def <{id}> Analyser")), "{}", f[0].text);
    let port = id_of(&f, "P::Analyser::resultOut").expect("the port has an id");
    assert!(is_minted_id(&port) && port != id);
    // The package that was there had none and gets none from a write beside it; a requirement id given is kept.
    assert_eq!(id_of(&f, "P"), None);
    run(&mut f, json!({"op": "create_element", "owner": "P", "kind": "requirement", "name": "speed", "short_name": "R-7"}));
    assert_eq!(id_of(&f, "P::speed").as_deref(), Some("R-7"));
    assert!(f[0].text.contains("requirement <'R-7'> speed"), "{}", f[0].text);
    assert_eq!(rules_found(&f).iter().filter(|r| *r == "duplicate-id").count(), 0);
}

#[test]
fn a_rename_keeps_the_id_and_a_reference_written_by_the_id_is_untouched() {
    let mut f = files("package P {\n}\n");
    run(&mut f, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Pump"}));
    let id = id_of(&f, "P::Pump").unwrap();
    // A usage typed by the id, as any tool or person may write it.
    f[0].text = f[0].text.replacen("package P {", &format!("package P {{\n    part pump : '{id}';"), 1);
    assert!(model(&f).find("P::pump").unwrap().refs.iter().any(|r| r.resolved().is_some()), "the id resolves:\n{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "P::Pump", "rename": "Compressor"}));
    assert_eq!(id_of(&f, "P::Compressor"), Some(id.clone()));
    assert!(f[0].text.contains(&format!("part pump : '{id}';")), "the reference by id is not rewritten:\n{}", f[0].text);
    let m = model(&f);
    let target = m.find("P::pump").unwrap().refs.iter().find_map(|r| r.resolved()).unwrap();
    assert_eq!(m.elements[target].qualified_name, "P::Compressor");
}

#[test]
fn a_copy_pasted_with_its_id_is_reminted_from_a_tool_and_reported_from_a_hand_edit() {
    let mut f = files("package P {\n    package Q {\n    }\n}\n");
    run(&mut f, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Pump"}));
    let id = id_of(&f, "P::Pump").unwrap();
    // A tool write of raw text that repeats the id elsewhere: written, the copy gets a new id, with a note.
    let mut tool = f.clone();
    let info = run(&mut tool, json!({"op": "add_element", "owner": "P::Q", "text": format!("port def <{id}> Inlet;")}));
    let notes = info["reminted"].as_array().expect("a re-mint note");
    assert_eq!(notes.len(), 1, "{info}");
    assert_eq!(notes[0]["rule"], "duplicate-id");
    assert_eq!(notes[0]["id_was"], json!(id));
    let copy = id_of(&tool, "P::Q::Inlet").unwrap();
    assert!(is_minted_id(&copy) && copy != id, "{}", tool[0].text);
    assert_eq!(notes[0]["id"], json!(copy));
    assert_eq!(id_of(&tool, "P::Pump"), Some(id.clone()), "the original keeps its id");
    assert!(!rules_found(&tool).contains(&"duplicate-id".to_string()));
    // The same paste by hand: reported as an error with the command that fixes it.
    let mut hand = f.clone();
    hand[0].text = hand[0].text.replacen("package Q {", &format!("package Q {{\n        port def <{id}> Inlet;"), 1);
    let found = rules::run_gates(&model(&hand), &["semantic"], None, None, &[]).findings;
    let dup = found.iter().find(|x| x["rule"] == "duplicate-id").expect("reported");
    assert_eq!(dup["severity"], "error");
    assert!(dup["fix"].as_str().unwrap().contains("mint_ids"), "{dup}");
    // The fix: a new id for the copy, the original keeps its own.
    let info = run(&mut hand, json!({"op": "mint_ids", "ref": "P::Q::Inlet"}));
    let fresh = id_of(&hand, "P::Q::Inlet").unwrap();
    assert!(is_minted_id(&fresh) && fresh != id, "{info}");
    assert_eq!(info["previous_id"], json!(id));
    assert_eq!(id_of(&hand, "P::Pump"), Some(id));
    assert!(!rules_found(&hand).contains(&"duplicate-id".to_string()));
}

#[test]
fn mint_ids_gives_a_model_written_before_ids_one_id_per_element_once() {
    let mut f = files(LEGACY);
    let dry = run(&mut f, json!({"op": "mint_ids", "dry_run": true}));
    assert_eq!(f[0].text, LEGACY, "a dry run writes nothing");
    let n = dry["minted"].as_u64().unwrap();
    // P, Pump, out1, Flow, Plant, pump: the requirement keeps R-1, the satisfy statement has no name.
    assert_eq!(n, 6, "{dry}");
    let info = run(&mut f, json!({"op": "mint_ids"}));
    assert_eq!(info["minted"], json!(n));
    for qn in ["P", "P::Pump", "P::Pump::out1", "P::Flow", "P::Plant", "P::Plant::pump"] {
        assert!(id_of(&f, qn).as_deref().map(is_minted_id).unwrap_or(false), "{qn}:\n{}", f[0].text);
    }
    assert_eq!(id_of(&f, "P::flowRate").as_deref(), Some("R-1"));
    // The text is the old text with the ids added, nothing else.
    assert_eq!(strip_ids(&f[0].text), LEGACY);
    assert!(model(&f).diagnostics.iter().all(|d| d.severity != sysml_core::model::Severity::Error), "{:?}", model(&f).diagnostics);
    // Run again: nothing left to mint.
    assert_eq!(run(&mut f, json!({"op": "mint_ids"}))["minted"], 0);
    // A scope mints only under it.
    let mut g = files(LEGACY);
    assert_eq!(run(&mut g, json!({"op": "mint_ids", "scope": "P::Plant"}))["minted"], 2);
}

#[test]
fn a_reference_written_by_an_id_follows_a_change_of_the_id() {
    let mut f = files("package P {\n    requirement <'R-12'> flowRate;\n    part def Plant;\n    satisfy 'R-12' by Plant;\n}\n");
    assert!(model(&f).diagnostics.is_empty(), "{:?}", model(&f).diagnostics);
    run(&mut f, json!({"op": "update_element", "ref": "P::flowRate", "short_name": "R-20"}));
    assert!(f[0].text.contains("requirement <'R-20'> flowRate;") && f[0].text.contains("satisfy 'R-20' by Plant;"), "{}", f[0].text);
    assert!(model(&f).diagnostics.is_empty(), "{:?}", model(&f).diagnostics);
}

#[test]
fn a_reqif_identifier_is_kept_across_a_rename_and_a_move_of_the_requirement() {
    let ids_of = |f: &[FileIn]| -> Vec<String> {
        let x = sysml_core::agent::reqif::export(&model(f), "P", &[]).unwrap()["reqif"].as_str().unwrap().to_string();
        x.split("<SPEC-OBJECT IDENTIFIER=\"").skip(1).map(|s| s[..s.find('"').unwrap()].to_string()).collect()
    };
    let mut f = files("package P {\n    package Q {\n    }\n}\n");
    run(&mut f, json!({"op": "create_element", "owner": "P", "kind": "requirement", "name": "holdFlow", "doc": "The pump shall hold the flow."}));
    let before = ids_of(&f);
    assert_eq!(before.len(), 1);
    run(&mut f, json!({"op": "update_element", "ref": "P::holdFlow", "rename": "keepFlow"}));
    assert_eq!(ids_of(&f), before, "a rename keeps the IDENTIFIER");
    run(&mut f, json!({"op": "update_element", "ref": "P::keepFlow", "owner": "P::Q"}));
    assert_eq!(ids_of(&f), before, "a move keeps the IDENTIFIER");
}

#[test]
fn the_change_report_pairs_an_element_by_its_id_when_it_was_renamed_and_changed_outside_the_tools() {
    let before = files("package P {\n    part def <ea0000a> Pump { attribute rate; }\n}\n");
    let after = files("package P {\n    part def <ea0000a> Engine { attribute speed; attribute torque; }\n}\n");
    let none = std::collections::BTreeMap::new();
    let r = sysml_core::agent::change::change_report(&before, &after, &none, &none, true);
    let text = r.to_string();
    let changed = r["elements"]["changed"].as_array().cloned().unwrap_or_default();
    assert!(changed.iter().any(|c| c["ref"] == "P::Engine" && c["previous_ref"] == "P::Pump"), "{text}");
    // The block is not reported removed and added; only the attribute that went is.
    let removed: Vec<&str> = r["elements"]["removed"].as_array().into_iter().flatten().filter_map(|x| x["ref"].as_str()).collect();
    assert_eq!(removed, ["P::Pump::rate"], "{text}");
}

#[test]
fn an_element_the_page_adds_as_text_gets_an_id_and_the_rest_of_the_model_is_left_alone() {
    use sysml_core::edit::EditOp;
    let f = files("package P {\n    part def Old;\n}\n");
    let changes = sysml_core::api::apply_edit(f, &EditOp::AddElement { owner: Some("P".into()), file: None, text: "part def Pump { port inlet; }".into() }).unwrap();
    let text = &changes[0].text;
    let after = files(text);
    assert!(id_of(&after, "P::Pump").as_deref().map(is_minted_id).unwrap_or(false), "{text}");
    assert!(id_of(&after, "P::Pump::inlet").as_deref().map(is_minted_id).unwrap_or(false), "{text}");
    // What was there before the edit is not touched: mint_ids is how an older model gets its ids.
    assert_eq!(id_of(&after, "P::Old"), None, "{text}");
    assert_eq!(id_of(&after, "P"), None, "{text}");
}
