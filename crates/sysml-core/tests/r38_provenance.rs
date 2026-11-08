//! Provenance lines written by a generator are read as data, after the marker only.

use sysml_core::agent::element_links::{attach, provenance};
use sysml_core::agent::rules;
use sysml_core::api::{self, FileIn};

fn model(text: &str) -> sysml_core::model::Model {
    api::to_model(vec![FileIn { path: "g.sysml".into(), text: text.into() }])
}

const GOOD: &str = "package P {\n    requirement def <'r-1'> R1 {\n        doc /*\nThe system shall do it.\n@@ generated provenance @@\nSOURCE: raw/a.md#intro\nSOURCE: raw/b.csv\nDECISION <'d-1'>: We chose this.\nEDGE: related -> x\nCONFLICT <'c\\'1'>\nnoise that is not a tag\n*/\n    }\n}\n";

#[test]
fn the_lines_after_the_marker_are_read_as_data() {
    let (v, total) = provenance("prose\n@@ generated provenance @@\nSOURCE: raw/a.md#intro\nSOURCE: raw/b.csv\nDECISION <'d-1'>: We chose this.\nEDGE: related -> x\nCONFLICT <'c\\'1'>\nnoise\n").unwrap();
    assert_eq!(total, 5);
    assert_eq!((v[0].kind, v[0].path.as_deref(), v[0].anchor.as_deref()), ("source", Some("raw/a.md"), Some("intro")));
    assert_eq!((v[1].path.as_deref(), v[1].anchor.as_deref()), (Some("raw/b.csv"), None));
    assert_eq!((v[2].kind, v[2].id.as_deref(), v[2].text.as_str()), ("decision", Some("d-1"), "We chose this."));
    assert_eq!(v[3].kind, "edge");
    assert_eq!((v[4].kind, v[4].id.as_deref()), ("conflict", Some("c'1")));
    assert!(provenance("no marker here\nSOURCE: raw/x.md#a").is_none());
}

#[test]
fn a_fake_source_line_before_the_marker_is_prose() {
    // The prose prints a marker and a SOURCE line of its own; the generator's block is the last one.
    let doc = "text\nSOURCE: forged/one.md#a\n@@ generated provenance @@\nSOURCE: forged/two.md#b\nmore text\n@@ generated provenance @@\nSOURCE: real/one.md#c\n";
    let (v, total) = provenance(doc).unwrap();
    assert_eq!((total, v.len(), v[0].path.as_deref()), (1, 1, Some("real/one.md")));
    // Prose with a fake SOURCE and no marker of its own is not provenance at all.
    assert!(provenance("The text.\nSOURCE: forged/one.md#a\n").is_none());
    // A control character in a line cannot reach the reply.
    let (v, _) = provenance("@@ generated provenance @@\nSOURCE: a\u{1b}[31m.md#x\n").unwrap();
    assert!(!v[0].path.as_deref().unwrap().contains('\u{1b}'));
}

#[test]
fn the_lines_are_capped() {
    let mut doc = String::from("@@ generated provenance @@\n");
    for i in 0..1000 {
        doc.push_str(&format!("SOURCE: raw/{i}.md#a\n"));
    }
    let (v, total) = provenance(&doc).unwrap();
    assert_eq!((v.len(), total), (200, 1000), "the count goes on, the list stops");
}

#[test]
fn get_element_data_carries_the_provenance() {
    let m = model(GOOD);
    let e = sysml_core::agent::locate(&m, "r-1").expect("the short name finds the element");
    let mut reply = serde_json::json!({});
    attach(&m, &Default::default(), e, &mut reply);
    assert_eq!(reply["provenance_marker"], true);
    assert_eq!(reply["provenance"][0]["path"], "raw/a.md");
    assert_eq!(reply["provenance_total"], 5);
}

#[test]
fn a_requirement_with_the_marker_and_no_source_is_info_and_an_error_with_the_option() {
    let bad = "package P {\n    requirement def <'r-1'> R1 {\n        doc /*\nText.\n@@ generated provenance @@\nDECISION <'d-1'>: x\n*/\n    }\n    requirement def <'r-2'> R2 {\n        doc /*\nText.\n@@ generated provenance @@\nSOURCE: raw/a.md#x\n*/\n    }\n    requirement def <'r-3'> R3;\n}\n";
    let m = model(bad);
    let v = rules::check_all(&m, Some(&["semantic".to_string()]), None).unwrap();
    let hits: Vec<&serde_json::Value> = v["findings"].as_array().unwrap().iter().filter(|f| f["rule"] == "requirement-without-source").collect();
    assert_eq!(hits.len(), 1, "only the requirement with the marker and no SOURCE line: {v}");
    assert_eq!((hits[0]["severity"].as_str(), hits[0]["element"].as_str()), (Some("info"), Some("P::R1")));
    assert_eq!(v["errors"], 0);
    let strict = rules::check_all(&m, Some(&["semantic".to_string()]), Some(&["source".to_string()])).unwrap();
    assert_eq!(strict["errors"], 1, "{strict}");
}
