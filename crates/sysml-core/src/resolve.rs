//! Name resolution across all files of a workspace.

use crate::model::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Names from the SysML v2 standard libraries. References that start with
/// one of these are not reported when no library source is loaded.
const LIBRARY_NAMES: &[&str] = &[
    "ScalarValues", "Real", "Integer", "String", "Boolean", "Natural", "Rational", "Complex", "Number",
    "Positive", "ISQ", "SI", "USCustomaryUnits", "Time", "Quantities", "MeasurementReferences", "Base",
    "Anything", "Occurrences", "Items", "Parts", "Ports", "Actions", "States", "Requirements", "Constraints",
    "Cases", "Views", "Metadata", "SysML", "KerML", "Collections", "ControlFunctions", "NumericalFunctions",
    "RealFunctions", "TrigFunctions", "SequenceFunctions", "BaseFunctions", "DataFunctions", "Links",
    "Objects", "Performances", "Transfers", "Connections", "Interfaces", "Allocations", "Calculations",
    "AnalysisCases", "VerificationCases", "UseCases", "Attributes", "StandardViewDefinitions",
    "ModelingMetadata", "RiskMetadata", "ParametersOfInterestMetadata", "RequirementDerivation",
    "CauseAndEffect", "DerivationConnections", "SampledFunctions", "StateSpaceRepresentation",
    "Geometry", "ShapeItems", "SpatialItems", "VectorValues", "TensorValues", "Duration", "TimeInstantValue",
    "start", "done", "self", "that", "result", "Part", "Item", "Action", "State", "Port", "Requirement",
    "TradeStudies", "IntegerFunctions", "StringFunctions", "BooleanFunctions", "ISQBase", "ISQSpaceTime",
    "ISQMechanics", "ISQThermodynamics", "ISQElectromagnetism", "SIPrefixes", "Clock", "Clocks",
];

/// The member tables are keyed by names a model's author chooses, so they use the standard keyed hasher: an unkeyed
/// hash lets a crafted set of names collide and turn each lookup into a scan.
type Fast<K, V> = HashMap<K, V>;

struct Index {
    /// For each owner, its members by name; the last slot is the root namespace. A lookup borrows the name.
    members: Vec<Fast<String, ElementId>>,
    /// The `import` and `expose` members of each element, in the order the element lists its members.
    imports: Vec<Vec<ElementId>>,
    /// The same for the root namespace.
    root_imports: Vec<ElementId>,
    /// Every name some namespace declares. A name outside this set is found through no import, so a namespace with many
    /// imports is not walked for it.
    names: HashSet<String>,
    /// What `resolve_first` found in one scope for one name. The model does not change while one pass resolves, so the answer
    /// for (scope, inherit, name) is worked out once; `resolve` empties the table before the model changes. Without it, N
    /// names looked up in a namespace of N imports cost N times N steps.
    ///
    /// A scope that has no import and nothing to inherit from is answered by one lookup in `members` and is not kept, so
    /// names looked up through deep nesting leave no entry for each level. The table holds at most one entry for each
    /// reference of a pass, and it is emptied after each pass.
    memo: RefCell<HashMap<(ElementId, bool), HashMap<String, Option<ElementId>>>>,
    /// For each element, which of its references name a type it inherits from (type, specialization, redefinition, and the
    /// target of a reference usage). An element with a long value holds thousands of names; a lookup that starts in it
    /// reads these few and not all of them.
    general: Vec<Vec<u32>>,
}

thread_local! {
    /// Members and imports read while resolving on this thread. A test reads it to show, with no clock, that the work grows
    /// in step with the text.
    static WORK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n as u64)));
}

/// The count of members and imports the resolver read on this thread since the last call; the count starts again at zero.
pub fn take_work_steps() -> u64 {
    WORK.with(|w| w.replace(0))
}

/// Owner used for the root namespace in the calls below.
const ROOT: usize = usize::MAX;

impl Index {
    fn build(model: &Model) -> Self {
        let mut members: Vec<Fast<String, ElementId>> = (0..=model.elements.len()).map(|_| Fast::default()).collect();
        let slot = |owner: Option<ElementId>| owner.unwrap_or(model.elements.len());
        for e in &model.elements {
            let table = &mut members[slot(e.owner)];
            if e.implied_name {
                // Only redefinitions add a member under their implied name,
                // and they must not hide a real declaration.
                if e.refs_of(RefRole::Redefines).next().is_none() {
                    continue;
                }
                if let Some(n) = &e.name {
                    table.entry(n.clone()).or_insert(e.id);
                }
                continue;
            }
            if let Some(n) = &e.name {
                table.insert(n.clone(), e.id);
            }
            if let Some(s) = &e.short_name {
                table.entry(s.clone()).or_insert(e.id);
            }
        }
        // Real declarations win over implied names.
        for e in &model.elements {
            if !e.implied_name {
                if let Some(n) = &e.name {
                    members[slot(e.owner)].insert(n.clone(), e.id);
                }
            }
        }
        let is_import = |c: &ElementId| matches!(model.elements[*c].kind.as_str(), "import" | "expose");
        // An import written again in the same namespace, letter for letter, gives no other name than the first: it is
        // left out, so the same line repeated k times is read once for a name and not k times.
        let imports = model
            .elements
            .iter()
            .map(|e| {
                let mut known: HashSet<(&str, Option<&str>)> = HashSet::new();
                e.children.iter().copied().filter(is_import).filter(|c| {
                    let ce = &model.elements[*c];
                    ce.refs.first().is_none_or(|r| known.insert((r.text.as_str(), ce.props.get("wildcard").map(|w| w.as_str()))))
                }).collect()
            })
            .collect();
        let root_imports = model.roots.iter().copied().filter(|r| model.elements[*r].kind == "import").collect();
        let names = members.iter().flat_map(|t| t.keys().cloned()).collect();
        let general = model
            .elements
            .iter()
            .map(|e| {
                let target = is_reference_usage(&e.kind);
                e.refs.iter().enumerate().filter(|(_, r)| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines) || (target && r.role == RefRole::Target)).map(|(i, _)| i as u32).collect()
            })
            .collect();
        Index { members, imports, root_imports, names, general, memo: RefCell::default() }
    }

    fn own(&self, owner: usize, name: &str) -> Option<ElementId> {
        let slot = if owner == ROOT { self.members.len() - 1 } else { owner };
        self.members.get(slot)?.get(name).copied()
    }
}

fn follow_alias(model: &Model, mut id: ElementId) -> ElementId {
    for _ in 0..8 {
        let e = &model.elements[id];
        if e.kind == "alias" {
            if let Some(t) = e.first_resolved(RefRole::Target) {
                id = t;
                continue;
            }
        }
        break;
    }
    id
}

/// Member lookup that also walks types, supertypes and redefined features.
fn find_member(
    model: &Model,
    idx: &Index,
    owner: ElementId,
    name: &str,
    inherit: bool,
    seen: &mut Vec<ElementId>,
) -> Option<ElementId> {
    if seen.contains(&owner) {
        return None;
    }
    seen.push(owner);
    if seen.len() > 64 {
        return None;
    }
    if let Some(m) = idx.own(owner, name) {
        return Some(follow_alias(model, m));
    }
    // Imports inside this namespace. An import gives only names that some namespace declares.
    let imports: &[ElementId] = if idx.names.contains(name) { &idx.imports[owner] } else { &[] };
    work(imports.len());
    for c in imports {
        let ce = &model.elements[*c];
        let Some(r) = ce.refs.first() else { continue };
        let Some(target) = r.resolved() else { continue };
        match ce.props.get("wildcard").map(|s| s.as_str()) {
            Some("*") => {
                if let Some(m) = idx.own(target, name) {
                    return Some(follow_alias(model, m));
                }
            }
            Some(_) => {
                if let Some(m) = idx.own(target, name) {
                    return Some(follow_alias(model, m));
                }
                for d in model.descendants(target) {
                    if model.elements[d].name.as_deref() == Some(name) && !model.elements[d].implied_name {
                        return Some(d);
                    }
                }
            }
            None => {
                if r.segments.last().map(|s| s.name.as_str()) == Some(name) {
                    return Some(target);
                }
            }
        }
    }
    if inherit {
        let e = &model.elements[owner];
        for r in idx.general[owner].iter().map(|i| &e.refs[*i as usize]) {
            if let Some(t) = r.resolved() {
                if let Some(m) = find_member(model, idx, t, name, true, seen) {
                    return Some(m);
                }
            }
        }
    }
    None
}

fn is_reference_usage(kind: &str) -> bool {
    matches!(kind, "perform" | "exhibit" | "satisfy" | "verify" | "include" | "event" | "assert" | "assume" | "require")
}

fn resolve_first(model: &Model, idx: &Index, scope: Option<ElementId>, name: &str, inherit: bool) -> Option<ElementId> {
    let mut cur = scope;
    while let Some(s) = cur {
        // One lookup answers a scope with no import and no type to inherit from: nothing to keep.
        let plain = idx.imports[s].is_empty() && !(inherit && !idx.general[s].is_empty());
        let known = if plain { None } else { idx.memo.borrow().get(&(s, inherit)).and_then(|t| t.get(name).copied()) };
        let found = match known {
            Some(f) => f,
            None if plain => idx.own(s, name).map(|m| follow_alias(model, m)),
            None => {
                let f = find_member(model, idx, s, name, inherit, &mut Vec::new());
                idx.memo.borrow_mut().entry((s, inherit)).or_default().insert(name.to_string(), f);
                f
            }
        };
        if let Some(m) = found {
            return Some(m);
        }
        cur = model.elements[s].owner;
    }
    if let Some(m) = idx.own(ROOT, name) {
        return Some(follow_alias(model, m));
    }
    // Imports at file level.
    for r in &idx.root_imports {
        let ce = &model.elements[*r];
        if let (Some(t), Some(_)) = (ce.refs.first().and_then(|x| x.resolved()), ce.props.get("wildcard")) {
            if let Some(m) = idx.own(t, name) {
                return Some(m);
            }
        }
    }
    None
}

/// True for a name written `$::A::b`: it starts at the global scope.
pub fn is_global(r: &Reference) -> bool {
    r.text.trim_start_matches('~').trim_start().starts_with('$')
}

fn resolve_global(model: &Model, idx: &Index, name: &str) -> Option<ElementId> {
    if let Some(m) = idx.own(ROOT, name) {
        return Some(follow_alias(model, m));
    }
    for r in &idx.root_imports {
        let ce = &model.elements[*r];
        if let (Some(t), Some(_)) = (ce.refs.first().and_then(|x| x.resolved()), ce.props.get("wildcard")) {
            if let Some(m) = idx.own(t, name) {
                return Some(m);
            }
        }
    }
    None
}

fn resolve_ref(model: &Model, idx: &Index, el: ElementId, r: &Reference, inherit: bool) -> Vec<Option<ElementId>> {
    let e = &model.elements[el];
    if r.role == RefRole::Assign {
        // A bare assignment sets a feature of the owner: its own, or one its types declare.
        let name = &r.segments[0].name;
        let found = e.owner.and_then(|o| {
            let oe = &model.elements[o];
            model
                .children(o)
                .find(|c| c.id != el && !c.implied_name && c.name.as_deref() == Some(name.as_str()))
                .map(|c| c.id)
                .or_else(|| {
                    oe.refs
                        .iter()
                        .filter(|x| matches!(x.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines))
                        .filter_map(|x| x.resolved())
                        .find_map(|t| find_member(model, idx, t, name, true, &mut Vec::new()))
                })
        });
        return vec![found];
    }
    let mut out = Vec::with_capacity(r.segments.len());
    // Imports and declaration references resolve from the owning namespace.
    // Ends of relationships may also name features of the element itself.
    let first_scope = if matches!(r.role, RefRole::Expr | RefRole::Call) || (matches!(r.role, RefRole::Source | RefRole::Target | RefRole::Other) && e.body_span.is_some()) {
        Some(el)
    } else {
        e.owner
    };
    let mut cur = if is_global(r) {
        // `$::A::b` starts at the root namespace, whatever the scope hides.
        resolve_global(model, idx, &r.segments[0].name)
    } else if r.role == RefRole::Redefines {
        // A redefined feature lives in a supertype of the owner, not in the owner itself.
        e.owner.and_then(|o| {
            let oe = &model.elements[o];
            let name = &r.segments[0].name;
            oe.refs
                .iter()
                .filter(|x| matches!(x.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines))
                .filter_map(|x| x.resolved())
                .find_map(|t| find_member(model, idx, t, name, true, &mut Vec::new()))
                .or_else(|| resolve_first(model, idx, oe.owner, name, inherit).filter(|m| *m != el))
        })
    } else {
        resolve_first(model, idx, first_scope, &r.segments[0].name, inherit).filter(|m| *m != el || r.segments.len() > 1 || matches!(r.role, RefRole::Expr | RefRole::Call))
    };
    out.push(cur);
    for seg in r.segments.iter().skip(1) {
        cur = cur.and_then(|c| find_member(model, idx, c, &seg.name, true, &mut Vec::new()));
        out.push(cur);
    }
    out
}

/// The segments of a qualified name, quotes removed. `::` inside quotes does not split.
pub fn split_name(q: &str) -> Vec<String> {
    let (mut out, mut cur, mut in_q) = (Vec::new(), String::new(), false);
    let mut chars = q.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // The escapes of a quoted name, as the lexer reads them.
            '\\' if in_q => cur.extend(chars.next().map(|c| match c {
                'n' => '\n',
                't' => '\t',
                c => c,
            })),
            '\'' => in_q = !in_q,
            ':' if !in_q && chars.peek() == Some(&':') => {
                chars.next();
                out.push(std::mem::take(&mut cur).trim().to_string());
            }
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// What each `(element, role, text)` would resolve to if `element` carried a reference with that text.
/// This is how a writer checks that a name it is about to emit means what it intends.
pub fn resolve_texts(model: &Model, queries: &[(ElementId, RefRole, String)]) -> Vec<Option<ElementId>> {
    let idx = Index::build(model);
    queries
        .iter()
        .map(|(el, role, text)| {
            let segments: Vec<Segment> = split_name(text.trim_start_matches('$').trim_start_matches("::")).into_iter().map(|name| Segment { name, span: Span::default(), resolved: None }).collect();
            let r = Reference { role: *role, text: text.clone(), span: Span::default(), segments, conjugated: false };
            let mut cur = resolve_ref(model, &idx, *el, &r, true);
            // Resolution is lexical on the first pass; later segments are followed through typing.
            cur.pop().flatten()
        })
        .collect()
}

/// Most passes over the references: a plain cap, never a verdict. The loop stops as soon as a pass changes nothing. Each
/// pass after the first can only move a reference to a nearer name that an import or a type found in the pass before
/// gives; a model needs one pass for each level of import that names an import.
const MAX_PASSES: usize = 8;

pub fn resolve(model: &mut Model) {
    let idx = Index::build(model);
    // Pass 0 is lexical only. Later passes use the types and the imports found so far, and they look at every reference
    // again: a name is looked up in the nearest namespace first, and the names of a namespace are those it owns, imports
    // and inherits (KerML 8.2.3.5.3 and 8.2.3.5.4). In pass 0 no import and no type is known yet, so a name declared in an
    // outer namespace is found where a nearer import or supertype gives the name; the later pass moves the reference to it.
    for pass in 0..MAX_PASSES {
        let mut updates: Vec<(ElementId, usize, Vec<Option<ElementId>>)> = Vec::new();
        for e in &model.elements {
            for (ri, r) in e.refs.iter().enumerate() {
                if pass == 0 && r.resolved().is_some() {
                    continue;
                }
                let res = resolve_ref(model, &idx, e.id, r, pass > 0);
                // A reference that names an alias stays on the alias: the pass that found it did not know what the
                // alias stands for, and that is no nearer name.
                let same = |v: &Option<ElementId>, seg: &Segment| *v == seg.resolved || seg.resolved.is_some_and(|o| Some(follow_alias(model, o)) == *v);
                if r.resolved().is_some() && res.iter().zip(&r.segments).all(|(v, seg)| same(v, seg)) {
                    continue;
                }
                if res.iter().any(|x| x.is_some()) {
                    updates.push((e.id, ri, res));
                }
            }
        }
        // The model changes now: what the pass found in each scope no longer holds.
        idx.memo.borrow_mut().clear();
        let mut changed = false;
        for (el, ri, res) in updates {
            let r = &mut model.elements[el].refs[ri];
            // A first name that moved to a nearer element takes the rest of the chain with it, found or not.
            let moved = res[0].is_some() && r.segments[0].resolved.is_some() && res[0] != r.segments[0].resolved;
            for (seg, v) in r.segments.iter_mut().zip(res) {
                if seg.resolved != v && (v.is_some() || moved) {
                    seg.resolved = v;
                    changed = true;
                }
            }
        }
        if !changed && pass > 0 {
            break;
        }
    }
    report(model);
}

fn report(model: &mut Model) {
    let mut diags = Vec::new();
    // Whether a namespace imports a library that is not loaded, worked out once for each namespace.
    let mut library_scopes: HashMap<usize, bool> = HashMap::new();
    // Whether the types above an element are all known, and whether an enum types it: worked out once for each element.
    // An element whose value holds N names that do not resolve is read once, not N times.
    let mut known = Known::default();
    for e in &model.elements {
        for r in &e.refs {
            if r.resolved().is_some() {
                continue;
            }
            let first = &r.segments[0].name;
            if LIBRARY_NAMES.contains(&first.as_str()) {
                continue;
            }
            if r.role == RefRole::Assign {
                // Only when the owner's types are all known: a library type may declare the feature.
                let Some(o) = e.owner else { continue };
                if !has_types(model, o) || !known.types(model, o) {
                    continue;
                }
                diags.push(Diagnostic {
                    file: e.file,
                    span: r.segments[0].span,
                    severity: Severity::Warning,
                    message: format!(
                        "cannot resolve '{first}': {} declares no feature of that name. Fix: use the declared feature name, or add the feature to the type.",
                        model.elements[o].refs.iter().find(|x| matches!(x.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines)).map(|x| x.text.clone()).unwrap_or_default()
                    ),
                });
                continue;
            }
            if r.role == RefRole::Call || (r.role == RefRole::Expr && !expr_reportable(model, e.id, r, &mut known)) {
                continue;
            }
            // A chain that starts inside a library element cannot be checked.
            let bad = r.segments.iter().position(|s| s.resolved.is_none()).unwrap_or(0);
            if bad > 0 {
                let mut prev = r.segments[bad - 1].resolved;
                let mut through_library = false;
                for _ in 0..8 {
                    let Some(p) = prev else { break };
                    let pe = &model.elements[p];
                    let unresolved_type = pe
                        .refs
                        .iter()
                        .any(|x| matches!(x.role, RefRole::Type | RefRole::Specializes) && x.resolved().is_none() && x.span != r.span);
                    if unresolved_type {
                        through_library = true;
                        break;
                    }
                    prev = pe.first_resolved(RefRole::Type);
                }
                if through_library {
                    continue;
                }
            }
            if bad == 0 && uses_unloaded_library(model, e.id, &mut library_scopes) {
                continue;
            }
            let seg = &r.segments[bad];
            let shadowed = bad > 0 && !is_global(r) && model.find(r.text.trim_start_matches('~')).is_some();
            // A name that a nearer element hides is wrong text, so it is an error. A name that points at
            // nothing yet stays a warning, so files can be written in any order.
            let severity = if shadowed { Severity::Error } else { Severity::Warning };
            let message = if bad > 0 && !is_global(r) {
                let hid = r.segments[bad - 1].resolved.map(|p| model.elements[p].qualified_name.clone()).unwrap_or_default();
                let hint = if shadowed {
                    format!(" Fix: '{}' names '{}' here, which hides the global name. Write $::{}, or rename one of them.", r.segments[0].name, hid, r.text.trim_start_matches('~'))
                } else {
                    format!(" Fix: '{}' has no member '{}'; use a name that exists.", hid, seg.name)
                };
                format!("cannot resolve '{}' in '{}'.{hint}", seg.name, r.text)
            } else {
                format!("cannot resolve '{}'", seg.name)
            };
            diags.push(Diagnostic { file: e.file, span: seg.span, severity, message });
        }
    }
    check_enum_values(model, &mut diags);
    // Duplicate names in one namespace.
    let mut seen: HashMap<(usize, &str), ElementId> = HashMap::new();
    for e in &model.elements {
        if e.implied_name {
            continue;
        }
        if let Some(n) = &e.name {
            let key = (e.owner.unwrap_or(ROOT), n.as_str());
            if let Some(first) = seen.get(&key) {
                // Packages may be spread over several files.
                if e.kind == "package" && model.elements[*first].kind == "package" {
                    continue;
                }
                diags.push(Diagnostic {
                    file: e.file,
                    span: e.name_span.unwrap_or(e.span),
                    severity: Severity::Warning,
                    message: format!("duplicate name '{n}' in this namespace"),
                });
            } else {
                seen.insert(key, e.id);
            }
        }
    }
    model.diagnostics.extend(diags);
}

/// True when a wildcard import of a library that is not loaded is in scope. `known` keeps the answer of each namespace
/// (`ROOT` for the top of the files), so a namespace of N members with N unresolved names is read once, not N times.
fn uses_unloaded_library(model: &Model, el: ElementId, known: &mut HashMap<usize, bool>) -> bool {
    let check = |children: &[ElementId]| {
        work(children.len());
        children.iter().any(|c| {
            let ce = &model.elements[*c];
            ce.kind == "import"
                && ce.props.contains_key("wildcard")
                && ce.refs.first().map(|r| {
                    r.resolved().is_none() && LIBRARY_NAMES.contains(&r.segments[0].name.as_str())
                }) == Some(true)
        })
    };
    let mut cur = model.elements[el].owner;
    while let Some(s) = cur {
        if *known.entry(s).or_insert_with(|| check(&model.elements[s].children)) {
            return true;
        }
        cur = model.elements[s].owner;
    }
    *known.entry(ROOT).or_insert_with(|| check(&model.roots))
}

/// True when `id` has a type, specialization or redefinition.
fn has_types(model: &Model, id: ElementId) -> bool {
    model.elements[id].refs.iter().any(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines))
}

/// True when every type, specialization and redefinition above `id` resolves to a model element.
/// A library type is not loaded, so what it declares is unknown.
fn types_known(model: &Model, id: ElementId) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![id];
    while let Some(x) = stack.pop() {
        if !seen.insert(x) || seen.len() > 256 {
            continue;
        }
        work(model.elements[x].refs.len());
        for r in model.elements[x].refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines)) {
            match r.resolved() {
                Some(t) => stack.push(t),
                None => return false,
            }
        }
    }
    true
}

/// Whether an unresolved name in a value expression is certain enough to report.
#[derive(Default)]
struct Known {
    types: HashMap<ElementId, bool>,
    enums: HashMap<ElementId, bool>,
}

impl Known {
    fn types(&mut self, model: &Model, id: ElementId) -> bool {
        *self.types.entry(id).or_insert_with(|| types_known(model, id))
    }
    fn has_enum(&mut self, model: &Model, id: ElementId) -> bool {
        *self.enums.entry(id).or_insert_with(|| enum_of(model, id).is_some())
    }
}

fn expr_reportable(model: &Model, el: ElementId, r: &Reference, known: &mut Known) -> bool {
    let bad = r.segments.iter().position(|s| s.resolved.is_none()).unwrap_or(0);
    if known.has_enum(model, el) && model.elements[el].value_span == Some(r.span) {
        // The value check says it better: it names the literals the enum has.
        return false;
    }
    if bad == 0 {
        // An enum literal or feature of a library type cannot be checked: the name may be one that the element, or a
        // definition or usage it stands in, inherits from a type that is not loaded.
        return std::iter::successors(Some(el), |x| model.elements[*x].owner).take_while(|x| model.elements[*x].kind != "package").take(64).all(|x| known.types(model, x));
    }
    match r.segments[bad - 1].resolved {
        Some(p) => {
            let pe = &model.elements[p];
            (pe.kind == "package" || pe.is_def || has_types(model, p)) && known.types(model, p)
        }
        None => false,
    }
}

/// The enum definition that types `id`, directly or through the feature it redefines or specializes.
fn enum_of(model: &Model, id: ElementId) -> Option<ElementId> {
    let mut cur = id;
    for _ in 0..8 {
        let x = &model.elements[cur];
        if x.is_def {
            return None;
        }
        if let Some(t) = x.first_resolved(RefRole::Type) {
            let te = &model.elements[t];
            return (te.kind == "enum" && te.is_def).then_some(t);
        }
        // A bare assignment (`level = L3;` in a metadata body) takes the type of the feature it sets.
        cur = x.first_resolved(RefRole::Redefines).or_else(|| x.first_resolved(RefRole::Specializes)).or_else(|| x.first_resolved(RefRole::Assign))?;
    }
    None
}

/// A string, number or boolean literal, or a name that is not a literal of the enum, given to an enum-typed feature.
fn check_enum_values(model: &Model, diags: &mut Vec<Diagnostic>) {
    for e in &model.elements {
        let (Some(vs), Some(v)) = (e.value_span, e.value.as_deref()) else { continue };
        let Some(en) = enum_of(model, e.id) else { continue };
        let enum_el = &model.elements[en];
        let lits: Vec<&str> = model.children(en).filter(|c| c.kind == "enum value").filter_map(|c| c.name.as_deref()).collect();
        if lits.is_empty() {
            continue;
        }
        let v = v.trim();
        let name = enum_el.name.clone().unwrap_or_default();
        let shown = lits.iter().take(8).copied().collect::<Vec<_>>().join(", ");
        let fix = format!("Fix: use a literal of '{name}': {shown}{}.", if lits.len() > 8 { ", ..." } else { "" });
        let what = if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 && !v[1..v.len() - 1].contains('"') {
            Some(format!("the string {v}"))
        } else if v.parse::<f64>().is_ok() || matches!(v, "true" | "false") {
            Some(format!("the literal {v}"))
        } else {
            None
        };
        let bad_name = e.refs_of(RefRole::Expr).find(|r| r.span == vs).and_then(|r| match r.resolved() {
            Some(t) => {
                let te = &model.elements[t];
                (te.kind == "enum value" && te.owner != Some(en)).then(|| format!("'{}'", r.text))
            }
            None => {
                let last_is_new = r.segments.len() == 1 || r.segments[..r.segments.len() - 1].iter().all(|s| s.resolved == Some(en));
                last_is_new.then(|| format!("'{}'", r.text))
            }
        });
        if let Some(w) = what.or(bad_name) {
            diags.push(Diagnostic {
                file: e.file,
                span: vs,
                severity: Severity::Warning,
                message: format!("value does not fit type: {w} is not a literal of enum def '{name}'. {fix}"),
            });
        }
    }
}
