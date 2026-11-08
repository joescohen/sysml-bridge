//! Who may own what. In Cameo every element has one owner through one owning feature of the UML/SysML metamodel
//! (`Class::ownedPort`, `StructuredClassifier::ownedConnector`, `Activity::node`/`edge`, `Region::transition`,
//! `Behavior::ownedParameter`, `Action::input`/`output`, `Package::packagedElement`, ...), and the containment tree lists
//! the element under that owner and nowhere else. Cameo refuses an element the owner has no feature for: a port cannot
//! be created in a package, a control flow outside an activity, a transition outside a region. In SysML v2 text the
//! owner is the body the element is written in; these rules say which bodies may hold which elements, so the tree and
//! every view show the element where Cameo shows it.
//! of each row.
//!
//! The rules run in the semantic gate, so every model is checked, and `refuse_new` refuses any write (a tool, a raw
//! edit, a batch, a move) that adds an ownership error the model did not have.

use super::{line_of, qshow, Fail, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// One rule: its id, its severity, and the Cameo owning feature it carries over.
pub struct OwnerRule {
    pub id: &'static str,
    pub severity: &'static str,
    pub meaning: &'static str,
}

pub const RULES: &[OwnerRule] = &[
    OwnerRule { id: "owner-port", severity: "error", meaning: "A port is owned by a part, item, occurrence, port, interface or connection (def or usage), never a behavior: Cameo's Class::ownedPort of a block or interface block; Cameo refuses a proxy, full or flow port on an activity or state machine" },
    OwnerRule { id: "owner-connector", severity: "error", meaning: "A connection, interface or bind with ends is owned by the structure that holds both ends, never a package: Cameo's StructuredClassifier::ownedConnector" },
    OwnerRule { id: "owner-behavior-step", severity: "error", meaning: "A succession, control node, accept, send, assign or loop is owned by a behavior, never a package: Cameo's Activity::node and Activity::edge" },
    OwnerRule { id: "owner-transition", severity: "error", meaning: "A transition is owned by a state (def or usage): Cameo's Region::transition" },
    OwnerRule { id: "owner-directed-feature", severity: "error", meaning: "A directed feature (in, out, inout) is owned by a behavior, a port or a definition, never a package: Cameo's Behavior::ownedParameter, Action pins and flow properties" },
    OwnerRule { id: "owner-perform", severity: "error", meaning: "A perform or exhibit is owned by the part or behavior that performs it, never a package: Cameo's performer is the owning block" },
    OwnerRule { id: "owner-end-feature", severity: "warning", meaning: "An end feature is owned by a connection, interface, allocation or flow (def or usage): Cameo's Association::memberEnd" },
    OwnerRule { id: "owner-nested-definition", severity: "warning", meaning: "A definition is owned by a package or another definition, not a usage: Cameo nests classifiers only in packages and classes (nestedClassifier)" },
    OwnerRule { id: "owner-requirement", severity: "warning", meaning: "A requirement is owned by a package, a requirement or the objective of a case, not a part, action, state, value or port: Cameo keeps a requirement only in a package, a model, a profile or another requirement" },
];

/// The structure kinds that own ports and connectors in Cameo (blocks, interface blocks, association blocks), def or usage.
const STRUCTURE: &[&str] = &["part", "item", "occurrence", "port", "interface", "connection", "allocation", "flow", "metadata", "ref", "individual part", "individual item", "individual occurrence"];
/// The kinds that hold an end feature (associations and association blocks).
const ASSOCIATION: &[&str] = &["connection", "interface", "allocation", "flow"];
/// Statements that are steps of a behavior.
const STEPS: &[&str] = &["succession", "fork node", "join node", "merge node", "decide node", "send action", "assign action", "if action", "while action", "loop action", "for action", "terminate action"];
/// Owners a requirement may have: requirements, and the objective and frame of a case or viewpoint, which SysML v2 writes
/// as a requirement inside them.
const REQUIREMENT_OWNERS: &[&str] = &["requirement", "concern", "use case", "include use case", "case", "analysis", "verification", "objective", "viewpoint", "view"];

/// True when an end of `e` is reached through an element that is not at the top of a package: the ends are features of
/// a part or a definition, and that element, not the package, is where the connector belongs. A connection between
/// usages that stand in the package itself (`connect d1 to d2` beside `part d1; part d2;`) has the package as its context.
fn ends_inside_structure(model: &Model, e: &Element) -> bool {
    e.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target)).any(|r| {
        // The head of the end: the element the qualified name before the first `.` names (`S::c` of `S::c.a`).
        let head_text = r.text.split('.').next().unwrap_or("");
        let n = crate::resolve::split_name(head_text).len();
        let Some(head) = n.checked_sub(1).and_then(|i| r.segments.get(i)).and_then(|s| s.resolved) else { return false };
        let h = &model.elements[super::graph::unalias(model, head)];
        let top = match h.owner {
            None => true,
            Some(o) => model.elements[o].kind == "package",
        };
        h.is_def || !top
    })
}

fn has_ends(e: &Element) -> bool {
    e.refs.iter().any(|r| matches!(r.role, RefRole::Source | RefRole::Target))
}

/// The ownership rule `e` breaks, if any: (rule id, what is wrong, the fix).
pub fn misplaced(model: &Model, e: &Element) -> Option<(&'static str, String, String)> {
    misplaced_in(model, e, e.owner)
}

/// The ownership rule `e` would break under `owner` (`None`: the top of the model), so a move can be refused before it
/// is written.
pub fn misplaced_in(model: &Model, e: &Element, owner: Option<ElementId>) -> Option<(&'static str, String, String)> {
    let o = owner.map(|o| &model.elements[o]).filter(|o| o.kind != "package");
    let in_pkg = o.is_none();
    let okind = o.map(|o| o.kind.as_str()).unwrap_or("package");
    // Written only for a rule that is broken: most elements break none, and the check reads every element.
    let where_of = || match o {
        Some(o) => format!("the {} {}", o.keyword(), qshow(&o.qualified_name)),
        None => match owner {
            Some(p) => format!("the package {}", qshow(&model.elements[p].qualified_name)),
            None => "the top of the model".to_string(),
        },
    };
    let move_fix = |to: &str| format!("Move it into {to}: update_element {{ref: {}, owner: <that element>}}. Cameo lists it there in the containment tree.", qshow(&e.qualified_name));
    let usage = !e.is_def;
    // Ports: Class::ownedPort. Nested ports (port in port) are ports on a port.
    if usage && e.kind == "port" && !STRUCTURE.contains(&okind) {
        let where_ = where_of();
        return Some(("owner-port", format!("The port {} is owned by {where_}. In Cameo a port is owned by its block or interface block and is listed under it; {} cannot own a port.", qshow(&e.qualified_name), article(okind)), move_fix("the part def (block) or port def (interface block) it is a port of")));
    }
    // Connectors: StructuredClassifier::ownedConnector.
    if usage && matches!(e.kind.as_str(), "connection" | "interface" | "binding") && has_ends(e) && (matches!(okind, "attribute" | "enum") || in_pkg && ends_inside_structure(model, e)) {
        let what = if e.kind == "binding" { "binding connector" } else { "connector" };
        let where_ = where_of();
        return Some(("owner-connector", format!("This {} ({}) is owned by {where_}. In Cameo a {what} is owned by the block whose internal structure holds both ends (the block of the IBD), never by a package or a value type.", e.kind, qshow(&e.qualified_name)), move_fix("the part def (or part) that holds both ends")));
    }
    // Activity nodes and edges.
    if STEPS.contains(&e.kind.as_str()) && in_pkg {
        let where_ = where_of();
        return Some(("owner-behavior-step", format!("The {} {} is owned by {where_}. In Cameo control flows, control nodes and actions are owned by their activity; a package cannot own one.", e.kind, qshow(&e.qualified_name)), move_fix("the action def (activity) or action it is a step of")));
    }
    if e.kind == "accept action" && in_pkg {
        let where_ = where_of();
        return Some(("owner-behavior-step", format!("The accept action {} is owned by {where_}. In Cameo an accept event action is owned by its activity.", qshow(&e.qualified_name)), move_fix("the action def (activity) or state it belongs to")));
    }
    // Region::transition.
    if e.kind == "transition" && !matches!(okind, "state" | "exhibit state") {
        let where_ = where_of();
        return Some(("owner-transition", format!("The transition {} is owned by {where_}. In Cameo a transition is owned by the region of a state machine or state; only a state (def or usage) can own one.", qshow(&e.qualified_name)), move_fix("the state def or state that holds its source and target")));
    }
    // Parameters, pins and flow properties.
    if usage && e.direction.is_some() && in_pkg {
        let where_ = where_of();
        return Some(("owner-directed-feature", format!("The {} feature {} is owned by {where_}. In Cameo a parameter is owned by its behavior, a pin by its action and a flow property by its block or interface block; a package owns none.", e.direction.as_deref().unwrap_or(""), qshow(&e.qualified_name)), move_fix("the action def, calc def, port def or part def it is a parameter or flow property of")));
    }
    // Performed behaviors.
    if matches!(e.kind.as_str(), "perform action" | "exhibit state" | "perform" | "exhibit") && in_pkg {
        let where_ = where_of();
        return Some(("owner-perform", format!("The {} {} is owned by {where_}, so nothing performs it. In Cameo the block that owns a behavior (ownedBehavior, classifierBehavior) is its performer.", e.kind, qshow(&e.qualified_name)), move_fix("the part def or part that performs it")));
    }
    // Association member ends.
    if usage && e.has_prefix("end") && !ASSOCIATION.contains(&okind) {
        let where_ = where_of();
        return Some(("owner-end-feature", format!("The end feature {} is owned by {where_}. In Cameo a member end belongs to an association or association block (connection, interface, allocation or flow def).", qshow(&e.qualified_name)), move_fix("the connection def or interface def it is an end of")));
    }
    if let Some(o) = o {
        // Nested classifiers: in packages and classes only.
        if e.is_def && !o.is_def && o.kind != "package" {
            return Some(("owner-nested-definition", format!("The definition {} is owned by the usage {}. In Cameo a classifier is owned by a package or nested in another classifier, never by a property.", qshow(&e.qualified_name), qshow(&o.qualified_name)), move_fix("a package or a definition")));
        }
        if matches!(e.kind.as_str(), "requirement" | "concern") && !REQUIREMENT_OWNERS.contains(&okind) {
            let where_ = where_of();
            return Some(("owner-requirement", format!("The requirement {} is owned by {where_}. In Cameo a requirement lives in a package, a model or another requirement; a block, behavior, value or port does not own one.", qshow(&e.qualified_name)), move_fix("a requirements package or a parent requirement")));
        }
    }
    None
}

fn article(kind: &str) -> String {
    let a = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{a} {kind}")
}

/// True for a rule whose finding is an error, which a write may not add.
pub fn is_error(rule: &str) -> bool {
    severity(rule) == "error"
}

fn severity(rule: &str) -> &'static str {
    RULES.iter().find(|r| r.id == rule).map(|r| r.severity).unwrap_or("error")
}

/// The general types and types a feature of `owner` may take its redefined features from: the owner, what it is typed
/// by or specializes, what the features it redefines or subsets are typed by, to a fixed point. `None` when a step does
/// not resolve (a library type that is not loaded): then nothing can be said.
fn inherited_from(model: &Model, owner: ElementId) -> Option<std::collections::HashSet<ElementId>> {
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![owner];
    while let Some(x) = todo.pop() {
        if !seen.insert(x) || seen.len() > 2000 {
            continue;
        }
        for r in model.elements[x].refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines | RefRole::References)) {
            match r.resolved() {
                Some(t) => todo.push(super::graph::unalias(model, t)),
                None => return None,
            }
        }
    }
    Some(seen)
}

/// Rules on how a feature relates to the features and types it names: a redefined feature is inherited, and only a port
/// takes a conjugated (`~`) port def.
fn feature_findings(model: &Model, out: &mut Vec<Value>) {
    let mut push = |e: &Element, rule: &str, message: String, fix: String| {
        out.push(json!({
            "gate": "semantic", "rule": rule, "severity": "error",
            "element": e.qualified_name, "file": model.files[e.file].path, "line": line_of(model, e),
            "message": message, "fix": fix,
        }));
    };
    for e in &model.elements {
        for r in e.refs_of(RefRole::Type).filter(|r| r.conjugated) {
            let t = r.resolved().map(|t| &model.elements[super::graph::unalias(model, t)]);
            if e.kind != "port" || e.is_def {
                push(e, "conjugated-type-not-port", format!("{} is a {} with the conjugated type ~{}. In Cameo only a port is conjugated (isConjugated); a {} takes its type as it is.", qshow(&e.qualified_name), e.keyword(), r.text, e.keyword()), format!("Drop the '~': update_element {{ref: {}, typed_by: '{}'}}.", qshow(&e.qualified_name), r.text));
            } else if let Some(t) = t.filter(|t| !(t.kind == "port" && t.is_def)) {
                push(e, "conjugated-type-not-port", format!("The port {} is conjugated to {}, which is a {}, not a port def. Only an interface block (port def) has a conjugate.", qshow(&e.qualified_name), qshow(&t.qualified_name), t.keyword()), "Type the port by a port def, or drop the '~'.".into());
            }
        }
        let Some(owner) = e.owner else { continue };
        let mut contexts: Option<Option<std::collections::HashSet<ElementId>>> = None;
        for r in e.refs_of(RefRole::Redefines) {
            let Some(f) = r.resolved().map(|f| super::graph::unalias(model, f)) else { continue };
            if r.text.contains('.') {
                continue;
            }
            // A feature at the top of a package is featured by everything, so any feature may redefine it.
            let Some(fo) = model.elements[f].owner.filter(|o| model.elements[*o].kind != "package") else { continue };
            let ctx = contexts.get_or_insert_with(|| inherited_from(model, owner));
            let Some(ctx) = ctx.as_ref() else { continue };
            if !ctx.contains(&fo) && !ctx.iter().any(|c| model.elements[*c].kind != "package" && std::iter::successors(Some(fo), |o| model.elements[*o].owner).any(|o| o == *c)) {
                let fe = &model.elements[f];
                push(e, "redefines-not-inherited", format!("{} redefines {}, but {} is no feature of {} or of any type it is typed by or specializes. In Cameo a redefined property must be inherited from a general.", qshow(&e.qualified_name), qshow(&fe.qualified_name), qshow(&fe.qualified_name), qshow(&model.elements[owner].qualified_name)), format!("Make {} specialize (or be typed by) the definition that owns {}, or redefine a feature that is inherited.", qshow(&model.elements[owner].qualified_name), qshow(&fe.qualified_name)));
            }
        }
    }
}

/// Every ownership finding of the model, for the semantic gate.
pub fn findings(model: &Model) -> Vec<Value> {
    let mut out = Vec::new();
    feature_findings(model, &mut out);
    for e in &model.elements {
        if let Some((rule, message, fix)) = misplaced(model, e) {
            out.push(json!({
                "gate": "semantic", "rule": rule, "severity": severity(rule),
                "element": e.qualified_name, "file": model.files[e.file].path, "line": line_of(model, e),
                "message": message, "fix": fix,
            }));
        }
    }
    out
}

/// The ownership errors of a model, counted by (rule, owner, kind): a key that an unrelated edit does not change (the
/// generated names of unnamed statements are renumbered by edits around them; their owner and kind are not).
fn error_keys(model: &Model) -> HashMap<(&'static str, String, String), (usize, ElementId)> {
    let mut m: HashMap<(&'static str, String, String), (usize, ElementId)> = HashMap::new();
    for e in &model.elements {
        let Some((rule, _, _)) = misplaced(model, e) else { continue };
        if severity(rule) != "error" {
            continue;
        }
        let owner = e.owner.map(|o| model.elements[o].qualified_name.clone()).unwrap_or_default();
        let slot = m.entry((rule, owner, e.keyword())).or_insert((0, e.id));
        slot.0 += 1;
    }
    m
}

/// Refuse a write that adds an ownership error: whatever tool made it (create, update, move, raw text, a statement with
/// an owner, a batch), the element must land under an owner its kind allows. An error the model already had is not the
/// edit's and stays.
pub fn refuse_new(before: &Model, after: &Model) -> Res<()> {
    let now = error_keys(after);
    if now.is_empty() {
        return Ok(());
    }
    let was = error_keys(before);
    let mut fresh: Vec<(&(&'static str, String, String), &(usize, ElementId))> = now.iter().filter(|(k, (n, _))| was.get(*k).map(|w| w.0).unwrap_or(0) < *n).collect();
    fresh.sort_by_key(|(k, _)| (*k).clone());
    let Some(((rule, _, _), (_, id))) = fresh.first() else { return Ok(()) };
    let e = &after.elements[*id];
    let (_, message, fix) = misplaced(after, e).unwrap_or((rule, String::new(), String::new()));
    Err(Fail::with(
        format!("refused: {message} Nothing was written. Fix: {fix}"),
        json!({ "rule": rule, "element": e.qualified_name, "owner": e.owner.map(|o| after.elements[o].qualified_name.clone()) }),
    ))
}
