//! Coverage metrics (`get_coverage_history`): for one state of the model, how many requirements are covered by each of four
//! measures (satisfaction, verification, derivation, refinement), under two owner rules, with the direct and the implied
//! coverage told apart. The server evaluates this for each baseline and for the workspace now and adds the change between rows.
//!
//! Owner rules, as the source tool names them:
//! - "Treat Owner As Requirement" (`requirement_rule`): a requirement is covered only by a link of its own.
//! - "Treat Owner As Grouping Element" (`grouping_rule`): a requirement is also covered when a requirement that owns it is covered
//!   by a link of its own, or when it owns requirements and every one of them is covered.
//!
//! A requirement is a requirement usage (a definition is a type). Derivation: something is derived from the requirement.
//! Refinement: something refines the requirement. Implied coverage exists for satisfaction and verification only (a link that
//! reaches the requirement through a part or a type, as `get_traceability` reads it); it counts only where there is no direct link.

use super::graph;
use super::implied;
use super::{locate, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

pub const MEASURES: &[(&str, &str)] = &[("satisfaction", "satisfy"), ("verification", "verify"), ("derivation", "derive"), ("refinement", "refine")];
/// Most baselines evaluated in one reply.
pub const MAX_BASELINES: usize = 50;
/// Most steps in one `get_coverage_history` call, all baselines together. A baseline costs: its text (one step for each 64
/// bytes parsed), one pass over its statements, four measures over its requirements, and at most 400 elements reached for each
/// counted requirement that has no direct link. The rules (two) read the same sets again and add no pass over the model.
pub const MAX_CALL_STEPS: usize = 50_000_000;

#[derive(Default, Clone)]
pub struct Opts {
    /// Only requirements under this element are counted.
    pub scope: Option<String>,
    /// A link counts only when its other end lies under one of these elements.
    pub covering_scope: Vec<String>,
    /// Count only requirements that own no requirement.
    pub atomic_only: bool,
}

fn under(e: &Element, scope: &[String]) -> bool {
    scope.is_empty() || scope.iter().any(|s| e.qualified_name == *s || e.qualified_name.starts_with(&format!("{s}::")))
}

fn is_usage_req(e: &Element) -> bool {
    e.kind == "requirement" && !e.is_def
}

/// `covered` under the grouping rule, given which requirements have a link of their own.
fn grouping(m: &Model, own: &HashSet<ElementId>, all: &[ElementId]) -> HashSet<ElementId> {
    let kids = |r: ElementId| -> Vec<ElementId> { m.elements[r].children.iter().copied().filter(|c| is_usage_req(&m.elements[*c])).collect() };
    let mut down: HashMap<ElementId, bool> = HashMap::new();
    fn down_of(m: &Model, own: &HashSet<ElementId>, memo: &mut HashMap<ElementId, bool>, r: ElementId) -> bool {
        if let Some(v) = memo.get(&r) {
            return *v;
        }
        let v = own.contains(&r) || m.elements[r].owner.filter(|o| is_usage_req(&m.elements[*o])).map(|o| down_of(m, own, memo, o)).unwrap_or(false);
        memo.insert(r, v);
        v
    }
    let mut memo: HashMap<ElementId, bool> = HashMap::new();
    fn group_of(m: &Model, own: &HashSet<ElementId>, down: &mut HashMap<ElementId, bool>, memo: &mut HashMap<ElementId, bool>, kids: &dyn Fn(ElementId) -> Vec<ElementId>, r: ElementId) -> bool {
        if let Some(v) = memo.get(&r) {
            return *v;
        }
        let ks = kids(r);
        let v = down_of(m, own, down, r) || (!ks.is_empty() && ks.iter().all(|k| group_of(m, own, down, memo, kids, *k)));
        memo.insert(r, v);
        v
    }
    all.iter().copied().filter(|r| group_of(m, own, &mut down, &mut memo, &kids, *r)).collect()
}

fn pct(n: usize, of: usize) -> Value {
    if of == 0 {
        Value::Null
    } else {
        json!((n as f64 * 1000.0 / of as f64).round() / 10.0)
    }
}

/// The four measures for one state of the model.
pub fn measure(m: &Model, o: &Opts) -> Res<Value> {
    let within: Option<BTreeSet<ElementId>> = match o.scope.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => {
            let e = locate(m, s)?;
            let mut set: BTreeSet<ElementId> = m.descendants(e.id).into_iter().collect();
            set.insert(e.id);
            Some(set)
        }
        None => None,
    };
    let all: Vec<ElementId> = m.elements.iter().filter(|e| is_usage_req(e)).map(|e| e.id).collect();
    let counted: Vec<ElementId> = all
        .iter()
        .copied()
        .filter(|r| within.as_ref().map(|w| w.contains(r)).unwrap_or(true))
        .filter(|r| !o.atomic_only || !m.elements[*r].children.iter().any(|c| is_usage_req(&m.elements[*c])))
        .collect();
    let n = counted.len();
    let counted_set: HashSet<ElementId> = counted.iter().copied().collect();
    let (stmts, cuts) = graph::statements_cut(m);
    let ends_of = |kind: &str| -> HashSet<ElementId> { stmts.iter().filter(|s| s.kind == kind).filter_map(|s| s.target.id).collect() };
    let mut reach: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    let mut measures = serde_json::Map::new();
    let mut steps = stmts.len();
    // Requirements whose implied reach hit the visit bound: their implied coverage may be understated.
    let mut reach_bound = 0usize;
    for (name, kind) in MEASURES {
        let ends = if matches!(*kind, "satisfy" | "verify") { Some(ends_of(kind)) } else { None };
        let mut direct: HashSet<ElementId> = HashSet::new();
        let mut implied_set: HashSet<ElementId> = HashSet::new();
        // The other ends of the links of this kind, by the requirement they end at: one pass, not one pass for each requirement.
        let mut by_target: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
        for s in stmts.iter().filter(|s| s.kind == *kind) {
            if let (Some(t), Some(src)) = (s.target.id, s.source.id) {
                by_target.entry(t).or_default().push(src);
            }
        }
        steps = steps.saturating_add(all.len());
        for r in &all {
            let ls: &[ElementId] = by_target.get(r).map(Vec::as_slice).unwrap_or(&[]);
            if ls.iter().any(|x| under(&m.elements[*x], &o.covering_scope)) {
                direct.insert(*r);
            } else if let Some(ends) = &ends {
                // Implied coverage is read only for the requirements that are counted.
                if counted_set.contains(r) {
                    let fresh = !reach.contains_key(r);
                    let rs = reach.entry(*r).or_insert_with(|| implied::reach_set(m, *r));
                    if fresh && rs.len() + 1 >= implied::MAX_VISITED {
                        reach_bound += 1;
                    }
                    if fresh {
                        steps = steps.saturating_add(rs.len() + 1);
                    }
                    if rs.iter().any(|x| ends.contains(x) && under(&m.elements[*x], &o.covering_scope)) {
                        implied_set.insert(*r);
                    }
                }
            }
        }
        let total_own: HashSet<ElementId> = direct.union(&implied_set).copied().collect();
        let row = |d: &HashSet<ElementId>, t: &HashSet<ElementId>| {
            let dn = counted.iter().filter(|r| d.contains(r)).count();
            let tn = counted.iter().filter(|r| t.contains(r)).count();
            json!({ "covered": tn, "direct": dn, "implied": tn - dn, "not_covered": n - tn, "percent": pct(tn, n), "percent_direct": pct(dn, n) })
        };
        let (gd, gt) = (grouping(m, &direct, &all), grouping(m, &total_own, &all));
        measures.insert(name.to_string(), json!({ "requirement_rule": row(&direct, &total_own), "grouping_rule": row(&gd, &gt), "implied_defined": ends.is_some() }));
    }
    let mut v = json!({ "requirements": n, "measures": Value::Object(measures), "steps": steps, "implied_reach_cut": reach_bound });
    graph::mark_cut(m, &mut v, &cuts);
    Ok(v)
}
