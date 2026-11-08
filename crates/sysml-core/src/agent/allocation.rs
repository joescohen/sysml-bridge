//! Allocation of behaviour to structure, read once from the model so every view says the same: an `allocate` statement, a
//! `perform` of an action by a part (`perform sense;`), and a `perform action` written in a part (the action is the
//! performed function). Every reader of allocations reads `allocations`; none keeps its own copy.
//! A call operation action of an imported project (`action x { comment calledOperation about Block::op; }`) is a link
//! too: the call action is the function and the block that owns the operation is the part, via `calls`. An action usage a
//! part owns is done by that part, via `owns`. What a part definition does, each part usage typed by it does: the
//! allocation names the usage, and `defined_on` the definition (kept as the part when nothing is typed by it).

use super::graph;
use crate::model::*;
use std::collections::{HashMap, HashSet};

/// One function allocated to one part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Allocation {
    /// The behaviour: the source of `allocate`, the action a `perform` names, or the `perform action` itself.
    pub function: ElementId,
    /// The structure: the target of `allocate`, or the part (definition or usage) that owns the `perform`.
    pub part: ElementId,
    /// `allocate`, `perform`, `calls` or `owns`.
    pub via: &'static str,
    /// The statement that says it: the `allocate` or `perform` element.
    pub statement: ElementId,
    /// For a step the part performs because it performs the action usage that owns the step (`perform cycle;` performs
    /// `cycle.acquire`): that owning usage. Such an allocation is implied, never a direct one; `None` for a stated one.
    pub through: Option<ElementId>,
    /// For `calls`: the operation the call action calls (an action the block owns).
    pub operation: Option<ElementId>,
    /// When the statement is in a part definition and `part` is a usage typed by it: that definition.
    pub defined_on: Option<ElementId>,
}

impl Allocation {
    /// Stated by the statement itself, not reached through an owning usage.
    pub fn is_direct(&self) -> bool {
        self.through.is_none()
    }

    /// Reached through a call: a step of the activity a performed call is typed by (not a step the usage owns).
    pub fn through_call(&self, model: &Model) -> bool {
        self.through.is_some_and(|t| !owned_below(model, self.function, t))
    }

    /// The part the statement is written in: the definition for a lifted allocation, else the part.
    pub fn stated_on(&self) -> ElementId {
        self.defined_on.unwrap_or(self.part)
    }
}

fn is_part(e: &Element) -> bool {
    e.kind == "part"
}

/// The component a partition proxy stands for. An imported ActivityPartition is `ref part lane ::> Component` inside the
/// activity, with `allocate node to lane`: the allocation reads node -> Component. Any other element is itself.
pub fn partition_component(model: &Model, mut id: ElementId) -> ElementId {
    for _ in 0..4 {
        let e = &model.elements[id];
        let proxy = e.kind == "part" && !e.is_def && e.has_prefix("ref") && e.owner.is_some_and(|o| model.elements[o].kind.ends_with("action"));
        let to = proxy.then(|| e.refs.iter().filter(|r| matches!(r.role, RefRole::References | RefRole::Specializes)).find_map(|r| r.resolved())).flatten();
        match to.map(|t| graph::unalias(model, t)).filter(|t| model.elements[*t].kind == "part") {
            Some(t) => id = t,
            None => break,
        }
    }
    id
}

/// Every allocation of the model. Only what a statement names is allocated: a part that performs an action usage, or a
/// `perform action` itself, allocates that element and not the other usages of its definition. An identical (function, part, via) is listed once, at its first statement; a function
/// allocated to two parts is listed twice. Ordered by function, then statement, so a function's first allocation comes first.
pub fn allocations(model: &Model) -> Vec<Allocation> {
    read_allocations(model)
}

fn read_allocations(model: &Model) -> Vec<Allocation> {
    let mut out: Vec<Allocation> = Vec::new();
    for e in &model.elements {
        // An import keeps a link between definitions as `#SysMLv1::Allocate dependency from A to B`: Cameo cannot take an
        // `allocate` whose end is a definition.
        let tagged = e.kind == "dependency" && graph::tag_kind(e) == Some("allocate");
        if (e.kind == "allocation" && !e.is_def) || tagged {
            for s in graph::statements_of(model, e.id) {
                if let (Some(f), Some(p)) = (s.source.id, s.target.id) {
                    out.push(Allocation { function: f, part: partition_component(model, p), via: "allocate", statement: e.id, through: None, operation: None, defined_on: None });
                }
            }
            continue;
        }
        // The note of a call operation action: the action calls the operation, so it runs on the block that owns it.
        if e.kind == "comment" && e.name.as_deref() == Some("calledOperation") {
            let Some(call) = e.owner.filter(|o| model.elements[*o].kind.ends_with("action") && !model.elements[*o].is_def) else { continue };
            for op in e.refs_of(RefRole::Other).filter_map(|r| r.resolved()).map(|t| graph::unalias(model, t)) {
                if let Some(block) = model.elements[op].owner.filter(|b| is_part(&model.elements[*b])) {
                    out.push(Allocation { function: call, part: block, via: "calls", statement: e.id, through: None, operation: Some(op), defined_on: None });
                }
            }
            continue;
        }
        // A call operation action (`perform action plan ::> nav.planPath { pins }` in a method): the part the chain names
        // does it, via `calls`. A one-segment chain (a state's `do` calling its block's own operation) names no part: the
        // block that owns the operation does it, lifted to each usage typed by it below.
        if let Some(c) = call_of(model, e.id) {
            let part = c.part.or_else(|| model.elements[c.op].owner.filter(|b| is_part(&model.elements[*b])));
            if let Some(p) = part {
                out.push(Allocation { function: e.id, part: p, via: "calls", statement: e.id, through: None, operation: Some(c.op), defined_on: None });
            }
            continue;
        }
        // An action usage written in a part: the part does it (its own statement says so).
        if !e.is_def && e.kind.ends_with("action") && e.kind != "perform action" && e.owner.is_some_and(|o| is_part(&model.elements[o])) {
            out.push(Allocation { function: e.id, part: e.owner.unwrap(), via: "owns", statement: e.id, through: None, operation: None, defined_on: None });
            continue;
        }
        let function = match e.kind.as_str() {
            "perform" => e.first_resolved(RefRole::Target).map(|t| graph::unalias(model, t)),
            "perform action" if !e.is_def => Some(e.id),
            _ => continue,
        };
        let (Some(f), Some(o)) = (function, e.owner) else { continue };
        if is_part(&model.elements[o]) {
            out.push(Allocation { function: f, part: o, via: "perform", statement: e.id, through: None, operation: None, defined_on: None });
            // The steps the performed usage owns itself (not those of its definition, which every usage of it shares).
            if !model.elements[f].is_def {
                let mut todo: Vec<ElementId> = model.elements[f].children.clone();
                while let Some(c) = todo.pop() {
                    let ce = &model.elements[c];
                    if ce.is_def || !ce.kind.ends_with("action") || out.len() > MAX_THROUGH {
                        continue;
                    }
                    out.push(Allocation { function: c, part: o, via: "perform", statement: e.id, through: Some(f), operation: None, defined_on: None });
                    todo.extend(ce.children.iter().copied());
                }
                // A call (an action usage typed by an activity) covers the steps of the activity it calls, at any depth
                // of calls: whoever performs the call performs them, unless a step has a perform of its own (below).
                // Only a call does: a usage written in a package is not a step of an activity, and its definition's
                // steps belong to every usage of it.
                let is_call = model.elements[f].owner.is_some_and(|o| model.elements[o].kind.ends_with("action"));
                let mut defs: Vec<ElementId> = called(model, f).into_iter().filter(|_| is_call).collect();
                let mut seen: HashSet<ElementId> = HashSet::new();
                while let Some(d) = defs.pop() {
                    if !seen.insert(d) || seen.len() > 64 || out.len() > MAX_THROUGH {
                        continue;
                    }
                    for c in model.elements[d].children.iter().copied() {
                        let ce = &model.elements[c];
                        if ce.is_def || !ce.kind.ends_with("action") || ce.kind == "perform action" {
                            continue;
                        }
                        out.push(Allocation { function: c, part: o, via: "perform", statement: e.id, through: Some(f), operation: None, defined_on: None });
                        defs.extend(called(model, c));
                    }
                }
            }
        }
    }
    let out = lift_to_usages(model, out);
    // A stated allocation comes before one through an owning usage, so the repeat that is dropped is the implied one.
    let mut out = out;
    out.sort_by_key(|a| (a.function, a.through.is_some(), a.statement, a.part, a.via));
    let mut seen = HashSet::new();
    out.retain(|a| seen.insert((a.function, a.part, a.via)));
    // A step of a called activity that a part performs itself is that part's: the call's performer does not cover it.
    let own: HashSet<ElementId> = out.iter().filter(|a| a.is_direct()).map(|a| a.function).collect();
    out.retain(|a| a.is_direct() || !own.contains(&a.function) || owned_below(model, a.function, a.through.unwrap()));
    // Of two calls that both cover a step, the inner one says who does it: a part that performs `plan` covers PlanPath's
    // `capture` and Capture's `shoot`, and the camera that performs `capture` covers `shoot` in its place.
    let mut reach: HashMap<ElementId, HashSet<ElementId>> = HashMap::new();
    let inherited: Vec<(ElementId, ElementId, ElementId)> = out.iter().filter_map(|a| a.through.filter(|t| !owned_below(model, a.function, *t)).map(|t| (a.function, a.part, t))).collect();
    if !inherited.is_empty() {
        let mut drop: HashSet<(ElementId, ElementId)> = HashSet::new();
        for (f, p, t) in &inherited {
            let covered = reach.entry(*t).or_insert_with(|| covered_steps(model, *t));
            if inherited.iter().any(|(g, q, u)| g == f && q != p && u != t && covered.contains(u)) {
                drop.insert((*f, *p));
            }
        }
        out.retain(|a| a.through.is_none() || !drop.contains(&(a.function, a.part)) || owned_below(model, a.function, a.through.unwrap()));
    }
    out
}

/// A call operation action as written in v2 (the OMG mapping of CallOperationAction): a `perform action` (or a bare
/// `perform`) that an action owns (a method, an inline action, a state's `do`), with a feature chain to an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call {
    /// The `perform action` element.
    pub call: ElementId,
    /// The operation the chain ends at: an action usage that a part definition (or a part usage) owns.
    pub op: ElementId,
    /// The part usage the chain names before the operation (`nav` in `nav.planPath`); `None` for a one-segment chain.
    pub part: Option<ElementId>,
}

/// `perform` or `perform action` written inside an action (at any depth), not in a part: a call, resolved or not.
pub fn is_call_site(model: &Model, id: ElementId) -> bool {
    let e = &model.elements[id];
    if e.is_def || !matches!(e.kind.as_str(), "perform action" | "perform") {
        return false;
    }
    for o in std::iter::successors(e.owner, |o| model.elements[*o].owner).take(64) {
        let k = model.elements[o].kind.as_str();
        if k == "part" || k == "package" {
            return false;
        }
        if k.ends_with("action") {
            return true;
        }
    }
    false
}

/// The chain a call names (`References` for `perform action x ::> a.b`, `Target` for `perform a.b`).
pub fn call_chain(model: &Model, id: ElementId) -> Option<&Reference> {
    let e = &model.elements[id];
    e.refs_of(RefRole::References).next().or_else(|| (e.kind == "perform").then(|| e.refs_of(RefRole::Target).next()).flatten())
}

/// An operation: an action usage owned by a part definition or a part usage.
pub fn is_operation(model: &Model, id: ElementId) -> bool {
    let e = &model.elements[id];
    !e.is_def && e.kind == "action" && e.owner.is_some_and(|o| is_part(&model.elements[o]))
}

/// The call `id` is, when it is one and its chain resolves to an operation.
pub fn call_of(model: &Model, id: ElementId) -> Option<Call> {
    if !is_call_site(model, id) {
        return None;
    }
    let r = call_chain(model, id)?;
    let op = graph::unalias(model, r.resolved()?);
    if !is_operation(model, op) {
        return None;
    }
    let n = r.segments.len();
    let part = (n >= 2).then(|| r.segments[n - 2].resolved).flatten().map(|p| graph::unalias(model, p)).filter(|p| {
        let pe = &model.elements[*p];
        is_part(pe) && !pe.is_def
    });
    Some(Call { call: id, op, part })
}

/// Every call of the model, in model order.
pub fn calls(model: &Model) -> Vec<Call> {
    model.elements.iter().filter_map(|e| call_of(model, e.id)).collect()
}

/// The chain of a call as written (`nav.planPath`).
pub fn chain_text(model: &Model, id: ElementId) -> Option<String> {
    call_chain(model, id).map(|r| r.text.clone())
}

/// An accept or send action (an activity's input or output, not a function of its own).
pub fn is_accept_or_send(e: &Element) -> bool {
    matches!(e.kind.as_str(), "accept action" | "send action") || matches!(e.props.get("node").map(String::as_str), Some("accept" | "send"))
}

/// The steps a call covers through the activities it calls, at any depth.
fn covered_steps(model: &Model, call: ElementId) -> HashSet<ElementId> {
    let mut out = HashSet::new();
    let mut defs: Vec<ElementId> = called(model, call).into_iter().collect();
    let mut seen = HashSet::new();
    while let Some(d) = defs.pop() {
        if !seen.insert(d) || seen.len() > 64 {
            continue;
        }
        for c in model.elements[d].children.iter().copied() {
            let ce = &model.elements[c];
            if !ce.is_def && ce.kind.ends_with("action") {
                out.insert(c);
                defs.extend(called(model, c));
            }
        }
    }
    out
}

/// The activity a call is typed by (`action plan : PlanPath`), when it is one.
fn called(model: &Model, call: ElementId) -> Option<ElementId> {
    let e = &model.elements[call];
    if e.is_def {
        return None;
    }
    e.refs_of(RefRole::Type).filter_map(|r| r.resolved()).map(|t| graph::unalias(model, t)).find(|t| {
        let te = &model.elements[*t];
        te.is_def && te.kind.ends_with("action")
    })
}

/// `step` is written inside `usage` (owned by it at some depth), not reached through the activity it calls.
fn owned_below(model: &Model, step: ElementId, usage: ElementId) -> bool {
    std::iter::successors(model.elements[step].owner, |o| model.elements[*o].owner).take(64).any(|o| o == usage)
}

/// Most allocations read in all: owned steps of performed usages stop being added past it.
const MAX_THROUGH: usize = 200_000;

/// An allocation to a part definition is one to each part usage typed by it, with the definition kept as `defined_on`.
/// A definition no usage is typed by keeps the allocation itself.
fn lift_to_usages(model: &Model, all: Vec<Allocation>) -> Vec<Allocation> {
    if !all.iter().any(|a| model.elements[a.part].is_def) {
        return all;
    }
    let mut usages: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for e in model.elements.iter().filter(|e| is_part(e) && !e.is_def && !is_lifeline(model, e)) {
        for t in part_types(model, e.id) {
            usages.entry(t).or_default().push(e.id);
        }
    }
    let mut out = Vec::with_capacity(all.len());
    for a in all {
        match usages.get(&a.part).filter(|_| model.elements[a.part].is_def) {
            Some(us) => out.extend(us.iter().map(|u| Allocation { part: *u, defined_on: Some(a.part), ..a })),
            None => out.push(a),
        }
    }
    out
}

/// A `ref part` an action declares: a sequence's lifeline, a reference to a part and not one that does the definition's
/// functions (each sequence would otherwise add a swimlane copy of every lifeline's part).
pub fn is_lifeline(model: &Model, e: &Element) -> bool {
    (e.kind == "ref" || e.has_prefix("ref")) && e.owner.is_some_and(|o| model.elements[o].kind == "action")
}

/// The `«perform»` / `«allocate»` mark a row ends with.
pub fn mark(via: &str) -> String {
    format!("«{via}»")
}

/// The definitions a part usage is typed by, which its allocations include.
fn part_types(model: &Model, id: ElementId) -> Vec<ElementId> {
    let e = &model.elements[id];
    if e.is_def {
        return Vec::new();
    }
    e.refs_of(RefRole::Type).filter_map(|r| r.resolved()).filter(|t| is_part(&model.elements[*t]) && model.elements[*t].is_def).collect()
}

/// Most packages that get an allocation matrix of their own.
pub const MAX_MATRIX_SCOPES: usize = 200;

/// Most generated views `appears_in` builds; past it the reply says `truncated`.
pub const MAX_VIEWS_BUILT: usize = 400;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{self, FileIn};

    fn model(text: &str) -> Model {
        let m = api::to_model(vec![FileIn { path: "a.sysml".into(), text: text.into() }]);
        assert!(!m.has_errors(), "{:?}", m.diagnostics);
        m
    }

    fn pairs(m: &Model) -> Vec<(String, String, &'static str)> {
        allocations(m).into_iter().filter(Allocation::is_direct).map(|a| (m.elements[a.function].qualified_name.clone(), m.elements[a.part].qualified_name.clone(), a.via)).collect()
    }

    #[test]
    fn perform_by_reference_names_the_action_and_the_owning_part() {
        let m = model("package P { action sense; part def Sensor { perform sense; } part sys { perform sense; } }");
        assert_eq!(pairs(&m), [("P::sense".to_string(), "P::Sensor".to_string(), "perform"), ("P::sense".into(), "P::sys".into(), "perform")]);
    }

    #[test]
    fn perform_action_is_its_own_function() {
        let m = model("package P { action def Act; part def Ctl { perform action act : Act; } }");
        assert_eq!(pairs(&m), [("P::Ctl::act".to_string(), "P::Ctl".to_string(), "perform")]);
    }

    #[test]
    fn allocate_statements_resolve_both_ends_and_skip_the_rest() {
        let m = model("package P { action f; part sys { part c; } allocate f to sys.c; allocate f to nowhere; }");
        assert_eq!(pairs(&m), [("P::f".to_string(), "P::sys::c".to_string(), "allocate")]);
    }

    #[test]
    fn one_function_on_two_parts_is_kept_twice_and_a_repeat_once() {
        let m = model("package P { action f; part a; part b; allocate f to a; allocate f to b; allocate f to a; part def D { perform f; } }");
        assert_eq!(
            pairs(&m),
            [("P::f".to_string(), "P::a".to_string(), "allocate"), ("P::f".into(), "P::b".into(), "allocate"), ("P::f".into(), "P::D".into(), "perform")]
        );
    }

    #[test]
    fn a_partition_proxy_reads_as_its_component() {
        let m = model("package P { part sys { part ctl; } action act { ref part lane ::> sys.ctl; action s1; allocate s1 to lane; } }");
        assert_eq!(pairs(&m), [("P::act::s1".to_string(), "P::sys::ctl".to_string(), "allocate")]);
    }

    #[test]
    fn another_usage_of_a_performed_definition_is_not_allocated() {
        let m = model("package P { action def Act; part def Ctl { perform action act : Act; } action other : Act; }");
        assert_eq!(pairs(&m), [("P::Ctl::act".to_string(), "P::Ctl".to_string(), "perform")]);
    }

    #[test]
    fn a_performed_usage_brings_its_own_steps_through_it_and_not_its_definitions() {
        let m = model("package P { action def Cycle { action shared; } action cycle : Cycle { action acquire; } action other : Cycle; part def Ctl { perform cycle; } }");
        let all = allocations(&m);
        let through: Vec<_> = all.iter().filter(|a| !a.is_direct()).map(|a| (m.elements[a.function].qualified_name.as_str(), a.through.map(|t| m.elements[t].qualified_name.as_str()))).collect();
        assert_eq!(through, [("P::cycle::acquire", Some("P::cycle"))]);
        assert_eq!(pairs(&m), [("P::cycle".to_string(), "P::Ctl".to_string(), "perform")]);
    }

    #[test]
    fn a_call_operation_action_runs_on_the_block_that_owns_the_operation() {
        let m = model("package P { part def Radio { action tune; } action def Use { action step { comment calledOperation about P::Radio::tune /* Calls operation tune */ } } }");
        assert_eq!(pairs(&m), [("P::Radio::tune".to_string(), "P::Radio".to_string(), "owns"), ("P::Use::step".into(), "P::Radio".into(), "calls")]);
        let a = allocations(&m)[1];
        assert_eq!(a.operation.map(|o| m.elements[o].qualified_name.as_str()), Some("P::Radio::tune"));
        assert!(a.is_direct());
    }

    #[test]
    fn an_action_a_part_owns_is_done_by_it_and_not_its_steps() {
        let m = model("package P { part sys { action run { action step; } } action free; }");
        assert_eq!(pairs(&m), [("P::sys::run".to_string(), "P::sys".to_string(), "owns")]);
    }

    #[test]
    fn what_a_definition_does_each_usage_typed_by_it_does() {
        let m = model("package P { action f; part def D { perform f; action own; } part def Lone { perform f; } part a : D; part b : D; }");
        assert_eq!(
            pairs(&m),
            [
                ("P::f".to_string(), "P::a".to_string(), "perform"),
                ("P::f".into(), "P::b".into(), "perform"),
                ("P::f".into(), "P::Lone".into(), "perform"),
                ("P::D::own".into(), "P::a".into(), "owns"),
                ("P::D::own".into(), "P::b".into(), "owns"),
            ]
        );
        let all = allocations(&m);
        assert!(all.iter().filter(|a| m.elements[a.part].name.as_deref() != Some("Lone")).all(|a| a.defined_on.map(|d| m.elements[d].qualified_name.as_str()) == Some("P::D")));
    }

    #[test]
    fn a_lifeline_does_not_do_what_its_definition_does() {
        let m = model("package P { action f; part def D { perform f; } part a : D; action def S { ref part l : D; message m from l to l; } }");
        assert_eq!(pairs(&m), [("P::f".to_string(), "P::a".to_string(), "perform")]);
    }

    #[test]
    fn a_perform_outside_a_part_allocates_nothing() {
        let m = model("package P { action f; action g { perform f; } }");
        assert!(allocations(&m).is_empty());
    }

    #[test]
    fn a_call_operation_action_is_done_by_the_part_its_chain_names() {
        let m = model("package P { item def C; part def Motor { action turn { in item c : C; } } part def Drive { part m1 : Motor; part m2 : Motor; \
            action run : Run { in item c : C; } action def Run { in item c : C; perform action t1 ::> m1.turn { in item c : C; } perform action t2 ::> m2.turn { in item c : C; } } } }");
        let p = pairs(&m);
        assert!(p.contains(&("P::Drive::Run::t1".to_string(), "P::Drive::m1".to_string(), "calls")), "{p:?}");
        assert!(p.contains(&("P::Drive::Run::t2".to_string(), "P::Drive::m2".to_string(), "calls")), "{p:?}");
        assert!(!p.contains(&("P::Drive::Run::t1".to_string(), "P::Drive::m2".to_string(), "calls")), "{p:?}");
        let a = allocations(&m).into_iter().find(|a| a.via == "calls").unwrap();
        assert_eq!(a.operation.map(|o| m.elements[o].qualified_name.as_str()), Some("P::Motor::turn"));
    }

    #[test]
    fn a_one_segment_call_runs_on_the_block_that_owns_the_operation() {
        let m = model("package P { part def Drive { action go; state def S { state on { do action r { perform action g ::> go; } } } } part d : Drive; }");
        let p = pairs(&m);
        assert!(p.contains(&("P::Drive::S::on::r::g".to_string(), "P::d".to_string(), "calls")), "{p:?}");
    }
}

/// The activity a step calls: the type of the operation a call names, or the action definition that types the step.
pub fn called_activity(model: &Model, step: ElementId) -> Option<ElementId> {
    let is_activity = |t: ElementId| model.elements[t].is_def && model.elements[t].kind.ends_with("action");
    if let Some(c) = call_of(model, step) {
        return model.elements[c.op].refs_of(RefRole::Type).filter_map(|r| r.resolved()).find(|t| is_activity(*t));
    }
    let e = &model.elements[step];
    if !matches!(e.kind.as_str(), "action" | "perform" | "perform action") {
        return None;
    }
    e.refs_of(RefRole::Type).filter_map(|r| r.resolved()).find(|t| is_activity(*t))
}
