//! Writes land and problems come back as notes: a scripted session of 300 edits, 60 of them subtree copies pasted with
//! the minted ids of their originals, ends with no edit refused and no id shared by two elements.

use serde_json::{json, Value};
use std::collections::HashMap;
use sysml_core::agent::intent;
use sysml_core::agent::mint::is_minted_id;
use sysml_core::api::FileIn;

fn text_of(f: &[FileIn], qn: &str) -> String {
    let m = intent::model_of(f);
    let e = m.find(qn).unwrap_or_else(|| panic!("{qn}"));
    m.files[e.file].text[e.span.start..e.span.end].to_string()
}

fn shared_ids(f: &[FileIn]) -> Vec<(String, usize)> {
    let m = intent::model_of(f);
    let mut n: HashMap<String, usize> = HashMap::new();
    for e in &m.elements {
        if let Some(s) = e.short_name.as_deref().filter(|s| is_minted_id(s)) {
            *n.entry(s.to_string()).or_default() += 1;
        }
    }
    let mut v: Vec<_> = n.into_iter().filter(|(_, c)| *c > 1).collect();
    v.sort();
    v
}

#[test]
fn r48_three_hundred_edits_with_sixty_subtree_copies_give_no_refusal_and_no_shared_id() {
    let mut f = vec![FileIn { path: "m.sysml".into(), text: "package Lib {\n}\npackage Sys {\n    part def Plant {\n    }\n}\npackage Copies {\n    part def Bench {\n    }\n}\n".into() }];
    let mut edits = 0usize;
    let mut refused: Vec<String> = Vec::new();
    let mut notes = 0usize;
    let mut step = |f: &mut Vec<FileIn>, op: Value| {
        edits += 1;
        match intent::apply_op(f, &op) {
            Ok(a) => {
                notes += a.info["reminted"].as_array().map(|v| v.len()).unwrap_or(0);
                *f = a.files;
            }
            Err(e) => refused.push(format!("{op}: {}", e.message)),
        }
    };
    // 60 definitions with features of their own, each with minted ids: 60 edits. (Definitions with the same members under
    // other names are the copied-definition rule, which still refuses an agent's copy; these are distinct.)
    for i in 0..60 {
        step(&mut f, json!({"op": "create_element", "owner": "Lib", "kind": "part def", "name": format!("D{i}"),
            "features": [{"kind": "port", "name": format!("inlet{i}")}, {"kind": "attribute", "name": format!("rate{i}")}]}));
    }
    // 60 usages in the plant, typed by them, each with a nested port: 60 edits.
    for i in 0..60 {
        step(&mut f, json!({"op": "create_element", "owner": "Sys::Plant", "kind": "part", "name": format!("u{i}"), "typed_by": format!("Lib::D{i}"),
            "features": [{"kind": "port", "name": "tap"}]}));
    }
    // 60 subtree copies pasted with the ids they carry: 30 definitions used as a template (renamed, one member added), 30
    // usages into another owner. A bare copy of a definition under another name is still refused (copied-definition,
    // checked below): that rule is about the model, not about ids.
    for i in 0..30 {
        let t = text_of(&f, &format!("Lib::D{i}")).replacen(&format!("D{i}"), &format!("D{i}Variant"), 1);
        let t = t.replacen('{', &format!("{{\n        attribute trim{i};"), 1);
        assert!(t.contains("<e"), "the copy carries ids: {t}");
        step(&mut f, json!({"op": "add_element", "owner": "Copies", "text": t}));
    }
    for i in 30..60 {
        let t = text_of(&f, &format!("Sys::Plant::u{i}"));
        assert!(t.contains("<e"), "the copy carries ids: {t}");
        step(&mut f, json!({"op": "add_element", "owner": "Copies::Bench", "text": t}));
    }
    // 120 more: an attribute on each definition, a doc on each usage.
    for i in 0..60 {
        step(&mut f, json!({"op": "create_element", "owner": format!("Lib::D{i}"), "kind": "attribute", "name": "mass"}));
    }
    for i in 0..60 {
        step(&mut f, json!({"op": "update_element", "ref": format!("Sys::Plant::u{i}"), "doc": format!("Usage {i}.")}));
    }
    assert_eq!(edits, 300);
    let shared = shared_ids(&f);
    println!("r48: edits {edits}, refused {}, re-mint notes {notes}, shared ids left {}", refused.len(), shared.len());
    assert!(refused.is_empty(), "{} refused:\n{}", refused.len(), refused.join("\n"));
    assert!(shared.is_empty(), "{shared:?}");
    // Each copy got new ids: the definitions and their two features, the usages and their nested port.
    assert_eq!(notes, 30 * 3 + 30 * 2, "one note per element that carried a copied id");
    // A bare copy under another name is still refused by the copied-definition rule, ids or not.
    let bare = text_of(&f, "Lib::D0").replacen("D0", "D0Copy", 1);
    let e = intent::apply_op(&f, &json!({"op": "add_element", "owner": "Copies", "text": bare})).expect_err("a bare copy is refused");
    assert!(e.message.contains("repeats part of") || e.message.contains("same members"), "{}", e.message);
    // The originals kept theirs.
    let m = intent::model_of(&f);
    for i in 0..60 {
        let a = m.find(&format!("Lib::D{i}")).unwrap().short_name.clone().unwrap();
        assert!(is_minted_id(&a));
    }
}

#[test]
fn r48_a_batch_of_copies_is_written_with_new_ids_in_one_call() {
    let mut f = vec![FileIn { path: "m.sysml".into(), text: "package Lib {\n}\npackage Copies {\n}\n".into() }];
    let defs: Vec<Value> = (0..20).map(|i| json!({"op": "create_element", "owner": "Lib", "kind": "part def", "name": format!("D{i}"), "features": [{"kind": "port", "name": "p"}]})).collect();
    f = intent::apply_batch(&f, &defs).unwrap_or_else(|(i, e)| panic!("{i}: {}", e.message)).files;
    let copies: Vec<Value> = (0..20).map(|i| json!({"op": "add_element", "owner": "Copies", "text": text_of(&f, &format!("Lib::D{i}")).replacen(&format!("D{i}"), &format!("C{i}"), 1).replacen('{', &format!("{{\n        attribute own{i};"), 1)})).collect();
    let b = intent::apply_batch(&f, &copies).unwrap_or_else(|(i, e)| panic!("{i}: {}", e.message));
    assert!(shared_ids(&b.files).is_empty(), "{:?}", shared_ids(&b.files));
    assert_eq!(b.reminted.len(), 40, "{:?}", b.reminted);
}

/// A short name that names one definition types the new usage by that definition's qualified name. Before, this panicked.
#[test]
fn a_short_type_name_that_names_one_definition_is_taken() {
    let mut f = vec![FileIn { path: "m.sysml".into(), text: "package Demo {\n    part def Pump;\n}\n".into() }];
    let op = json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "pump", "typed_by": "Pump"});
    f = intent::apply_op(&f, &op).unwrap_or_else(|e| panic!("{}", e.message)).files;
    assert!(text_of(&f, "Demo::pump").contains("Demo::Pump"), "{}", f[0].text);
    let missing = json!({"op": "create_element", "owner": "Demo", "kind": "part", "name": "valve", "typed_by": "Valve"});
    assert!(intent::apply_op(&f, &missing).is_err());
}
