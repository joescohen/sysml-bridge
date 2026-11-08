//! Independently authored examples of the prose Quick Find contract.
use serde_json::Value;
use sysml_core::{
    agent::find::{find, Opts},
    api::{self, FileIn},
    Model,
};

fn model(names: &[&str]) -> Model {
    let text = names.iter().map(|n| format!("part def '{n}';")).collect::<String>();
    api::to_model(vec![FileIn { path: "find.sysml".into(), text }])
}
fn opts(text: &str) -> Opts {
    Opts { text: text.into(), scope: "any".into(), offset: 0, limit: 0, match_anywhere: false, use_camel_case: true }
}
fn result(m: &Model, o: &Opts) -> Value {
    find(m, o).unwrap()
}
fn names(m: &Model, o: &Opts) -> Vec<String> {
    result(m, o)["hits"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().into()).collect()
}
#[test]
fn prefix_is_case_insensitive_not_implicit_contains() {
    let m = model(&["Fuel Pump", "fuelPipe", "Main Fuel Pump", "Other"]);
    assert_eq!(names(&m, &opts("fUeL")), ["Fuel Pump", "fuelPipe"]);
}
#[test]
fn contains_option_and_leading_stars() {
    let m = model(&["Fuel", "Main Fuel", "Other"]);
    let mut o = opts("fuel");
    assert_eq!(names(&m, &o), ["Fuel"]);
    o.match_anywhere = true;
    assert_eq!(names(&m, &o), ["Fuel", "Main Fuel"]);
    assert_eq!(names(&m, &opts("***fuel")), names(&m, &o));
}
#[test]
fn glob_has_only_two_special_characters() {
    let m = model(&["Fuel Pump", "Feel Map", "Fel Pump", "XFuel Pump"]);
    assert_eq!(names(&m, &opts("F?el*p")), ["Feel Map", "Fuel Pump"]);
    let m = model(&["F.el(p)[x]+", "Fxelppx"]);
    assert_eq!(names(&m, &opts("F.el(p)[x]+")), ["F.el(p)[x]+"]);
    assert_eq!(names(&model(&["AB", "AxxB", "AC"]), &opts("A*B")), ["AB", "AxxB"]);
}
#[test]
fn humps_and_lowercase_conversion_and_option() {
    let m = model(&["FuelRail", "fuelRail", "Fire", "FRU"]);
    for q in ["fr", "FR"] {
        let n = names(&m, &opts(q));
        assert_eq!(n.len(), 3);
        assert!(!n.contains(&"Fire".into()));
    }
    let mut off = opts("fr");
    off.use_camel_case = false;
    assert_eq!(names(&m, &off), ["FRU"]);
    // Digit-bearing input bypasses the independent word-initial extension.
    let m = model(&["FuelRail2", "FuelRail3"]);
    assert_eq!(names(&m, &opts("FR2")), ["FuelRail2"]);
    let mut o = opts("FR2");
    o.use_camel_case = false;
    assert!(names(&m, &o).is_empty());
}
#[test]
fn mixed_case_digits_and_anchoring() {
    let m = model(&["FuelRail12", "FrontRail12", "FuelRail102", "MainFuelRail12", "FuelBigRail12"]);
    assert_eq!(names(&m, &opts("FuR12")), ["FuelRail12"]);
    let mut o = opts("FuR12");
    o.match_anywhere = true;
    assert_eq!(names(&m, &o), ["FuelRail12", "MainFuelRail12"]);
}
#[test]
fn whitespace_and_unfiltered_queries() {
    let mut m = model(&["Fuel Pump", "Other"]);
    m.elements[0].name = Some("Fuel\u{a0}Pump".into());
    assert_eq!(names(&m, &opts("fuel\tp")), ["Fuel\u{a0}Pump"]);
    m.elements[1].name = Some(String::new());
    for q in ["", "*", "***"] {
        assert_eq!(names(&m, &opts(q)).len(), 2);
    }
    assert!(names(&m, &opts("?")).iter().all(|n| !n.is_empty()));
}
#[test]
fn kind_priority_beats_match_position_and_name() {
    for kind in [
        "dependency",
        "connection",
        "binding",
        "succession",
        "flow",
        "message",
        "allocation",
        "satisfy",
        "verify",
        "comment",
        "metadata",
        "value",
        "slot",
        "instance",
        "enum value",
        "expression",
        "transition",
    ] {
        let mut m = model(&["Fuel 1", "Z Fuel 10"]);
        m.elements[0].kind = kind.into();
        assert_eq!(names(&m, &opts("*Fuel")), ["Z Fuel 10", "Fuel 1"], "{kind}");
    }
}
#[test]
fn contains_ranks_each_piece_and_missing_literals_last() {
    let m = model(&["XFuel", "ZZFuel", "Fuel 10", "Fuel 2"]);
    assert_eq!(names(&m, &opts("*fuel")), ["Fuel 2", "Fuel 10", "XFuel", "ZZFuel"]);
    let m = model(&["AxxB", "AxB", "AB"]);
    assert_eq!(names(&m, &opts("*a*b")), ["AB", "AxB", "AxxB"]);
    let m = model(&["FuelRail", "XFR"]);
    assert_eq!(names(&m, &opts("*FR")), ["XFR", "FuelRail"]);
}
#[test]
fn natural_order_symbols_and_empty_last() {
    let mut m = model(&["Req 10", "Req 2", "(A", "Zed", "Empty", "  +A"]);
    m.elements[4].name = Some(String::new());
    assert_eq!(names(&m, &opts("")), ["Req 2", "Req 10", "Zed", "  +A", "(A", ""]);
}
#[test]
fn extensions_scopes_paging_ids_paths_and_initials() {
    let mut m = model(&["Flight Control System", "Wanted", "ID9 other"]);
    m.elements[1].short_name = Some("ID9".into());
    assert_eq!(names(&m, &opts("id9")), ["Wanted", "ID9 other"]);
    let mut o = opts("fcs");
    o.use_camel_case = false;
    assert_eq!(names(&m, &o), ["Flight Control System"]);
    m.elements[1].qualified_name = "P::Wanted".into();
    assert_eq!(names(&m, &opts("P::Want")), ["Wanted"]);
    o = opts("*");
    o.scope = "type".into();
    o.limit = 1;
    let r = result(&m, &o);
    assert_eq!(r["total"], 3);
    assert_eq!(r["more"], true);
    assert_eq!(r["next_offset"], 1);
    o.offset = 1;
    assert_ne!(result(&m, &o)["hits"][0], r["hits"][0]);
    o.scope = "bad".into();
    assert!(find(&m, &o).is_err());
    o = opts(&"x".repeat(201));
    assert!(find(&m, &o).is_err());
}
#[test]
fn tool_envelope_validates_and_applies_options_for_client_and_agent() {
    use serde_json::json;
    use sysml_core::agent::tools::{call_offline, MemoryCtx};
    let ctx = MemoryCtx { files: vec![FileIn { path: "m.sysml".into(), text: "part def FuelRail2; part def 'Main Fuel';".into() }], other: None };
    let call = |a: Value| {
        let r = call_offline(&ctx, "find", &a);
        assert_ne!(r["isError"], true, "{r}");
        r["structuredContent"].clone()
    };
    assert_eq!(call(json!({"text":"fuel"}))["total"], 1);
    assert_eq!(call(json!({"text":"fuel", "match_anywhere":true}))["total"], 2);
    assert_eq!(call(json!({"text":"FR2"}))["total"], 1);
    assert_eq!(call(json!({"text":"FR2", "use_camel_case":false}))["total"], 0);
    assert_eq!(call_offline(&ctx, "find", &json!({"text":"fuel", "match_anywhere":"yes"}))["isError"], true);
}

#[test]
fn hump_lowercase_start_and_unicode_question_mark() {
    let m = model(&["fuelRail2", "FuelRail2", "XfuelRail2"]);
    assert_eq!(names(&m, &opts("fR2")), ["fuelRail2"]);
    let m = model(&["AéB", "A猫B", "AB", "AééB"]);
    assert_eq!(names(&m, &opts("A?B")), ["AéB", "A猫B"]);
}
