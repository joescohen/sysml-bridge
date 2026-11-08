//! The rule registry and the gate runner: every rule id of every validation gate, one runner for
//! `validate_model` and `check_all`, and the report text. A rule cannot be added without a row here, and the
//! ratchet test (`tests/rule_fixtures.rs`) fails for a row that has no known-bad fixture.

use super::{connect, graph, trace, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const GATES: [&str; 7] = ["syntax", "semantic", "traceability", "connections", "coverage", "official", "spec"];
/// The gates `validate_model` runs when it is given no gate. The connections, coverage, official and spec gates are opt-in; `check_all` runs every gate.
pub const DEFAULT_GATES: [&str; 3] = ["syntax", "semantic", "traceability"];

#[derive(Debug, Clone)]
pub struct RuleInfo {
    pub id: String,
    pub gate: &'static str,
    pub severity: &'static str,
    pub group: String,
    pub meaning: String,
}

fn row(gate: &'static str, id: &str, severity: &'static str, group: &str, meaning: &str) -> RuleInfo {
    RuleInfo { id: id.into(), gate, severity, group: group.into(), meaning: meaning.into() }
}

/// Every rule of every gate.
pub fn registry() -> Vec<RuleInfo> {
    let mut v = vec![
        row("syntax", "syntax", "error", "Syntax", "The text does not parse, or uses a word or prefix order SysML v2 does not allow"),
        row("semantic", "requirement-operand-definition", "error", "Semantic", "The requirement of a satisfy or verify is a requirement def, not a usage"),
        row("semantic", "trace-operand-definition", "warning", "Semantic", "A satisfy, verify or allocate operand is a definition, not a usage"),
        row("semantic", "verify-outside-objective", "error", "Semantic", "A verify is not in the objective of a verification case"),
        row("semantic", "connection-between-definitions", "error", "Semantic", "A connect, flow or bind end is a definition"),
        row("semantic", "relationship-end-kind", "error", "Relationship ends", "The supplier of a satisfy, verify or derive is not a requirement, the client of a derive is not a requirement, or a connection end is not a property, port or parameter (only the ends Cameo restricts)"),
        row("semantic", "relationship-end-unusual", "warning", "Relationship ends", "An end the language accepts but this checker flags: a satisfy by something other than a part, a function, a constraint or an interface; an unusual allocate or flow end; a refine with no requirement at either end"),
        row("semantic", "connection-end-not-port", "warning", "Relationship ends", "An end of a connection is a part or a value, not a port"),
        row("semantic", "usage-definition-kind-mismatch", "warning", "Semantic", "A structural usage is typed by a behavior definition, or the reverse"),
        row("semantic", "unresolved-reference", "error", "Semantic", "A name does not resolve to any element"),
        row("semantic", "unresolved-unit", "error", "Semantic", "A unit in brackets (12 [V]) does not resolve: no import of SI or USCustomaryUnits is in scope"),
        row("semantic", "value-type-mismatch", "warning", "Semantic", "A value does not fit the enum def that types the feature"),
        row("semantic", "duplicate-name", "warning", "Semantic", "Two members of one namespace share a name"),
        row("semantic", "duplicate-definition", "warning", "One model", "Two definitions of one kind whose names match once case, spaces and punctuation are dropped, or that share a short id, neither specializing the other: one concept modelled twice. A write that adds one is refused"),
        row("semantic", "duplicate-usage", "warning", "One model", "Two usages of one kind in one body whose names match once case, spaces and punctuation are dropped: one element shown twice. A write that adds one is refused"),
        row("semantic", "similar-element", "info", "One model", "One intent under two names or owners: names whose words one holds all of the other's but one, definitions with the same members by kind, type and value, or actions and requirements whose texts share most words. Merge them, or say how they differ; a write that makes one asks whether to use the existing element"),
        row("semantic", "copied-usage", "warning", "One model", "A perform that declares another usage of an action definition another body already uses: one function shown as two elements. A write that adds one is refused"),
        row("semantic", "copied-definition", "warning", "One model", "Two definitions of one kind with other names and the same members (name, kind, type and value each): a definition copied under another name. A write that adds one is refused"),
        row("semantic", "view-copy", "warning", "One model", "A definition that repeats part of another (two or more of its members by name, kind and type, or one when its name says view, IBD, functional or the like; nothing of its own) with ports or parts among them, under its name with view words only added (`PumpFunctional` beside `Pump`): a block copied so a diagram shows fewer ports. Draw the one block and hide the ports on that diagram. A write that adds one is refused"),
        row("semantic", "duplicate-requirement", "warning", "One model", "Two requirements with one text, neither typed by the other: one requirement written twice. A write that adds one is refused"),
        row("semantic", "duplicate-requirement-id", "error", "Requirement ids", "Two requirements have the same id"),
        row("semantic", "duplicate-id", "error", "Element ids", "Two elements have the same minted id (short name): a copy pasted with its id. A tool write that adds one gives the copy a new id in the same write, with a note; mint_ids {ref} does the same for a copy made by hand"),
        row("semantic", "broken-text-reference", "warning", "Text references", "A reference in text names an element that is gone"),
        row("semantic", "requirement-without-source", "info", "Provenance", "A generated requirement has a provenance block with no SOURCE line (an error with require: source)"),
        row("semantic", "reserved-name-hides-marker", "warning", "Semantic", "A member named start or done hides the built-in start or done of its behavior"),
        row("semantic", "redefines-not-inherited", "error", "Relationship ends", "A redefined feature is not a feature of a general type of the redefining feature's owner (UML redefinedProperty must be inherited)"),
        row("semantic", "conjugated-type-not-port", "error", "Relationship ends", "A ~ (conjugated) type is written on a feature that is not a port, or names a type that is not a port def"),
        row("traceability", "requirement-not-satisfied", "error", "Traceability", "No satisfy reaches the requirement"),
        row("traceability", "requirement-not-verified", "error", "Traceability", "No verification case verifies the requirement"),
        row("traceability", "component-without-job", "info", "Traceability", "A leaf part performs no function, is allocated nothing and satisfies no requirement"),
        row("traceability", "dangling-trace-end", "error", "Traceability", "A trace statement names an element that does not exist"),
        row("connections", "unresolved-end", "error", "Connections", "An end of a connection does not resolve"),
        row("connections", "connection-between-definitions", "error", "Connections", "An end of a connection is a definition"),
        row("connections", "unreachable-ends", "warning", "Connections", "An end is written through a definition, so the owner cannot reach it"),
        row("connections", "connector-crosses-boundary", "warning", "Connections", "An end reaches a part inside another part from outside it through no port of that part: diagrams draw a crossing point on the border (abstraction rule A2)"),
        row("connections", "flow-item-not-on-port", "warning", "Connections", "A flow carries an item its end ports cannot carry: it may sit on the wrong connector, between the wrong parts (an ICD item on another ICD's link)"),
        row("connections", "same-direction-ports", "error", "Connections", "Both connected ports send, or both receive"),
        row("connections", "port-type-mismatch", "error", "Connections", "The two connected ports are typed by unrelated port defs"),
        row("coverage", "coverage-not-satisfied", "warning", "Coverage", "UnsatisfiedReq: a requirement usage is satisfied by nothing (optional Cameo owner interpretation)"),
        row("coverage", "coverage-not-verified", "warning", "Coverage", "UnverifiedReq: a requirement usage is verified by nothing (optional Cameo owner interpretation)"),
        row("coverage", "coverage-not-refined", "warning", "Coverage", "UnrefinedReq: in Cameo owner mode, a requirement has no incoming refine link of its own or through grouping"),
        row("coverage", "coverage-not-derived", "warning", "Coverage", "UnderivedReq: in Cameo grouping mode, no deriveReqt reaches this supplier from a derived requirement, directly or through grouping"),
    ];
    for r in super::ownership::RULES {
        v.push(row("semantic", r.id, r.severity, "Ownership", r.meaning));
    }
    for (id, group, text) in [
        ("spec-variation-member", "Variations", "A variation (or an enum def) owns only its variants (validate...VariationOwnedFeatureMembership)"),
        ("spec-variation-specialization", "Variations", "A variation does not specialize another variation (validate...VariationSpecialization)"),
        ("spec-specialization-kind", "Definitions", "A definition specializes only definitions of a compatible kind: data types, classes, associations, structures and behaviors (validateDataTypeSpecialization, validateClassSpecialization, validateStructureSpecialization, validateBehaviorSpecialization)"),
        ("spec-attribute-composite", "Definitions", "An attribute owns no composite occurrence (validateAttributeDefinitionFeatures, validateAttributeUsageFeatures)"),
        ("spec-part-definition", "Definitions", "A part is typed by at least one part definition (validatePartUsagePartDefinition)"),
        ("spec-individual-definition", "Definitions", "An individual usage has an individual definition (validateOccurrenceUsageIndividualUsage)"),
        ("spec-state-subaction-kind", "States", "A state has at most one entry, one do and one exit (validate...StateSubactionKind)"),
        ("spec-parallel-state-transition", "States", "No transition runs between the substates of a parallel state (validate...ParallelSubactions)"),
        ("spec-control-node-successions", "Activities", "A decide or fork node has at most one incoming succession; a join or merge node at most one outgoing (validate...NodeIncomingSuccessions, validate...NodeOutgoingSuccessions)"),
        ("spec-membership-owner", "Requirements and cases", "Subject, actor, stakeholder, objective, require/assume/frame, entry/do/exit, expose and render members stand only in owners of the kind the specification allows (validate...MembershipOwningType, validateExposeOwningNamespace)"),
        ("spec-one-subject", "Requirements and cases", "A requirement or a case has at most one subject (validate...OnlyOneSubject)"),
        ("spec-subject-first", "Requirements and cases", "The subject is the first input of a requirement or a case (validate...SubjectParameterPosition)"),
        ("spec-one-objective", "Requirements and cases", "A case has at most one objective (validate...OnlyOneObjective)"),
        ("spec-one-render", "Views", "A view has at most one rendering (validate...OnlyOneViewRendering)"),
        ("spec-flow-def-ends", "Features", "A flow def has at most two ends (validateFlowDefinitionFlowEnds)"),
        ("spec-feature-value-overriding", "Features", "A value is given only to a feature whose redefined features have default values, not bound values (validateFeatureValueOverriding)"),
        ("spec-port-composite", "Features", "A port owns only referential usages (directed, ref or end) and nested ports (validatePortDefinitionOwnedUsagesNotComposite, validatePortUsageNestedUsagesNotComposite)"),
        ("spec-multiplicity-bounds", "Features", "A multiplicity range has its lower bound no greater than its upper bound (validateMultiplicityRangeBounds)"),
        ("spec-top-level-import", "Imports", "An import outside any package is private (validateImportTopLevelVisibility)"),
    ] {
        v.push(row("spec", id, "error", group, text));
    }
    v.push(row("official", OFFICIAL_RULE, "error", "Official grammar", "A file does not follow the official OMG SysML v2 textual grammar (the recogniser of sysml-conform)"));
    for (id, meaning) in super::grammar::RULES {
        v.push(row("syntax", id, "error", "Syntax", meaning));
    }
    v
}

/// The rule id of a file the official grammar rejects. A file the checker could not read to the end gets a note
/// (`graph::CUT_RULE`, a warning), never an error.
pub const OFFICIAL_RULE: &str = "official";
/// How many of the tokens the grammar allowed a finding lists.
const ALLOWED_SHOWN: usize = 12;

/// What the official-grammar recogniser said about one file.
#[derive(Debug, Clone, PartialEq)]
pub enum OfficialVerdict {
    Valid,
    Invalid { line: u32, column: u32, found: String, found_kind: String, allowed: Vec<String> },
    /// The checker could not read the file to the end (a text it reads in more than linear time). Neither a pass nor a
    /// fault: a note.
    NotChecked(String),
}

/// The recogniser: the text of one file; it answers with the verdict and the steps it took. The recogniser bounds the
/// work on one text in proportion to the size of the text; there is no budget shared by the files of a run and no
/// limit on their total size.
pub type OfficialFn = fn(&str) -> (OfficialVerdict, u64);
static OFFICIAL: std::sync::OnceLock<OfficialFn> = std::sync::OnceLock::new();

/// Register the recogniser of the official grammar. The server does this at start; the core does not depend on it, so the
/// wasm build stays free of it. Only the first call counts.
pub fn set_official_checker(f: OfficialFn) {
    let _ = OFFICIAL.set(f);
}

/// The registered recogniser, if there is one.
pub fn official_checker() -> Option<OfficialFn> {
    OFFICIAL.get().copied()
}

/// The note for a file the official grammar could not be checked against: a warning, so the gate still passes.
fn official_not_checked(path: &str, why: &str) -> Value {
    json!({
        "gate": "official", "rule": graph::CUT_RULE, "severity": "warning", "element": null, "file": path, "line": null,
        "message": format!("not checked: {path} was not checked against the official grammar: {why}."),
        "fix": "Nothing is required. To have it checked, write the long expression it holds in shorter parts.",
    })
}

/// The finding for a file the official grammar rejects.
fn official_invalid(path: &str, line: u32, column: u32, found: String, found_kind: String, allowed: Vec<String>) -> Value {
    let n = allowed.len();
    let mut shown: Vec<String> = allowed.into_iter().take(ALLOWED_SHOWN).collect();
    if n > ALLOWED_SHOWN {
        shown.push(format!("... {} more", n - ALLOWED_SHOWN));
    }
    json!({
        "gate": "official", "rule": OFFICIAL_RULE, "severity": "error", "element": null, "file": path, "line": line, "column": column,
        "found": found, "found_kind": found_kind, "allowed": shown,
        "message": format!("{path}:{line}:{column}: the official grammar does not allow {} '{}' here; it allows {}.", found_kind, found, if shown.is_empty() { "nothing".to_string() } else { shown.join(" ") }),
        "fix": if found_kind == "DECIMAL_VALUE" {
            "A name that starts with a digit is written in quotes in the file ('5out'); every write through the engine adds the quotes for you."
        } else {
            "Rewrite that statement in the form the grammar allows (see 'allowed'), or write it with create_element so the tool picks the form."
        },
    })
}

/// The findings of the official gate: one error per file that the grammar rejects, and one note (a warning) per file
/// the checker could not read to the end. Every file is checked, whatever the size of the model.
pub fn official_findings_with(model: &Model, check: &dyn Fn(&str) -> (OfficialVerdict, u64)) -> Vec<Value> {
    let mut out = Vec::new();
    for f in &model.files {
        match check(&f.text).0 {
            OfficialVerdict::Valid => {}
            OfficialVerdict::NotChecked(why) => out.push(official_not_checked(&f.path, &why)),
            OfficialVerdict::Invalid { line, column, found, found_kind, allowed } => out.push(official_invalid(&f.path, line, column, found, found_kind, allowed)),
        }
    }
    out
}

/// One file a write changes: its path, the text it had (`None` for a new file) and the text the write gives it.
pub struct WrittenFile<'a> {
    pub path: &'a str,
    pub before: Option<&'a str>,
    pub after: &'a str,
}

/// The official grammar on the files a write changes, as a report: the write is never refused. Each changed file is read
/// once, in its new text. Returns one note for each written file that is not valid: `{file, official: "invalid", line,
/// column, found, found_kind, allowed, why}` for a fault (a parse fault: the reply lists it first) and `{file, official:
/// "not checked", why}` for a file the checker could not read to the end.
/// Without a registered recogniser (the wasm build) nothing is checked.
pub fn official_write_notes(files: &[WrittenFile]) -> Vec<Value> {
    let Some(check) = official_checker() else { return Vec::new() };
    official_write_notes_with(files, &check)
}

/// `official_write_notes` with the recogniser given by the caller.
pub fn official_write_notes_with(files: &[WrittenFile], check: &dyn Fn(&str) -> (OfficialVerdict, u64)) -> Vec<Value> {
    let mut notes = Vec::new();
    for f in files {
        if f.before == Some(f.after) {
            continue;
        }
        match check(f.after).0 {
            OfficialVerdict::Valid => {}
            OfficialVerdict::NotChecked(why) => notes.push(json!({ "file": f.path, "official": "not checked", "why": why })),
            OfficialVerdict::Invalid { line, column, found, found_kind, allowed } => {
                let shown: Vec<String> = allowed.into_iter().take(ALLOWED_SHOWN).collect();
                let why = format!(
                    "{}:{line}:{column}: the official grammar does not allow {found_kind} {} here; it allows {}. Fix: write that statement in a form the grammar allows (create_element and create_relationship pick the form)",
                    f.path,
                    crate::agent::text::quote_clip(&found),
                    if shown.is_empty() { "nothing".to_string() } else { shown.join(" ") }
                );
                notes.push(json!({ "file": f.path, "official": "invalid", "rule": OFFICIAL_RULE, "line": line, "column": column, "found": found, "found_kind": found_kind, "allowed": shown, "why": why }));
            }
        }
    }
    notes
}

fn official_findings(model: &Model) -> Vec<Value> {
    match official_checker() {
        Some(f) => official_findings_with(model, &f),
        None => vec![json!({
            "gate": "official", "rule": graph::CUT_RULE, "severity": "warning", "element": null, "file": null, "line": null,
            "message": "not checked: this build has no checker for the official grammar, so no file was checked against it.",
            "fix": "Use a build that holds the checker for the official grammar.",
        })],
    }
}

fn slug(s: &str) -> String {
    s.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// `list_rules`: every rule, optionally for one gate and one group.
pub fn list_rules(gate: Option<&str>, group: Option<&str>) -> Res<Value> {
    if let Some(g) = gate {
        if !GATES.contains(&g) {
            return Err(format!("unknown gate '{g}'. Use one of: {}.", GATES.join(", ")).into());
        }
    }
    let rules: Vec<Value> = registry()
        .into_iter()
        .filter(|r| gate.map(|g| g == r.gate).unwrap_or(true) && group.map(|g| slug(g) == slug(&r.group)).unwrap_or(true))
        .map(|r| json!({ "id": r.id, "gate": r.gate, "severity": r.severity, "group": r.group, "meaning": r.meaning }))
        .collect();
    if rules.is_empty() {
        return Err("no rule matches that gate and group. Call list_rules without arguments to see them all.".into());
    }
    Ok(json!({ "count": rules.len(), "rules": rules }))
}

pub struct Selection<'a> {
    /// Gates to run; `None` for the default of the caller.
    pub gates: Option<Vec<String>>,
    pub rules: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub require: Option<&'a [String]>,
}

pub fn parse_require(require: Option<&[String]>) -> Res<Vec<&'static str>> {
    let mut required: Vec<&'static str> = Vec::new();
    for r in require.unwrap_or(&[]) {
        match r.trim() {
            "satisfy" => required.push("satisfy"),
            "verify" => required.push("verify"),
            "source" => required.push("source"),
            other => return Err(format!("unknown require entry '{other}'. Fix: use satisfy, verify or source, for example require: [\"satisfy\"].").into()),
        }
    }
    if require.map(|r| r.is_empty()).unwrap_or(false) {
        return Err("require is empty. Fix: name what the gate demands: [\"satisfy\"], [\"verify\"] or [\"satisfy\", \"verify\"]; omit it for both.".into());
    }
    required.sort();
    required.dedup();
    // `source` adds to what the traceability gate demands; satisfy and verify are both demanded unless one is named.
    if !required.iter().any(|r| *r == "satisfy" || *r == "verify") {
        required.extend(["satisfy", "verify"]);
        required.sort();
    }
    Ok(required)
}

fn coverage_findings(model: &Model, owner: Option<trace::CoverageOwner>) -> Vec<Value> {
    let rep = match owner {
        Some(owner) => trace::coverage_report_with_owner(model, None, owner),
        None => trace::coverage_report(model, None),
    }.unwrap_or(Value::Null);
    let mut out = Vec::new();
    if let Some(m) = rep["not_checked"].as_str() {
        out.push(json!({ "gate": "coverage", "rule": graph::CUT_RULE, "severity": "error", "message": m, "fix": "Split the very large or repeated statements so the model states fewer than 100000 relationship pairs." }));
    }
    for r in rep["requirements"].as_array().into_iter().flatten() {
        let mut checks = vec![("not satisfied", "coverage-not-satisfied", "create_relationship {kind: 'satisfy', source: <part usage>, target: <requirement>}"), ("not verified", "coverage-not-verified", "create_relationship {kind: 'verify', source: <verification case>, target: <requirement>}")];
        if owner.is_some() {
            checks.push(("not refined", "coverage-not-refined", "create_relationship {kind: 'refine', source: <refining element>, target: <requirement>}"));
        }
        if owner == Some(trace::CoverageOwner::Grouping) {
            checks.push(("not derived", "coverage-not-derived", "create_relationship {kind: 'derive', source: <derived requirement>, target: <requirement>}"));
        }
        for (gap, rule, fix) in checks {
            if r["gaps"].as_array().map(|g| g.iter().any(|x| x == gap)).unwrap_or(false) {
                out.push(json!({
                    "gate": "coverage", "rule": rule, "severity": "warning",
                    "element": r["ref"], "file": r["file"], "line": r["line"],
                    "message": format!("requirement {} is {}.", r["ref"].as_str().unwrap_or(""), gap),
                    "fix": fix,
                }));
            }
        }
    }
    out
}

fn connection_findings(model: &Model) -> Vec<Value> {
    let (_, mut f) = connect::check_in(model, None);
    for x in f.iter_mut() {
        x["gate"] = json!("connections");
    }
    f
}

/// The result of running gates: per-gate counts and all findings.
pub struct GateRun {
    pub gates: serde_json::Map<String, Value>,
    pub findings: Vec<Value>,
    pub cut: Vec<(String, usize)>,
    pub ran: Vec<&'static str>,
}

/// Run `gates` (in the order of `GATES`), keeping findings that pass the `rules` and `groups` filters.
pub fn run_gates(model: &Model, gates: &[&str], rules: Option<&HashSet<String>>, groups: Option<&HashSet<String>>, required: &[&str]) -> GateRun {
    run_gates_with_owner(model, gates, rules, groups, required, None)
}

pub fn run_gates_with_owner(model: &Model, gates: &[&str], rules: Option<&HashSet<String>>, groups: Option<&HashSet<String>>, required: &[&str], owner: Option<trace::CoverageOwner>) -> GateRun {
    let reg = registry();
    let group_of = |gate: &str, rule: &str| reg.iter().find(|r| r.gate == gate && r.id == rule).map(|r| slug(&r.group));
    let keep = |f: &Value| -> bool {
        let (g, r) = (f["gate"].as_str().unwrap_or(""), f["rule"].as_str().unwrap_or(""));
        r == graph::CUT_RULE || rules.map(|s| s.contains(r)).unwrap_or(true) && groups.map(|s| group_of(g, r).map(|x| s.contains(&x)).unwrap_or(false)).unwrap_or(true)
    };
    let mut run = GateRun { gates: serde_json::Map::new(), findings: Vec::new(), cut: Vec::new(), ran: Vec::new() };
    for g in GATES.iter().filter(|g| gates.contains(g)) {
        let mut f: Vec<Value> = match *g {
            "syntax" => trace::syntax_findings(model),
            "semantic" => trace::semantic_findings(model),
            "traceability" => trace::traceability_findings(model, required.contains(&"satisfy"), required.contains(&"verify")),
            "connections" => connection_findings(model),
            "coverage" => coverage_findings(model, owner),
            "official" => official_findings(model),
            "spec" => super::spec::findings(model),
            _ => Vec::new(),
        };
        if *g == "semantic" && required.contains(&"source") {
            trace::upgrade_source(&mut f);
        }
        f.retain(|x| keep(x));
        f.extend(graph::unread_findings(model, g));
        let count = |sev: &str| f.iter().filter(|x| x["severity"] == sev).count();
        let errors = count("error");
        run.gates.insert(g.to_string(), json!({ "passed": errors == 0, "errors": errors, "warnings": count("warning"), "infos": count("info") }));
        run.findings.extend(f);
        run.ran.push(g);
    }
    run
}

/// Resolve the `rules` and `groups` arguments; an unknown id or group is an error that lists the choices.
pub fn filters(rules: Option<&[String]>, groups: Option<&[String]>) -> Res<(Option<HashSet<String>>, Option<HashSet<String>>)> {
    let reg = registry();
    let rs = match rules {
        None => None,
        Some(l) => {
            let mut set = HashSet::new();
            for r in l {
                let r = r.trim();
                if !reg.iter().any(|x| x.id == r) {
                    return Err(format!("unknown rule id {}. Call list_rules for the ids.", crate::agent::text::quote_clip(r)).into());
                }
                set.insert(r.to_string());
            }
            Some(set)
        }
    };
    let gs = match groups {
        None => None,
        Some(l) => {
            let mut set = HashSet::new();
            for g in l {
                let s = slug(g);
                if !reg.iter().any(|x| slug(&x.group) == s) {
                    let mut all: Vec<String> = reg.iter().map(|x| x.group.clone()).collect();
                    all.sort();
                    all.dedup();
                    return Err(format!("unknown group {}. Groups: {}.", crate::agent::text::quote_clip(g), all.join(", ")).into());
                }
                set.insert(s);
            }
            Some(set)
        }
    };
    Ok((rs, gs))
}

fn by_rule(findings: &[Value]) -> Vec<Value> {
    let mut m: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for f in findings {
        *m.entry((f["gate"].as_str().unwrap_or("").into(), f["rule"].as_str().unwrap_or("").into(), f["severity"].as_str().unwrap_or("").into())).or_default() += 1;
    }
    m.into_iter().map(|((gate, rule, severity), count)| json!({ "gate": gate, "rule": rule, "severity": severity, "count": count })).collect()
}

fn totals(run: &GateRun) -> (usize, usize, usize) {
    let c = |s: &str| run.findings.iter().filter(|x| x["severity"] == s).count();
    (c("error"), c("warning"), c("info"))
}

/// The reply of `validate_model` when a filter is in play.
pub fn validate_with_filters(model: &Model, gate: Option<&str>, sel: &Selection) -> Res<Value> {
    validate_with_filters_owner(model, gate, sel, None)
}

pub fn validate_with_filters_owner(model: &Model, gate: Option<&str>, sel: &Selection, owner: Option<trace::CoverageOwner>) -> Res<Value> {
    let required = parse_require(sel.require)?;
    let (rs, gs) = filters(sel.rules.as_deref(), sel.groups.as_deref())?;
    if let Some(g) = gate {
        if !GATES.contains(&g) {
            return Err(format!("unknown gate '{g}'. Use {}, or omit gate to run syntax, semantic and traceability.", GATES.join(", ")).into());
        }
    }
    let gates: Vec<&str> = match gate {
        Some(g) => vec![g],
        None if rs.is_some() || gs.is_some() => {
            let reg = registry();
            GATES.iter().copied().filter(|g| reg.iter().any(|r| r.gate == *g && rs.as_ref().map(|s| s.contains(&r.id)).unwrap_or(true) && gs.as_ref().map(|s| s.contains(&slug(&r.group))).unwrap_or(true))).collect()
        }
        None => DEFAULT_GATES.to_vec(),
    };
    let run = run_gates_with_owner(model, &gates, rs.as_ref(), gs.as_ref(), &required, owner);
    let (errors, warnings, infos) = totals(&run);
    let mut v = json!({
        "gate": gate.unwrap_or("selected"),
        "passed": errors == 0,
        "errors": errors,
        "warnings": warnings,
        "infos": infos,
        "gates": run.gates,
        "required": required,
        "findings": run.findings,
    });
    if let Some(owner) = owner { v["coverage_owner"] = json!(match owner { trace::CoverageOwner::Requirement => "requirement", trace::CoverageOwner::Grouping => "grouping" }); }
    Ok(v)
}

/// `check_all`: every gate (or the ones named), one report.
pub fn check_all(model: &Model, gates: Option<&[String]>, require: Option<&[String]>) -> Res<Value> {
    check_all_with_owner(model, gates, require, None)
}

pub fn check_all_with_owner(model: &Model, gates: Option<&[String]>, require: Option<&[String]>, owner: Option<trace::CoverageOwner>) -> Res<Value> {
    let required = parse_require(require)?;
    let chosen: Vec<&str> = match gates {
        None => GATES.to_vec(),
        Some(l) => {
            let mut v = Vec::new();
            for g in l {
                let g = g.trim();
                match GATES.iter().find(|x| **x == g) {
                    Some(x) => v.push(*x),
                    None => return Err(format!("unknown gate {}. Use any of: {}.", crate::agent::text::quote_clip(g), GATES.join(", ")).into()),
                }
            }
            if v.is_empty() {
                return Err("gates is empty. Name at least one of: syntax, semantic, traceability, connections, coverage, official, spec; omit it for all.".into());
            }
            v
        }
    };
    let run = run_gates_with_owner(model, &chosen, None, None, &required, owner);
    let (errors, warnings, infos) = totals(&run);
    let mut v = json!({
        "passed": errors == 0,
        "errors": errors,
        "warnings": warnings,
        "infos": infos,
        "gates": run.gates,
        "required": required,
        "by_rule": by_rule(&run.findings),
        "findings": run.findings,
    });
    if let Some(owner) = owner { v["coverage_owner"] = json!(match owner { trace::CoverageOwner::Requirement => "requirement", trace::CoverageOwner::Grouping => "grouping" }); }
    Ok(v)
}

/// The report a person reads: one line per gate, then the findings (at most `limit`).
pub fn report_text(v: &Value, limit: usize) -> String {
    let mut s = format!("{}: {} errors, {} warnings, {} infos\n", if v["passed"] == true { "PASSED" } else { "FAILED" }, v["errors"], v["warnings"], v["infos"]);
    for (g, c) in v["gates"].as_object().into_iter().flatten() {
        s.push_str(&format!("  {:<13} {:<6} {} errors, {} warnings, {} infos\n", g, if c["passed"] == true { "passed" } else { "FAILED" }, c["errors"], c["warnings"], c["infos"]));
    }
    let f = v["findings"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for x in f.iter().take(limit) {
        let loc = match (x["file"].as_str(), x["line"].as_u64()) {
            (Some(p), Some(l)) => format!("{p}:{l}"),
            (Some(p), None) => p.to_string(),
            _ => "-".into(),
        };
        s.push_str(&format!("[{}/{}] {} {} {}: {}", x["gate"].as_str().unwrap_or(""), x["rule"].as_str().unwrap_or(""), x["severity"].as_str().unwrap_or(""), loc, x["element"].as_str().unwrap_or("-"), x["message"].as_str().unwrap_or("")));
        if let Some(d) = x["detail"].as_str() {
            s.push_str(&format!(" ({d})"));
        }
        s.push_str(&format!(" Fix: {}\n", x["fix"].as_str().unwrap_or("")));
    }
    if f.len() > limit {
        s.push_str(&format!("... {} more findings; see structuredContent or --json.\n", f.len() - limit));
    }
    s
}
