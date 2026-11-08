//! Relation map (`get_relation_map`): from one context element, a breadth-first walk over the relationships a reader chose (derive,
//! satisfy, verify, refine, trace, allocate, containment, type, specialization), each in a direction, to a depth (2 unless asked),
//! returning the nodes and edges; a client lays them out. Each element is one node by default; a second arrival is an edge marked
//! `back_link`. A requirement the active criteria leave without a satisfier or a verifier carries a gap flag. The walk is bounded
//! (500 nodes in the reply); the reply says how many elements it found past the bound.

use super::graph;
use super::line_of;
use super::{Fail, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub const MAX_NODES: usize = 500;
pub const MAX_FOUND: usize = 20_000;
pub const MAX_DEPTH: usize = 10;
pub const DEFAULT_DEPTH: usize = 2;
/// Most entries in a list of the request and characters in one text of it.
pub const MAX_REQUEST_LIST: usize = 50;
pub const MAX_REQUEST_TEXT: usize = 500;
/// Most criteria in one call.
/// Most implied steps read for one node.
pub const MAX_IMPLIED_PER_NODE: usize = 200;
pub const MAX_CRITERIA: usize = 12;

/// The criteria a map can follow, with the words of the link statements they read.
pub const KINDS: &[&str] = &["derive", "satisfy", "verify", "refine", "trace", "allocate", "containment", "type", "specialization"];

/// The presets: the five of the source tool by name, and two of ours.
pub const PRESETS: &[&str] = &["requirement_down", "design_up", "activity_decomposition", "structure_decomposition", "instance_map", "requirement_containment", "requirement_derivation"];

#[derive(Clone)]
pub struct Criterion {
    pub kind: String,
    /// `to_source`, `to_target` or `both`.
    pub direction: String,
}

#[derive(Default)]
pub struct Opts {
    pub context: String,
    pub preset: Option<String>,
    pub criteria: Vec<Criterion>,
    pub types: Vec<String>,
    pub scope: Vec<String>,
    /// 0 means the default (2); more than `MAX_DEPTH` is cut to it.
    pub depth: usize,
    pub all_depth: bool,
    pub repeat: bool,
    /// Also follow the links the implied rule adds (a link drawn on an element a node is part of, typed by or specializes).
    pub implied: bool,
}

fn crit(kind: &str, direction: &str) -> Criterion {
    Criterion { kind: kind.into(), direction: direction.into() }
}

fn preset_criteria(p: &str) -> Option<Vec<Criterion>> {
    Some(match p {
        "requirement_down" => vec![crit("derive", "to_source"), crit("satisfy", "to_source"), crit("verify", "to_source"), crit("refine", "to_source")],
        "design_up" => vec![crit("satisfy", "to_target"), crit("allocate", "to_source"), crit("type", "to_target")],
        "activity_decomposition" | "structure_decomposition" | "instance_map" | "requirement_containment" => vec![crit("containment", "to_source")],
        "requirement_derivation" => vec![crit("derive", "to_source")],
        _ => return None,
    })
}

/// The types a preset keeps when the call names none.
fn preset_types(p: &str) -> Vec<String> {
    match p {
        "activity_decomposition" => vec!["action".into(), "action def".into()],
        "structure_decomposition" => vec!["part".into(), "part def".into()],
        "requirement_containment" | "requirement_derivation" => vec!["requirement".into(), "requirement def".into()],
        _ => Vec::new(),
    }
}

fn kind_word(k: &str) -> &'static str {
    match k {
        "type" => "typing",
        "derive" => "derive",
        "satisfy" => "satisfy",
        "verify" => "verify",
        "refine" => "refine",
        "trace" => "trace",
        "allocate" => "allocate",
        "specialization" => "specialization",
        _ => "containment",
    }
}

fn is_req(e: &Element) -> bool {
    e.kind == "requirement"
}

fn under(e: &Element, scope: &[String]) -> bool {
    scope.is_empty() || scope.iter().any(|s| e.qualified_name == *s || e.qualified_name.starts_with(&format!("{s}::")))
}

/// One step of the walk: the elements next to `x` for one criterion, with the link that joins them.
struct Step {
    other: ElementId,
    kind: &'static str,
    direction: &'static str,
    via: Option<ElementId>,
    /// For an implied step: the element the link is drawn on, which the node reaches.
    anchor: Option<ElementId>,
}

struct Links {
    /// (kind word) -> element -> (other, via) where the element is the target / the source.
    as_target: HashMap<(&'static str, ElementId), Vec<(ElementId, ElementId)>>,
    as_source: HashMap<(&'static str, ElementId), Vec<(ElementId, ElementId)>>,
}

fn steps(m: &Model, links: &Links, crits: &[Criterion], x: ElementId) -> Vec<Step> {
    let mut out = Vec::new();
    for c in crits {
        let k = kind_word(&c.kind);
        let kk: &'static str = KINDS.iter().find(|n| **n == c.kind).copied().unwrap_or("containment");
        if k == "containment" {
            let e = &m.elements[x];
            if c.direction != "to_target" {
                for ch in &e.children {
                    if m.elements[*ch].name.is_some() {
                        out.push(Step { other: *ch, kind: kk, direction: "to_source", via: None, anchor: None });
                    }
                }
            }
            if c.direction != "to_source" {
                if let Some(o) = e.owner {
                    out.push(Step { other: o, kind: kk, direction: "to_target", via: None, anchor: None });
                }
            }
            continue;
        }
        if c.direction != "to_target" {
            for (o, v) in links.as_target.get(&(k, x)).into_iter().flatten() {
                out.push(Step { other: *o, kind: kk, direction: "to_source", via: Some(*v), anchor: None });
            }
        }
        if c.direction != "to_source" {
            for (o, v) in links.as_source.get(&(k, x)).into_iter().flatten() {
                out.push(Step { other: *o, kind: kk, direction: "to_target", via: Some(*v), anchor: None });
            }
        }
    }
    out
}

/// The size of the request, tested before the model is read: criteria, types, scope entries and the length of each text.
pub fn check_request(o: &Opts) -> Res<()> {
    if o.criteria.len() > MAX_CRITERIA {
        return Err(Fail::from(format!("a map follows at most {MAX_CRITERIA} criteria, not {}.", o.criteria.len())));
    }
    for (what, n) in [("types", o.types.len()), ("scope entries", o.scope.len())] {
        if n > MAX_REQUEST_LIST {
            return Err(Fail::from(format!("{what}: {n} given; at most {MAX_REQUEST_LIST} are accepted. Send fewer.")));
        }
    }
    let texts = std::iter::once(&o.context).chain(o.preset.iter()).chain(&o.types).chain(&o.scope).chain(o.criteria.iter().flat_map(|c| [&c.kind, &c.direction]));
    for t in texts {
        if t.len() > MAX_REQUEST_TEXT {
            return Err(Fail::from(format!("a text argument is {} characters; at most {MAX_REQUEST_TEXT} are accepted.", t.len())));
        }
    }
    Ok(())
}

pub fn relation_map(m: &Model, o: &Opts) -> Res<Value> {
    check_request(o)?;
    let ctx = m.find(o.context.trim()).ok_or_else(|| Fail::from(format!("context {} is not an element of the model. Give the qualified name in the argument context, for example Package::Element.", super::text::quote_clip(&o.context))))?;
    let preset = o.preset.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let mut crits: Vec<Criterion> = o.criteria.clone();
    let mut types = o.types.clone();
    if let Some(p) = preset {
        let base = preset_criteria(p).ok_or_else(|| Fail::from(format!("preset is one of {}, not {}.", PRESETS.join(", "), super::text::quote_clip(p))))?;
        if crits.is_empty() {
            crits = base;
        }
        if types.is_empty() {
            types = preset_types(p);
        }
    }
    if crits.is_empty() {
        crits = preset_criteria("requirement_down").unwrap();
    }
    if crits.len() > MAX_CRITERIA {
        return Err(Fail::from(format!("a map follows at most {MAX_CRITERIA} criteria, not {}.", crits.len())));
    }
    for c in &crits {
        if !KINDS.contains(&c.kind.as_str()) {
            return Err(Fail::from(format!("criterion kind is one of {}, not {}.", KINDS.join(", "), super::text::quote_clip(&c.kind))));
        }
        if !matches!(c.direction.as_str(), "to_source" | "to_target" | "both") {
            return Err(Fail::from(format!("criterion direction is to_source, to_target or both, not {}.", super::text::quote_clip(&c.direction))));
        }
    }
    let depth = if o.all_depth { MAX_DEPTH } else if o.depth == 0 { DEFAULT_DEPTH } else { o.depth.min(MAX_DEPTH) };
    let (edges, cuts) = graph::edges_cut(m);
    let mut links = Links { as_target: HashMap::new(), as_source: HashMap::new() };
    for (s, a, b) in &edges {
        let (a, b) = (graph::unalias(m, *a), graph::unalias(m, *b));
        let k: &'static str = s.kind;
        links.as_target.entry((k, b)).or_default().push((a, s.via));
        links.as_source.entry((k, a)).or_default().push((b, s.via));
    }
    // A part performing an action is an allocation too, read from the one list every view reads (stated ones only).
    if crits.iter().any(|c| c.kind == "allocate") {
        for al in super::allocation::allocations(m).into_iter().filter(|al| al.via == "perform" && al.is_direct()) {
            links.as_target.entry(("allocate", al.part)).or_default().push((al.function, al.statement));
            links.as_source.entry(("allocate", al.function)).or_default().push((al.part, al.statement));
        }
    }
    let keeps = |x: ElementId| -> bool {
        let e = &m.elements[x];
        (types.is_empty() || types.iter().any(|t| e.keyword() == *t || (e.kind == *t && !e.is_def))) && under(e, &o.scope)
    };
    // The walk. A node is (element, depth, parent node); with `repeat` an element can stand in many nodes, never twice on one path.
    struct Node {
        el: ElementId,
        depth: usize,
        parent: Option<usize>,
    }
    let mut nodes: Vec<Node> = vec![Node { el: ctx.id, depth: 0, parent: None }];
    let mut seen: HashMap<ElementId, usize> = HashMap::from([(ctx.id, 0)]);
    let mut out_edges: Vec<Value> = Vec::new();
    let mut found: HashSet<ElementId> = HashSet::from([ctx.id]);
    let mut queue: VecDeque<usize> = VecDeque::from([0]);
    let mut has_more: HashMap<usize, usize> = HashMap::new();
    let mut hidden_by_bound = 0usize;
    let mut implied_cut = false;
    // A link read from both of its ends is drawn once.
    let mut drawn_links: HashSet<(ElementId, ElementId, ElementId)> = HashSet::new();
    while let Some(ni) = queue.pop_front() {
        let (x, d) = (nodes[ni].el, nodes[ni].depth);
        let mut next = steps(m, &links, &crits, x);
        if o.implied {
            // The links drawn on what the node is part of, typed by or specializes: one walk up (bounded), then the same lookup.
            let mut added = 0usize;
            let reach = super::implied::reach_set(m, x);
            // The reach walk stops at its visit bound without saying so: a full set is treated as possibly cut.
            if reach.len() + 1 >= super::implied::MAX_VISITED {
                implied_cut = true;
            }
            let relations: Vec<Criterion> = crits.iter().filter(|c| kind_word(&c.kind) != "containment").cloned().collect();
            'up: for a in reach {
                for s in steps(m, &links, &relations, a) {
                    if added >= MAX_IMPLIED_PER_NODE {
                        implied_cut = true;
                        break 'up;
                    }
                    added += 1;
                    next.push(Step { anchor: Some(a), ..s });
                }
            }
        }
        if d >= depth {
            let unseen = next.iter().filter(|s| keeps(s.other) && (o.repeat || !seen.contains_key(&s.other))).count();
            if unseen > 0 {
                has_more.insert(ni, unseen);
            }
            continue;
        }
        let mut here: HashSet<(ElementId, &str)> = HashSet::new();
        for s in next {
            if !keeps(s.other) || !here.insert((s.other, s.kind)) {
                continue;
            }
            if let Some(v) = s.via {
                if !drawn_links.insert((v, x.min(s.other), x.max(s.other))) {
                    continue;
                }
            }
            let via = s.via.map(|v| json!({ "ref": m.elements[v].qualified_name, "file": m.files[m.elements[v].file].path, "line": line_of(m, &m.elements[v]) }));
            // The basis of the edge: direct, or implied with the path from the node to the element the link is drawn on.
            let basis = match s.anchor {
                None => json!("direct"),
                Some(_) => json!("implied"),
            };
            let through = s.anchor.and_then(|a| super::implied::up_path(m, x, a)).map(|p| super::implied::through_json(m, x, &p));
            let ancestors = |mut n: usize| -> bool {
                loop {
                    if nodes[n].el == s.other {
                        return true;
                    }
                    match nodes[n].parent {
                        Some(p) => n = p,
                        None => return false,
                    }
                }
            };
            if !o.repeat {
                if let Some(&to) = seen.get(&s.other) {
                    out_edges.push(json!({ "from": ni, "to": to, "kind": s.kind, "direction": s.direction, "via": via, "basis": basis, "through": through, "back_link": true }));
                    continue;
                }
            } else if ancestors(ni) {
                continue;
            }
            found.insert(s.other);
            if nodes.len() >= MAX_NODES {
                hidden_by_bound += 1;
                if found.len() > MAX_FOUND {
                    break;
                }
                continue;
            }
            let at = nodes.len();
            nodes.push(Node { el: s.other, depth: d + 1, parent: Some(ni) });
            if !o.repeat {
                seen.insert(s.other, at);
            }
            out_edges.push(json!({ "from": ni, "to": at, "kind": s.kind, "direction": s.direction, "via": via, "basis": basis, "through": through, "back_link": false }));
            queue.push_back(at);
        }
    }
    // Gap flags: only for the criteria that are on.
    let follows = |k: &str| crits.iter().any(|c| c.kind == k);
    let (mut no_sat, mut no_ver) = (0usize, 0usize);
    let rows: Vec<Value> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let e = &m.elements[n.el];
            let mut j = super::brief(m, e);
            j["id"] = json!(i);
            j["depth"] = json!(n.depth);
            if let Some(c) = has_more.get(&i) {
                j["has_more"] = json!(c);
            }
            if is_req(e) && !e.is_def {
                let missing = |k: &str| links.as_target.get(&(k, n.el)).map(|v| v.is_empty()).unwrap_or(true);
                let g_sat = follows("satisfy") && missing("satisfy");
                let g_ver = follows("verify") && missing("verify");
                no_sat += usize::from(g_sat);
                no_ver += usize::from(g_ver);
                if g_sat || g_ver {
                    j["gap"] = json!({ "no_satisfier": g_sat, "no_verifier": g_ver });
                }
            }
            j
        })
        .collect();
    let levels = nodes.iter().map(|n| n.depth).max().unwrap_or(0);
    let mut bounds: Vec<String> = Vec::new();
    if hidden_by_bound > 0 {
        bounds.push(format!("nodes: showing {} of {}{}; narrow the criteria, the types or the depth", nodes.len(), found.len(), if found.len() > MAX_FOUND { " or more" } else { "" }));
    }
    if !o.all_depth && o.depth > MAX_DEPTH {
        bounds.push(format!("depth: cut to {MAX_DEPTH}"));
    }
    if implied_cut {
        bounds.push(format!("implied: a node had more than {MAX_IMPLIED_PER_NODE} implied links or a reach past the visit bound; the implied edges drawn are not all of them, and no implied link is declared absent"));
    }
    if o.implied {
        bounds.push(format!("implied: edges marked basis implied follow the rule in get_traceability ({}); at most {MAX_IMPLIED_PER_NODE} for a node", super::implied::RULE));
    } else {
        bounds.push("implied links are not drawn; pass implied: true to draw them, or use get_traceability".into());
    }
    let mut reply = json!({
        "context": super::brief(m, ctx),
        "preset": preset,
        "criteria": crits.iter().map(|c| json!({ "kind": c.kind, "direction": c.direction })).collect::<Vec<_>>(),
        "types": types,
        "scope": o.scope,
        "depth": depth,
        "single_node": !o.repeat,
        "nodes": rows,
        "edges": out_edges,
        "summary": { "elements": nodes.len(), "found": found.len(), "levels": levels, "no_satisfier": no_sat, "no_verifier": no_ver, "edges": out_edges_len(&nodes) },
        "truncated": hidden_by_bound > 0 || implied_cut,
        "bounds": bounds,
        "limits": { "nodes": MAX_NODES, "depth": MAX_DEPTH },
    });
    reply["summary"]["edges"] = json!(reply["edges"].as_array().map(Vec::len).unwrap_or(0));
    graph::mark_cut(m, &mut reply, &cuts);
    Ok(reply)
}

fn out_edges_len<T>(_n: &[T]) -> usize {
    0
}

/// A relation map kept from an import: the definition only (the map is drawn here from it).
pub struct Def {
    pub key: String,
    pub title: String,
    pub context: String,
    pub criteria: Vec<String>,
    pub depth: usize,
}

/// The relation-map definitions of one `.diagrams.json` file.
pub fn definitions(file: &str, text: &str) -> Vec<Def> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["kind"] == "relation_map")
        .take(MAX_DEFS)
        .map(|d| Def {
            key: format!("stored|{}|{}", file, d["id"].as_str().unwrap_or("")),
            title: d["name"].as_str().unwrap_or("").chars().take(200).collect(),
            context: d["definition"]["context"].as_str().unwrap_or("").chars().take(500).collect(),
            criteria: d["definition"]["criteria"].as_array().into_iter().flatten().filter_map(|c| c.as_str()).map(|c| c.chars().take(120).collect()).take(MAX_CRITERIA).collect(),
            depth: d["definition"]["depth"].as_u64().unwrap_or(0) as usize,
        })
        .collect()
}

/// Most definitions listed.
pub const MAX_DEFS: usize = 200;

/// The criterion a source tool's criterion name stands for, if it is one this reader walks.
pub fn criterion_of(name: &str) -> Option<&'static str> {
    let n: String = name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
    Some(match n.as_str() {
        "satisfy" | "satisfies" | "satisfiedby" => "satisfy",
        "verify" | "verifies" | "verifiedby" => "verify",
        "derive" | "derivereqt" | "derived" | "derivedfrom" | "derivation" => "derive",
        "refine" | "refines" | "refinedby" => "refine",
        "trace" | "tracedto" | "tracedfrom" => "trace",
        "allocate" | "allocated" | "allocatedto" | "allocatedfrom" => "allocate",
        "owner" | "ownedelements" | "owned" | "containment" | "ownedby" => "containment",
        "generalization" | "specialization" | "generalize" | "specialize" => "specialization",
        "type" | "typedby" | "typing" => "type",
        _ => return None,
    })
}
