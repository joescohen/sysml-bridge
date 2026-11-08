use serde_json::Value;
use sysml_core::agent::{self, rules, trace};
use sysml_core::api::{self, FileIn};

fn model(text: &str) -> sysml_core::Model {
    let mut m = api::to_model(vec![FileIn {
        path: "coverage_owner.sysml".into(),
        text: text.into(),
    }]);
    agent::add_namespace_checks(&mut m);
    m
}

fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["ref"] == name)
        .unwrap_or_else(|| panic!("missing requirement {name} in {report}"))
}

fn has_gap(row: &Value, gap: &str) -> bool {
    row["gaps"].as_array().unwrap().iter().any(|g| g == gap)
}

#[test]
fn grouping_inherits_from_an_owner_and_requirement_mode_does_not() {
    let m = model(
        "package P {
        part def Part;
        requirement parent { requirement child; }
        requirement derivedReq;
        satisfy parent by Part;
        #SysMLv1::Verify dependency from Part to parent;
        #SysMLv1::Refine dependency from Part to parent;
        #SysMLv1::DeriveReqt dependency from derivedReq to parent;
    }",
    );
    let direct =
        trace::coverage_report_with_owner(&m, None, trace::CoverageOwner::Requirement).unwrap();
    let grouped =
        trace::coverage_report_with_owner(&m, None, trace::CoverageOwner::Grouping).unwrap();
    for gap in ["not satisfied", "not verified", "not refined"] {
        assert!(has_gap(row(&direct, "P::parent::child"), gap), "{direct}");
        assert!(
            !has_gap(row(&grouped, "P::parent::child"), gap),
            "{grouped}"
        );
    }
    assert!(
        !has_gap(row(&grouped, "P::parent::child"), "not derived"),
        "{grouped}"
    );
    assert_eq!(
        row(&grouped, "P::parent")["owner_coverage"]["derivation"]["direct"],
        true
    );
    assert_eq!(
        row(&grouped, "P::derivedReq")["owner_coverage"]["derivation"]["direct"],
        false
    );
    assert!(row(&direct, "P::parent")["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g != "not derived"));
}

#[test]
fn grouping_requires_every_child_subtree_when_parent_has_no_link() {
    let text = "package P {
        part def Part;
        requirement parent { requirement a; requirement b; }
        satisfy parent::a by Part;
        #SysMLv1::Verify dependency from Part to parent::a;
        #SysMLv1::Refine dependency from Part to parent::a;
        #SysMLv1::DeriveReqt dependency from parent::b to parent::a;
    }";
    let one = trace::coverage_report_with_owner(&model(text), None, trace::CoverageOwner::Grouping)
        .unwrap();
    for gap in [
        "not satisfied",
        "not verified",
        "not refined",
        "not derived",
    ] {
        assert!(has_gap(row(&one, "P::parent"), gap), "{one}");
    }
    let both = text.replace("#SysMLv1::DeriveReqt dependency from parent::b to parent::a;", "#SysMLv1::DeriveReqt dependency from parent::b to parent::a;\n        satisfy parent::b by Part;\n        #SysMLv1::Verify dependency from Part to parent::b;\n        #SysMLv1::Refine dependency from Part to parent::b;\n        #SysMLv1::DeriveReqt dependency from parent::a to parent::b;");
    let all =
        trace::coverage_report_with_owner(&model(&both), None, trace::CoverageOwner::Grouping)
            .unwrap();
    assert!(
        row(&all, "P::parent")["gaps"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{all}"
    );
    assert!(
        !row(&all, "P::parent")["owner_coverage"]["satisfaction"]["direct"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn nested_uncovered_subtree_stays_uncovered_without_recursion() {
    let m = model(
        "package P {
        requirement top {
            requirement left { requirement leaf; }
            requirement right;
        }
    }",
    );
    let report =
        trace::coverage_report_with_owner(&m, None, trace::CoverageOwner::Grouping).unwrap();
    for name in [
        "P::top",
        "P::top::left",
        "P::top::left::leaf",
        "P::top::right",
    ] {
        assert!(
            has_gap(row(&report, name), "not satisfied"),
            "{name}: {report}"
        );
    }
}

#[test]
fn scoped_report_keeps_ancestor_links_and_rolls_up_multiple_levels() {
    let m = model(
        "package P {
        part def Part;
        requirement top {
            requirement left { requirement leaf; }
            requirement right;
        }
        satisfy top::left::leaf by Part;
        satisfy top::right by Part;
    }",
    );
    let full = trace::coverage_report_with_owner(&m, None, trace::CoverageOwner::Grouping).unwrap();
    assert!(!has_gap(row(&full, "P::top"), "not satisfied"), "{full}");
    let inherited = model(
        "package P {
        part def Part;
        requirement top { requirement child; }
        satisfy top by Part;
    }",
    );
    let scoped = trace::coverage_report_with_owner(
        &inherited,
        Some("P::top::child"),
        trace::CoverageOwner::Grouping,
    )
    .unwrap();
    assert_eq!(scoped["totals"]["requirements"], 1);
    assert!(
        !has_gap(row(&scoped, "P::top::child"), "not satisfied"),
        "{scoped}"
    );
}

#[test]
fn default_report_and_gate_keep_their_original_checks() {
    let m = model("package P { requirement only; }");
    let default = trace::coverage_report(&m, None).unwrap();
    assert_eq!(
        row(&default, "P::only")["gaps"],
        serde_json::json!(["not satisfied", "not verified"])
    );
    assert!(default.get("coverage_owner").is_none());
    let existing = rules::run_gates(&m, &["coverage"], None, None, &["satisfy", "verify"]);
    assert_eq!(existing.findings.len(), 2);
    let requirement = rules::run_gates_with_owner(
        &m,
        &["coverage"],
        None,
        None,
        &["satisfy", "verify"],
        Some(trace::CoverageOwner::Requirement),
    );
    assert_eq!(requirement.findings.len(), 3);
    let grouping = rules::run_gates_with_owner(
        &m,
        &["coverage"],
        None,
        None,
        &["satisfy", "verify"],
        Some(trace::CoverageOwner::Grouping),
    );
    assert_eq!(grouping.findings.len(), 4);
    let selected = rules::check_all_with_owner(
        &m,
        Some(&["coverage".into()]),
        None,
        Some(trace::CoverageOwner::Grouping),
    )
    .unwrap();
    assert_eq!(selected["coverage_owner"], "grouping");
    assert_eq!(selected["gates"]["coverage"]["warnings"], 4);
    assert!(trace::CoverageOwner::parse(Some("group")).is_err());
}

#[test]
fn optional_rule_fixtures_have_a_known_bad_and_a_clean_pair() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/rule_fixtures");
    for rule in ["coverage-not-refined", "coverage-not-derived"] {
        let bad = std::fs::read_to_string(dir.join(format!("{rule}.sysml"))).unwrap();
        let good =
            std::fs::read_to_string(dir.join("passing").join(format!("{rule}.sysml"))).unwrap();
        let findings = |text: &str| {
            rules::run_gates_with_owner(
                &model(text),
                &["coverage"],
                None,
                None,
                &["satisfy", "verify"],
                Some(trace::CoverageOwner::Grouping),
            )
            .findings
        };
        let failures = findings(&bad);
        assert!(
            failures.iter().any(|f| f["rule"] == rule),
            "{rule}: {failures:?}"
        );
        assert!(
            findings(&good).is_empty(),
            "{rule} passing fixture has coverage findings"
        );
    }
}

#[test]
fn tool_calls_select_owner_mode_and_reject_unknown_modes() {
    use sysml_core::agent::tools::{self, Ctx};
    struct Context;
    impl Ctx for Context {
        fn model(&self) -> std::sync::Arc<sysml_core::Model> {
            std::sync::Arc::new(model("package P { requirement parent { requirement child; } }"))
        }
        fn files(&self) -> Vec<FileIn> {
            vec![]
        }
        fn all_files(&self) -> Vec<FileIn> {
            vec![]
        }
        fn ids(&self) -> std::collections::BTreeMap<String, String> {
            Default::default()
        }
        fn read_file(&self, _: &str) -> Result<String, String> {
            Err("no files".into())
        }
    }
    for (tool, mut args) in [
        ("coverage_report", serde_json::json!({})),
        ("validate_model", serde_json::json!({"gate": "coverage"})),
        ("check_all", serde_json::json!({"gates": ["coverage"]})),
    ] {
        let default = tools::call_offline(&Context, tool, &args);
        assert_eq!(default["isError"], false, "{default}");
        assert!(default["structuredContent"].get("coverage_owner").is_none());
        args["coverage_owner"] = serde_json::json!("grouping");
        let selected = tools::call_offline(&Context, tool, &args);
        assert_eq!(selected["isError"], false, "{selected}");
        assert_eq!(selected["structuredContent"]["coverage_owner"], "grouping");
        for bad in [serde_json::json!("bad"), serde_json::json!(42)] {
            args["coverage_owner"] = bad;
            assert_eq!(tools::call_offline(&Context, tool, &args)["isError"], true);
        }
    }
}
