//! Waves: runs of independent `create_element` edits that share one analysis.
//!
//! One edit costs one parse and resolve of the whole workspace, because the next edit needs the model of the text this
//! one leaves. A batch of 190 creates then costs 190 analyses, and each grows with the model. Most creates of a build do
//! not need the result of the one before: they add a member to a different owner and name nothing the other makes.
//! Those edits are planned against the same model, their text patches are collected, and the patches are written
//! together; one analysis then reads the whole wave back.
//!
//! The wave path is a fast path for the case where every edit goes through, and it must give the same files, replies
//! and ids as running the edits one by one. Three rules keep that:
//!
//! 1. Two edits share a wave only when they cannot see each other: different owners, no name that one writes appears in
//!    what the other refers to, no shared source id, and patches that are not within `MARGIN` bytes of each other.
//!    Edits that fail this test wait for the next wave, in their written order.
//! 2. The read-back of every edit runs on the model of the whole wave: the new element is where it was planned, with the
//!    name it was given; the new text has no control character, no statement the grammar does not know, and no name a
//!    nearer element hides (which the one-by-one path would rewrite); no element other than the new ones appeared or
//!    disappeared.
//! 3. When anything is in doubt (an edit that fails, one that writes text some other way, a read-back that does not
//!    hold, a patch that touches another) `run` answers `None` and the caller runs the edits one by one. That path
//!    gives the replies and the failure messages; this one never has to.
//!
//! `SYSML_NO_WAVES=1` turns the fast path off.

use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Patches closer than this many bytes are not planned together: the text around a patch can carry the layout choice
/// (an indent, a blank line) that the nearer patch would change.
const MARGIN: usize = 16;

/// Smallest batch worth a wave.
const MIN_EDITS: usize = 6;

static ANALYSES: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static SWITCH: Cell<Option<bool>> = const { Cell::new(None) };
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

/// How many times a workspace was parsed and resolved by edits since the process started.
pub fn analyses_run() -> usize {
    ANALYSES.load(Ordering::Relaxed)
}

pub(super) fn count_analysis() {
    ANALYSES.fetch_add(1, Ordering::Relaxed);
}

/// Turn the wave path on or off for the calling thread (`None` follows `SYSML_NO_WAVES`). For tests that compare the two paths.
pub fn set_waves(on: Option<bool>) {
    SWITCH.with(|s| s.set(on));
}

pub(super) fn enabled() -> bool {
    SWITCH.with(Cell::get).unwrap_or_else(|| std::env::var_os("SYSML_NO_WAVES").is_none())
}

/// A patch to one file, in the coordinates of the text the wave was planned on.
struct Patch {
    file: String,
    start: usize,
    end: usize,
    text: String,
}

/// What the read-back checks of one deferred create, run on the model of the whole wave.
struct Entry {
    edit: usize,
    owner: Option<String>,
    /// Members the owner had before.
    before: Option<usize>,
    top_name: Option<String>,
    top: String,
    names: Vec<String>,
    /// Further names that must exist after the write (the slots of an annotation).
    require: Vec<String>,
}

struct Active {
    patches: Vec<Patch>,
    entries: Vec<Entry>,
    /// Index of the edit being planned.
    current: usize,
}

/// A create that was written: where, how many lines it added, and in which step (wave or lone edit) it went in. A row of
/// `created` carries the line the element had in the text after its own edit. The one-by-one path has edits 0..=i in that
/// text; a wave has the edits of its steps so far. `settle` moves each row by the lines of the edits that differ and sit above.
struct Rec {
    edit: usize,
    file: String,
    pos: usize,
    lines: i64,
    step: usize,
}

fn newlines(t: &str) -> i64 {
    t.bytes().filter(|b| *b == b'\n').count() as i64
}

/// Correct the line of every created row of a group of edits that ended at a barrier (or at the end of the batch).
fn settle(recs: &mut Vec<Rec>, results: &mut [Option<Value>]) {
    for i in recs.iter() {
        let corr: i64 = recs
            .iter()
            .filter(|j| j.file == i.file && j.edit != i.edit && j.pos < i.pos)
            .map(|j| j.lines * (i64::from(j.edit < i.edit) - i64::from(j.step <= i.step)))
            .sum();
        if corr == 0 {
            continue;
        }
        for row in results[i.edit].as_mut().map(|r| &mut r["created"]).and_then(Value::as_array_mut).into_iter().flatten() {
            if row["file"] == i.file.as_str() {
                if let Some(l) = row["line"].as_i64() {
                    row["line"] = json!(l + corr);
                }
            }
        }
    }
    recs.clear();
}

struct ActiveGuard;

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        ACTIVE.with(|a| *a.borrow_mut() = None);
    }
}

pub(super) fn active() -> bool {
    ACTIVE.with(|a| a.borrow().is_some())
}

fn bail() -> Fail {
    Fail::with("held back from the wave", json!({ "wave": "bail" }))
}

/// `place_new`, while a wave is being planned: work out where the text goes and keep the patch; write nothing.
/// Fails with `bail` for everything this path does not do itself, and the edit then waits or runs alone.
#[allow(clippy::too_many_arguments)]
pub(super) fn offer(work: &[FileIn], model: &Model, owner_el: Option<&Element>, a: &Value, text: &str, top_name: Option<String>, features: &[Spec], defer: bool) -> Res<Value> {
    if !defer || file_arg(a).is_some() {
        return Err(bail());
    }
    let Some(file) = model.files.first().map(|f| f.path.clone()) else { return Err(bail()) };
    let before = owner_el.map(|o| o.children.len());
    // Edits that add to an owner whose body already holds something write at the end of that body: they can share a wave,
    // in the order written, when each is named (so none depends on how many members come before it).
    let others = owner_el.map_or(0, |o| ACTIVE.with(|a| a.borrow().as_ref().map_or(0, |act| act.entries.iter().filter(|e| e.owner.as_deref() == Some(o.qualified_name.as_str())).count())));
    if others > 0 && !owner_open(model, work, owner_el) {
        return Err(bail());
    }
    let slot = before.map(|b| b + others);
    let changes = edit::apply(model, &EditOp::AddElement { owner: owner_el.map(|o| o.qualified_name.clone()), file: Some(file), text: text.to_string() }).map_err(|_| bail())?;
    let [c] = changes.as_slice() else { return Err(bail()) };
    let old = work.iter().find(|f| f.path == c.path).ok_or_else(bail)?;
    let (p, q) = changed_region(&old.text, &c.text);
    let end = old.text.len().checked_sub(c.text.len() - q).filter(|e| *e >= p).ok_or_else(bail)?;
    let patch = Patch { file: c.path.clone(), start: p, end, text: c.text.get(p..q).ok_or_else(bail)?.to_string() };
    let top = match (owner_el, &top_name) {
        (Some(o), Some(n)) => format!("{}::{n}", o.qualified_name),
        (Some(o), None) => format!("{}::${}", o.qualified_name, slot.unwrap_or(0) + 1),
        (None, Some(n)) => n.clone(),
        (None, None) => return Err(bail()),
    };
    let (names, ids) = feature_names(&top, features);
    ACTIVE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let act = guard.as_mut().ok_or_else(bail)?;
        let mine = owner_el.map(|o| o.qualified_name.as_str());
        let clash = act.patches.iter().zip(&act.entries).any(|(o, e)| {
            let stacked = others > 0 && e.owner.as_deref() == mine && o.start == o.end && patch.start == patch.end && o.start == patch.start;
            !stacked && o.file == patch.file && patch.start < o.end + MARGIN && o.start < patch.end + MARGIN
        });
        if clash || (others > 0 && act.entries.iter().any(|e| e.owner.as_deref() == mine && e.names.iter().any(|n| names.contains(n)))) {
            return Err(bail());
        }
        act.patches.push(patch);
        act.entries.push(Entry { edit: act.current, owner: owner_el.map(|o| o.qualified_name.clone()), before: slot, top_name, top: top.clone(), names, require: Vec::new() });
        Ok(())
    })?;
    let mut info = json!({ "ref": top, "created": [] });
    if !ids.is_empty() {
        info["feature_ids"] = json!(ids);
    }
    Ok(info)
}

/// A name the wave's read-back must find, for the entry planned last.
pub(super) fn require(qn: &str) {
    ACTIVE.with(|a| {
        if let Some(e) = a.borrow_mut().as_mut().and_then(|act| act.entries.last_mut()) {
            e.require.push(qn.to_string());
        }
    });
}

/// True when the owner's body holds a member already, so a new member goes at the end of that body.
fn owner_open(model: &Model, work: &[FileIn], owner: Option<&Element>) -> bool {
    let Some(o) = owner else { return false };
    let Some(b) = o.body_span else { return false };
    let text = work.iter().find(|f| f.path == model.files[o.file].path).map(|f| f.text.as_str()).unwrap_or("");
    b.end >= b.start + 2 && text.get(b.start + 1..b.end - 1).is_some_and(|inner| !inner.trim().is_empty())
}

// ---- what an edit refers to and what it makes ---------------------------------

struct Meta {
    /// A `create_element` that may share a wave.
    candidate: bool,
    owner: Option<String>,
    /// Simple names the edit writes (its own, its features', short names).
    names: HashSet<String>,
    /// The same with the owner path in front: `Owner::name`, `Owner::name::feature`.
    full_names: HashSet<String>,
    /// First segments of the names the edit refers to, and every word of its expressions: what a nearer element could capture.
    first: HashSet<String>,
    /// Every leading part of each qualified name the edit refers to: what an element written beside could supply.
    full: HashSet<String>,
    ids: Vec<String>,
    /// May share an owner with another edit in a wave: no check on the owner's members depends on the other edit.
    stackable: bool,
}

/// Fields whose text is not a name that another element could capture.
const TEXT_KEYS: &[&str] = &["body", "doc", "source_id", "source_ids", "name", "short_name"];
/// Fields that hold an expression: every word in them may name an element.
const EXPR_KEYS: &[&str] = &["value", "multiplicity", "guard", "effect", "trigger", "condition", "expression", "default"];

fn segments(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split("::").map(|p| p.trim().trim_matches('\'').to_string()).filter(|p| !p.is_empty())
}

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(str::to_string)
}

fn collect(v: &Value, key: &str, prefix: &str, m: &mut Meta, ok: &mut bool) {
    match v {
        Value::String(s) => {
            if TEXT_KEYS.contains(&key) {
                return;
            }
            if s.contains("source_id:") || s.split("::").any(|p| p.trim().starts_with('$')) {
                *ok = false;
            }
            let segs: Vec<String> = segments(s).collect();
            if let Some(f) = segs.first() {
                m.first.insert(f.clone());
            }
            for n in 1..=segs.len() {
                m.full.insert(segs[..n].join("::"));
            }
            if EXPR_KEYS.contains(&key) {
                m.first.extend(words(s));
            }
        }
        // The slots of an annotation are members of an unnamed element: their names are seen by nobody else, only their values refer to others.
        Value::Array(l) if key == "slots" => {
            for x in l {
                if let Some(v) = x.get("value") {
                    collect(v, "value", prefix, m, ok);
                }
            }
        }
        Value::Array(l) => l.iter().for_each(|x| collect(x, key, prefix, m, ok)),
        Value::Object(o) => {
            let mut own = prefix.to_string();
            for k in ["name", "short_name"] {
                if let Some(Value::String(n)) = o.get(k) {
                    let n = n.trim().trim_matches('\'');
                    if !n.is_empty() {
                        m.names.insert(n.to_string());
                        let full = if prefix.is_empty() { n.to_string() } else { format!("{prefix}::{n}") };
                        m.full_names.insert(full.clone());
                        if k == "name" {
                            own = full;
                        }
                    }
                }
            }
            for (k, x) in o {
                if k != "name" && k != "short_name" {
                    collect(x, k, if k == "features" { &own } else { prefix }, m, ok);
                }
            }
        }
        _ => {}
    }
}

impl Meta {
    fn of(op: &Value) -> Meta {
        let name = op.get("op").or_else(|| op.get("tool")).and_then(Value::as_str);
        let mut m = Meta {
            candidate: name == Some("create_element"),
            owner: None,
            names: HashSet::new(),
            full_names: HashSet::new(),
            first: HashSet::new(),
            full: HashSet::new(),
            ids: Vec::new(),
            stackable: false,
        };
        if !m.candidate {
            return m;
        }
        let mut ok = !super::super::order::asks_for_place(op);
        let mut prefix = String::new();
        if let Some(o) = op.get("owner") {
            match o.as_str() {
                Some(s) => {
                    let segs: Vec<String> = segments(s).collect();
                    prefix = segs.join("::");
                    m.owner = Some(prefix.clone());
                }
                None if o.is_null() => {}
                None => ok = false,
            }
        }
        // The owner is a name the edit refers to; the names it writes stand below it.
        if let Some(Value::String(s)) = op.get("owner") {
            collect(&Value::String(s.clone()), "owner", "", &mut m, &mut ok);
        }
        if let Some(obj) = op.as_object() {
            let own = match obj.get("name").and_then(Value::as_str).map(|n| n.trim().trim_matches('\'')).filter(|n| !n.is_empty()) {
                Some(n) if prefix.is_empty() => n.to_string(),
                Some(n) => format!("{prefix}::{n}"),
                None => prefix.clone(),
            };
            for (k, x) in obj {
                if k == "owner" || k == "name" || k == "short_name" {
                    continue;
                }
                collect(x, k, if k == "features" { &own } else { &prefix }, &mut m, &mut ok);
            }
            for k in ["name", "short_name"] {
                if let Some(Value::String(n)) = obj.get(k) {
                    let n = n.trim().trim_matches('\'');
                    if !n.is_empty() {
                        m.names.insert(n.to_string());
                        m.full_names.insert(if prefix.is_empty() { n.to_string() } else { format!("{prefix}::{n}") });
                    }
                }
            }
        }
        if let Some(Value::String(s)) = op.get("source_id") {
            m.ids.push(s.trim().to_string());
        }
        if let Some(Value::Array(l)) = op.get("source_ids") {
            m.ids.extend(l.iter().filter_map(Value::as_str).map(|s| s.trim().to_string()));
        }
        if op.get("source_ids").is_some_and(|v| !v.is_array() && !v.is_null()) {
            ok = false;
        }
        m.candidate = ok;
        // A statement with ends is refused when the owner already states it, and an unnamed member when it repeats one
        // (unless the call says repeats are fine): both look at the other members, so they do not stack.
        let kind = op.get("kind").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
        let flag = |k: &str| op.get(k).and_then(Value::as_bool).unwrap_or(false);
        let named = op.get("name").and_then(Value::as_str).is_some_and(|n| !n.trim().is_empty());
        m.stackable = !matches!(kind.as_str(), "bind" | "binding" | "succession" | "import" | "expose") && !flag("if_absent") && (named || flag("allow_duplicate"));
        m
    }

    /// True when the two edits could see each other's result, or both write to the same place.
    fn meets(&self, other: &Meta, stack: bool) -> bool {
        (self.owner == other.owner && !(stack && self.stackable && other.stackable && self.full_names.is_disjoint(&other.full_names)))
            || !self.first.is_disjoint(&other.names)
            || !other.first.is_disjoint(&self.names)
            || !self.full.is_disjoint(&other.full_names)
            || !other.full.is_disjoint(&self.full_names)
            || self.ids.iter().any(|i| other.ids.contains(i))
    }
}

/// True when a batch is worth planning in waves: enough creates, and no edit that moves a package (a move defers its
/// own check to the end of the batch, which the wave path does not carry).
pub(super) fn worth_trying(edits: &[Value]) -> bool {
    if !enabled() || edits.len() < MIN_EDITS {
        return false;
    }
    let creates = edits.iter().filter(|e| e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) == Some("create_element")).count();
    let moves = edits.iter().any(|e| e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) == Some("update_element") && e.get("owner").is_some_and(|o| !o.is_null()));
    // A write of a unit adds the import of the unit library; that is done one edit at a time.
    let units = edits.iter().any(super::op_writes_unit);
    creates >= MIN_EDITS && !moves && !units
}

// ---- the batch ------------------------------------------------------------------

/// Run `edits` in waves. `None` when the edits must be run one by one instead (see the module notes).
pub(super) fn run(files: &[FileIn], edits: &[Value], ids: &BTreeMap<String, String>, pre: &Arc<HashMap<String, String>>) -> Option<Batch> {
    let meta: Vec<Meta> = edits.iter().map(Meta::of).collect();
    let mut work = files.to_vec();
    let mut track = Track::for_batch(ids.clone(), pre.clone());
    let mut results: Vec<Option<Value>> = vec![None; edits.len()];
    let mut events: Vec<Vec<IdEvent>> = vec![Vec::new(); edits.len()];
    let mut remaining: Vec<usize> = (0..edits.len()).collect();
    let mut recs: Vec<Rec> = Vec::new();
    let mut step = 0usize;
    let mut deferred = false;
    while !remaining.is_empty() {
        let seg: Vec<usize> = remaining.iter().copied().take_while(|i| meta[*i].candidate).collect();
        if seg.is_empty() {
            // A barrier: the edits before it are all in, and the rows of the group are settled.
            settle(&mut recs, &mut results);
        }
        step += 1;
        let mut done = if seg.is_empty() { Vec::new() } else { round(&mut work, &seg, edits, &meta, &mut track, &mut results, &mut events, &mut recs, step)? };
        if done.is_empty() {
            // Not a create, or one that needs the text of the edits before it: run it alone, as the one-by-one path does.
            let i = remaining[0];
            let before = work.clone();
            let mut cache: Option<Arc<Model>> = None;
            // A plain link or retype leaves the whole-model checks to one pass over the batch, below.
            let defer = super::deferrable_checks() && super::deferrable(&edits[i]);
            deferred |= defer;
            super::DEFER_CHECKS.with(|c| c.set(defer));
            let got = apply_in_place(&mut work, &edits[i], &mut cache, &mut track);
            super::DEFER_CHECKS.with(|c| c.set(false));
            let (info, ev) = got.ok()?;
            if meta[i].candidate {
                for (b, a) in before.iter().zip(&work) {
                    if b.text != a.text {
                        let (p, q) = changed_region(&b.text, &a.text);
                        let end = b.text.len().checked_sub(a.text.len() - q)?;
                        for r in recs.iter_mut().filter(|r| r.file == a.path && r.pos >= end) {
                            r.pos = r.pos + (q - p) - (end - p);
                        }
                        let lines = newlines(&a.text[p..q]) - newlines(b.text.get(p..end)?);
                        recs.push(Rec { edit: i, file: a.path.clone(), pos: p, lines, step });
                    }
                }
            }
            results[i] = Some(info);
            events[i] = ev;
            done.push(i);
        }
        remaining.retain(|i| !done.contains(i));
    }
    settle(&mut recs, &mut results);
    let events: Vec<IdEvent> = events.into_iter().flatten().collect();
    // Links that left their checks: those checks once, on the batch (a refusal runs the batch edit by edit, which names
    // the edit and gives the reply of the one-by-one path).
    if deferred && super::batch_checks(files, &work, &events).is_err() {
        return None;
    }
    // The edits of a wave skip the check for captured names; `round` runs it once for each wave, on the model the wave
    // started from and the model it left. Here only the faults of a namespace are checked, on the whole batch.
    // A copy that carries another element's minted id gets a new one, as the edit-by-edit path gives it.
    let reminted = crate::agent::mint::remint_copies(&model_of(files), &mut work);
    if crate::agent::capture::refuse_namespace_faults(&model_of(files), &model_of(&work)).is_err() || crate::agent::ownership::refuse_new(&model_of(files), &model_of(&work)).is_err()
        || crate::agent::duplicates::refuse_new(&model_of(files), &model_of(&work)).is_err()
    {
        return None;
    }
    let source_id_moves = track.notes.iter().map(|n| n.to_json()).collect();
    Some(Batch { files: work, results: results.into_iter().map(|r| r.unwrap_or(Value::Null)).collect(), moved: Vec::new(), events, source_id_moves, reminted })
}

/// One wave over the leading creates `seg`. Returns the edits it completed (none when the first one cannot be planned).
#[allow(clippy::too_many_arguments)]
fn round(work: &mut Vec<FileIn>, seg: &[usize], edits: &[Value], meta: &[Meta], track: &mut Track, results: &mut [Option<Value>], events: &mut [Vec<IdEvent>], recs: &mut Vec<Rec>, step: usize) -> Option<Vec<usize>> {
    let base = work.clone();
    let m0 = model_of(work);
    ACTIVE.with(|a| *a.borrow_mut() = Some(Active { patches: Vec::new(), entries: Vec::new(), current: 0 }));
    let _clear = ActiveGuard;
    let mut accepted: Vec<usize> = Vec::new();
    let mut held: Vec<usize> = Vec::new();
    let mut staged: Vec<(usize, Value, Vec<IdEvent>)> = Vec::new();
    for &i in seg {
        // Same-owner edits may stack at the end of an owner that already has members, but never past an edit that is held back.
        let stack = meta[i].owner.as_ref().is_some_and(|o| m0.find(o).is_some_and(|el| owner_open(&m0, work, Some(el))));
        if accepted.iter().any(|j| meta[i].meets(&meta[*j], stack)) || held.iter().any(|j| meta[i].meets(&meta[*j], false)) {
            held.push(i);
            continue;
        }
        let planned = ACTIVE.with(|a| {
            let mut g = a.borrow_mut();
            let act = g.as_mut()?;
            act.current = i;
            Some(act.entries.len())
        })?;
        let mut cache: Option<Arc<Model>> = None;
        let r = apply_in_place(work, &edits[i], &mut cache, track);
        if !same_text(&base, work) {
            // The edit wrote text on its own: it cannot share a wave, and what it did is not undone here.
            return None;
        }
        let grew = ACTIVE.with(|a| a.borrow().as_ref().map(|act| act.entries.len())).unwrap_or(planned) - planned;
        match r {
            Ok((info, ev)) if grew == 1 => {
                accepted.push(i);
                staged.push((i, info, ev));
            }
            // `if_absent` found the member already there: the answer needs no text.
            Ok((info, ev)) if grew == 0 && info["existed"] == true => {
                accepted.push(i);
                staged.push((i, info, ev));
            }
            Ok(_) => return None,
            // Nothing was planned and nothing recorded: the edit needs the model after the ones before it, or it fails alone.
            Err(_) if grew == 0 => held.push(i),
            Err(_) => return None,
        }
    }
    let act = ACTIVE.with(|a| a.borrow_mut().take())?;
    if act.entries.is_empty() && staged.is_empty() {
        return Some(Vec::new());
    }
    let mut by_file: BTreeMap<String, Vec<&Patch>> = BTreeMap::new();
    for p in &act.patches {
        by_file.entry(p.file.clone()).or_default().push(p);
    }
    // Where each patch ends up, once the patches above it are in: the creates written earlier move with them.
    let mut placed: Vec<(usize, Rec)> = Vec::new();
    for (path, mut list) in by_file {
        let f = work.iter_mut().find(|f| f.path == path)?;
        let edit_of = |p: &&Patch| act.entries.iter().zip(&act.patches).find(|(_, q)| std::ptr::eq(*q, *p)).map_or(usize::MAX, |(e, _)| e.edit);
        list.sort_by_key(|p| (p.start, edit_of(p)));
        let mut grown: isize = 0;
        // Records of earlier waves, in the coordinates of the text before this wave's patches.
        let mut moved: Vec<isize> = vec![0; recs.len()];
        for p in &list {
            if p.end > f.text.len() {
                return None;
            }
            let old = base.iter().find(|b| b.path == path)?.text.get(p.start..p.end)?;
            let edit = act.entries.iter().zip(&act.patches).find(|(_, q)| std::ptr::eq(*q, *p))?.0.edit;
            placed.push((edit, Rec { edit, file: path.clone(), pos: (p.start as isize + grown) as usize, lines: newlines(&p.text) - newlines(old), step }));
            let by = p.text.len() as isize - (p.end - p.start) as isize;
            for (r, m) in recs.iter().zip(moved.iter_mut()) {
                if r.file == path && r.pos >= p.end {
                    *m += by;
                }
            }
            grown += by;
        }
        for (r, m) in recs.iter_mut().zip(moved) {
            r.pos = (r.pos as isize + m) as usize;
        }
        list.reverse();
        for p in list {
            if !f.text.is_char_boundary(p.start) || !f.text.is_char_boundary(p.end) {
                return None;
            }
            f.text.replace_range(p.start..p.end, &p.text);
        }
    }
    recs.extend(placed.iter().map(|(_, r)| Rec { edit: r.edit, file: r.file.clone(), pos: r.pos, lines: r.lines, step }));
    let created = read_back(&m0, work, &base, &act.entries)?;
    // No reference that was there before the wave may name another element after it. The comparison is of this wave
    // alone: over a whole batch, a delete of one edit and a create of another would hide each other. When a reference
    // changed its target the edits run one by one instead, and the one that did it is refused with its own message.
    crate::agent::capture::refuse(&m0, &model_of(work), &[]).ok()?;
    for (e, made) in act.entries.iter().zip(created) {
        let (_, info, _) = staged.iter_mut().find(|(i, _, _)| *i == e.edit)?;
        if made.first().map(|c| &c["ref"]) != Some(&info["ref"]) {
            return None;
        }
        info["created"] = json!(made);
    }
    for (i, info, ev) in staged {
        results[i] = Some(info);
        events[i] = ev;
    }
    Some(accepted)
}

/// The read-back of a wave, on the model of the text it left. `None` when anything differs from what the one-by-one path
/// would have checked; otherwise the `created` rows of each entry.
fn read_back(m0: &Model, work: &[FileIn], base: &[FileIn], entries: &[Entry]) -> Option<Vec<Vec<Value>>> {
    refuse_new_controls(base, work).ok()?;
    let f = model_of(work);
    refuse_unrecognised(m0, &f, "").ok()?;
    let mut new: HashSet<ElementId> = HashSet::new();
    let mut created: Vec<Vec<Value>> = Vec::new();
    let mut grown: HashMap<&str, usize> = HashMap::new();
    for e in entries {
        let top = match (&e.owner, e.before) {
            (Some(o), Some(n)) => {
                let oe = f.find(o)?;
                let want = grown.entry(o.as_str()).or_insert(0);
                *want = (*want).max(n + 1);
                &f.elements[*oe.children.get(n)?]
            }
            _ => f.find(&e.top)?,
        };
        if top.qualified_name != e.top {
            return None;
        }
        if let Some(want) = &e.top_name {
            if top.name.as_deref().map(quote_name).as_deref() != Some(want.as_str()) {
                return None;
            }
        }
        if e.require.iter().any(|q| f.find(q).is_none()) {
            return None;
        }
        new.extend(subtree_ids(&f, top.id));
        created.push(describe(&f, &e.names).ok()?);
    }
    // Each owner holds exactly the members it had plus the ones this wave added.
    if grown.iter().any(|(o, n)| f.find(o).is_none_or(|oe| oe.children.len() != *n)) {
        return None;
    }
    // A name that a nearer element hides would be rewritten by the one-by-one path; here it means the plan was wrong.
    for id in &new {
        let el = &f.elements[*id];
        for r in &el.refs {
            if matches!(r.role, RefRole::Import | RefRole::Expr | RefRole::Call | RefRole::Assign | RefRole::Meta) || crate::resolve::is_global(r) || r.segments.iter().all(|s| s.resolved.is_some()) {
                continue;
            }
            if f.find(&r.text).is_some_and(|t| t.id != el.id && t.kind != "alias") {
                return None;
            }
        }
    }
    refuse_stray_elements("the new elements", m0, &f, &|_| false, &|x| new.contains(&x.id)).ok()?;
    Some(created)
}
