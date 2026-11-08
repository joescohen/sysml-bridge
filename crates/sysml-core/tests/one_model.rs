//! One model, many views: a stored view that points at an element the model no longer holds is listed as stale, a concept
//! defined twice is reported, and a component nothing traces to is reported.

use sysml_core::agent::{self, rules};
use sysml_core::api::{self, FileIn};

fn model(text: &str) -> sysml_core::model::Model {
    let mut m = api::to_model(vec![FileIn { path: "m.sysml".into(), text: text.into() }]);
    agent::add_namespace_checks(&mut m);
    m
}

fn rules_of(text: &str, gate: &str) -> Vec<String> {
    rules::run_gates(&model(text), &[gate], None, None, &["satisfy"]).findings.iter().map(|f| f["rule"].as_str().unwrap().to_string()).collect()
}

#[test]
fn one_concept_is_one_definition() {
    let twice = "package P { part def Pump; package Other { part def Pump; } }";
    assert_eq!(rules_of(twice, "semantic").iter().filter(|r| *r == "duplicate-definition").count(), 2);
    // A specialization of a same-named definition is a variant, not a copy; a library package holds shared types.
    assert!(!rules_of("package P { part def Pump; package Other { part def Pump :> P::Pump; } }", "semantic").contains(&"duplicate-definition".to_string()));
    assert!(!rules_of("library package L { part def Pump; } package P { part def Pump; }", "semantic").contains(&"duplicate-definition".to_string()));
    // Two kinds may share a name.
    assert!(!rules_of("package P { part def Pump; package Other { item def Pump; } }", "semantic").contains(&"duplicate-definition".to_string()));
}

#[test]
fn a_component_with_no_job_is_reported() {
    let idle = "package P { part def Pump; part def Sys { part pump : Pump; } part sys : Sys; }";
    let got = rules_of(idle, "traceability");
    assert_eq!(got.iter().filter(|r| *r == "component-without-job").count(), 1, "only the leaf pump, not the assembly: {got:?}");
    for busy in [
        "package P { action def Run { action spin; } part def Pump { perform action r : Run; } part def Sys { part pump : Pump; } part sys : Sys; }",
        "package P { action run; part def Pump; part def Sys { part pump : Pump; } part sys : Sys; allocate run to sys.pump; }",
        "package P { requirement r; part def Pump; part def Sys { part pump : Pump; } part sys : Sys; satisfy r by sys.pump; }",
    ] {
        assert!(!rules_of(busy, "traceability").contains(&"component-without-job".to_string()), "{busy}");
    }
}

/// `allocate f to ctx.missionComputer`: the part is the chain's last feature, as `allocations` resolves it; the impact of
/// the chain's head lists the statement (a change to it reaches the chain) but no behaviour allocated to it.
#[test]
fn a_feature_chain_allocate_credits_only_the_last_feature() {
    let text = "package A { action def Behaviour { action collisionAvoidance; } action behaviour : Behaviour; }\n\
        package C {\n part def Computer;\n part def AutonomyContext { part missionComputer : Computer; }\n\
        part def CrossDomainContext { part autonomyContext : AutonomyContext; }\n part cross : CrossDomainContext;\n\
        allocate A::Behaviour::collisionAvoidance to C::CrossDomainContext::autonomyContext.missionComputer;\n}\n";
    let m = model(text);
    let credited: Vec<String> = agent::allocation::allocations(&m).iter().filter(|a| a.via == "allocate").map(|a| m.elements[a.part].qualified_name.clone()).collect();
    assert_eq!(credited, ["C::AutonomyContext::missionComputer"]);
    let impact = |name: &str| agent::impact::get_impact(&m, &std::collections::BTreeMap::new(), name, 1, 100).unwrap_or_else(|e| panic!("get_impact {name}: {}", e.message));
    let allocated = |name: &str| -> Vec<String> { impact(name)["behaviour"]["allocated"].as_array().unwrap().iter().map(|x| x["ref"].as_str().unwrap().to_string()).collect() };
    assert_eq!(allocated("C::AutonomyContext::missionComputer"), ["A::Behaviour::collisionAvoidance"]);
    assert!(allocated("C::CrossDomainContext::autonomyContext").is_empty());
    assert!(allocated("C::cross").is_empty());
    // The allocation row on the head names the resolved part as its target, not the head.
    let rows = impact("C::CrossDomainContext::autonomyContext")["traceability"]["allocations"].clone();
    assert!(rows.as_array().unwrap().iter().all(|r| r["target"] == "C::AutonomyContext::missionComputer"), "{rows}");
}
