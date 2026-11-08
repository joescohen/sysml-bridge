//! Relationships read from the element graph: typing, specialization,
//! trace links, connectors. One extraction serves queries, traceability
//! and the semantic rules, so they cannot disagree.

use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

/// One end of a relationship statement. `id` is `None` when the name does not resolve.
#[derive(Debug, Clone)]
pub struct End {
    pub text: String,
    pub id: Option<ElementId>,
    /// The elements a feature chain passes through on its way to the end (see `through`), worked out once for the
    /// statement, so a statement with many ends does not search its references again for each pair.
    chain: Arc<Vec<ElementId>>,
}

/// Most source-target pairs one statement yields. `dependency from a1..aN to b1..bN` states N times N relationships in
/// a text of 2N names; the rest are counted (`Cut`) and not made.
pub const MAX_PAIRS_PER_STATEMENT: usize = 10_000;
/// Most pairs `statements` yields for the whole model.
pub const MAX_PAIRS_TOTAL: usize = 1_000_000;
/// Most ends of one role one statement is read with. `dependency from a, a, ... to b` of 50 000 names would
/// otherwise build 50 000 chains; the ends past the limit are counted (`Cut`) and not read.
pub const MAX_ENDS_PER_STATEMENT: usize = 20_000;

/// A statement whose pairs `statements` cut short.
#[derive(Debug, Clone)]
pub struct Cut {
    pub via: ElementId,
    pub sources: usize,
    pub targets: usize,
    pub kept: usize,
    /// The model-wide limit `MAX_PAIRS_TOTAL` cut this statement, so statements after it in file order are not read at all.
    pub by_total: bool,
}

impl Cut {
    pub fn pairs(&self) -> u64 {
        self.sources as u64 * self.targets as u64
    }
}

/// The notice a read tool carries when statements were cut: its answer is built from relationship pairs, so a link may be
/// missing and "no links" proves nothing. `None` when nothing was cut.
pub fn read_notice(cuts: &[Cut]) -> Option<String> {
    if cuts.is_empty() {
        return None;
    }
    let n = cuts.len();
    Some(if cuts.iter().any(|c| c.by_total) {
        format!("not checked: {n} statements were cut because the model states more than {MAX_PAIRS_TOTAL} relationship pairs (the limit for one run), so this answer can leave links out and an empty answer proves nothing. Fix: split the repeated or very large statements, then ask again.")
    } else {
        format!("not checked: {n} statements were cut because each states more than {MAX_PAIRS_PER_STATEMENT} source-target pairs and only the first {MAX_PAIRS_PER_STATEMENT} pairs of each are listed, so this answer can leave links out. Fix: split those statements, then ask again.")
    })
}

/// Put `pairs_truncated` and the `not_checked` notice into a reply when statements were cut; leave it alone otherwise.
pub fn mark_cut(model: &Model, reply: &mut Value, cuts: &[Cut]) {
    if let Some(t) = cut_json(model, cuts) {
        reply["not_checked"] = t["not_checked"].clone();
        reply["pairs_truncated"] = t;
    }
}

/// `truncated` for a reply: absent when nothing was cut. The real number of pairs is given for each statement.
pub fn cut_json(model: &Model, cuts: &[Cut]) -> Option<Value> {
    if cuts.is_empty() {
        return None;
    }
    Some(json!({
        "not_checked": read_notice(cuts),
        "statements": cuts.len(),
        "pairs_listed": cuts.iter().map(|c| c.kept as u64).sum::<u64>(),
        "pairs_stated": cuts.iter().map(Cut::pairs).sum::<u64>(),
        "limit_per_statement": MAX_PAIRS_PER_STATEMENT,
        "truncated_at": format!("truncated at {} of {} pairs ({} statement(s) cut; at most {MAX_PAIRS_PER_STATEMENT} pairs a statement, {MAX_PAIRS_TOTAL} in the model)", cuts.iter().map(|c| c.kept as u64).sum::<u64>(), cuts.iter().map(Cut::pairs).sum::<u64>(), cuts.len()),
        "note": "a statement with many sources and many targets states every source-target pair; only the first pairs of each are listed. The text is unchanged.",
        "items": cuts.iter().take(20).map(|c| json!({
            "statement": model.elements[c.via].qualified_name,
            "sources": c.sources,
            "targets": c.targets,
            "pairs": c.pairs(),
            "listed": c.kept,
        })).collect::<Vec<_>>(),
    }))
}

/// The statements of the whole model, or the reason they cannot all be read: the model-wide pair limit hid some
/// statements, so a check built on the list would pass without having seen them. A check that needs every statement
/// uses this, and refuses or reports "not checked" on `Err`.
pub fn statements_checked(model: &Model) -> Result<Vec<Stmt>, String> {
    let (out, cuts) = statements_cut(model);
    match check_notice(&cuts) {
        None => Ok(out),
        Some(why) => Err(why),
    }
}

/// The sentence of a check that did not read every relationship pair, whatever cut them: the limit for the model, or the
/// limit for one statement (its pairs or its ends). `None` when nothing was cut. A check that reads the pairs never
/// passes on a part of them.
pub fn check_notice(cuts: &[Cut]) -> Option<String> {
    if cuts.is_empty() {
        return None;
    }
    Some(match cuts.iter().filter(|c| c.by_total).count() {
        0 => format!(
            "not checked: {} statement(s) each state more than {MAX_PAIRS_PER_STATEMENT} source-target pairs or more than {MAX_ENDS_PER_STATEMENT} ends, and only the first of them were read, so this check cannot say whether anything is missing, repeated or wrong. Fix: split each such statement into statements with fewer ends, then run it again.",
            cuts.len()
        ),
        n => not_checked(n),
    })
}

/// True for an element that states relationships: what `statements` reads pairs from.
pub fn is_statement(e: &Element) -> bool {
    matches!(e.kind.as_str(), "satisfy" | "satisfy requirement" | "verify" | "verify requirement" | "allocation" | "dependency" | "connection" | "interface" | "flow" | "binding" | "succession" | "transition")
}

/// The rule id of the finding a gate gives when the model-wide pair limit hid statements from it.
pub const CUT_RULE: &str = "not-fully-checked";

/// The error finding that makes a gate fail, not pass, when `cuts` hid pairs from it: statements the limit for the model
/// left out, or one statement read only in part. `None` when it saw all.
pub fn cut_finding(model: &Model, cuts: &[Cut], gate: &str) -> Option<Value> {
    let message = check_notice(cuts)?;
    let total = cuts.iter().any(|c| c.by_total);
    let first = cuts.iter().find(|c| c.by_total == total).map(|c| &model.elements[c.via]);
    Some(json!({
        "gate": gate, "rule": CUT_RULE, "severity": "error",
        "element": first.map(|e| e.qualified_name.clone()),
        "file": first.map(|e| model.files[e.file].path.clone()),
        "line": first.map(|e| super::line_of(model, e)),
        "message": message,
        "fix": if total { format!("Split the very large or repeated statements so the model states fewer than {MAX_PAIRS_TOTAL} relationship pairs, then run the gate again.") } else { "Split each such statement into statements with fewer ends, then run the gate again.".to_string() },
    }))
}

/// The error finding each gate gives for a file that is not UTF-8 text: the gate read other text than the disk holds, so
/// it may not pass. One finding for each such file.
pub fn unread_findings(model: &Model, gate: &str) -> Vec<Value> {
    model
        .files
        .iter()
        .filter_map(|f| {
            let at = f.text.find(crate::lexer::NOT_TEXT)?;
            Some(json!({
                "gate": gate, "rule": CUT_RULE, "severity": "error", "element": null,
                "file": f.path, "line": f.text[..at].matches('\n').count() + 1,
                "message": format!("not checked: {}.", crate::lexer::NOT_TEXT_MESSAGE),
                "fix": "Save the file as UTF-8 by hand (the tools do not write over bytes they cannot read), then run the gate again.",
            }))
        })
        .collect()
}

/// The plain sentence for a check that could not read the whole model.
pub fn not_checked(n: usize) -> String {
    format!(
        "not checked: {n} statements were cut because the model states more than {MAX_PAIRS_TOTAL} relationship pairs (the limit for one run), so this check cannot say whether anything is missing, repeated or wrong. Fix: split the repeated or very large statements, then run it again."
    )
}

/// A relationship as written in the text.
#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: &'static str,
    /// The element whose text declares it.
    pub via: ElementId,
    /// `statement` for a keyword form, `dependency` for a tagged `dependency`, `ref` for typing and specialization.
    pub form: &'static str,
    pub source: End,
    pub target: End,
}

pub const TRACE_KINDS: &[&str] = &["satisfy", "verify", "allocate", "derive", "refine"];

/// The element an alias stands for; any other element is itself. An alias whose target does not resolve (`alias final for done`)
/// stays: it is the end.
pub fn unalias(model: &Model, mut id: ElementId) -> ElementId {
    for _ in 0..8 {
        let e = &model.elements[id];
        match (e.kind == "alias").then(|| e.first_resolved(RefRole::Target)).flatten() {
            Some(t) => id = t,
            None => break,
        }
    }
    id
}

/// An end of a statement. A name that is an alias is the element it stands for, so a statement that says `final` and one that
/// says the full name of what `final` stands for are the same statement.
fn end_of(model: &Model, r: &Reference) -> End {
    End { text: r.text.clone(), id: r.resolved().map(|i| unalias(model, i)), chain: Arc::default() }
}

/// What the references of one statement pass through, by the text of the reference: the resolved elements of every
/// segment of every end-like reference (`Source`, `Target`, `Type`) that has that text. Built once for a statement,
/// once for each distinct text (the same text has the same segments), so repeating an end adds no work.
fn chain_index<'a>(model: &Model, e: &'a Element) -> HashMap<&'a str, Vec<ElementId>> {
    let mut by_text: HashMap<&str, Vec<ElementId>> = HashMap::new();
    for r in e.refs.iter().filter(|r| matches!(r.role, RefRole::Source | RefRole::Target | RefRole::Type)) {
        by_text.entry(r.text.as_str()).or_insert_with(|| r.segments.iter().filter_map(|g| g.resolved).map(|x| unalias(model, x)).collect());
    }
    by_text
}

/// An end of a statement together with the elements its feature chain passes through. `memo` holds the finished end
/// of each distinct text, so a text repeated N times is worked out once and shared.
fn end_with_chain<'a>(model: &Model, r: &'a Reference, index: &HashMap<&str, Vec<ElementId>>, memo: &mut HashMap<&'a str, End>) -> End {
    if let Some(e) = memo.get(r.text.as_str()) {
        return e.clone();
    }
    let mut end = end_of(model, r);
    if let (Some(last), Some(all)) = (end.id, index.get(r.text.as_str())) {
        let mut up: BTreeSet<ElementId> = BTreeSet::new();
        let mut o = model.elements[last].owner;
        while let Some(x) = o {
            if !up.insert(x) {
                break;
            }
            o = model.elements[x].owner;
        }
        end.chain = Arc::new(all.iter().copied().filter(|x| *x != last && !up.contains(x)).collect());
    }
    memo.insert(r.text.as_str(), end.clone());
    end
}

/// The elements a feature chain passes through on its way to the end of a statement: `satisfy R by sys.engine.power`
/// passes through `sys` and `engine`. The path that qualifies a name (`A::B::C`) is no chain: the owners of the end are
/// left out, so a statement hits the same elements whether it is written short or in full.
/// The end itself is not in the list. An end that does not resolve passes through nothing.
pub fn through(_model: &Model, _via: ElementId, end: &End) -> Vec<ElementId> {
    end.chain.to_vec()
}

fn owner_end(model: &Model, id: ElementId) -> End {
    End { text: model.elements[id].qualified_name.clone(), id: Some(id), chain: Arc::default() }
}

pub(crate) fn tag_kind(e: &Element) -> Option<&'static str> {
    let tag = e.prefixes.iter().find_map(|p| p.strip_prefix('#'))?;
    tag_edge_kind(tag)
}

/// The edge kind a metadata tag such as `SysMLv1::Verify` gives a `dependency`.
pub fn tag_edge_kind(tag: &str) -> Option<&'static str> {
    Some(match tag.rsplit("::").next().unwrap_or(tag) {
        "DeriveReqt" => "derive",
        "Refine" => "refine",
        "Trace" => "trace",
        "Verify" => "verify",
        "Satisfy" => "satisfy",
        "Allocate" => "allocate",
        // An information flow of an imported SysML v1 project. It is a flow of items, not a dependency.
        "ItemFlow" => "itemflow",
        _ => return None,
    })
}

/// An item flow of an imported project: `#SysMLv1::ItemFlow dependency 'flow for X' from A to B { comment conveyed about X;
/// comment realizedBy about C; }`. The `conveyed` note names the items; the `realizedBy` note names the connection that
/// carries the flow. A flow without that note has no stated carrier, and none is guessed.
#[derive(Debug, Clone)]
pub struct ItemFlow {
    /// The dependency statement.
    pub via: ElementId,
    pub source: End,
    pub target: End,
    /// What the flow conveys.
    pub items: Vec<End>,
    /// The connections named by `realizedBy`.
    pub carriers: Vec<End>,
}

/// The item flow behind a statement of kind `itemflow`.
pub fn item_flow(model: &Model, s: &Stmt) -> ItemFlow {
    let mut items = Vec::new();
    let mut carriers = Vec::new();
    for c in model.children(s.via).filter(|c| c.kind == "comment") {
        let list = match c.name.as_deref() {
            Some("conveyed") => &mut items,
            Some("realizedBy") => &mut carriers,
            _ => continue,
        };
        list.extend(c.refs_of(RefRole::Other).map(|r| end_of(model, r)));
    }
    ItemFlow { via: s.via, source: s.source.clone(), target: s.target.clone(), items, carriers }
}

/// The verification case that owns a `verify` statement, skipping its objective.
pub fn verifier_of(model: &Model, e: &Element) -> Option<ElementId> {
    let mut o = e.owner;
    while let Some(x) = o {
        if model.elements[x].kind == "objective" {
            o = model.elements[x].owner;
        } else {
            break;
        }
    }
    o
}

/// Every relationship statement, with unresolved ends kept so checks can see them. A statement with many sources and many
/// targets yields at most `MAX_PAIRS_PER_STATEMENT` pairs, and the model at most `MAX_PAIRS_TOTAL`; `statements_cut`
/// says what was left out.
pub fn statements(model: &Model) -> Vec<Stmt> {
    statements_cut(model).0
}

/// What one statement element says: its kind, form and its source and target ends. `None` for an element that is no statement.
type Parts = (&'static str, &'static str, Vec<End>, Vec<End>);

fn statement_parts(model: &Model, e: &Element) -> Option<Parts> {
    // Elements that are no relationship statement build no index.
    if !is_statement(e) {
        return None;
    }
    let index = chain_index(model, e);
    let mut memo: HashMap<&str, End> = HashMap::new();
    let mut ends = |role: RefRole| -> Vec<End> { e.refs_of(role).take(MAX_ENDS_PER_STATEMENT).map(|r| end_with_chain(model, r, &index, &mut memo)).collect() };
    Some(match e.kind.as_str() {
        "satisfy" | "satisfy requirement" | "verify" | "verify requirement" => {
            let satisfy = e.kind.starts_with("satisfy");
            let mut targets = ends(RefRole::Target);
            if targets.is_empty() {
                targets = ends(RefRole::Type);
            }
            let mut sources = ends(RefRole::Source);
            if sources.is_empty() {
                let owner = if satisfy { e.owner } else { verifier_of(model, e) };
                sources = owner.map(|o| vec![owner_end(model, o)]).unwrap_or_default();
            }
            (if satisfy { "satisfy" } else { "verify" }, "statement", sources, targets)
        }
        "allocation" if !e.is_def => ("allocate", "statement", ends(RefRole::Source), ends(RefRole::Target)),
        "dependency" => {
            let (kind, form) = match tag_kind(e) {
                Some(k) => (k, "dependency"),
                None => ("dependency", "statement"),
            };
            (kind, form, ends(RefRole::Source), ends(RefRole::Target))
        }
        "connection" | "interface" | "flow" | "binding" | "succession" | "transition" if !e.is_def => {
            let kind = match e.kind.as_str() {
                "connection" | "interface" => "connect",
                "binding" => "bind",
                "flow" => "flow",
                "succession" => "succession",
                _ => "transition",
            };
            // `connect (a, b, c)` stores the first end as source and the rest as targets.
            (kind, "statement", ends(RefRole::Source), ends(RefRole::Target))
        }
        _ => return None,
    })
}

pub fn statements_cut(model: &Model) -> (Vec<Stmt>, Vec<Cut>) {
    let mut out = Vec::new();
    let mut cuts = Vec::new();
    for e in &model.elements {
        let Some((kind, form, sources, targets)) = statement_parts(model, e) else { continue };
        // The cap applies before the pairs are made.
        let real = |ends: &Vec<End>, roles: &[RefRole]| if ends.len() >= MAX_ENDS_PER_STATEMENT { roles.iter().map(|r| e.refs_of(*r).count()).sum() } else { ends.len() };
        let stated = sources.len().saturating_mul(targets.len());
        let total_room = MAX_PAIRS_TOTAL.saturating_sub(out.len());
        let room = MAX_PAIRS_PER_STATEMENT.min(total_room);
        let keep = stated.min(room);
        let capped = sources.len() >= MAX_ENDS_PER_STATEMENT || targets.len() >= MAX_ENDS_PER_STATEMENT;
        if keep < stated || capped {
            let (sr, tr) = if e.kind.starts_with("satisfy") || e.kind.starts_with("verify") { (&[RefRole::Source][..], &[RefRole::Target, RefRole::Type][..]) } else { (&[RefRole::Source][..], &[RefRole::Target][..]) };
            cuts.push(Cut { via: e.id, sources: real(&sources, sr), targets: real(&targets, tr), kept: keep, by_total: keep < stated && total_room < MAX_PAIRS_PER_STATEMENT.min(stated) });
        }
        pairs(e.id, kind, form, &sources, &targets, keep, &mut out);
    }
    (out, cuts)
}

fn pairs(via: ElementId, kind: &'static str, form: &'static str, sources: &[End], targets: &[End], keep: usize, out: &mut Vec<Stmt>) {
    let mut made = 0;
    'pairs: for s in sources {
        for t in targets {
            if made == keep {
                break 'pairs;
            }
            out.push(Stmt { kind, via, form, source: s.clone(), target: t.clone() });
            made += 1;
        }
    }
}

/// The pairs one statement element states (at most `MAX_PAIRS_PER_STATEMENT`): what `statements` holds for it, without
/// the search through the whole model.
pub fn statements_of(model: &Model, via: ElementId) -> Vec<Stmt> {
    let mut out = Vec::new();
    if let Some((kind, form, sources, targets)) = statement_parts(model, &model.elements[via]) {
        let keep = sources.len().saturating_mul(targets.len()).min(MAX_PAIRS_PER_STATEMENT);
        pairs(via, kind, form, &sources, &targets, keep, &mut out);
    }
    out
}

/// Typing, specialization, redefinition and reference (`::>`) edges, resolved ends only.
pub fn structural(model: &Model) -> Vec<Stmt> {
    let mut out = Vec::new();
    for e in &model.elements {
        if e.kind.starts_with("satisfy") || e.kind.starts_with("verify") {
            continue;
        }
        for r in &e.refs {
            let kind = match r.role {
                RefRole::Type => "typing",
                RefRole::Specializes => "specialization",
                RefRole::Redefines => "redefinition",
                RefRole::References => "reference",
                _ => continue,
            };
            if let Some(t) = r.resolved() {
                out.push(Stmt { kind, via: e.id, form: "ref", source: owner_end(model, e.id), target: End { text: r.text.clone(), id: Some(t), chain: Arc::default() } });
            }
        }
    }
    out
}

/// Resolved edges of every kind.
pub fn edges(model: &Model) -> Vec<(Stmt, ElementId, ElementId)> {
    edges_cut(model).0
}

/// `edges`, and the statements whose pairs were cut short.
pub fn edges_cut(model: &Model) -> (Vec<(Stmt, ElementId, ElementId)>, Vec<Cut>) {
    let (stmts, cuts) = statements_cut(model);
    let mut v = Vec::new();
    for s in stmts.into_iter().chain(structural(model)) {
        if let (Some(a), Some(b)) = (s.source.id, s.target.id) {
            v.push((s, a, b));
        }
    }
    (v, cuts)
}

fn kind_matches(have: &str, want: &str) -> bool {
    let want = match want {
        "specialize" | "specializes" => "specialization",
        "type" | "typed_by" => "typing",
        "redefine" => "redefinition",
        "references" | "subsets_ref" => "reference",
        "connection" | "connection_end" => "connect",
        "binding" => "bind",
        "allocation" => "allocate",
        other => other,
    };
    have == want
}

pub fn declared_at(model: &Model, via: ElementId) -> Value {
    let v = &model.elements[via];
    json!({ "element": v.qualified_name, "file": model.files[v.file].path, "line": model.line_col(v.file, v.span.start).0 })
}

pub fn edge_json(model: &Model, s: &Stmt, a: ElementId, b: ElementId) -> Value {
    json!({
        "kind": s.kind,
        "source": model.elements[a].qualified_name,
        "target": model.elements[b].qualified_name,
        "declared_at": declared_at(model, s.via),
    })
}

/// Breadth-first walk from `root`. `kinds` limits edge kinds (empty means all).
/// Returns edges with the depth at which each was found and the direction relative to the node it was reached from.
pub fn walk(
    model: &Model,
    root: ElementId,
    kinds: &dyn Fn(&str) -> bool,
    out: bool,
    inn: bool,
    depth: usize,
) -> (Vec<Value>, Vec<(ElementId, usize)>) {
    let (found, nodes, _) = walk_cut(model, root, kinds, out, inn, depth);
    (found, nodes)
}

/// `walk`, and the statements whose pairs were cut short.
pub fn walk_cut(
    model: &Model,
    root: ElementId,
    kinds: &dyn Fn(&str) -> bool,
    out: bool,
    inn: bool,
    depth: usize,
) -> (Vec<Value>, Vec<(ElementId, usize)>, Vec<Cut>) {
    let (every, cuts) = edges_cut(model);
    let all: Vec<(Stmt, ElementId, ElementId)> = every.into_iter().filter(|(s, _, _)| kinds(s.kind)).collect();
    // The edges at each node, in the order of the edge list (an edge that leaves and enters one node counts as leaving first),
    // so a node costs its own edges and not all of them.
    let mut at: HashMap<ElementId, Vec<(usize, bool)>> = HashMap::new();
    for (i, (_, a, b)) in all.iter().enumerate() {
        if out {
            at.entry(*a).or_default().push((i, true));
        }
        if inn {
            at.entry(*b).or_default().push((i, false));
        }
    }
    let mut seen: BTreeSet<ElementId> = BTreeSet::from([root]);
    let mut nodes = vec![(root, 0usize)];
    let mut found: Vec<Value> = Vec::new();
    let mut used: BTreeSet<usize> = BTreeSet::new();
    let mut q = VecDeque::from([(root, 0usize)]);
    while let Some((n, d)) = q.pop_front() {
        if d >= depth {
            continue;
        }
        for &(i, forward) in at.get(&n).map(Vec::as_slice).unwrap_or(&[]) {
            let (s, a, b) = &all[i];
            let to = if forward { *b } else { *a };
            // Each edge is reported once, the first time the walk reaches it.
            if !used.insert(i) {
                continue;
            }
            let mut e = edge_json(model, s, *a, *b);
            e["depth"] = json!(d + 1);
            e["from"] = json!(model.elements[n].qualified_name);
            e["direction"] = json!(if forward { "out" } else { "in" });
            found.push(e);
            if seen.insert(to) {
                nodes.push((to, d + 1));
                q.push_back((to, d + 1));
            }
        }
    }
    (found, nodes, cuts)
}

/// `query_relationships`: edges around one element.
pub fn query(model: &Model, root: ElementId, kind: Option<&str>, direction: &str, depth: usize) -> Result<Value, String> {
    let (out, inn) = match direction {
        "out" => (true, false),
        "in" => (false, true),
        "both" | "" => (true, true),
        other => return Err(format!("direction must be 'in', 'out' or 'both', not '{other}'")),
    };
    let want = kind.filter(|k| !k.is_empty());
    let (edges, nodes, cuts) = walk_cut(model, root, &|k| want.map(|w| kind_matches(k, w)).unwrap_or(true), out, inn, depth);
    let mut reply = json!({
        "ref": model.elements[root].qualified_name,
        "direction": if out && inn { "both" } else if out { "out" } else { "in" },
        "depth": depth,
        "count": edges.len(),
        "edges": edges,
        "nodes": nodes.iter().map(|(n, d)| json!({ "ref": model.elements[*n].qualified_name, "kind": model.elements[*n].keyword(), "depth": d })).collect::<Vec<_>>(),
    });
    mark_cut(model, &mut reply, &cuts);
    Ok(reply)
}
