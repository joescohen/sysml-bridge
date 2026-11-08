//! Cameo's "Realizing Element" / "All Realizing Elements" and "Specifying Element" / "All Specifying Elements".
//!
//! Cameo lists these four in a requirement's Specification window (the shipped MD Customization for Requirements profile names
//! realizingElement, allRealizingElements, specifyingElement and allSpecifyingElements). Its UML Standard Profile defines the
//! one-hop form over Abstraction (satisfy, verify, refine, trace, derive and allocate all extend it) and the All form as a
//! recursive collection of the one-hop form, so the All form crosses link kinds, which no single-kind "All ..." property does.

use serde_json::Value;
use sysml_core::agent::{self, trace_props};
use sysml_core::api::{self, FileIn};

const TEXT: &str = "package P {
    requirement top;
    requirement sub;
    #SysMLv1::DeriveReqt dependency from sub to top;
    part def Pump;
    satisfy sub by Pump;
    verification def PumpTest;
    verify top by PumpTest;
}
";

fn props(qn: &str) -> Value {
    let mut m = api::to_model(vec![FileIn { path: "p.sysml".into(), text: TEXT.into() }]);
    agent::add_namespace_checks(&mut m);
    let id = m.elements.iter().find(|e| e.qualified_name == qn).unwrap_or_else(|| panic!("no {qn}")).id;
    trace_props::of(&m, &trace_props::index(&m), id)
}

fn prop<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["properties"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("no property {name}"))
}

fn refs(p: &Value, key: &str) -> Vec<String> {
    let mut r: Vec<String> = p[key].as_array().unwrap().iter().map(|x| x["ref"].as_str().unwrap().to_string()).collect();
    r.sort();
    r
}

#[test]
fn all_realizing_elements_cross_link_kinds() {
    let v = props("P::top");
    let r = prop(&v, "Realizing Element");
    assert_eq!(r["all_name"], "All Realizing Elements");
    // One hop: what derives from top, and what verifies it.
    assert_eq!(refs(r, "items"), ["P::PumpTest", "P::sub"], "{r}");
    // The closure goes on through sub to the part that satisfies it, a satisfy link reached through a derive link.
    assert_eq!(refs(r, "all_items"), ["P::Pump", "P::PumpTest", "P::sub"], "{r}");
    // No single-kind closure reaches Pump from top.
    for name in ["Satisfied By", "Verified By", "Derived"] {
        assert!(!refs(prop(&v, name), "all_items").contains(&"P::Pump".to_string()), "{name}");
    }
}

#[test]
fn all_specifying_elements_run_the_other_way() {
    let v = props("P::Pump");
    let s = prop(&v, "Specifying Element");
    assert_eq!(refs(s, "items"), ["P::sub"], "{s}");
    assert_eq!(refs(s, "all_items"), ["P::sub", "P::top"], "{s}");
}
