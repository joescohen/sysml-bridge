//! Traceability and the validation gates.

use super::graph::{self, Stmt, TRACE_KINDS};
use super::implied;
use super::{line_of, locate, qshow, Res};
use crate::api;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageOwner {
    Requirement,
    Grouping,
}

impl CoverageOwner {
    pub fn parse(value: Option<&str>) -> Res<Option<Self>> {
        match value {
            None => Ok(None),
            Some("requirement") => Ok(Some(Self::Requirement)),
            Some("grouping") => Ok(Some(Self::Grouping)),
            Some(other) => Err(format!("unknown coverage_owner '{other}'. Use requirement or grouping.").into()),
        }
    }

    fn name(self) -> &'static str {
        match self { Self::Requirement => "requirement", Self::Grouping => "grouping" }
    }
}

fn qn(model: &Model, id: ElementId) -> String {
    model.elements[id].qualified_name.clone()
}

fn is_requirement(e: &Element) -> bool {
    e.kind == "requirement"
}

/// Requirements under `scope`, or all of them.
fn requirements(model: &Model, scope: Option<&str>) -> Res<Vec<ElementId>> {
    let within: Option<BTreeSet<ElementId>> = match scope.filter(|s| !s.is_empty()) {
        Some(s) => {
            let e = locate(model, s)?;
            let mut set: BTreeSet<ElementId> = model.descendants(e.id).into_iter().collect();
            set.insert(e.id);
            Some(set)
        }
        None => None,
    };
    Ok(model.elements.iter().filter(|e| is_requirement(e)).filter(|e| within.as_ref().map(|w| w.contains(&e.id)).unwrap_or(true)).map(|e| e.id).collect())
}

fn sorted_refs(model: &Model, ids: impl IntoIterator<Item = ElementId>) -> Vec<String> {
    let set: BTreeSet<String> = ids.into_iter().map(|i| qn(model, i)).collect();
    set.into_iter().collect()
}

/// Ends of statements of `kind` whose other end is `req` (or a usage typed by `req` when it is a definition).
pub(super) fn linked(model: &Model, stmts: &[Stmt], kind: &str, req: ElementId, req_is_target: bool) -> Vec<ElementId> {
    let mut targets = vec![req];
    if model.elements[req].is_def {
        targets.extend(model.elements.iter().filter(|u| !u.is_def && u.refs_of(RefRole::Type).any(|r| r.resolved() == Some(req))).map(|u| u.id));
    }
    stmts
        .iter()
        .filter(|s| s.kind == kind)
        .filter_map(|s| {
            let (mine, other) = if req_is_target { (&s.target, &s.source) } else { (&s.source, &s.target) };
            mine.id.filter(|m| targets.contains(m)).and(other.id)
        })
        .collect()
}

/// The ends of the statements, indexed once: (kind, true for the target end, the element at that end) -> the element at the
/// other end of each statement, in the order of the statements. A requirement usage reads its links here in constant time,
/// and the reports do not read every statement once for each requirement.
struct Links {
    ends: HashMap<(&'static str, bool, ElementId), Vec<ElementId>>,
}

impl Links {
    fn new(stmts: &[Stmt]) -> Links {
        let mut ends: HashMap<(&'static str, bool, ElementId), Vec<ElementId>> = HashMap::new();
        for s in stmts {
            for (at_target, mine, other) in [(true, &s.target, &s.source), (false, &s.source, &s.target)] {
                if let (Some(a), Some(b)) = (mine.id, other.id) {
                    ends.entry((s.kind, at_target, a)).or_default().push(b);
                }
            }
        }
        Links { ends }
    }

    /// What `linked` gives, the same list in the same order.
    fn of(&self, model: &Model, stmts: &[Stmt], kind: &'static str, req: ElementId, req_is_target: bool) -> Vec<ElementId> {
        // A definition is linked through its usages too: read as before.
        if model.elements[req].is_def {
            return linked(model, stmts, kind, req, req_is_target);
        }
        self.ends.get(&(kind, req_is_target, req)).cloned().unwrap_or_default()
    }
}

pub fn coverage_report(model: &Model, scope: Option<&str>) -> Res<Value> {
    // Requirement usages only, like the traceability gate: a definition is a type, and its usages are what get satisfied.
    let mut reqs = requirements(model, scope)?;
    reqs.retain(|r| !model.elements[*r].is_def);
    let (stmts, cuts) = graph::statements_cut(model);
    let links = Links::new(&stmts);
    let linked = |model: &Model, stmts: &[Stmt], kind: &'static str, req: ElementId, req_is_target: bool| links.of(model, stmts, kind, req, req_is_target);
    let mut rows = Vec::new();
    let (mut sat, mut ver, mut both) = (0usize, 0usize, 0usize);
    // Where a satisfy or a verify link ends: a requirement that is not at one of these but reaches one through a part or a
    // type is covered by an implied link.
    let ends_of = |kind: &str| -> BTreeSet<ElementId> { stmts.iter().filter(|s| s.kind == kind).filter_map(|s| s.target.id).collect() };
    let (sat_ends, ver_ends) = (ends_of("satisfy"), ends_of("verify"));
    let (mut sat_implied, mut ver_implied) = (0usize, 0usize);
    for r in &reqs {
        let satisfied_by = sorted_refs(model, linked(model, &stmts, "satisfy", *r, true));
        let verified_by = sorted_refs(model, linked(model, &stmts, "verify", *r, true));
        let mut gaps = Vec::new();
        if satisfied_by.is_empty() {
            gaps.push("not satisfied");
        }
        if verified_by.is_empty() {
            gaps.push("not verified");
        }
        let reach = if satisfied_by.is_empty() || verified_by.is_empty() { implied::reach_set(model, *r) } else { Vec::new() };
        let sat_basis = if !satisfied_by.is_empty() { "direct" } else if reach.iter().any(|x| sat_ends.contains(x)) { "implied" } else { "none" };
        let ver_basis = if !verified_by.is_empty() { "direct" } else if reach.iter().any(|x| ver_ends.contains(x)) { "implied" } else { "none" };
        sat_implied += usize::from(sat_basis == "implied");
        ver_implied += usize::from(ver_basis == "implied");
        sat += usize::from(!satisfied_by.is_empty());
        ver += usize::from(!verified_by.is_empty());
        both += usize::from(gaps.is_empty());
        let e = &model.elements[*r];
        rows.push(json!({
            "ref": e.qualified_name,
            "kind": e.keyword(),
            "file": model.files[e.file].path,
            "line": line_of(model, e),
            "satisfied_by": satisfied_by,
            "satisfied_basis": sat_basis,
            "verified_by": verified_by,
            "verified_basis": ver_basis,
            "derived_from": sorted_refs(model, linked(model, &stmts, "derive", *r, false)),
            "derives": sorted_refs(model, linked(model, &stmts, "derive", *r, true)),
            "refined_by": sorted_refs(model, linked(model, &stmts, "refine", *r, true)),
            "allocated_to": sorted_refs(model, linked(model, &stmts, "allocate", *r, false)),
            "gaps": gaps,
        }));
    }
    let n = reqs.len();
    let pct = |x: usize| if n == 0 { Value::Null } else { json!((x as f64 * 1000.0 / n as f64).round() / 10.0) };
    let mut reply = json!({
        "scope": scope.filter(|s| !s.is_empty()),
        "totals": {
            "requirements": n,
            "satisfied": sat,
            "verified": ver,
            "fully_covered": both,
            "percent_satisfied": pct(sat),
            "percent_verified": pct(ver),
            "percent_covered": pct(both),
            "satisfied_direct": sat,
            "satisfied_implied": sat_implied,
            "verified_direct": ver,
            "verified_implied": ver_implied,
        },
        "basis_rule": implied::RULE,
        "requirements": rows,
    });
    graph::mark_cut(model, &mut reply, &cuts);
    Ok(reply)
}

/// Cameo's optional owner interpretation. The ordinary report remains the default; this
/// variant evaluates matching dependencies of a requirement itself, then applies the owner
/// rule to each of the four validation measures. A derive link covers its supplier (target).
pub fn coverage_report_with_owner(model: &Model, scope: Option<&str>, owner: CoverageOwner) -> Res<Value> {
    let mut reply = coverage_report(model, scope)?;
    let (stmts, _) = graph::statements_cut(model);
    let all: Vec<ElementId> = model.elements.iter().filter(|e| is_requirement(e) && !e.is_def).map(|e| e.id).collect();
    let mut own: Vec<BTreeSet<ElementId>> = vec![BTreeSet::new(); 4];
    for s in &stmts {
        let Some(target) = s.target.id else { continue };
        if !is_requirement(&model.elements[target]) || model.elements[target].is_def || s.source.id.is_none() { continue; }
        let measure = match s.kind { "satisfy" => 0, "verify" => 1, "refine" => 2, "derive" => 3, _ => continue };
        own[measure].insert(target);
    }

    fn ancestor_own(model: &Model, id: ElementId, own: &BTreeSet<ElementId>) -> bool {
        let mut current = Some(id);
        while let Some(at) = current {
            if own.contains(&at) { return true; }
            current = model.elements[at].owner.filter(|parent| is_requirement(&model.elements[*parent]) && !model.elements[*parent].is_def);
        }
        false
    }

    fn covered(model: &Model, id: ElementId, own: &BTreeSet<ElementId>, memo: &mut HashMap<ElementId, bool>) -> bool {
        if let Some(value) = memo.get(&id) { return *value; }
        let is_req = |child: &ElementId| is_requirement(&model.elements[*child]) && !model.elements[*child].is_def;
        let value = ancestor_own(model, id, own) || {
                let mut kids = model.elements[id].children.iter().filter(|child| is_req(child)).peekable();
                kids.peek().is_some() && kids.all(|child| covered(model, *child, own, memo))
            };
        memo.insert(id, value);
        value
    }

    let mut status: Vec<HashMap<ElementId, bool>> = Vec::with_capacity(4);
    for own_set in &own {
        let mut memo = HashMap::new();
        let covered_set = all.iter().map(|id| (*id, if owner == CoverageOwner::Requirement { own_set.contains(id) } else { covered(model, *id, own_set, &mut memo) })).collect();
        status.push(covered_set);
    }
    let gaps = ["not satisfied", "not verified", "not refined", "not derived"];
    let mut counts = [0usize; 4];
    let mut fully = 0usize;
    if let Some(rows) = reply["requirements"].as_array_mut() {
        for row in rows {
            let Some(id) = model.find(row["ref"].as_str().unwrap_or("")).map(|e| e.id) else { continue; };
            let mut missing = Vec::new();
            let mut coverage = serde_json::Map::new();
            for (i, name) in ["satisfaction", "verification", "refinement", "derivation"].iter().enumerate() {
                let is_covered = status[i].get(&id).copied().unwrap_or(false);
                counts[i] += usize::from(is_covered);
                coverage.insert((*name).into(), json!({ "covered": is_covered, "direct": own[i].contains(&id) }));
                if !is_covered && (i != 3 || owner == CoverageOwner::Grouping) { missing.push(gaps[i]); }
            }
            fully += usize::from(missing.is_empty());
            row["gaps"] = json!(missing);
            row["owner_coverage"] = Value::Object(coverage);
        }
    }
    let n = reply["requirements"].as_array().map_or(0, Vec::len);
    let pct = |x: usize| if n == 0 { Value::Null } else { json!((x as f64 * 1000.0 / n as f64).round() / 10.0) };
    reply["coverage_owner"] = json!(owner.name());
    reply["totals"]["satisfied"] = json!(counts[0]);
    reply["totals"]["verified"] = json!(counts[1]);
    reply["totals"]["refined"] = json!(counts[2]);
    if owner == CoverageOwner::Grouping { reply["totals"]["derived"] = json!(counts[3]); }
    reply["totals"]["fully_covered"] = json!(fully);
    reply["totals"]["percent_satisfied"] = pct(counts[0]);
    reply["totals"]["percent_verified"] = pct(counts[1]);
    reply["totals"]["percent_refined"] = pct(counts[2]);
    if owner == CoverageOwner::Grouping { reply["totals"]["percent_derived"] = pct(counts[3]); }
    reply["totals"]["percent_covered"] = pct(fully);
    Ok(reply)
}

pub fn traceability(model: &Model, name: &str, depth: usize) -> Res<Value> {
    let root = locate(model, name)?;
    let (mut edges, mut nodes, cuts) = graph::walk_cut(model, root.id, &|k| TRACE_KINDS.contains(&k) || k == "trace", true, true, depth.clamp(1, 10));
    allocation_edges(model, root.id, &mut edges, &mut nodes);
    let lines: Vec<String> = edges
        .iter()
        .map(|e| format!("{} --{}--> {}  (depth {})", e["source"].as_str().unwrap_or(""), e["kind"].as_str().unwrap_or(""), e["target"].as_str().unwrap_or(""), e["depth"]))
        .collect();
    let mut reply = json!({
        "ref": root.qualified_name,
        "kind": root.keyword(),
        "depth": depth.clamp(1, 10),
        "edge_count": edges.len(),
        "nodes": nodes.iter().map(|(n, d)| json!({ "ref": qn(model, *n), "kind": model.elements[*n].keyword(), "depth": d })).collect::<Vec<_>>(),
        "edges": edges,
        "lines": lines,
        "traceability_properties": super::trace_props::of(model, &super::trace_props::index_with_allocations(model), root.id),
    });
    graph::mark_cut(model, &mut reply, &cuts);
    Ok(reply)
}

/// The allocations of the root as edges at depth 1, read from the one list every view reads (`allocation::allocations`):
/// an `allocate`, a `perform`, a call operation action (`calls`) and an action a part owns (kind `perform`, `via: owns`).
/// They replace the walk's own `allocate` edges at the root, so a partition proxy reads as its component. `via` says how
/// the part does it; a perform written in a part definition names the usage, with `defined_on` the definition, or the
/// definition itself when it is the root.
fn allocation_edges(model: &Model, root: ElementId, edges: &mut Vec<Value>, nodes: &mut Vec<(ElementId, usize)>) {
    let all = super::allocation::allocations(model);
    let mine: Vec<_> = all.iter().filter(|a| a.is_direct() && (a.function == root || a.part == root || a.defined_on == Some(root))).collect();
    if mine.is_empty() {
        return;
    }
    let root_qn = &model.elements[root].qualified_name;
    edges.retain(|e| !(e["kind"] == "allocate" && e["depth"] == 1 && (e["source"] == root_qn.as_str() || e["target"] == root_qn.as_str())));
    let mut seen = BTreeSet::new();
    for a in mine {
        let part = if a.part != root && a.defined_on == Some(root) { root } else { a.part };
        if a.function == part || !seen.insert((a.function, part, a.via)) {
            continue;
        }
        let out = a.function == root;
        let mut e = json!({
            "kind": if a.via == "owns" { "perform" } else { a.via },
            "source": model.elements[a.function].qualified_name,
            "target": model.elements[part].qualified_name,
            "declared_at": graph::declared_at(model, a.statement),
            "via": a.via,
            "depth": 1,
            "from": root_qn,
            "direction": if out { "out" } else { "in" },
        });
        if let Some(op) = a.operation {
            e["operation"] = json!(model.elements[op].qualified_name);
        }
        if let Some(d) = a.defined_on.filter(|d| *d != part) {
            e["defined_on"] = json!(model.elements[d].qualified_name);
        }
        edges.push(e);
        let other = if out { part } else { a.function };
        if !nodes.iter().any(|(n, _)| *n == other) {
            nodes.push((other, 1));
        }
    }
}

pub(super) fn finding(model: &Model, gate: &str, rule: &str, severity: &str, el: Option<&Element>, message: String, fix: String) -> Value {
    json!({
        "gate": gate,
        "rule": rule,
        "severity": severity,
        "element": el.map(|e| e.qualified_name.clone()),
        "file": el.map(|e| model.files[e.file].path.clone()),
        "line": el.map(|e| line_of(model, e)),
        "message": message,
        "fix": fix,
    })
}

pub(crate) fn syntax_findings(model: &Model) -> Vec<Value> {
    let mut grammar = super::grammar::findings(model);
    let parsed = api::diagnostics_out(model)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| {
            json!({
                "gate": "syntax", "rule": "syntax", "severity": "error", "element": null,
                "file": d.file, "line": d.line,
                "message": d.message,
                "fix": format!("Fix the text at {} line {} and run validate_model again.", d.file, d.line),
            })
        })
        .collect::<Vec<_>>();
    grammar.extend(parsed);
    grammar
}

fn is_verification_case(e: &Element) -> bool {
    e.kind == "verification" || e.kind.starts_with("verification")
}

thread_local! {
    /// Diagnostics and elements read on this thread to find the element of each finding of the semantic gate.
    static WORK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n as u64)));
}

/// The count since the last call; it starts again at zero. A test reads it to show, with no clock, that the semantic gate
/// reads the model once and not once for each finding.
pub fn take_work_steps() -> u64 {
    WORK.with(|w| w.replace(0))
}

/// Which kinds of element may stand at each end of a relationship: (relationship, role, allowed kinds, what is allowed in words).
/// Only the ends Cameo itself restricts (Magic Systems of Systems Architect 2022x Refresh2, read from its SysML
/// customization `typesForSource` / `typesForTarget` and the IBD connector tool):
/// a satisfy, verify or derive names a requirement as its supplier, a derive a requirement as its client, a connector
/// joins properties, ports and parameters. Cameo restricts no end of a refine, trace, allocate, dependency or item flow,
/// so neither does this engine. The table holds for text written by hand, by a generator and by the importer alike. A kind is
/// the keyword without `def` (a definition and a usage of one kind are judged alike; `trace-operand-definition` and
/// `connection-between-definitions` say which of the two is right).
pub(crate) const END_KINDS: &[(&str, &str, &[&str], &str)] = &[
    ("satisfy", "target", &["requirement", "concern"], "a requirement"),
    ("verify", "target", &["requirement", "concern"], "a requirement"),
    ("derive", "source", &["requirement", "concern"], "a requirement"),
    ("derive", "target", &["requirement", "concern"], "a requirement"),
    // Only a port says what crosses. A part, a value or an item is legal SysML and is what an imported v1 connector often
    // joins: those give the warning `connection-end-not-port`, not an error. A constraint usage is Cameo's constraint
    // property, which its IBD lets a connector join.
    ("connect", "source", &["port", "part", "attribute", "item", "ref", "reference", "occurrence", "parameter", "constraint"], "a port, a part, a value, a constraint usage or a parameter"),
    ("connect", "target", &["port", "part", "attribute", "item", "ref", "reference", "occurrence", "parameter", "constraint"], "a port, a part, a value, a constraint usage or a parameter"),
];

/// Stricter ends than the language requires. Each gives the warning `relationship-end-unusual`, so the model still reads
/// and the doubtful end is still named.
pub(crate) const STRICT_END_KINDS: &[(&str, &str, &[&str], &str)] = &[
    ("satisfy", "source", &["part", "action", "state", "calc", "use case", "analysis", "constraint", "interface", "connection"], "a part, a function (action, state, calculation, use case, analysis), a constraint or an interface"),
    ("allocate", "source", &["action", "state", "calc", "use case", "analysis", "part", "item", "requirement", "concern", "connection", "interface", "flow", "port"], "a function, a part, an item, a connection or a requirement"),
    ("allocate", "target", &["part", "action", "state", "connection", "interface", "port", "item"], "a part, a function, a connection or a port"),
    ("flow", "source", &["item", "attribute", "parameter", "port", "part", "action", "occurrence", "ref", "reference", "fork node", "join node", "merge node", "decide node"], "an item, a value, a parameter, a port, a part, an action or a control node (an object fork, join, merge or decision)"),
    ("flow", "target", &["item", "attribute", "parameter", "port", "part", "action", "occurrence", "ref", "reference", "fork node", "join node", "merge node", "decide node"], "an item, a value, a parameter, a port, a part, an action or a control node (an object fork, join, merge or decision)"),
];

/// The kind an end counts as: a usage written with its own keyword pair is the kind it uses (`perform action` is an action).
fn end_kind(kind: &str) -> &str {
    match kind {
        "perform action" | "perform" => "action",
        "exhibit state" | "exhibit" => "state",
        "include use case" | "include" => "use case",
        "assert constraint" | "assert" => "constraint",
        "event occurrence" | "event" => "occurrence",
        "satisfy requirement" => "requirement",
        k => k,
    }
}

/// The requirement a satisfy names is a performance requirement (on itself or on its definition): Cameo's
/// PerformanceRequirement[1] then asks for a value as the satisfier.
fn is_performance(model: &Model, marks: Option<&[Vec<String>]>, s: &Stmt) -> bool {
    let (Some(marks), Some(r)) = (marks, s.target.id) else { return false };
    let r = &model.elements[graph::unalias(model, r)];
    std::iter::once(r.id).chain(r.refs_of(RefRole::Type).filter_map(|x| x.resolved())).any(|id| marks[id].iter().any(|m| m == "performanceRequirement"))
}

fn end_kind_findings(model: &Model, s: &Stmt, via: &Element, marks: Option<&[Vec<String>]>, out: &mut Vec<Value>) {
    let ends = [("source", &s.source), ("target", &s.target)];
    for (role, end) in ends {
        let Some(id) = end.id else { continue };
        let t = &model.elements[graph::unalias(model, id)];
        let kind = end_kind(&t.kind);
        let performance_value = s.kind == "satisfy" && kind == "attribute" && is_performance(model, marks, s);
        if let Some((_, _, _, words)) = STRICT_END_KINDS.iter().find(|(k, r, usual, _)| *k == s.kind && *r == role && !usual.contains(&kind)).filter(|_| !performance_value) {
            out.push(finding(
                model,
                "semantic",
                "relationship-end-unusual",
                "warning",
                Some(via),
                format!("The {role} of this {} is {} ({}). Cameo accepts it, but the {role} of a {} is usually {words}.", s.kind, qshow(&t.qualified_name), t.keyword(), s.kind),
                format!("Check that {} is meant here; name {words} as the {role} if not.", qshow(&t.qualified_name)),
            ));
        }
        let Some((_, _, allowed, words)) = END_KINDS.iter().find(|(k, r, _, _)| *k == s.kind && *r == role) else { continue };
        if !allowed.contains(&kind) {
            out.push(finding(
                model,
                "semantic",
                "relationship-end-kind",
                "error",
                Some(via),
                format!("The {role} of this {} is {} ({}). The {role} of a {} is {words}.", s.kind, qshow(&t.qualified_name), t.keyword(), s.kind),
                format!("Name {words} as the {role}, or delete the statement if the model has no such element. A {} cannot stand there.", t.keyword()),
            ));
        } else if s.kind == "connect" && kind != "port" && !t.is_def {
            out.push(finding(
                model,
                "semantic",
                "connection-end-not-port",
                "warning",
                Some(via),
                format!("The {role} end of this connection is the {} {}, not a port: the model does not say through which port the two are connected or what crosses.", t.keyword(), qshow(&t.qualified_name)),
                "Give each part a port typed by a port def and connect the ports (add_interface creates both ports and the connection).".into(),
            ));
        }
    }
    // A refine usually has a requirement at one end; Cameo restricts neither end.
    if s.kind == "refine" {
        let req = |e: &graph::End| e.id.map(|id| matches!(model.elements[graph::unalias(model, id)].kind.as_str(), "requirement" | "concern"));
        if req(&s.source) == Some(false) && req(&s.target) == Some(false) {
            out.push(finding(
                model,
                "semantic",
                "relationship-end-unusual",
                "warning",
                Some(via),
                "Neither end of this refine is a requirement. Cameo accepts it, but a refine usually relates a requirement and the element that states it more exactly.".into(),
                "Check the two ends; name a requirement at one end, or use a plain dependency.".into(),
            ));
        }
    }
}

pub(crate) fn semantic_findings(model: &Model) -> Vec<Value> {
    let mut out = Vec::new();
    let (stmts, cuts) = graph::statements_cut(model);
    out.extend(graph::cut_finding(model, &cuts, "semantic"));
    let marks = stmts.iter().any(|s| s.kind == "satisfy").then(|| metadata_marks(model));
    let def_fix = |what: &str, e: &Element| {
        format!(
            "Name a usage, not the definition. Create one with create_element {{owner, kind: '{}', name, typed_by: {}}} and use that usage as the {what}.",
            e.kind, qshow(&e.qualified_name)
        )
    };
    for s in &stmts {
        let via = &model.elements[s.via];
        let ends = [("source", &s.source), ("target", &s.target)];
        end_kind_findings(model, s, via, marks.as_deref(), &mut out);
        match s.kind {
            // A tagged dependency (`#SysMLv1::Allocate dependency from A to B`) is how an import keeps a link between definitions.
            "satisfy" | "allocate" | "verify" if s.form != "dependency" => {
                // One finding for the statement, however many of its ends are definitions.
                let mut defs: Vec<(&str, &Element)> = Vec::new();
                for (role, end) in ends.iter() {
                    // The verifier of a `verify` may be a definition: the objective lives in it.
                    if s.kind == "verify" && *role == "source" {
                        continue;
                    }
                    let Some(id) = end.id else { continue };
                    let t = &model.elements[id];
                    if t.is_def && *role == "target" && s.kind != "allocate" && via.kind != "dependency" && t.kind.starts_with("requirement") {
                        // The requirement of a satisfy or verify must be a usage. Cameo 2026x (SysML v2 plugin 2026.0.0) refuses
                        // to synchronize `satisfy R by x;` with R a requirement def: "cannot be cast to <Feature>".
                        // Only the SysML v2 statement: a v1 import's `#SysMLv1::Satisfy dependency` is a plain dependency, and an end
                        // that is no requirement at all is relationship-end-kind's.
                        out.push(finding(
                            model,
                            "semantic",
                            "requirement-operand-definition",
                            "error",
                            Some(via),
                            format!(
                                "{} requirement {} is a {}. The requirement of a {} must be a requirement usage; Cameo refuses the whole file on this line.",
                                s.kind,
                                qshow(&t.qualified_name),
                                t.keyword(),
                                s.kind
                            ),
                            def_fix("requirement", t),
                        ));
                    } else if t.is_def {
                        defs.push((role, t));
                    }
                }
                if let Some((_, first)) = defs.first() {
                    let named = defs.iter().map(|(role, t)| format!("{role} {} ({})", qshow(&t.qualified_name), t.keyword())).collect::<Vec<_>>().join(" and ");
                    let fix = defs.iter().map(|(role, t)| def_fix(role, t)).collect::<Vec<_>>().join(" ");
                    out.push(finding(
                        model,
                        "semantic",
                        "trace-operand-definition",
                        // A warning, not an error: SysML v1 projects relate definitions, and an imported model must pass.
                        // create_relationship still refuses to make a new one.
                        "warning",
                        Some(via),
                        format!(
                            "{} {named} {} a definition. Prefer a usage: {} relates usages in SysML v2.",
                            s.kind,
                            if defs.len() > 1 { "are" } else { "is" },
                            s.kind
                        ),
                        if defs.len() > 1 { fix } else { def_fix(defs[0].0, first) },
                    ));
                }
            }
            "connect" | "flow" | "bind" => {
                for (role, end) in ends.iter() {
                    let Some(id) = end.id else { continue };
                    let t = &model.elements[id];
                    if t.is_def {
                        out.push(finding(
                            model,
                            "semantic",
                            "connection-between-definitions",
                            "error",
                            Some(via),
                            format!("{} {role} end {} is a definition ({}); connections join usages.", via.kind, qshow(&t.qualified_name), t.keyword()),
                            "Connect the parts or ports of a context, for example engine.torque, not the definitions. Use add_interface to create ports and the connection.".into(),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    // A usage typed by a definition of the other family: a part typed by an action def, an action typed by a part def.
    let structural = |k: &str| matches!(k, "part" | "item" | "port" | "attribute");
    let behavioral = |k: &str| matches!(k, "action" | "state" | "calc");
    for e in model.elements.iter().filter(|e| !e.is_def && (structural(&e.kind) || behavioral(&e.kind))) {
        let Some(t) = e.refs_of(RefRole::Type).find_map(|r| r.resolved()).map(|t| &model.elements[t]) else { continue };
        if !t.is_def || !((structural(&e.kind) && behavioral(&t.kind)) || (behavioral(&e.kind) && structural(&t.kind))) {
            continue;
        }
        let fix = if structural(&e.kind) {
            format!("Make {} an action usage typed by {} (for example create_element {{kind: 'action', typed_by}}), or type the {} by a {} definition with update_element {{ref, typed_by}}.", qshow(&e.qualified_name), qshow(&t.qualified_name), e.kind, e.kind)
        } else {
            format!("Make {} a part usage, or type the {} by an {} definition with update_element {{ref, typed_by}}.", qshow(&e.qualified_name), e.kind, e.kind)
        };
        out.push(finding(
            model,
            "semantic",
            "usage-definition-kind-mismatch",
            "warning",
            Some(e),
            format!("{} {} is typed by the {} {}; a {} is typed by a {} definition.", e.keyword(), qshow(&e.qualified_name), t.keyword(), qshow(&t.qualified_name), e.kind, e.kind),
            fix,
        ));
    }
    for e in &model.elements {
        if matches!(e.kind.as_str(), "verify" | "verify requirement") {
            let in_objective = e.owner.map(|o| model.elements[o].kind == "objective").unwrap_or(false);
            let case_ok = graph::verifier_of(model, e).map(|v| is_verification_case(&model.elements[v])).unwrap_or(false);
            if !(in_objective && case_ok) {
                out.push(finding(
                    model,
                    "semantic",
                    "verify-outside-objective",
                    "error",
                    Some(e),
                    format!("'verify {}' is not inside the objective of a verification case.", e.name.clone().unwrap_or_default()),
                    "Move it into `objective { verify R; }` of a verification case, or recreate it with create_relationship {kind: 'verify', source: <verification case>, target: <requirement>}.".into(),
                ));
            }
        }
    }
    // A member named `start` or `done` inside a behavior hides the built-in end of that behavior: `first start` resolves to it.
    for e in &model.elements {
        let hides = matches!(e.name.as_deref(), Some("start" | "done")) && !e.implied_name && e.owner.map(|o| matches!(model.elements[o].kind.as_str(), "action" | "state" | "use case" | "analysis" | "verification" | "case")).unwrap_or(false);
        if hides {
            let n = e.name.clone().unwrap_or_default();
            out.push(finding(
                model,
                "semantic",
                "reserved-name-hides-marker",
                "warning",
                Some(e),
                format!("{} {} is named '{n}', which hides the built-in {n} of {}: a succession or transition that says `{n}` resolves to this element, not to the behavior's {n}.", e.keyword(), qshow(&e.qualified_name), qshow(&model.elements[e.owner.unwrap_or(0)].qualified_name)),
                format!("Rename it with update_element {{ref: {}, rename: '<another name>'}}. create_relationship still writes `{n}` as the marker.", qshow(&e.qualified_name)),
            ));
        }
    }
    // A unit in brackets (`12 [V]`) must resolve: the unit libraries of the official sysml.library are `SI` and `USCustomaryUnits`.
    for g in super::units::gaps(model) {
        let e = &model.elements[g.element];
        let name = g.unit.rsplit("::").next().unwrap_or(&g.unit);
        if !super::units::declares("SI", name) && !super::units::declares("USCustomaryUnits", name) {
            out.push(finding(
                model,
                "semantic",
                "unresolved-unit",
                "error",
                Some(e),
                format!("the unit '{}' in {} does not resolve: neither SI nor USCustomaryUnits of the official sysml.library declares '{name}', so an import does not help (Cameo reports it as a reference it cannot resolve).", g.unit, qshow(&e.qualified_name)),
                "Write the unit as a product or quotient of library units (kW*h for kWh, ms as s with the number divided by 1000), declare it as an element of the model, or keep the number with a comment that names the unit, for example `= 12; /* unit: kWh */`.".to_string(),
            ));
            continue;
        }
        out.push(finding(
            model,
            "semantic",
            "unresolved-unit",
            "error",
            Some(e),
            format!("the unit '{}' in {} does not resolve: no import of a unit library is in scope for it, and no element is named '{}'.", g.unit, qshow(&e.qualified_name), g.unit),
            format!("Import the unit library of the official sysml.library: create_element {{kind: 'import', owner: {}, target: '{}'}} (SI holds V, K, s, m, kg; USCustomaryUnits holds ft, lb), or write the unit with its package, for example [SI::{}]. A write tool that writes a unit adds this import itself.", qshow(&model.elements[super::units::import_home(model, e.id)].qualified_name), super::units::DEFAULT_IMPORT, g.unit),
        ));
    }
    // The element each finding is about, looked up in two tables that are built once: the place of the first diagnostic of a
    // (file, line, message), and the first element that holds a name or a reference at a place. Without them every finding
    // read every diagnostic and every element: 8 000 lines that each name a missing tag took minutes.
    let mut at_of: HashMap<(usize, usize, &str), Span> = HashMap::new();
    let mut holder: HashMap<(usize, usize, usize), ElementId> = HashMap::new();
    if !model.diagnostics.is_empty() {
        work(model.diagnostics.len() + model.elements.len());
        for x in &model.diagnostics {
            at_of.entry((x.file, model.line_col(x.file, x.span.start).0, x.message.as_str())).or_insert(x.span);
        }
        for e in &model.elements {
            for sp in e.refs.iter().flat_map(|r| r.segments.iter().map(|s| s.span)).chain(e.name_span) {
                holder.entry((e.file, sp.start, sp.end)).or_insert(e.id);
            }
        }
    }
    let file_of: HashMap<&str, usize> = model.files.iter().enumerate().rev().map(|(i, f)| (f.path.as_str(), i)).collect();
    for d in api::diagnostics_out(model).into_iter().filter(|d| d.severity == Severity::Warning || d.message.starts_with("cannot resolve")) {
        let (rule, fix) = if d.message.starts_with("cannot resolve") {
            ("unresolved-reference", "Correct the name, or create the element it points at with create_element.")
        } else if d.message.starts_with("duplicate name") {
            ("duplicate-name", "Rename one of the elements with update_element {ref, rename}.")
        } else if d.message.starts_with("value does not fit type") {
            ("value-type-mismatch", "Give the feature one of the enum literals, or change its type.")
        } else {
            continue;
        };
        work(1);
        let el = file_of.get(d.file.as_str()).and_then(|f| {
            let at = at_of.get(&(*f, d.line, d.message.as_str()))?;
            holder.get(&(*f, at.start, at.end)).map(|i| &model.elements[*i])
        });
        // A name that resolves to nothing is an error for the gate, although the parser only warns.
        let mut v = finding(model, "semantic", rule, if rule == "unresolved-reference" { "error" } else { "warning" }, el, d.message.clone(), fix.into());
        v["file"] = json!(d.file);
        v["line"] = json!(d.line);
        out.push(v);
    }
    out.extend(duplicate_id_findings(model));
    out.extend(super::duplicates::findings(model));
    out.extend(super::duplicates::id_findings(model));
    out.extend(broken_text_reference_findings(model));
    out.extend(requirement_source_findings(model));
    out.extend(super::ownership::findings(model));
    out
}

/// The rule that a generated requirement names where it came from: its documentation has the provenance marker and no `SOURCE:`
/// line after it. Info by default; `require: ["source"]` makes it an error (see `upgrade_source`).
pub const SOURCE_RULE: &str = "requirement-without-source";

fn requirement_source_findings(model: &Model) -> Vec<Value> {
    let mut out = Vec::new();
    let mut cut = false;
    for e in model.elements.iter().filter(|e| e.kind == "requirement") {
        let Some((lines, _)) = e.doc.as_deref().and_then(super::element_links::provenance) else { continue };
        if lines.iter().any(|p| p.kind == "source") {
            continue;
        }
        if out.len() >= 1000 {
            cut = true;
            break;
        }
        out.push(finding(
            model,
            "semantic",
            SOURCE_RULE,
            "info",
            Some(e),
            format!("requirement {} has a generated provenance block but no SOURCE line: nothing says where it came from.", qshow(&e.qualified_name)),
            "Regenerate the file from the knowledge graph, or add the SOURCE line of the document the requirement comes from.".into(),
        ));
    }
    if cut {
        out.push(finding(model, "semantic", SOURCE_RULE, "info", None, "the check for requirements without a source stopped at 1000 findings; requirements after it were not checked.".to_string(), "Fix the listed requirements and check again.".into()));
    }
    out
}

/// With `require: ["source"]` a requirement without a source is an error, not an info.
pub(crate) fn upgrade_source(findings: &mut [Value]) {
    for f in findings.iter_mut().filter(|f| f["rule"] == SOURCE_RULE) {
        f["severity"] = json!("error");
    }
}

/// A reference in text, `(element <id>)`, to an element that is gone: a warning on the element that owns the text. The ids come from
/// `Model::source_ids`; with none set, only a reference by qualified name can be checked and an id is not called broken.
fn broken_text_reference_findings(model: &Model) -> Vec<Value> {
    let (refs, read_cut) = super::element_links::mentions(model, &model.source_ids);
    let mut out = Vec::new();
    let mut listed_cut = false;
    for r in refs.iter().filter(|r| r.target.is_none()) {
        // Without any id known, a name that is not a qualified name may be an id this reader cannot see.
        if model.source_ids.is_empty() && !r.text.contains("::") {
            continue;
        }
        let e = &model.elements[r.owner];
        out.push(finding(
            model,
            "semantic",
            "broken-text-reference",
            "warning",
            Some(e),
            format!("the text of this element refers to {}, which is not an element of the model.", super::text::quote_clip(&r.text)),
            "Update or remove the reference in the text (update_element {ref, doc}).".into(),
        ));
        if out.len() >= 1000 {
            listed_cut = true;
            break;
        }
    }
    // A bound that was hit never reads as "none found": a finding on the model root says the check is incomplete.
    if read_cut || listed_cut {
        out.push(finding(
            model,
            "semantic",
            "broken-text-reference",
            "warning",
            None,
            "the check of references in text stopped at its bound (50000 references read, 1000 findings listed); references after it were not checked, so no reference is declared sound.".to_string(),
            "Check a smaller part of the model, or fix the listed references and run the check again.".into(),
        ));
    }
    out
}

/// The kinds whose definitions name one concept of the system: two of one kind and one name in different namespaces are
/// probably one thing modelled twice, and a view that shows one of them hides the links of the other.
pub(super) const CONCEPT_DEFS: &[&str] = &["part", "item", "port", "action", "state", "interface", "connection", "requirement", "attribute", "enum", "flow", "allocation", "constraint", "calc", "verification", "use case"];


pub(super) fn in_library(model: &Model, mut id: ElementId) -> bool {
    loop {
        let e = &model.elements[id];
        if e.kind == "package" && e.has_prefix("library") {
            return true;
        }
        match e.owner {
            Some(o) => id = o,
            None => return false,
        }
    }
}

pub(super) fn specializes(model: &Model, a: ElementId, b: ElementId) -> bool {
    let (mut seen, mut todo) = (std::collections::HashSet::new(), vec![a]);
    while let Some(x) = todo.pop() {
        if !seen.insert(x) || seen.len() > 64 {
            continue;
        }
        for t in model.elements[x].refs_of(RefRole::Specializes).filter_map(|r| r.resolved()) {
            if t == b {
                return true;
            }
            todo.push(t);
        }
    }
    false
}

/// The rule that every component has a job: a leaf part usage that performs no function, is allocated nothing, satisfies no
/// requirement and owns no behavior, itself or through its definition. Info: it is a gap to look at, not a fault of the text.
pub const COMPONENT_WITHOUT_JOB_RULE: &str = "component-without-job";

fn component_job_findings(model: &Model, stmts: &[Stmt]) -> Vec<Value> {
    let is_part = |e: &Element| e.kind == "part";
    let mut busy: std::collections::HashSet<ElementId> = std::collections::HashSet::new();
    for s in stmts.iter().filter(|s| matches!(s.kind, "satisfy" | "allocate")) {
        busy.extend(s.source.id);
        busy.extend(s.target.id);
    }
    for e in &model.elements {
        let behaves = e.kind == "perform" || e.kind.starts_with("perform ") || e.kind.starts_with("exhibit") || (e.kind == "action" && !e.is_def) || e.kind == "state";
        if behaves {
            if let Some(o) = e.owner.filter(|o| is_part(&model.elements[*o])) {
                busy.insert(o);
            }
        }
    }
    let mut out = Vec::new();
    for p in model.elements.iter().filter(|e| is_part(e) && !e.is_def && !e.has_prefix("ref") && !e.has_prefix("end") && !in_library(model, e.id)) {
        let defs: Vec<ElementId> = p.refs_of(RefRole::Type).filter_map(|r| r.resolved()).collect();
        let has_parts = |id: ElementId| model.elements[id].children.iter().any(|c| is_part(&model.elements[*c]) && !model.elements[*c].is_def);
        if has_parts(p.id) || defs.iter().any(|d| has_parts(*d)) {
            continue;
        }
        if busy.contains(&p.id) || defs.iter().any(|d| busy.contains(d)) {
            continue;
        }
        if out.len() >= 1000 {
            break;
        }
        out.push(finding(
            model,
            "traceability",
            COMPONENT_WITHOUT_JOB_RULE,
            "info",
            Some(p),
            format!("component {} performs no function, is allocated nothing and satisfies no requirement, so nothing traces to it.", qshow(&p.qualified_name)),
            format!("create_relationship {{kind: 'perform', source: <its part def>, target: <the action usage it does>}}, or {{kind: 'satisfy', source: {}, target: <requirement>}}.", qshow(&p.qualified_name)),
        ));
    }
    out
}

/// Two requirements with one id: an error on each of them. The id is the short name of the requirement or the value of its id attribute.
fn duplicate_id_findings(model: &Model) -> Vec<Value> {
    let mut by_id: BTreeMap<String, Vec<ElementId>> = BTreeMap::new();
    for e in model.elements.iter().filter(|e| is_requirement(e) && !e.is_def) {
        let id = super::text::id_of(model, e);
        if !id.is_empty() {
            by_id.entry(id).or_default().push(e.id);
        }
    }
    let mut out = Vec::new();
    for (id, els) in by_id.into_iter().filter(|(_, v)| v.len() > 1) {
        for x in &els {
            let e = &model.elements[*x];
            out.push(finding(
                model,
                "semantic",
                "duplicate-requirement-id",
                "error",
                Some(e),
                format!("requirement id {} is also the id of {} other requirement(s).", super::text::quote_clip(&id), els.len() - 1),
                "Give each requirement its own id (update_element {ref, ...} or a renumber proposal).".into(),
            ));
        }
    }
    out
}

pub(crate) fn traceability_findings(model: &Model, need_satisfy: bool, need_verify: bool) -> Vec<Value> {
    let mut out = Vec::new();
    let (stmts, cuts) = graph::statements_cut(model);
    out.extend(graph::cut_finding(model, &cuts, "traceability"));
    out.extend(component_job_findings(model, &stmts));
    let links = Links::new(&stmts);
    let linked = |model: &Model, stmts: &[Stmt], kind: &'static str, req: ElementId, req_is_target: bool| links.of(model, stmts, kind, req, req_is_target);
    for r in requirements(model, None).unwrap_or_default() {
        let e = &model.elements[r];
        // A requirement definition is a type; its usages are what must be satisfied and verified.
        if e.is_def {
            continue;
        }
        let sat = !linked(model, &stmts, "satisfy", r, true).is_empty();
        let ver = !linked(model, &stmts, "verify", r, true).is_empty();
        if !sat && need_satisfy {
            // A trace or an allocation reaches a function or a part, as the trace chain reads it: the requirement is
            // still not satisfied (a weaker link does not hide the gap), and the finding names what it reaches.
            let mut weak: Vec<(ElementId, &str)> = Vec::new();
            for (kind, word) in [("trace", "a trace link"), ("allocate", "an allocation")] {
                for x in linked(model, &stmts, kind, r, true).into_iter().chain(linked(model, &stmts, kind, r, false)) {
                    if matches!(model.elements[x].kind.as_str(), "action" | "part") && !weak.iter().any(|w| w.0 == x) {
                        weak.push((x, word));
                    }
                }
            }
            let (message, fix) = match weak.first() {
                None => (format!("requirement {} is not satisfied by any element.", qshow(&e.qualified_name)), format!("create_relationship {{kind: 'satisfy', source: <part usage>, target: {}}}", qshow(&e.qualified_name))),
                Some((x, word)) => (
                    format!(
                        "requirement {} is not satisfied by any element. It reaches {}{} only by {word}, which does not state that the requirement is met.",
                        qshow(&e.qualified_name),
                        qshow(&model.elements[*x].qualified_name),
                        if weak.len() > 1 { format!(" and {} more", weak.len() - 1) } else { String::new() }
                    ),
                    format!("create_relationship {{kind: 'satisfy', source: {}, target: {}}} when that element satisfies it", qshow(&model.elements[*x].qualified_name), qshow(&e.qualified_name)),
                ),
            };
            out.push(finding(model, "traceability", "requirement-not-satisfied", "error", Some(e), message, fix));
        }
        if !ver && need_verify {
            out.push(finding(
                model,
                "traceability",
                "requirement-not-verified",
                "error",
                Some(e),
                format!("requirement {} is not verified by any verification case.", qshow(&e.qualified_name)),
                format!("create_relationship {{kind: 'verify', source: <verification case>, target: {}}}", qshow(&e.qualified_name)),
            ));
        }
    }
    for s in &stmts {
        if !(TRACE_KINDS.contains(&s.kind) || s.kind == "trace") {
            continue;
        }
        for (role, end) in [("source", &s.source), ("target", &s.target)] {
            if end.id.is_none() {
                out.push(finding(
                    model,
                    "traceability",
                    "dangling-trace-end",
                    "error",
                    Some(&model.elements[s.via]),
                    format!("{} {role} '{}' does not resolve to any element.", s.kind, end.text),
                    "Correct the name, or create the missing element, then run validate_model again.".into(),
                ));
            }
        }
    }
    out
}

pub fn validate_model(model: &Model, gate: Option<&str>) -> Res<Value> {
    validate_model_with(model, gate, None)
}

/// `require` limits what the traceability gate demands of a requirement: `satisfy`, `verify` or both (the default).
pub fn validate_model_with(model: &Model, gate: Option<&str>, require: Option<&[String]>) -> Res<Value> {
    let mut required: Vec<&str> = Vec::new();
    for r in require.unwrap_or(&[]) {
        match r.trim() {
            k @ ("satisfy" | "verify" | "source") => {
                if !required.contains(&k) {
                    required.push(k);
                }
            }
            other => return Err(format!("unknown require entry '{other}'. Fix: use satisfy, verify or source, for example require: [\"satisfy\"].").into()),
        }
    }
    if require.map(|r| r.is_empty()).unwrap_or(false) {
        return Err("require is empty. Fix: name what the gate demands: [\"satisfy\"], [\"verify\"] or [\"satisfy\", \"verify\"]; omit it for both.".into());
    }
    // `source` is an addition to what the traceability gate demands, not a replacement for it.
    if !required.iter().any(|r| *r == "satisfy" || *r == "verify") {
        required.extend(["satisfy", "verify"]);
    }
    required.sort();
    let want = gate.filter(|g| !g.is_empty());
    if let Some(g) = want {
        if !matches!(g, "syntax" | "semantic" | "traceability") {
            return Err(format!("unknown gate '{g}'. Use syntax, semantic or traceability, or omit gate to run all three.").into());
        }
    }
    let mut findings = Vec::new();
    let mut gates = serde_json::Map::new();
    for g in ["syntax", "semantic", "traceability"] {
        if want.map(|w| w != g).unwrap_or(false) {
            continue;
        }
        let mut f = match g {
            "syntax" => syntax_findings(model),
            "semantic" => semantic_findings(model),
            _ => traceability_findings(model, required.contains(&"satisfy"), required.contains(&"verify")),
        };
        if required.contains(&"source") {
            upgrade_source(&mut f);
        }
        f.extend(super::graph::unread_findings(model, g));
        let errors = f.iter().filter(|x| x["severity"] == "error").count();
        gates.insert(g.into(), json!({ "passed": errors == 0, "errors": errors, "warnings": f.len() - errors }));
        findings.extend(f);
    }
    let errors = findings.iter().filter(|x| x["severity"] == "error").count();
    Ok(json!({
        "gate": want.unwrap_or("all"),
        "passed": errors == 0,
        "errors": errors,
        "warnings": findings.len() - errors,
        "gates": gates,
        "required": required,
        "findings": findings,
    }))
}

/// `validate_model` with the opt-in gates and the `rules` and `groups` filters. A call that uses none of them
/// gives exactly the reply of `validate_model_with`.
pub fn validate_model_sel(model: &Model, gate: Option<&str>, sel: &super::rules::Selection) -> Res<Value> {
    validate_model_sel_with_owner(model, gate, sel, None)
}

pub fn validate_model_sel_with_owner(model: &Model, gate: Option<&str>, sel: &super::rules::Selection, owner: Option<CoverageOwner>) -> Res<Value> {
    let opt_in = matches!(gate, Some("connections" | "coverage" | "official" | "spec"));
    if opt_in || sel.rules.is_some() || sel.groups.is_some() || owner.is_some() {
        return super::rules::validate_with_filters_owner(model, gate, sel, owner);
    }
    validate_model_with(model, gate, sel.require)
}

/// The metadata names that mark each element, by element id: its `#Name` prefixes and the `@Name` annotations about it
/// (or written in it), each as its last name.
pub(crate) fn metadata_marks(m: &Model) -> Vec<Vec<String>> {
    let last = |s: &str| s.rsplit("::").next().unwrap_or(s).to_string();
    let mut t: Vec<Vec<String>> = vec![Vec::new(); m.elements.len()];
    for c in &m.elements {
        t[c.id].extend(c.prefixes.iter().filter(|p| p.starts_with('#')).map(|p| last(&p[1..])));
        if c.kind == "metadata" {
            let names: Vec<String> = c.refs_of(RefRole::Type).filter_map(|r| r.segments.last().map(|s| s.name.clone())).collect();
            // `@M about x;` marks x, not the namespace the annotation is written in.
            let about: Vec<ElementId> = c.refs_of(RefRole::Other).filter_map(|r| r.resolved()).collect();
            if !about.is_empty() {
                for x in about {
                    t[x].extend(names.iter().cloned());
                }
            } else if let Some(o) = c.owner {
                t[o].extend(names);
            }
        }
    }
    t
}
