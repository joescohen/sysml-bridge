//! An index of the connections and interfaces of a model: for each connection usage its two ends (the part usages each
//! path names and the port it ends on), the items it conveys, and its owner; with the part usages typed by each
//! definition and the requirements each element satisfies. Built once per question and read only.

use super::graph;
use crate::model::*;
use std::collections::{BTreeSet, HashMap};

pub(crate) fn type_of(model: &Model, id: ElementId) -> Option<ElementId> {
    model.elements[id].refs_of(RefRole::Type).find_map(|r| r.resolved())
}

fn is_part_usage(e: &Element) -> bool {
    e.kind == "part" && !e.is_def
}

/// One end of a connection: the part usages its path names (outermost first), and the port it ends on.
#[derive(Clone, Default)]
pub(crate) struct End {
    pub(crate) parts: Vec<ElementId>,
    pub(crate) port: Option<ElementId>,
}

impl End {
    pub(crate) fn last(&self) -> Option<ElementId> {
        self.parts.last().copied()
    }
}

fn end_info(model: &Model, via: &Element, r: &Reference) -> End {
    let up: BTreeSet<ElementId> = std::iter::successors(via.owner, |i| model.elements[*i].owner).collect();
    let mut out = End::default();
    for s in r.segments.iter().skip_while(|s| s.resolved.map(|i| up.contains(&i)).unwrap_or(false)) {
        let Some(id) = s.resolved else { break };
        let e = &model.elements[id];
        if e.kind == "port" {
            out.port = Some(id);
            break;
        }
        if e.kind == "part" && !e.is_def {
            out.parts.push(id);
        } else {
            break;
        }
    }
    out
}

/// A connection or interface usage of the model with what it conveys.
pub(crate) struct Conn {
    pub(crate) id: ElementId,
    pub(crate) owner: Option<ElementId>,
    pub(crate) ends: [End; 2],
    /// (item definition, the flow or item flow statement that conveys it).
    pub(crate) items: Vec<(ElementId, ElementId)>,
}

/// The requirements each element satisfies.
pub(crate) struct Satisfied {
    /// Requirement by each element that satisfies it (satisfy, or a trace link between a requirement and another element).
    pub(crate) satisfied: HashMap<ElementId, BTreeSet<ElementId>>,
}

fn satisfied_index(model: &Model) -> Satisfied {
    let mut satisfied: HashMap<ElementId, BTreeSet<ElementId>> = HashMap::new();
    let req = |x: ElementId| model.elements[x].kind.starts_with("requirement");
    // A usage that satisfies also stands for its definition.
    let mut note = |x: ElementId, r: ElementId| {
        satisfied.entry(x).or_default().insert(r);
        if !model.elements[x].is_def {
            for t in model.elements[x].refs_of(RefRole::Type).filter_map(|t| t.resolved()) {
                satisfied.entry(t).or_default().insert(r);
            }
        }
    };
    for s in graph::statements(model) {
        let (Some(a), Some(b)) = (s.source.id, s.target.id) else { continue };
        match s.kind {
            // Satisfy, refine, trace or a dependency between a requirement and another element: the element meets or details it.
            "verify" | "derive" | "allocate" => {}
            _ if req(a) != req(b) => {
                let (r, x) = if req(a) { (a, b) } else { (b, a) };
                note(x, r);
            }
            _ => {}
        }
    }
    // A requirement whose subject is typed by a definition is satisfied by that definition.
    for r in model.elements.iter().filter(|e| e.kind.starts_with("requirement")) {
        for c in r.children.iter().map(|c| &model.elements[*c]).filter(|c| c.kind.contains("subject") || c.has_prefix("subject")) {
            for t in c.refs_of(RefRole::Type).filter_map(|x| x.resolved()) {
                note(t, r.id);
            }
        }
    }
    Satisfied { satisfied }
}

/// The connections of a model, the usages of each part definition, and what each element satisfies.
pub(crate) struct Ix {
    pub(crate) conns: Vec<Conn>,
    pub(crate) seq: Satisfied,
    /// Connection index by element id.
    pub(crate) conn_at: HashMap<ElementId, usize>,
    /// Part definition -> the part usages typed by it.
    pub(crate) usages: HashMap<ElementId, Vec<ElementId>>,
}

impl Ix {
    pub(crate) fn new(model: &Model) -> Ix {
        let stmts = graph::statements(model);
        let conveyed = super::connect::conveyed_on(model, &stmts);
        let mut conns = Vec::new();
        let mut conn_at = HashMap::new();
        for e in model.elements.iter().filter(|e| !e.is_def && matches!(e.kind.as_str(), "connection" | "interface")) {
            let (Some(s), Some(t)) = (e.refs_of(RefRole::Source).next(), e.refs_of(RefRole::Target).next()) else { continue };
            let ends = [end_info(model, e, s), end_info(model, e, t)];
            conn_at.insert(e.id, conns.len());
            conns.push(Conn { id: e.id, owner: e.owner, ends, items: conveyed.get(&e.id).cloned().unwrap_or_default() });
        }
        let mut usages: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
        for e in model.elements.iter().filter(|e| is_part_usage(e) && !e.owner.is_some_and(|o| model.elements[o].kind.ends_with("action"))) {
            if let Some(t) = type_of(model, e.id) {
                usages.entry(t).or_default().push(e.id);
            }
        }
        Ix { conns, seq: satisfied_index(model), conn_at, usages }
    }
}
