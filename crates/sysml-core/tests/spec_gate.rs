//! The opt-in `spec` gate: well-formedness constraints of the SysML v2 and KerML specifications. Each rule has a
//! known-bad fixture in tests/rule_fixtures; these tests hold the forms the specifications allow, which must stay quiet.

use serde_json::Value;
use sysml_core::{
    agent::rules,
    api::{self, FileIn},
};

fn spec(text: &str) -> Vec<Value> {
    let m = api::to_model(vec![FileIn { path: "s.sysml".into(), text: text.into() }]);
    assert!(!m.diagnostics.iter().any(|d| format!("{:?}", d.severity) == "Error"), "{:?}", m.diagnostics);
    rules::run_gates(&m, &["spec"], None, None, &[]).findings
}

const VALID: &str = "private import ScalarValues::*;
package P {
    part def A { attribute m default = 5; attribute n; }
    part a : A { :>> m = 6; :>> n = 1; }
    part b : A { attribute redefines m = 7; }
    attribute def AD { ref part p; attribute x; in item i; }
    enum def Colour { red; enum green; doc /* The colours. */ }
    variation part def V { doc /* A choice. */ variant part a1 : A; variant a2 : A; }
    part def V2 :> A;
    connection def C { end part x : A; end part y : A; }
    part c : C;
    individual part def Unit1 :> A;
    individual part u : Unit1;
    state def S { entry action e1; do action d1; exit action x1; state s1; state s2; transition first s1 then s2; }
    state def Par parallel { state x; state y; }
    requirement def R { subject s : A; in z; require constraint { true } }
    use case def UC { subject s : A; actor user : A; objective o { doc /* Deliver. */ } }
    view def VD { render asTree; }
    view v : VD { expose P::*; }
    action def Fl {
        action a; action b; action c;
        fork f; first a then f; then b; then c;
        join j; first b then j; first c then j; then done;
    }
    flow def FD { end a; end b; }
}
";

#[test]
fn forms_the_specifications_allow_raise_nothing() {
    let f = spec(VALID);
    assert!(f.is_empty(), "{f:#?}");
}

#[test]
fn each_finding_names_the_specification_constraint() {
    let f = spec("package P { action def Act; part def Bad :> Act; item def It; part p : It; part def Q { subject s; } }");
    let rules: Vec<&str> = f.iter().map(|x| x["rule"].as_str().unwrap()).collect();
    assert_eq!(rules, ["spec-specialization-kind", "spec-part-definition", "spec-membership-owner"], "{f:#?}");
    assert!(f[0]["message"].as_str().unwrap().starts_with("validateStructureSpecialization"));
    assert!(f[1]["message"].as_str().unwrap().starts_with("validatePartUsagePartDefinition"));
    assert!(f[2]["message"].as_str().unwrap().contains("owned only by a requirement or a case"));
    assert!(f.iter().all(|x| x["gate"] == "spec" && x["severity"] == "error"));
}

#[test]
fn a_value_redefines_through_a_chain_of_redefinitions() {
    // b redefines a's m, which redefines the bound m of A: the bound value is still overridden.
    let f = spec("package P { part def A { attribute m = 5; } part def B :> A { attribute redefines m; } part b : B { :>> m = 6; } }");
    assert_eq!(f.iter().filter(|x| x["rule"] == "spec-feature-value-overriding").count(), 1, "{f:#?}");
}

#[test]
fn the_gate_is_opt_in_and_listed() {
    assert!(!rules::DEFAULT_GATES.contains(&"spec"));
    assert_eq!(rules::GATES.last(), Some(&"spec"));
    let n = rules::registry().iter().filter(|r| r.gate == "spec").count();
    assert_eq!(n, 19);
}
