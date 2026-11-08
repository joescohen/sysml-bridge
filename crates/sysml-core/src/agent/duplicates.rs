//! One element, one representation: no duplicate representations of the same element or specification across
//! different diagrams of one type (ibd to ibd) or of different types (ibd, activity, bdd and so on). A view only renders the containment tree, so a duplicate in a view is a duplicate in
//! the tree: a concept defined twice, a function given a second usage only to say who performs it, a definition copied
//! under a new name, or one requirement written twice. Each is found here, reported by the semantic gate, and refused on
//! every write that adds one (`refuse_new`), whatever tool made it.
//!
//! - `duplicate-definition`: two definitions of one kind whose names are the same once case, spaces and punctuation are
//!   dropped (`PayloadBus`, `Payload_Bus`, `payload bus`), or that carry the same short id, neither specializing the
//!   other and neither in a library package.
//! - `duplicate-usage`: two usages of one kind in one body whose names match as read (`payloadBus`, `payload_bus`).
//! - `copied-usage`: `perform action x : D;` declares a usage of `D` beside one another body already holds. The part
//!   performs the existing usage instead (`perform cycle.x;`).
//! - `copied-definition`: two definitions of one kind with other names but the same members (name, kind, type and value each;
//!   at least three, performs aside),
//!   neither specializing the other: one was copied from the other.
//! - `similar-element` (info, never refused): one intent under two names or owners, short of certain: names whose words
//!   one holds all of the other's but one (`Payload Bus`, `Payload Data Bus`), definitions of one shape (the same members
//!   by kind and type, names aside), actions or requirements whose texts share most words. A write that makes one gets
//!   `looks_like` in its reply: the element it looks like, and the question "use the existing element?".
//! - `view-copy`: a definition that repeats part of another (some of its members, each by name, kind and type, and
//!   nothing of its own, ports or parts among them) under a name that is the other's with view words only added or dropped (`PumpFunctional`,
//!   `Pump_IBD`, `PumpPowerView` beside `Pump`): a block copied so one diagram shows fewer ports. The diagram draws the
//!   one block and hides what it does not need (hide ports on a diagram and use the same block instead of
//!   making a new one).
//! - `duplicate-requirement`: two requirements with one text, neither typed by the other (one id twice is the rule
//!   `duplicate-requirement-id`).

use super::trace::{finding, in_library, specializes, CONCEPT_DEFS};
use super::{Fail, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

pub const DUPLICATE_DEFINITION_RULE: &str = "duplicate-definition";
pub const COPIED_USAGE_RULE: &str = "copied-usage";
pub const DUPLICATE_USAGE_RULE: &str = "duplicate-usage";
pub const SIMILAR_ELEMENT_RULE: &str = "similar-element";
pub const COPIED_DEFINITION_RULE: &str = "copied-definition";
pub const DUPLICATE_REQUIREMENT_RULE: &str = "duplicate-requirement";
pub const VIEW_COPY_RULE: &str = "view-copy";
pub const DUPLICATE_ID_RULE: &str = "duplicate-id";

/// Words that only a view of a thing is named by: a copy with one member is read as a view's copy when its name has one.
const VIEW_ONLY_WORDS: &[&str] = &["view", "views", "ibd", "bdd", "diagram", "functional", "curated", "simplified", "reduced", "subset", "copy"];

/// Words that name a view of a thing, not the thing: dropped when a copy's name is read against the original's.
const VIEW_WORDS: &[&str] = &[
    "view", "views", "ibd", "bdd", "diagram", "functional", "function", "logical", "physical", "curated", "simplified", "simple", "reduced",
    "partial", "subset", "summary", "overview", "context", "copy", "alt", "alternate", "detail", "detailed", "interface", "interfaces", "port",
    "ports", "only", "lite", "mini", "short", "power", "data", "signal", "signals", "electrical", "mechanical", "thermal", "comms", "control",
];

/// Candidates one pairwise comparison reads; past it that comparison is left out of the run and says so.
const MAX_CANDIDATES: usize = 4000;
/// Findings the semantic gate lists from this module; past it the rest are counted in a note.
const MAX_FINDINGS: usize = 1000;

thread_local! {
    /// The comparisons this thread left out since the last `take_skipped`, as sentences.
    static SKIPPED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn skipped(what: String) {
    SKIPPED.with(|s| {
        let mut s = s.borrow_mut();
        if !s.contains(&what) {
            s.push(what);
        }
    });
}

fn take_skipped() -> Vec<String> {
    SKIPPED.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// The note (a warning, never an error) for a comparison that was not run on the whole model: the same "not checked"
/// finding `graph` gives for a read that was cut.
fn not_checked(model: &Model, what: &str) -> Value {
    finding(model, "semantic", super::graph::CUT_RULE, "warning", None, format!("not checked: {what}, so this check cannot say whether the model holds a duplicate there."), "Nothing is required; the rest of the model was checked.".into())
}

/// A name as a person reads it: case, spaces and punctuation dropped.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// One duplicate: the elements that repeat one another, and the key that names what they repeat.
struct Group {
    rule: &'static str,
    key: String,
    /// Each element that repeats another, with the others it repeats.
    pairs: Vec<(ElementId, Vec<ElementId>)>,
}

fn related(model: &Model, a: ElementId, b: ElementId) -> bool {
    specializes(model, a, b) || specializes(model, b, a)
}

fn typed_by(model: &Model, a: ElementId, b: ElementId) -> bool {
    model.elements[a].refs_of(RefRole::Type).any(|r| r.resolved() == Some(b))
}

/// Within each set of candidates, every element that has another it is not related to.
fn pairs_of(model: &Model, els: &[ElementId], apart: impl Fn(ElementId, ElementId) -> bool) -> Vec<(ElementId, Vec<ElementId>)> {
    let els: Vec<ElementId> = els.iter().copied().filter(|x| !in_library(model, *x)).collect();
    els.iter()
        .filter_map(|x| {
            let others: Vec<ElementId> = els.iter().copied().filter(|y| y != x && apart(*x, *y)).collect();
            (!others.is_empty()).then_some((*x, others))
        })
        .collect()
}

fn groups(model: &Model) -> Vec<Group> {
    let mut out = Vec::new();
    let defs: Vec<&Element> = model.elements.iter().filter(|e| e.is_def && !e.implied_name && CONCEPT_DEFS.contains(&e.kind.as_str())).collect();

    // One concept, one definition: by name as read, and by short id.
    let mut by_name: BTreeMap<(String, String), Vec<ElementId>> = BTreeMap::new();
    for e in &defs {
        if let Some(n) = e.name.as_deref().map(norm).filter(|n| !n.is_empty()) {
            by_name.entry((e.kind.clone(), n)).or_default().push(e.id);
        }
        if let Some(s) = e.short_name.as_deref().map(norm).filter(|s| !s.is_empty()) {
            by_name.entry((e.kind.clone(), format!("<{s}>"))).or_default().push(e.id);
        }
    }
    for ((kind, n), els) in by_name.into_iter().filter(|(_, v)| v.len() > 1) {
        // The same name in one namespace is the duplicate-name error; a different spelling there is still one concept twice.
        let pairs = pairs_of(model, &els, |x, y| {
            let (a, b) = (&model.elements[x], &model.elements[y]);
            // Two ICD documents (or two register blocks of one) may each name a register CTRL: the documents' own words,
            // each in its own package. One name inside one package stays the duplicate-name error.
            let icd_apart = a.owner != b.owner && super::icd::icd_package_of(model, x).is_some() && super::icd::icd_package_of(model, y).is_some();
            (a.owner != b.owner || a.name != b.name) && !related(model, x, y) && !icd_apart
        });
        if !pairs.is_empty() {
            out.push(Group { rule: DUPLICATE_DEFINITION_RULE, key: format!("{kind} {n}"), pairs });
        }
    }

    // One element, one usage in its owner: two usages of one kind in one body whose names match as read (`payloadBus`,
    // `payload_bus`). The same name twice is the duplicate-name error.
    let mut by_usage: BTreeMap<(Option<ElementId>, String, String), Vec<ElementId>> = BTreeMap::new();
    for e in model.elements.iter().filter(|e| !e.is_def && !e.implied_name && e.owner.is_some()) {
        if let Some(n) = e.name.as_deref().map(norm).filter(|n| !n.is_empty()) {
            by_usage.entry((e.owner, e.kind.clone(), n)).or_default().push(e.id);
        }
    }
    for ((owner, kind, n), els) in by_usage.into_iter().filter(|(_, v)| v.len() > 1) {
        let pairs = pairs_of(model, &els, |x, y| model.elements[x].name != model.elements[y].name);
        if !pairs.is_empty() {
            let o = owner.map(|o| model.elements[o].qualified_name.clone()).unwrap_or_default();
            out.push(Group { rule: DUPLICATE_USAGE_RULE, key: format!("{o} {kind} {n}"), pairs });
        }
    }

    // One function, one usage: a perform that declares a second usage of an action definition another body already uses.
    let mut uses: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for e in model.elements.iter().filter(|e| !e.is_def && (e.kind == "action" || e.kind == "perform action")) {
        for d in e.refs_of(RefRole::Type).filter_map(|r| r.resolved()) {
            uses.entry(d).or_default().push(e.id);
        }
    }
    let mut copied: Vec<(ElementId, Vec<ElementId>)> = uses
        .iter()
        .filter(|(_, us)| us.len() > 1)
        .flat_map(|(_, us)| {
            us.iter().filter(|u| model.elements[**u].kind == "perform action").map(move |u| {
                let others: Vec<ElementId> = us.iter().copied().filter(|o| o != u && model.elements[*o].kind != "perform action").collect();
                (*u, others)
            })
        })
        .filter(|(_, others)| !others.is_empty())
        .collect();
    copied.sort();
    for (u, others) in copied {
        let d = model.elements[u].first_resolved(RefRole::Type).map(|d| model.elements[d].qualified_name.clone()).unwrap_or_default();
        out.push(Group { rule: COPIED_USAGE_RULE, key: d, pairs: vec![(u, others)] });
    }

    // One definition, not a copy under another name: the same members, each by name, kind and type.
    let mut by_members: BTreeMap<(String, String), Vec<ElementId>> = BTreeMap::new();
    for e in &defs {
        // The channel port defs of one ICD hold the same items on purpose: a register a client writes and
        // reads back rides both the cmd and the rpt channel. Its registers too: INT_STATUS, INT_ENABLE and INT_MASK name
        // the same bits at three addresses, which is how register maps are written, not a copy. And their coded values:
        // hundreds of fields read 0 OFF / 1 ON, each field's own list in the document (a work-scale map has thousands).
        if (e.kind == "port" || e.kind == "item" || e.kind == "enum") && super::icd::icd_package_of(model, e.id).is_some() {
            continue;
        }
        let mut members: Vec<String> = e
            .children
            .iter()
            .map(|c| &model.elements[*c])
            // the steps of a method are members too: two pass-through methods with one item each (the SDP's StartDwell
            // calls carrier.startDwell, the carrier's calls tcf.schedulePulses) are two levels, not a copy
            .filter(|c| c.name.is_some() && !c.implied_name && (!c.kind.starts_with("perform") || e.kind == "action"))
            .map(|c| {
                if c.kind.starts_with("perform") {
                    return format!("{} {} ::> {}", c.keyword(), c.name.as_deref().unwrap_or(""), c.refs.iter().map(|r| r.text.clone()).collect::<Vec<_>>().join(","));
                }
                // A conjugated port is the other side of an interface, not the same member: mirror images are no copy.
                let t = c.refs_of(RefRole::Type).map(|r| format!("{}{}", if r.conjugated { "~" } else { "" }, r.text)).collect::<Vec<_>>().join(",");
                format!("{} {} {} : {t}{}", c.keyword(), c.direction.as_deref().unwrap_or(""), c.name.as_deref().unwrap_or(""), value_of(c))
            })
            .collect();
        // Two parts that each have one power port and one data port are two parts: a copy repeats more than that.
        if members.len() < 3 {
            continue;
        }
        members.sort();
        by_members.entry((e.kind.clone(), members.join("; "))).or_default().push(e.id);
    }
    for ((kind, members), els) in by_members.into_iter().filter(|(_, v)| v.len() > 1) {
        let pairs = pairs_of(model, &els, |x, y| {
            let (a, b) = (&model.elements[x], &model.elements[y]);
            a.name.as_deref().map(norm) != b.name.as_deref().map(norm) && !related(model, x, y)
        });
        if !pairs.is_empty() {
            out.push(Group { rule: COPIED_DEFINITION_RULE, key: format!("{kind} {{{members}}}"), pairs });
        }
    }

    // One block, many diagrams: a definition that holds part of another's members and nothing of its own, named as a view
    // of it, was copied so a diagram shows fewer ports. Two members at least (one shared port is not a copy), or one
    // when the name says it is a view (`AnalyserIBD`).
    let sigs: Vec<(ElementId, std::collections::BTreeSet<String>)> = defs
        .iter()
        .filter(|e| !in_library(model, e.id))
        .map(|e| (e.id, member_signatures(model, e)))
        .filter(|(_, m)| !m.is_empty())
        .collect();
    let core = |id: ElementId| -> std::collections::BTreeSet<String> {
        words(model.elements[id].name.as_deref().unwrap_or("")).into_iter().filter(|w| !VIEW_WORDS.contains(&w.as_str())).map(|w| stem(&w)).collect()
    };
    if sigs.len() > MAX_CANDIDATES {
        skipped(format!("the view-copy check compares definitions pairwise and the model has {} candidates (it reads at most {MAX_CANDIDATES})", sigs.len()));
    }
    if sigs.len() <= MAX_CANDIDATES {
        for (a, ma) in &sigs {
            let viewish = words(model.elements[*a].name.as_deref().unwrap_or("")).iter().any(|w| VIEW_ONLY_WORDS.contains(&w.as_str()));
            // A diagram copy repeats ports or parts; definitions of attributes alone (verification records) are not one.
            let structural = model.elements[*a].children.iter().any(|c| matches!(model.elements[*c].kind.as_str(), "port" | "part"));
            if !structural || (ma.len() < 2 && !viewish) {
                continue;
            }
            let ca = core(*a);
            let others: Vec<ElementId> = sigs
                .iter()
                .filter(|(b, mb)| {
                    let (ea, eb) = (&model.elements[*a], &model.elements[*b]);
                    b != a && ea.kind == eb.kind && ma.is_subset(mb) && (ma.len() < mb.len() || norm(ea.name.as_deref().unwrap_or("")).len() > norm(eb.name.as_deref().unwrap_or("")).len())
                        && norm(ea.name.as_deref().unwrap_or("")) != norm(eb.name.as_deref().unwrap_or("")) && !related(model, *a, *b) && {
                            // The names differ by view words only: `VerTp01Pwr` beside `VerTp01` is another record.
                            !ca.is_empty() && ca == core(*b)
                        }
                })
                .map(|(b, _)| *b)
                .collect();
            if !others.is_empty() {
                out.push(Group { rule: VIEW_COPY_RULE, key: model.elements[*a].qualified_name.clone(), pairs: vec![(*a, others)] });
            }
        }
    }

    // One requirement, one statement: by its text (one id twice is the rule duplicate-requirement-id). A usage typed by a
    // requirement definition repeats it on purpose.
    let mut by_req: BTreeMap<String, Vec<ElementId>> = BTreeMap::new();
    for e in model.elements.iter().filter(|e| e.kind == "requirement") {
        if let Some(t) = e.doc.as_deref().map(|d| d.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()).filter(|t| t.len() >= 24) {
            by_req.entry(format!("text {t}")).or_default().push(e.id);
        }
    }
    for (key, els) in by_req.into_iter().filter(|(_, v)| v.len() > 1) {
        let pairs = pairs_of(model, &els, |x, y| !typed_by(model, x, y) && !typed_by(model, y, x) && !related(model, x, y));
        if !pairs.is_empty() {
            out.push(Group { rule: DUPLICATE_REQUIREMENT_RULE, key, pairs });
        }
    }
    out
}

/// A member's value as written (` = "Test"`), so two records with one shape and other values are two records.
fn value_of(c: &Element) -> String {
    c.value.as_deref().map(|v| format!(" = {}", v.split_whitespace().collect::<Vec<_>>().join(" "))).unwrap_or_default()
}

/// A definition's members as a copy repeats them: keyword, name and type (conjugation kept), performs aside.
fn member_signatures(model: &Model, e: &Element) -> std::collections::BTreeSet<String> {
    e.children
        .iter()
        .map(|c| &model.elements[*c])
        .filter(|c| c.name.is_some() && !c.implied_name && !c.kind.starts_with("perform"))
        .map(|c| {
            let t = c.refs_of(RefRole::Type).map(|r| format!("{}{}", if r.conjugated { "~" } else { "" }, r.resolved().map(|d| model.elements[d].qualified_name.clone()).unwrap_or_else(|| r.text.clone()))).collect::<Vec<_>>().join(",");
            format!("{} {} : {t}{}", c.keyword(), norm(c.name.as_deref().unwrap_or("")), value_of(c))
        })
        .collect()
}

/// The words of a name or a text: camelCase, spaces and punctuation split, lower case, numbers kept.
fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_numeric();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A word without the ending English adds to it (`routed`, `routes`, `route` are one word; `taken`, `take`), so two texts that say one
/// thing in other forms share it.
fn stem(w: &str) -> String {
    for suffix in ["ing", "ed", "en", "es", "s", "e"] {
        if let Some(b) = w.strip_suffix(suffix) {
            if b.chars().count() >= 3 {
                return b.to_string();
            }
        }
    }
    w.to_string()
}

/// Words that carry no intent of their own in a requirement or a description.
const FILLER: &[&str] = &["the", "a", "an", "of", "to", "and", "or", "shall", "will", "be", "is", "are", "in", "on", "for", "with", "by", "each", "every", "all", "its", "it", "that", "this", "from", "at", "as"];

fn jaccard(a: &std::collections::BTreeSet<String>, b: &std::collections::BTreeSet<String>) -> f64 {
    let i = a.intersection(b).count() as f64;
    let u = a.union(b).count() as f64;
    if u == 0.0 { 0.0 } else { i / u }
}

/// `jaccard` of two sets given as sorted lists without repeats (words as numbers): the same value for the same sets.
fn jaccard_sorted(a: &[u32], b: &[u32]) -> f64 {
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    let u = (a.len() + b.len() - shared) as f64;
    if u == 0.0 { 0.0 } else { shared as f64 / u }
}

/// Two elements that say they are two: requirements that declare other ids (a system requirement and the subsystem
/// requirement derived from it, `PWR-02` and `CPS-BMS-01`), definitions whose members differ both ways (a signal and its
/// forwarded copy on another link, interfaces that carry other items), or descriptions that say other things.
fn told_apart(model: &Model, a: ElementId, b: ElementId) -> bool {
    let (ea, eb) = (&model.elements[a], &model.elements[b]);
    if declares_other_id(model, a, b) {
        return true;
    }
    let (ma, mb) = (member_signatures(model, ea), member_signatures(model, eb));
    if !ma.is_empty() && !mb.is_empty() && !ma.is_subset(&mb) && !mb.is_subset(&ma) {
        return true;
    }
    // Its own description and its members' (an item def whose one attribute says "kind: UHF data link" and one whose
    // attribute says "kind: Ethernet" are two signals).
    let doc = |e: &Element| {
        let text: Vec<&str> = e.doc.as_deref().into_iter().chain(e.children.iter().filter_map(|c| model.elements[*c].doc.as_deref())).collect();
        Some(text.iter().flat_map(|d| words(d)).filter(|w| !FILLER.contains(&w.as_str())).map(|w| stem(&w)).collect::<std::collections::BTreeSet<_>>()).filter(|w| w.len() >= 2)
    };
    if let (Some(x), Some(y)) = (doc(ea), doc(eb)) {
        if jaccard(&x, &y) < 0.5 {
            return true;
        }
    }
    false
}

/// Two elements that declare different ids are two records, however alike their words: requirements (`PWR-02` and the
/// derived `CPS-BMS-01`, or their slugs `'req-pwr-02'` and `'req-cps-bms-01'`), interfaces, ports and items. An id the
/// tools minted (`ek7m2qd`) says nothing about what the element is, so it does not tell two look-alikes apart.
fn declares_other_id(model: &Model, a: ElementId, b: ElementId) -> bool {
    let id = |e: &Element| e.short_name.as_deref().filter(|s| !super::mint::is_minted_id(s)).map(norm).filter(|s| !s.is_empty());
    matches!((id(&model.elements[a]), id(&model.elements[b])), (Some(x), Some(y)) if x != y)
}

/// The words of the model's top packages (`Autonomy`, `Safety`, `System`): a name that adds only one of them, or a
/// number, to another's is the same element modelled again in that domain (`AutonomyMissionComputer` beside
/// `MissionComputer`, `InterfaceEstopActiveSafety`, `ESTOP_ACTIVE_2`), whatever ids, ports and descriptions say.
fn domain_words(model: &Model) -> std::collections::BTreeSet<String> {
    model
        .elements
        .iter()
        .filter(|e| e.kind == "package" && e.owner.is_none())
        .filter(|e| !in_library(model, e.id))
        .flat_map(|e| words(e.name.as_deref().unwrap_or("")))
        .chain(["system".to_string()])
        .collect()
}

/// Elements that look like one intent under two names or two owners, short of a certain duplicate: names whose words one
/// holds all of the other's (`Payload Bus`, `Payload Data Bus`), definitions of one shape (the same members by kind and
/// type, whatever their names: two interface blocks with the same ports), and actions or requirements whose texts say the
/// same thing. Not refused: a write that makes one is answered with the element it looks like, to reuse it or merge.
/// `sure`: the model's `groups`, whose elements are certain duplicates and not looked at again here.
fn similar_groups(model: &Model, sure: &[Group]) -> Vec<Group> {
    use std::collections::BTreeSet;
    let mut out = Vec::new();
    let sure: std::collections::HashSet<ElementId> = sure.iter().flat_map(|g| g.pairs.iter().map(|p| p.0)).collect();
    let cands: Vec<&Element> = model
        .elements
        .iter()
        // An ICD's registers, fields and coded values look alike by design (LANE0_CTRL .. LANE3_CTRL, hundreds of
        // OFF/ON lists): the document's own words, not one intent under two names. A work-scale map made this check
        // list every new register against every old one (3 GB for one edit).
        .filter(|e| e.is_def && !e.implied_name && CONCEPT_DEFS.contains(&e.kind.as_str()) && !in_library(model, e.id) && !sure.contains(&e.id) && super::icd::icd_package_of(model, e.id).is_none())
        .collect();
    let named: Vec<(ElementId, BTreeSet<String>)> = cands.iter().filter_map(|e| e.name.as_deref().map(|n| (e.id, words(n).into_iter().collect::<BTreeSet<_>>()))).collect();
    let mut by_kind: BTreeMap<&str, Vec<&(ElementId, BTreeSet<String>)>> = BTreeMap::new();
    for n in &named {
        by_kind.entry(model.elements[n.0].kind.as_str()).or_default().push(n);
    }
    let domains = domain_words(model);
    for (kind, ns) in by_kind {
        if ns.len() > MAX_CANDIDATES {
            skipped(format!("the look-alike name check of {kind} definitions compares them pairwise and the model has {} (it reads at most {MAX_CANDIDATES})", ns.len()));
            continue;
        }
        // A look-alike name has the words of the other and one more (`small != big`, a subset, one word apart): only the
        // names one word away are read, found by the name with each of its words dropped, not every pair of the kind.
        let mut minus_one: HashMap<Vec<&str>, Vec<usize>> = HashMap::new();
        let mut exact: HashMap<Vec<&str>, Vec<usize>> = HashMap::new();
        for (j, (_, w)) in ns.iter().map(|n| (n.0, &n.1)).enumerate() {
            let all: Vec<&str> = w.iter().map(String::as_str).collect();
            for k in 0..all.len() {
                let mut less = all.clone();
                less.remove(k);
                minus_one.entry(less).or_default().push(j);
            }
            exact.entry(all).or_default().push(j);
        }
        for (i, (x, wx)) in ns.iter().map(|n| (n.0, &n.1)).enumerate() {
            let all: Vec<&str> = wx.iter().map(String::as_str).collect();
            let mut near: Vec<usize> = minus_one.get(&all).cloned().unwrap_or_default();
            for k in 0..all.len() {
                let mut less = all.clone();
                less.remove(k);
                near.extend(exact.get(&less).into_iter().flatten().copied());
            }
            near.retain(|j| *j > i);
            near.sort_unstable();
            near.dedup();
            let others: Vec<ElementId> = near
                .into_iter()
                .map(|j| ns[j])
                .filter(|(y, wy)| {
                    let (small, big) = if wx.len() <= wy.len() { (wx, wy) } else { (wy, wx) };
                    small.len() >= 2 && small != big && small.is_subset(big) && big.len() - small.len() <= 1 && !related(model, x, *y) && {
                        // The word one name adds: a domain or a number is the element again; anything else (`Fwd`,
                        // `Frame`, `Power`) may name another thing, so ids, members and descriptions decide.
                        let extra: Vec<&String> = big.difference(small).collect();
                        let again = extra.iter().all(|w| domains.contains(*w) || w.chars().all(|c| c.is_ascii_digit()));
                        again || !told_apart(model, x, *y)
                    }
                })
                .map(|n| n.0)
                .collect();
            if !others.is_empty() {
                out.push(Group { rule: SIMILAR_ELEMENT_RULE, key: format!("{kind} name {}", model.elements[x].qualified_name), pairs: vec![(x, others)] });
            }
        }
    }
    // One shape: the same members by kind, type and value, names aside (at least three typed members). Records with one
    // shape and other values (verification defs of TP-01 and TP-02) are two records.
    let mut by_shape: BTreeMap<(String, String), Vec<ElementId>> = BTreeMap::new();
    for e in &cands {
        let mut shape: Vec<String> = e
            .children
            .iter()
            .map(|c| &model.elements[*c])
            .filter(|c| c.name.is_some() && !c.implied_name && !c.kind.starts_with("perform"))
            .filter_map(|c| c.refs_of(RefRole::Type).next().map(|r| format!("{} : {}{}{}", c.keyword(), if r.conjugated { "~" } else { "" }, r.text, value_of(c))))
            .collect();
        if shape.len() < 3 {
            continue;
        }
        shape.sort();
        by_shape.entry((e.kind.clone(), shape.join("; "))).or_default().push(e.id);
    }
    for ((kind, shape), els) in by_shape.into_iter().filter(|(_, v)| v.len() > 1) {
        let pairs = pairs_of(model, &els, |x, y| !related(model, x, y) && !declares_other_id(model, x, y));
        if !pairs.is_empty() {
            out.push(Group { rule: SIMILAR_ELEMENT_RULE, key: format!("{kind} shape {shape}"), pairs });
        }
    }
    // One text: actions and requirements whose descriptions share most of their words.
    let texts: Vec<(ElementId, BTreeSet<String>)> = model
        .elements
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "requirement" | "action" | "use case") && !in_library(model, e.id) && !sure.contains(&e.id))
        .filter_map(|e| e.doc.as_deref().map(|d| (e.id, words(d).into_iter().filter(|w| !FILLER.contains(&w.as_str())).map(|w| stem(&w)).collect::<BTreeSet<_>>())))
        .filter(|(_, w)| w.len() >= 4)
        .collect();
    if texts.len() > MAX_CANDIDATES {
        skipped(format!("the same-text check of actions and requirements compares texts pairwise and the model has {} (it reads at most {MAX_CANDIDATES})", texts.len()));
    }
    if texts.len() <= MAX_CANDIDATES {
        // The words of each text as numbers in order, so a pair is read by merging two short lists, not two sets of strings
        // (the same share of words: `jaccard_sorted`). Texts whose word counts alone put them below the share are not read.
        let mut number: HashMap<&str, u32> = HashMap::new();
        let nums: Vec<Vec<u32>> = texts
            .iter()
            .map(|(_, w)| {
                let mut v: Vec<u32> = w.iter().map(|s| { let n = number.len() as u32; *number.entry(s.as_str()).or_insert(n) }).collect();
                v.sort_unstable();
                v
            })
            .collect();
        let near = |a: &[u32], b: &[u32]| (a.len().min(b.len()) as f64 / a.len().max(b.len()) as f64) >= 0.7 && jaccard_sorted(a, b) >= 0.7;
        for (i, (x, _)) in texts.iter().enumerate() {
            let others: Vec<ElementId> = texts[i + 1..]
                .iter()
                .zip(&nums[i + 1..])
                .filter(|((y, _), ny)| model.elements[*x].kind == model.elements[*y].kind && near(&nums[i], ny) && !typed_by(model, *x, *y) && !typed_by(model, *y, *x) && !related(model, *x, *y) && !declares_other_id(model, *x, *y))
                .map(|(n, _)| n.0)
                .collect();
            if !others.is_empty() {
                out.push(Group { rule: SIMILAR_ELEMENT_RULE, key: format!("text {}", model.elements[*x].qualified_name), pairs: vec![(*x, others)] });
            }
        }
    }
    out
}

/// What a write made that looks like an element already there: one entry for each new look-alike, naming the element to
/// reuse. A write tool returns these with its result, as the question "use the existing element?".
pub fn looks_like(before: &Model, after: &Model) -> Vec<Value> {
    // One set of what was there, not a scan of every element per candidate (a work-scale model has 100,000 elements).
    let before_set: std::collections::HashSet<(&str, &str)> = before.elements.iter().map(|b| (b.qualified_name.as_str(), b.kind.as_str())).collect();
    let had = |m: &Model, id: ElementId| before_set.contains(&(m.elements[id].qualified_name.as_str(), m.elements[id].kind.as_str()));
    let mut out = Vec::new();
    // Only a named element the write made can be a new look-alike: a write that made none (a link, a retype, a doc)
    // skips the whole-model comparison, which costs a large share of each edit on a work-scale model.
    if !after.elements.iter().any(|e| e.name.is_some() && !e.implied_name && !before_set.contains(&(e.qualified_name.as_str(), e.kind.as_str()))) {
        return out;
    }
    for g in similar_groups(after, &groups(after)) {
        for (x, others) in &g.pairs {
            let all: Vec<ElementId> = std::iter::once(*x).chain(others.iter().copied()).collect();
            let (new, old): (Vec<ElementId>, Vec<ElementId>) = all.iter().partition(|id| !had(after, **id));
            if new.is_empty() || old.is_empty() {
                continue;
            }
            for n in new {
                let e = &after.elements[n];
                let o = &after.elements[old[0]];
                out.push(json!({
                    "rule": SIMILAR_ELEMENT_RULE,
                    "element": e.qualified_name,
                    "looks_like": old.iter().take(5).map(|o| after.elements[*o].qualified_name.clone()).collect::<Vec<_>>(),
                    "looks_like_count": old.len(),
                    "question": format!("{} {} looks like the existing {} {}: use the existing element?", e.keyword(), e.qualified_name, o.keyword(), o.qualified_name),
                    "if_yes": format!("delete {} and point its usages and links at {} (update_element {{ref, typed_by}}); if they are two different things, give them names and texts that say how they differ", e.qualified_name, o.qualified_name),
                }));
            }
        }
    }
    out
}

fn names(model: &Model, ids: &[ElementId]) -> String {
    ids.iter().take(3).map(|o| super::qshow(&model.elements[*o].qualified_name)).collect::<Vec<_>>().join(", ")
}

fn says(model: &Model, rule: &str, e: &Element, others: &[ElementId]) -> (String, String) {
    let o = names(model, others);
    match rule {
        DUPLICATE_ID_RULE => (
            format!("{} has the id {} that {o} has: an element copied with its id, so every view, trace and history that points at the id now points at two elements.", super::qshow(&e.qualified_name), e.short_name.as_deref().unwrap_or("")),
            format!("give it a new id: mint_ids {{\"ref\": {}}} (an element that is the same as {o} is deleted instead).", serde_json::json!(e.qualified_name)),
        ),
        COPIED_USAGE_RULE => (
            format!("{} declares another usage of {} beside {o}: one function shown as two elements, so a view of one does not show the other's links.", super::qshow(&e.qualified_name), e.refs_of(RefRole::Type).map(|r| r.text.clone()).next().unwrap_or_default()),
            format!("Perform the usage that is there ({}) instead: delete this one and write `perform <that usage>;` in this part.", o),
        ),
        DUPLICATE_USAGE_RULE => (
            format!("{} {} is {o} again under another spelling: one element shown as two in every view of its owner.", e.keyword(), super::qshow(&e.qualified_name)),
            "Keep one usage and move every link and statement to it, then delete the other; or, when they are two things, give them names that say how they differ.".into(),
        ),
        SIMILAR_ELEMENT_RULE => (
            format!("{} {} looks like {o}: one intent under two names or owners drifts apart as each is edited.", e.keyword(), super::qshow(&e.qualified_name)),
            "If they are one thing, merge them: keep one, point every usage and link at it (update_element {ref, typed_by}), delete the other. If they are two, give each a name and text that say how they differ.".into(),
        ),
        VIEW_COPY_RULE => (
            format!("{} {} repeats part of {o} (the same members, nothing of its own) under a view's name: a block copied so a diagram shows fewer ports, so links and edits on one never reach the other.", e.keyword(), super::qshow(&e.qualified_name)),
            format!("Draw {o} itself: give the curated drawing a diagram of its own beside the block's own (create_view {{base, name}}, a named view of the internal block diagram) and hide there the ports it does not need (hide_in_view {{view, element, ports}}). For one diagram of every level (nested parts down to the deepest, every connector) give the named view a depth instead of flattening copies into one block: create_view {{base, name, depth: \"all\"}}. Point every usage of the copy at {o} (update_element {{ref, typed_by}}), then delete the copy. When it is a kind of {o}, make it specialize {o} instead."),
        ),
        COPIED_DEFINITION_RULE => (
            format!("{} {} has the same members as {o}: a definition copied under another name, so links to one never reach the other.", e.keyword(), super::qshow(&e.qualified_name)),
            "Keep one definition and point every usage at it (update_element {ref, typed_by}), then delete the copy; or make one specialize the other when one is a kind of the other.".into(),
        ),
        "duplicate-requirement-id" => (
            format!("requirement {} has the id {} that {o} has: one requirement written twice.", super::qshow(&e.qualified_name), e.short_name.as_deref().unwrap_or("")),
            "Keep one requirement and move every satisfy, verify and derive link to it; or give this one an id of its own.".into(),
        ),
        DUPLICATE_REQUIREMENT_RULE => (
            format!("requirement {} states the same text as {o}: one requirement written twice, so its satisfy and verify links split between the two.", super::qshow(&e.qualified_name)),
            "Keep one requirement and move every satisfy, verify and derive link to it, then delete the other; or, when they are two requirements, give each its own id and text.".into(),
        ),
        _ => (
            format!("{} {} is defined again as {o}: one concept modelled twice keeps its links apart, so a view or trace of one does not show the other.", e.keyword(), super::text::quote_clip(e.name.as_deref().unwrap_or(""))),
            "Keep one definition and point every usage at it (update_element {ref, typed_by}), then delete the other; or, when they are two different things, rename one, or make one specialize the other.".into(),
        ),
    }
}

/// The semantic gate's findings: a warning on each element that repeats another, naming the others.
pub fn findings(model: &Model) -> Vec<Value> {
    take_skipped();
    let mut out = Vec::new();
    let sure = groups(model);
    let similar = similar_groups(model, &sure);
    let all: Vec<Group> = sure.into_iter().chain(similar).collect();
    let total: usize = all.iter().map(|g| g.pairs.len()).sum();
    'groups: for g in all {
        let severity = if g.rule == SIMILAR_ELEMENT_RULE { "info" } else { "warning" };
        for (x, others) in &g.pairs {
            if out.len() >= MAX_FINDINGS {
                skipped(format!("{} more duplicate findings were found past the first {MAX_FINDINGS} and are not listed", total - MAX_FINDINGS));
                break 'groups;
            }
            let e = &model.elements[*x];
            let (message, fix) = says(model, g.rule, e, others);
            let mut f = finding(model, "semantic", g.rule, severity, Some(e), message, fix);
            // What it repeats, by qualified name, for a tool that folds it back (fold_copy {copy, into}).
            f["same_as"] = json!(others.iter().map(|o| model.elements[*o].qualified_name.clone()).collect::<Vec<_>>());
            out.push(f);
        }
    }
    for what in take_skipped() {
        out.push(not_checked(model, &what));
    }
    out
}

/// One id on two requirements: reported by the gate as `duplicate-requirement-id` (an error), refused here too, so raw
/// text cannot add one where the tools' error gate does not reach.
fn requirement_id_groups(model: &Model) -> Vec<Group> {
    let mut by: BTreeMap<String, Vec<ElementId>> = BTreeMap::new();
    for e in model.elements.iter().filter(|e| e.kind == "requirement") {
        if let Some(s) = e.short_name.as_deref().filter(|s| !s.is_empty()) {
            by.entry(s.to_string()).or_default().push(e.id);
        }
    }
    by.into_iter()
        .filter(|(_, v)| v.len() > 1)
        .filter_map(|(id, els)| {
            let pairs = pairs_of(model, &els, |x, y| !typed_by(model, x, y) && !typed_by(model, y, x) && !related(model, x, y));
            (!pairs.is_empty()).then(|| Group { rule: "duplicate-requirement-id", key: id, pairs })
        })
        .collect()
}

/// One minted id on two elements, of any kind and anywhere in the model: a copy pasted with its id. An id is the
/// element's pointer, so two elements must never share one. A tool write gives the copy a new id in the same write
/// (`mint::remint_copies`); a hand edit is reported as an error.
fn minted_id_groups(model: &Model) -> Vec<Group> {
    let mut by: BTreeMap<&str, Vec<ElementId>> = BTreeMap::new();
    for e in &model.elements {
        if let Some(s) = e.short_name.as_deref().filter(|s| super::mint::is_minted_id(s)) {
            by.entry(s).or_default().push(e.id);
        }
    }
    by.into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(id, els)| Group { rule: DUPLICATE_ID_RULE, key: id.to_string(), pairs: els.iter().map(|x| (*x, els.iter().copied().filter(|y| y != x).collect())).collect() })
        .collect()
}

/// The duplicate-id findings, errors: each element that shares its id names the others and the command that gives it a new one.
pub fn id_findings(model: &Model) -> Vec<Value> {
    let mut out = Vec::new();
    for g in minted_id_groups(model) {
        for (x, others) in g.pairs.iter().skip(1) {
            let e = &model.elements[*x];
            let (message, fix) = says(model, g.rule, e, others);
            out.push(finding(model, "semantic", g.rule, "error", Some(e), message, fix));
        }
    }
    out
}

fn counts(model: &Model) -> &HashMap<(&'static str, String), (usize, ElementId, Vec<ElementId>)> {
    // Kept on the model: the model after one edit of a batch is the model before the next.
    model.dup_counts.get_or_init(|| {
        let mut m = HashMap::new();
        // A shared minted id is not here: a write gives the copy a new id (`mint::remint_copies`) instead of being refused.
        for g in groups(model).into_iter().chain(requirement_id_groups(model)) {
            let first = g.pairs[0].clone();
            m.insert((g.rule, g.key), (g.pairs.len(), first.0, first.1));
        }
        m
    })
}

/// Refuse a write that adds a duplicate the model did not have: a create, a rename, a retype, a move, raw text or a batch
/// must reuse the element that is there. A duplicate the model already had is reported by the gate, not refused.
pub fn refuse_new(before: &Model, after: &Model) -> Res<()> {
    refuse_new_over(&[before], after)
}

/// `refuse_new` against several starting points: a duplicate is new when it has more members than in any of them. An
/// import passes the model before it and the imported file alone, so a duplicate inside the source project is reported,
/// and only one between the import and the model already there is refused.
pub fn refuse_new_over(befores: &[&Model], after: &Model) -> Res<()> {
    let now = counts(after);
    if now.is_empty() {
        return Ok(());
    }
    let was: Vec<_> = befores.iter().map(|b| counts(b)).collect();
    let before = befores[0];
    let mut fresh: Vec<_> = now.iter().filter(|(k, (n, _, _))| was.iter().map(|w| w.get(*k).map(|w| w.0).unwrap_or(0)).max().unwrap_or(0) < *n).collect();
    fresh.sort_by(|a, b| a.0.cmp(b.0));
    let Some(((rule, _), (_, x, others))) = fresh.first() else { return Ok(()) };
    // Name the element the write made, not the one that was there: of the pair, the one the model before lacked.
    let had = |id: ElementId| before.elements.iter().any(|b| b.qualified_name == after.elements[id].qualified_name && b.kind == after.elements[id].kind);
    let (new, old): (ElementId, Vec<ElementId>) = if had(*x) {
        match others.iter().find(|o| !had(**o)) {
            Some(n) => (*n, vec![*x]),
            None => (*x, others.clone()),
        }
    } else {
        (*x, others.clone())
    };
    let e = &after.elements[new];
    let (message, fix) = says(after, rule, e, &old);
    Err(Fail::with(
        format!("refused: {message} Nothing was written. Fix: {fix}"),
        json!({ "rule": rule, "element": e.qualified_name, "repeats": old.iter().map(|o| after.elements[*o].qualified_name.clone()).collect::<Vec<_>>() }),
    ))
}
