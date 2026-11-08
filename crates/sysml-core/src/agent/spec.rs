//! Well-formedness constraints of the OMG SysML v2 and KerML specifications that a text can break and that the other
//! gates do not check. Each rule names the constraint it checks (`validate...` in the specifications' abstract syntax).
//! Constraints that the textual grammar already guarantees are not repeated here. The gate is opt-in.

use super::graph;
use crate::lexer;
use crate::model::{Element, ElementId, Model, RefRole};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// The definitions and usages of requirements (a concern, a viewpoint and an objective are requirements).
fn requirement_like(e: &Element) -> bool {
    matches!(e.kind.as_str(), "requirement" | "concern" | "viewpoint" | "objective")
        || e.kind.starts_with("satisfy ")
        || e.kind.starts_with("frame ")
}

/// The definitions and usages of cases.
fn case_like(e: &Element) -> bool {
    matches!(e.kind.as_str(), "case" | "analysis" | "verification" | "use case") || e.kind == "include use case"
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DefClass {
    /// An attribute or enumeration definition (a KerML DataType).
    Data,
    /// An item, part, port, view, rendering or metadata definition (a Structure).
    Structure,
    /// A connection, interface or allocation definition (an AssociationStructure).
    AssocStructure,
    /// A flow definition (an Interaction: an Association and a Behavior).
    Interaction,
    /// An action, state, calculation, constraint, requirement or case definition (a Behavior).
    Behavior,
    /// An occurrence definition (a Class only).
    Class,
}

fn def_class(e: &Element) -> Option<DefClass> {
    if !e.is_def {
        return None;
    }
    Some(match e.kind.as_str() {
        "attribute" | "enum" | "datatype" => DefClass::Data,
        "item" | "part" | "port" | "view" | "rendering" | "metadata" | "struct" | "metaclass" => DefClass::Structure,
        "connection" | "interface" | "allocation" | "assoc struct" => DefClass::AssocStructure,
        "flow" | "message" | "interaction" => DefClass::Interaction,
        "action" | "state" | "calc" | "constraint" | "requirement" | "concern" | "viewpoint" | "case" | "analysis" | "verification"
        | "use case" | "behavior" | "function" | "predicate" => DefClass::Behavior,
        "occurrence" | "class" => DefClass::Class,
        _ => return None,
    })
}

/// Why a definition of class `d` may not specialize one of class `g`, with the constraint that says so.
fn specialization_conflict(d: DefClass, g: DefClass) -> Option<&'static str> {
    use DefClass::*;
    let is_assoc = |c: DefClass| matches!(c, AssocStructure | Interaction);
    let is_structure = |c: DefClass| matches!(c, Structure | AssocStructure);
    let is_behavior = |c: DefClass| matches!(c, Behavior | Interaction);
    if d == Data && g != Data {
        return Some("validateDataTypeSpecialization: an attribute definition (a data type) must not specialize a class or an association");
    }
    if d != Data && g == Data {
        return Some("validateClassSpecialization: a class must not specialize an attribute definition (a data type)");
    }
    if !is_assoc(d) && is_assoc(g) {
        return Some("validateClassSpecialization: a class may specialize an association only if it is an association itself");
    }
    if is_structure(d) && is_behavior(g) {
        return Some("validateStructureSpecialization: a structure must not specialize a behavior");
    }
    if is_behavior(d) && is_structure(g) {
        return Some("validateBehaviorSpecialization: a behavior must not specialize a structure");
    }
    None
}

/// A child that is not a feature of its owner: notes, imports, aliases, relationships and nested definitions.
fn not_a_feature(c: &Element) -> bool {
    c.is_def
        || matches!(
            c.kind.as_str(),
            "comment" | "doc" | "textual representation" | "import" | "expose" | "alias" | "metadata" | "dependency" | "package"
        )
}

/// Usage kinds that are occurrences, so composite unless written `ref` (a port outside a port is referential).
fn occurrence_kind(kind: &str) -> bool {
    matches!(
        kind,
        "part" | "item" | "occurrence" | "action" | "state" | "connection" | "interface" | "allocation" | "flow" | "requirement"
            | "concern" | "case" | "analysis" | "verification" | "use case" | "view"
    )
}

fn referential(c: &Element) -> bool {
    c.has_prefix("ref") || c.has_prefix("end") || c.direction.is_some()
}

/// Whether a bound value is written `default` (a default value), not `=` or `:=` alone.
fn value_is_default(model: &Model, e: &Element) -> bool {
    let Some(v) = e.value_span else { return false };
    let text = &model.files[e.file].text[e.span.start..v.start];
    let (tokens, _) = lexer::lex(text);
    tokens.iter().any(|t| t.kind == lexer::TokKind::Ident && &text[t.span.start..t.span.end] == "default")
}

fn article(word: &str) -> String {
    let an = word.starts_with(|c: char| "aeiou".contains(c));
    format!("{} {word}", if an { "an" } else { "a" })
}

fn resolved(model: &Model, e: &Element, roles: &[RefRole]) -> Vec<ElementId> {
    e.refs.iter().filter(|r| roles.contains(&r.role)).filter_map(|r| r.resolved()).map(|t| graph::unalias(model, t)).collect()
}

/// Successions (`first a then b`, and `then b` after a node), from source node to target node. Flows are not successions.
fn successions(model: &Model) -> Vec<(ElementId, Vec<ElementId>, Vec<ElementId>)> {
    let node_like = |e: &Element| {
        (e.kind == "action" && !e.is_def)
            || e.kind.ends_with(" node")
            || (e.kind.ends_with(" action") && !matches!(e.kind.as_str(), "entry action" | "do action" | "exit action"))
    };
    let mut out = Vec::new();
    for o in &model.elements {
        let mut prev: Option<ElementId> = None;
        for c in model.children(o.id) {
            if c.kind == "succession" && !c.is_def {
                let mut src = resolved(model, c, &[RefRole::Source]);
                if src.is_empty() && c.has_prefix("then") {
                    src.extend(prev);
                }
                out.push((c.id, src, resolved(model, c, &[RefRole::Target])));
                continue;
            }
            if node_like(c) {
                prev = Some(c.id);
            }
        }
    }
    out
}

pub(crate) fn findings(model: &Model) -> Vec<Value> {
    let mut out = Vec::new();
    let mut push = |rule: &str, at: &Element, message: String, fix: &str| {
        out.push(json!({
            "gate": "spec", "rule": rule, "severity": "error", "element": at.qualified_name, "file": model.files[at.file].path,
            "line": super::line_of(model, at), "message": message, "fix": fix,
        }));
    };
    let variation = |e: &Element| e.has_prefix("variation") || (e.kind == "enum" && e.is_def);

    for e in &model.elements {
        let kids: Vec<&Element> = model.children(e.id).collect();
        let name = e.display_name();

        // Variations.
        if variation(e) {
            for c in kids.iter().filter(|c| !not_a_feature(c)) {
                let variant = c.has_prefix("variant") || (e.kind == "enum" && matches!(c.kind.as_str(), "enum" | "enum value"));
                if !variant {
                    push(
                        "spec-variation-member",
                        c,
                        format!("validateDefinitionVariationOwnedFeatureMembership / validateUsageVariationOwnedFeatureMembership: variation '{name}' owns '{}', which is not a variant. A variation owns only its variants.", c.display_name()),
                        "Make the member a variant (variant part x;), or move it out of the variation.",
                    );
                }
            }
            let roles: &[RefRole] = if e.is_def { &[RefRole::Specializes] } else { &[RefRole::Type, RefRole::Specializes] };
            for g in resolved(model, e, roles) {
                let ge = model.get(g);
                if variation(ge) {
                    push(
                        "spec-variation-specialization",
                        e,
                        format!("validateDefinitionVariationSpecialization / validateUsageVariationSpecialization: variation '{name}' specializes variation '{}'.", ge.display_name()),
                        "Specialize a definition that is not a variation, or make this element a variant of that variation.",
                    );
                }
            }
        }

        // Definitions.
        if let Some(d) = def_class(e) {
            for g in resolved(model, e, &[RefRole::Specializes]) {
                let ge = model.get(g);
                if let Some(why) = def_class(ge).and_then(|gc| specialization_conflict(d, gc)) {
                    push(
                        "spec-specialization-kind",
                        e,
                        format!("{why}: {} def '{name}' specializes {} def '{}'.", e.kind, ge.kind, ge.display_name()),
                        "Specialize a definition of a compatible kind, or use a feature typed by it instead.",
                    );
                }
            }
        }
        if e.kind == "attribute" || (e.kind == "enum" && e.is_def) {
            for c in kids.iter().filter(|c| !c.is_def && occurrence_kind(&c.kind) && !referential(c)) {
                push(
                    "spec-attribute-composite",
                    c,
                    format!("validateAttributeDefinitionFeatures / validateAttributeUsageFeatures: attribute '{name}' owns the composite {} '{}'. All features of an attribute must be referential.", c.kind, c.display_name()),
                    "Write the member as ref, or model the owner as an item or part.",
                );
            }
        }
        if e.kind == "part" && !e.is_def {
            let types = resolved(model, e, &[RefRole::Type]);
            let all_resolved = e.refs_of(RefRole::Type).all(|r| r.resolved().is_some());
            let part_kind = |t: &Element| matches!(def_class(t), Some(DefClass::Structure | DefClass::AssocStructure)) && !matches!(t.kind.as_str(), "item" | "port" | "metadata" | "struct" | "metaclass");
            // A behavior type is reported by the semantic gate (usage-definition-kind-mismatch).
            let behavior = types.iter().any(|t| matches!(def_class(model.get(*t)), Some(DefClass::Behavior | DefClass::Interaction)));
            if !types.is_empty() && all_resolved && !behavior && !types.iter().any(|t| part_kind(model.get(*t))) {
                let t = model.get(types[0]);
                push(
                    "spec-part-definition",
                    e,
                    format!("validatePartUsagePartDefinition: part '{name}' is typed by {} def '{}'. At least one type of a part must be a part definition.", t.kind, t.display_name()),
                    "Type the part by a part def, or write the usage with the kind of its type (item, attribute, port).",
                );
            }
        }
        if !e.is_def && e.has_prefix("individual") {
            let types = resolved(model, e, &[RefRole::Type]);
            let all_resolved = e.refs_of(RefRole::Type).all(|r| r.resolved().is_some());
            if all_resolved && !types.iter().any(|t| model.get(*t).has_prefix("individual")) {
                push(
                    "spec-individual-definition",
                    e,
                    format!("validateOccurrenceUsageIndividualUsage: individual '{name}' has no individual definition."),
                    "Type the usage by an individual def (individual part def X;).",
                );
            }
        }

        // States.
        if e.kind == "state" {
            for k in ["entry action", "do action", "exit action"] {
                let n = kids.iter().filter(|c| c.kind == k).count();
                if n > 1 {
                    let first = kids.iter().find(|c| c.kind == k).unwrap();
                    push(
                        "spec-state-subaction-kind",
                        first,
                        format!("validateStateDefinitionStateSubactionKind / validateStateUsageStateSubactionKind: state '{name}' has {n} {} subactions. A state has at most one of each kind.", k.trim_end_matches(" action")),
                        "Keep one entry, one do and one exit; put the steps of several in one action.",
                    );
                }
            }
        }

        // Owners of memberships.
        let owner = e.owner.map(|o| model.get(o));
        let owner_ok = |f: &dyn Fn(&Element) -> bool| owner.is_some_and(|o| f(o));
        let wrong = match e.kind.as_str() {
            "subject" if !owner_ok(&|o| requirement_like(o) || case_like(o)) => Some(("validateSubjectMembershipOwningType", "a requirement or a case")),
            "actor" if !owner_ok(&|o| requirement_like(o) || case_like(o)) => Some(("validateActorMembershipOwningType", "a requirement or a case")),
            "stakeholder" if !owner_ok(&requirement_like) => Some(("validateStakeholderMembershipOwningType", "a requirement")),
            "objective" if !owner_ok(&case_like) => Some(("validateObjectiveMembershipOwningType", "a case")),
            k if (k == "require" || k == "assume" || k.starts_with("require ") || k.starts_with("assume ") || k == "frame" || k.starts_with("frame "))
                && !owner_ok(&requirement_like) =>
            {
                Some(("validateRequirementConstraintMembershipOwningType", "a requirement"))
            }
            "entry action" | "do action" | "exit action" if !owner_ok(&|o| o.kind == "state") => Some(("validateStateSubactionMembershipOwningType", "a state")),
            "expose" if !owner_ok(&|o| o.kind == "view" && !o.is_def) => Some(("validateExposeOwningNamespace", "a view usage")),
            k if (k == "render" || k.starts_with("render ")) && !owner_ok(&|o| o.kind == "view") => Some(("validateViewRenderingMembershipOwningType", "a view")),
            _ => None,
        };
        if let Some((rule, place)) = wrong {
            let o = owner.map(|o| format!("{} '{}'", if o.is_def { format!("{} def", o.kind) } else { o.kind.clone() }, o.display_name())).unwrap_or_else(|| "the root namespace".into());
            push(
                "spec-membership-owner",
                e,
                format!("{rule}: {} member may be owned only by {place}; this one is owned by {o}.", article(e.kind.split(' ').next().unwrap_or(&e.kind))),
                "Move the member into an element of the kind the specification allows.",
            );
        }

        // Requirements and cases.
        if requirement_like(e) || case_like(e) {
            let subjects: Vec<&&Element> = kids.iter().filter(|c| c.kind == "subject").collect();
            if subjects.len() > 1 {
                push(
                    "spec-one-subject",
                    subjects[1],
                    format!("validateRequirementDefinitionOnlyOneSubject / validateCaseDefinitionOnlyOneSubject: '{name}' has {} subjects. It may have at most one.", subjects.len()),
                    "Keep one subject; make the others parameters (in x : T;).",
                );
            }
            if let Some(s) = subjects.first() {
                let before = kids.iter().take_while(|c| c.id != s.id).find(|c| c.direction.as_deref().is_some_and(|d| d == "in" || d == "inout"));
                if let Some(p) = before {
                    push(
                        "spec-subject-first",
                        s,
                        format!("validateRequirementDefinitionSubjectParameterPosition / validateCaseDefinitionSubjectParameterPosition: the subject of '{name}' must be its first input; input '{}' comes before it.", p.display_name()),
                        "Declare the subject before the other inputs.",
                    );
                }
            }
        }
        if case_like(e) {
            let n = kids.iter().filter(|c| c.kind == "objective").count();
            if n > 1 {
                push(
                    "spec-one-objective",
                    e,
                    format!("validateCaseDefinitionOnlyOneObjective / validateCaseUsageOnlyOneObjective: '{name}' has {n} objectives. A case has at most one."),
                    "Merge the objectives into one objective with several requirements.",
                );
            }
        }
        if e.kind == "view" {
            let n = kids.iter().filter(|c| c.kind == "render" || c.kind.starts_with("render ")).count();
            if n > 1 {
                push(
                    "spec-one-render",
                    e,
                    format!("validateViewDefinitionOnlyOneViewRendering / validateViewUsageOnlyOneViewRendering: view '{name}' has {n} renderings. A view has at most one."),
                    "Keep one render member.",
                );
            }
        }
        if e.kind == "flow" && e.is_def {
            let n = kids.iter().filter(|c| c.has_prefix("end")).count();
            if n > 2 {
                push(
                    "spec-flow-def-ends",
                    e,
                    format!("validateFlowDefinitionFlowEnds: flow def '{name}' has {n} ends. A flow definition has at most two."),
                    "Keep two ends, or use a connection def for more.",
                );
            }
        }

        // Features.
        if e.value.is_some() {
            let mut seen = HashSet::new();
            let mut stack = resolved(model, e, &[RefRole::Redefines]);
            while let Some(r) = stack.pop() {
                if !seen.insert(r) || r == e.id {
                    continue;
                }
                let re = model.get(r);
                if re.value.is_some() && !value_is_default(model, re) {
                    push(
                        "spec-feature-value-overriding",
                        e,
                        format!("validateFeatureValueOverriding: '{name}' gives a value to '{}', which already has a bound value. Only a default value (default = ...) can be redefined.", re.qualified_name),
                        "Write the first value as a default (attribute m default = 5;), or do not give a new value here.",
                    );
                    break;
                }
                stack.extend(resolved(model, re, &[RefRole::Redefines]));
            }
        }

        // Ports: every usage a port owns or holds through its definition is referential (nested ports are fine).
        if e.kind == "port" {
            for c in kids.iter().filter(|c| !c.is_def && occurrence_kind(&c.kind) && !referential(c)) {
                push(
                    "spec-port-composite",
                    c,
                    format!("validatePortDefinitionOwnedUsagesNotComposite / validatePortUsageNestedUsagesNotComposite: port '{name}' owns the composite {} '{}'. The usages of a port must be referential, except nested ports.", c.kind, c.display_name()),
                    "Give the member a direction (in item x;), or write it as ref.",
                );
            }
        }
        // Multiplicity: a lower bound no greater than the upper bound, when both are numbers.
        if let Some((lo, hi)) = e.multiplicity.as_deref().and_then(|m| m.split_once("..")) {
            if let (Ok(l), Ok(h)) = (lo.trim().parse::<u64>(), hi.trim().parse::<u64>()) {
                if l > h {
                    push(
                        "spec-multiplicity-bounds",
                        e,
                        format!("validateMultiplicityRangeBounds: '{name}' has the multiplicity [{l}..{h}]. The lower bound must not be greater than the upper bound."),
                        "Swap the bounds, or write one number.",
                    );
                }
            }
        }

        // Imports.
        if e.kind == "import" && e.owner.is_none() && !e.has_prefix("private") {
            push(
                "spec-top-level-import",
                e,
                "validateImportTopLevelVisibility: an import outside any package must be private.".into(),
                "Write private import ..., or move the import into a package.",
            );
        }
    }

    // Parallel states: no transitions into or out of their own states and actions.
    let parallel: HashSet<ElementId> = model.elements.iter().filter(|e| e.kind == "state" && e.has_prefix("parallel")).map(|e| e.id).collect();
    if !parallel.is_empty() {
        for t in model.elements.iter().filter(|t| t.kind == "transition" && !t.is_def) {
            let ends = resolved(model, t, &[RefRole::Source, RefRole::Target]);
            if let Some(p) = ends.iter().find_map(|x| model.get(*x).owner.filter(|o| parallel.contains(o))) {
                let pe = model.get(p);
                push(
                    "spec-parallel-state-transition",
                    t,
                    format!("validateStateDefinitionParallelSubactions / validateStateUsageParallelSubactions: parallel state '{}' has a transition between its own substates. The substates of a parallel state run together.", pe.display_name()),
                    "Remove the transition, or make the state not parallel.",
                );
            }
        }
    }

    // Control nodes.
    let (mut ins, mut outs): (HashMap<ElementId, usize>, HashMap<ElementId, usize>) = (HashMap::new(), HashMap::new());
    for (_, src, dst) in successions(model) {
        for s in src {
            *outs.entry(s).or_default() += 1;
        }
        for d in dst {
            *ins.entry(d).or_default() += 1;
        }
    }
    for n in model.elements.iter().filter(|e| e.kind.ends_with(" node")) {
        let (count, dir, rule) = match n.kind.as_str() {
            "decide node" => (ins.get(&n.id), "incoming", "validateDecisionNodeIncomingSuccessions"),
            "fork node" => (ins.get(&n.id), "incoming", "validateForkNodeIncomingSuccessions"),
            "join node" => (outs.get(&n.id), "outgoing", "validateJoinNodeOutgoingSuccessions"),
            "merge node" => (outs.get(&n.id), "outgoing", "validateMergeNodeOutgoingSuccessions"),
            _ => continue,
        };
        if let Some(&k) = count.filter(|k| **k > 1) {
            push(
                "spec-control-node-successions",
                n,
                format!("{rule}: {} '{}' has {k} {dir} successions. It may have at most one.", n.kind, n.display_name()),
                if dir == "incoming" { "Join or merge the incoming successions first, then lead one succession into this node." } else { "Lead one succession out, into a fork or a decision." },
            );
        }
    }
    out
}
