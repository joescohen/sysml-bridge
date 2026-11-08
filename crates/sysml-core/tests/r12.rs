//! Tools round 12: the syntax gate refuses what the official OMG SysML v2 grammar refuses, and the writers never write it.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{intent, trace};
use sysml_core::api::{self, FileIn};
use sysml_core::Severity;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn errors(text: &str) -> Vec<String> {
    api::to_model(files(text)).diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
}

fn gate(text: &str) -> Value {
    trace::validate_model(&api::to_model(files(text)), Some("syntax")).unwrap()
}

#[test]
fn r12_end_ref_part_is_refused_and_end_part_is_not() {
    let e = errors("package P { part def T; part def A { end ref part x : T; } }");
    assert!(e.iter().any(|m| m.contains("end part x")), "{e:?}");
    assert!(errors("package P { part def T; part def A { end part x : T; end port p; } }").is_empty());
    // The gate of validate_model reports it as an error.
    assert!(gate("package P { part def A { end ref part x; } }")["errors"].as_u64().unwrap() >= 1);
    assert_eq!(gate("package P { part def A { end part x; } }")["errors"], 0);
}

#[test]
fn r12_readonly_is_refused_and_constant_is_not() {
    let e = errors("package P { part def A { readonly attribute x; } }");
    assert!(e.iter().any(|m| m.contains("constant")), "{e:?}");
    assert!(errors("package P { part def A { constant attribute x; } }").is_empty());
}

#[test]
fn r12_a_reserved_word_as_a_name_must_be_quoted() {
    let e = errors("package P { action new; }");
    assert!(e.iter().any(|m| m.contains("Quote the name")), "{e:?}");
    assert!(errors("package P { action 'new'; part def Holder { port ok; } }").is_empty());
    assert!(errors("package P { part def A { port out : B; } part def B; }").iter().any(|m| m.contains("'out'")));
}

#[test]
fn r12_prefix_words_in_the_wrong_order_are_refused() {
    let e = errors("package P { part def A { abstract derived attribute x; } }");
    assert!(e.iter().any(|m| m.contains("must come before")), "{e:?}");
    assert!(errors("package P { part def A { in constant ref attribute x; derived abstract attribute y; } }").is_empty());
    let e = errors("package P { part def A { ref in attribute x; } }");
    assert!(e.iter().any(|m| m.contains("must come before")), "{e:?}");
}

#[test]
fn r12_the_writers_never_write_the_three() {
    let f = files("package P { part def A; }");
    let run = |op: Value| intent::apply_op(&f, &op).map_err(|e| e.message);
    // readonly is refused with the official word; constant is written.
    let e = run(json!({"op": "create_element", "owner": "P", "kind": "attribute", "name": "x", "modifiers": ["readonly"]})).unwrap_err();
    assert!(e.contains("constant"), "{e}");
    let ok = run(json!({"op": "create_element", "owner": "P", "kind": "attribute", "name": "x", "modifiers": ["constant"]})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&ok.files[0].text).contains("constant attribute x"));
    // end and ref do not go together.
    let e = run(json!({"op": "create_element", "owner": "P::A", "kind": "part", "name": "e", "modifiers": ["ref", "end"]})).unwrap_err();
    assert!(e.contains("end part"), "{e}");
    let ok = run(json!({"op": "create_element", "owner": "P::A", "kind": "part", "name": "e", "modifiers": ["end"]})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&ok.files[0].text).contains("end part e"));
    // Order: derived, then abstract; the result passes the gate.
    let ok = run(json!({"op": "create_element", "owner": "P::A", "kind": "attribute", "name": "d", "modifiers": ["constant", "derived", "ref"]})).unwrap();
    assert!(errors(&ok.files[0].text).is_empty(), "{}", ok.files[0].text);
    // A name equal to a reserved word is quoted.
    let ok = run(json!({"op": "create_element", "owner": "P", "kind": "action", "name": "new"})).unwrap();
    assert!(sysml_core::agent::mint::strip_ids(&ok.files[0].text).contains("action 'new'"), "{}", ok.files[0].text);
    assert!(errors(&ok.files[0].text).is_empty());
}
