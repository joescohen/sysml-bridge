//! Security round: model-declared derived properties are bounded. Every fixture is invented.

use serde_json::Value;
use std::time::Instant;
use sysml_core::agent::{self, model_props, trace_props};
use sysml_core::api::{self, FileIn};

fn model(text: &str) -> sysml_core::Model {
    let mut m = api::to_model(vec![FileIn { path: "m.sysml".into(), text: text.into() }]);
    agent::add_namespace_checks(&mut m);
    m
}

fn decl(name: &str, hops: &str) -> String {
    format!("        attribute {name} {{\n            @SysMLv1::derivedPropertySpecification {{\n                derivedName = \"{name}\";\n                appliesTo = \"Requirement\";\n                hops = \"{hops}\";\n                evaluation = \"evaluated\";\n            }}\n        }}\n")
}

fn wrap(decls: &str) -> String {
    format!("package P {{\n    package SysMLv1 {{\n        metadata def derivedPropertySpecification {{\n            attribute derivedName;\n            attribute appliesTo;\n            attribute hops;\n            attribute evaluation;\n        }}\n    }}\n    requirement r1;\n    part def Impl;\n    satisfy r1 by Impl;\n    part def Cust {{\n{decls}    }}\n}}\n")
}

#[test]
fn a_chain_of_more_than_eight_hops_is_listed_and_not_evaluated_and_a_huge_one_costs_no_time() {
    let eight = vec!["satisfy:opposite"; 8].join(", ");
    let nine = vec!["satisfy:opposite"; 9].join(", ");
    let huge = vec!["satisfy:opposite"; 100_000].join(", ");
    let m = model(&wrap(&format!("{}{}{}", decl("Eight", &eight), decl("Nine", &nine), decl("Huge", &huge))));
    let t = Instant::now();
    let table = agent::trace_props::table_for(&m);
    let ix = trace_props::index(&m);
    let id = m.find("P::r1").unwrap().id;
    let rows = model_props::of(&m, &ix, id);
    assert!(t.elapsed().as_secs_f64() < 2.0, "{:?}", t.elapsed());
    let by = |n: &str| rows.iter().find(|r| r["name"] == n).unwrap_or_else(|| panic!("{n}: {rows:?}"));
    assert_eq!(by("Eight")["evaluated"], true, "{rows:?}");
    for n in ["Nine", "Huge"] {
        assert_eq!(by(n)["evaluated"], false, "{n}");
        assert!(by(n)["reason"].as_str().unwrap().contains("not evaluated: more than 8 hops"), "{}", by(n));
        assert!(by(n).get("items").is_none());
    }
    assert!(table["properties"].as_array().unwrap().iter().any(|p| p["name"] == "Huge" && p["evaluated"] == false));
}

#[test]
fn a_call_that_runs_out_of_steps_lists_the_rest_as_not_evaluated_with_the_reason() {
    let chain = vec!["satisfy:opposite"; 8].join(", ");
    let decls: String = (0..6).map(|i| decl(&format!("D{i}"), &chain)).collect();
    let m = model(&wrap(&decls));
    let ix = trace_props::index(&m);
    let id = m.find("P::r1").unwrap().id;
    // An empty chain takes two steps here (the second hop finds nothing); a budget of 5 serves two of them (two steps each here) and runs out in the third.
    let rows = model_props::of_with(&m, &ix, id, 5);
    let done: Vec<bool> = rows.iter().map(|r| r["evaluated"] == true).collect();
    assert_eq!(done, vec![true, true, false, false, false, false], "{rows:?}");
    for r in &rows[2..] {
        assert!(r["reason"].as_str().unwrap().contains("step budget of one call (5 steps)"), "{r}");
    }
    // The default budget serves all six.
    assert!(model_props::of(&m, &ix, id).iter().all(|r: &Value| r["evaluated"] == true));
}
