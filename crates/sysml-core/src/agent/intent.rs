//! Intent-level writes. Each one generates canonical SysML v2 text and
//! applies it through the same edit engine as `edit_model`, so the text
//! stays the source of truth and the parser has the last word.

use super::idrefs::{self, Track};
use super::ids::{self, IdEvent};
use super::graph::Stmt;
use super::{chain_steps, connect, member_named, graph, locate, locate_ref, qshow, referrers, Fail, Located, Res};
use crate::api::{self, FileIn};
use crate::edit::{self, EditOp};
use crate::lexer;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::cell::Cell;
use std::sync::Arc;

mod wave;
mod retype_wave;
mod rel_ops;
pub use wave::{analyses_run, set_waves};

/// Files after an operation, and what it did.
#[derive(Debug)]
pub struct Applied {
    pub files: Vec<FileIn>,
    pub info: Value,
    /// What the edit did to names, for the source-id map.
    pub events: Vec<IdEvent>,
}

/// Called after every successful `apply_op_ids` and `apply_batch_ids` with the ops, the files before and the files after. It is
/// for tests: they register a function that checks the saved text under the official grammar. Production code registers none.
pub type WriteObserver = fn(&[&Value], &[FileIn], &[FileIn]);
static WRITE_OBSERVER: std::sync::OnceLock<WriteObserver> = std::sync::OnceLock::new();

/// Register the write observer. Only the first call counts.
pub fn set_write_observer(f: WriteObserver) {
    let _ = WRITE_OBSERVER.set(f);
}

fn observe(ops: &[&Value], before: &[FileIn], after: &[FileIn]) {
    if let Some(f) = WRITE_OBSERVER.get() {
        f(ops, before, after);
    }
}

/// The raw edits `apply_edits` and `edit_model` take.
pub const RAW_OPS: &[&str] = &["add_element", "rename", "delete", "replace", "set_doc", "add_specialization"];
/// The intent tools `apply_edits` takes by name.
pub const INTENT_OPS: &[&str] = &["create_element", "update_element", "delete_element", "create_relationship", "add_interface", "set_source_id", "reorder_members", "fold_copy", "mint_ids", "move_part", "delete_relationship"];

/// Every op name an `apply_edits` edit accepts, in the order the tool description and the docs list them.
pub fn edit_op_names() -> Vec<&'static str> {
    RAW_OPS.iter().chain(INTENT_OPS).copied().collect()
}

/// Edits that take a `source_id`: the id names the element the edit made, found or changed.
const SOURCE_ID_OPS: &[&str] = &["create_element", "create_relationship", "add_interface", "update_element", "set_source_id", "add_element"];

/// Element keywords an agent may create. `def` is added by the caller.
const CREATABLE: &[&str] = &[
    "part", "attribute", "item", "port", "action", "state", "requirement", "constraint", "calc", "interface", "connection",
    "allocation", "enum", "occurrence", "concern", "view", "viewpoint", "rendering", "metadata", "use case", "analysis",
    "verification", "package", "subject", "entry action", "do action", "exit action", "perform action", "decide", "merge",
    "fork", "join", "parameter", "actor", "include use case", "feature", "assume constraint", "require constraint",
    "exhibit state",
];

/// Kinds that are statements, not declarations: they have their own arguments and no `def` form.
const STATEMENT_KINDS: &[&str] = &["succession", "alias", "bind", "comment", "doc", "message", "import", "textual representation", "accept action", "send action"];

/// Kinds with no `def` form that a caller may leave without a name.
const NAMELESS_OK: &[&str] = &["subject", "entry action", "do action", "exit action", "decide", "merge", "fork", "join"];

/// Usage kinds that may be left without a name; the definition form of `actor` needs one.
const NAMELESS_USAGE_OK: &[&str] = &["actor", "include use case", "assume constraint", "require constraint"];

/// `start` and `done`: the ends of a behavior, which a statement names without a model element.
const MARKERS: &[&str] = &["start", "done"];

/// Every modifier word, in the order they are written. These are the words `export_facts` prints in `prefixes`
/// (the entries that do not start with `#`); `modifiers` takes the same words on create_element and update_element.
fn modifier_words() -> Vec<&'static str> {
    edit::VISIBILITY.iter().chain(edit::LEADING_MODIFIERS).chain(edit::TAIL_MODIFIERS).copied().collect()
}

/// The modifier words as given: lower case, known, once each, at most one visibility, and each one fits `base`.
/// The result is in the order they are written.
fn check_modifiers(given: &[String], base: &str, is_def: bool) -> Res<Vec<String>> {
    let kind = if is_def { format!("{base} def") } else { base.to_string() };
    let mut mods: Vec<String> = Vec::new();
    for m in given {
        let m = m.trim().trim_start_matches('#').to_lowercase();
        if m == "readonly" {
            return Err("modifier 'readonly' is not SysML v2 (the official word is 'constant'). Fix: pass 'constant'.".to_string().into());
        }
        if edit::modifier_rank(&m) == usize::MAX {
            return Err(format!("unknown modifier {}. Use one of: {} (the words export_facts prints in prefixes; entries that start with # go to metadata; in, out and inout go to direction).", crate::agent::text::quote_clip(&m), modifier_words().join(", ")).into());
        }
        let ok = match m.as_str() {
            "parallel" => base == "state",
            "event" => base == "occurrence" && !is_def,
            "library" => base == "package",
            "ref" | "end" | "derived" | "constant" | "var" | "composite" | "portion" | "variant" | "return" | "snapshot" | "timeslice" | "then" | "ordered" | "nonunique" => !is_def,
            _ => true,
        };
        if !ok {
            let why = match m.as_str() {
                "parallel" => "only a state can be parallel",
                "event" => "only an occurrence usage can be an event",
                "library" => "only a package can be a library",
                _ => "only a usage takes this word",
            };
            return Err(format!("modifier '{m}' does not fit {kind}: {why}.").into());
        }
        if !mods.contains(&m) {
            mods.push(m);
        }
    }
    if mods.iter().any(|m| m == "end") && mods.iter().any(|m| m == "ref") {
        return Err("modifiers 'end' and 'ref' do not go together: the official form is `end part x`, an end feature without `ref`. Fix: drop 'ref'.".to_string().into());
    }
    if let Some(v) = mods.iter().filter(|m| edit::VISIBILITY.contains(&m.as_str())).nth(1) {
        return Err(format!("modifiers hold one visibility word, but also '{v}'. Fix: keep one of {}.", edit::VISIBILITY.join(", ")).into());
    }
    mods.sort_by_key(|m| edit::modifier_rank(m));
    Ok(mods)
}

/// Library names that are valid types without a source file.
const LIBRARY_TYPES: &[&str] = &["Real", "Integer", "String", "Boolean", "Natural", "Positive", "Number", "Rational", "Complex", "ScalarValues", "ISQ", "SI"];

thread_local! {
    /// Set while a batch runs: how many moves skipped their own "names left unresolved" check. A batch checks once,
    /// after every edit is applied, so packages that refer to each other by short name can move together.
    static DEFERRED_MOVES: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

// ---- read-back guards ---------------------------------------------------------

/// Control characters a tool must not write: all but line break, carriage return and tab. A name, a value or an end
/// has none of them at all (checked where it is read); documentation keeps line breaks and tabs.
#[allow(dead_code)]
fn is_bad_control(c: char) -> bool {
    edit::char_fault(c, false).is_some()
}

/// Refuse a write that put a control character into a file. Every tool write passes here, so a character that a field
/// check missed (one after a backslash inside quotes, one in a name taken from another argument) still cannot reach the file.
fn refuse_new_controls(before: &[FileIn], after: &[FileIn]) -> Res<()> {
    for f in after {
        let old = before.iter().find(|o| o.path == f.path).map(|o| o.text.as_str()).unwrap_or("");
        if f.text == old {
            continue;
        }
        let (p, q) = changed_region(old, &f.text);
        let tail = f.text.len() - q;
        let new_part = f.text.get(p..q).unwrap_or("");
        let old_part = old.get(p..old.len().saturating_sub(tail).max(p)).unwrap_or("");
        // Each character by itself: a new hidden character may not take the place of one that was there.
        if let Some(c) = edit::gained_bad_char(old_part, new_part) {
            return Err(Fail::with(
                format!("the edit would write the control character U+{:04X} into '{}'; nothing was written. Fix: remove control characters from names, values, ends and documentation (line breaks and tabs stay allowed in documentation).", c as u32, f.path),
                json!({ "rule": "control-character" }),
            ));
        }
    }
    Ok(())
}

/// Whether the check on the elements an edit did not name compares their text too. A rename and a move rewrite the
/// references of other elements on purpose, so they compare kinds and names only. A short name changes the text of its
/// own element and of no other, so it compares text like every other update.
fn compares_text(name: &str, op: &Value) -> bool {
    matches!(name, "set_doc" | "add_specialization") || (name == "update_element" && ["rename", "owner", "new_name"].iter().all(|k| op.get(*k).map(Value::is_null).unwrap_or(true)))
}

fn element_key(e: &Element) -> (&str, &str) {
    (e.kind.as_str(), if e.implied_name { "" } else { e.name.as_deref().unwrap_or("") })
}

/// The text an element states by itself: its own words, without the members inside its braces, with runs of white
/// space as one space and without the closing `;` or an empty `{ }`. An owner gaining a member keeps the same head.
fn head_text(m: &Model, e: &Element) -> String {
    let text = &m.files[e.file].text;
    let start = e.span.start.min(text.len());
    let first_child = e.children.iter().map(|c| m.elements[*c].span.start).filter(|s| *s >= start && *s <= e.span.end).min();
    let end = first_child.unwrap_or(e.span.end).clamp(start, text.len());
    let raw = text.get(start..end).unwrap_or("");
    let mut head = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    for _ in 0..3 {
        let t = head.trim_end_matches([' ', ';', '{', '}']).to_string();
        if t == head {
            break;
        }
        head = t;
    }
    head
}

/// Exact multiset of the elements a model holds: kind, definition flag and declared name, with how many.
fn census(m: &Model, skip: &dyn Fn(&Element) -> bool) -> HashMap<(String, bool, String), i64> {
    let mut c: HashMap<(String, bool, String), i64> = HashMap::new();
    for e in m.elements.iter().filter(|e| !skip(e)) {
        let (k, n) = element_key(e);
        *c.entry((k.to_string(), e.is_def, n.to_string())).or_default() += 1;
    }
    c
}

/// Exact multiset of (qualified name, own source text) for the elements a model holds. The text itself is the key, not a
/// hash of it: two texts with the same hash would count as one, and for a hash with no secret key such texts can be made
/// on purpose. The table that holds the keys uses the keyed hasher of the standard library.
fn text_census(m: &Model, skip: &dyn Fn(&Element) -> bool) -> HashMap<(String, String), i64> {
    text_census_with(m, skip, None)
}

/// `text_census` with one id read as another (`(new, old)`): a change of an element's id rewrites the references
/// written by the old id on purpose, and only those.
fn text_census_with(m: &Model, skip: &dyn Fn(&Element) -> bool, swap: Option<(&str, &str)>) -> HashMap<(String, String), i64> {
    let mut c: HashMap<(String, String), i64> = HashMap::new();
    // An element inside a skipped one is the skipped one's own text or members, so it is left out too.
    let under_skipped = |e: &Element| {
        let mut o = e.owner;
        while let Some(x) = o {
            if skip(&m.elements[x]) {
                return true;
            }
            o = m.elements[x].owner;
        }
        false
    };
    for e in m.elements.iter().filter(|e| !skip(e) && !under_skipped(e)) {
        let head = head_text(m, e);
        let head = match swap {
            Some((new, old)) => head.replace(new, old),
            None => head,
        };
        *c.entry((e.qualified_name.clone(), head)).or_default() += 1;
    }
    c
}

/// What changed between two models, kind and name of up to three elements each way, for a message.
fn census_diff(before: &Model, after: &Model, skip_b: &dyn Fn(&Element) -> bool, skip_a: &dyn Fn(&Element) -> bool) -> String {
    let mut count: HashMap<(String, String), i64> = HashMap::new();
    for e in before.elements.iter().filter(|e| !skip_b(e)) {
        let (k, n) = element_key(e);
        *count.entry((k.to_string(), n.to_string())).or_default() -= 1;
    }
    for e in after.elements.iter().filter(|e| !skip_a(e)) {
        let (k, n) = element_key(e);
        *count.entry((k.to_string(), n.to_string())).or_default() += 1;
    }
    let show = |sign: i64| -> String {
        let mut v: Vec<String> = count
            .iter()
            .filter(|(_, c)| **c * sign > 0)
            .map(|((k, n), _)| if n.is_empty() { format!("an unnamed {k}") } else { format!("{k} {}", crate::agent::text::quote_clip(n)) })
            .collect();
        v.sort();
        let more = v.len().saturating_sub(3);
        v.truncate(3);
        if more > 0 {
            v.push(format!("{more} more"));
        }
        v.join(", ")
    };
    let (gone, new) = (show(-1), show(1));
    match (gone.is_empty(), new.is_empty()) {
        (false, false) => format!("gone: {gone}; new: {new}"),
        (false, true) => format!("gone: {gone}"),
        (true, false) => format!("new: {new}"),
        (true, true) => "the same kinds and names, counted differently".to_string(),
    }
}

/// An edit meant to change one element, or to add one, may not add or remove any other element. `skip_b` and `skip_a`
/// pick the elements the edit is allowed to change (the target, or the new subtree) in the model before and after.
fn refuse_stray_elements(what: &str, before: &Model, after: &Model, skip_b: &dyn Fn(&Element) -> bool, skip_a: &dyn Fn(&Element) -> bool) -> Res<()> {
    refuse_stray(what, before, after, skip_b, skip_a, true, None)
}

/// As `refuse_stray_elements`; `text` also compares the source text of every other element by qualified name, so an
/// edit that rewrites a neighbour in place (same kind, same name) is found. Pass `false` for an edit that renames or
/// moves something, because that rewrites the references of other elements on purpose.
/// `swap` is an id change (`(new, old)` as written): the references written by the old id may read the new one.
fn refuse_stray(what: &str, before: &Model, after: &Model, skip_b: &dyn Fn(&Element) -> bool, skip_a: &dyn Fn(&Element) -> bool, text: bool, swap: Option<(&str, &str)>) -> Res<()> {
    // Only the files the edit changed can differ: a file with the same text has the same elements, names and heads
    // (the parse is the file's own). Their elements are compared when the skips treat the untouched files alike.
    if swap.is_none() {
        if let Some((fb, fa)) = changed_only(before, after, skip_b, skip_a) {
            let (sb, sa) = (|e: &Element| !fb.contains(&e.file) || skip_b(e), |e: &Element| !fa.contains(&e.file) || skip_a(e));
            if census(before, &sb) == census(after, &sa) && (!text || text_census(before, &sb) == text_census(after, &sa)) {
                return Ok(());
            }
        }
    } else if census(before, skip_b) == census(after, skip_a) && (!text || text_census(before, skip_b) == text_census_with(after, skip_a, swap)) {
        return Ok(());
    }
    if swap.is_none() && census(before, skip_b) == census(after, skip_a) && (!text || text_census(before, skip_b) == text_census(after, skip_a)) {
        return Ok(());
    }
    let diff = if census(before, skip_b) == census(after, skip_a) { "the text of an element beside it changed".to_string() } else { census_diff(before, after, skip_b, skip_a) };
    Err(Fail::with(
        format!(
            "{what} would have changed the elements around it ({diff}); nothing was written. Fix: the text of a value, name, end or note ran into the text beside it. Check it for comment markers (// or /*), quotes and a trailing '/'.",
        ),
        json!({ "rule": "stray-elements" }),
    ))
}

/// The files an edit changed, in the model before and after (by index), when that is all `refuse_stray` must compare:
/// every other file has the same path and text on both sides, and each of its elements is skipped on both sides or
/// on neither. `None` (compare everything) when a file's elements cannot be paired.
fn changed_only(before: &Model, after: &Model, skip_b: &dyn Fn(&Element) -> bool, skip_a: &dyn Fn(&Element) -> bool) -> Option<(HashSet<usize>, HashSet<usize>)> {
    let ranges = |m: &Model| -> Option<Vec<(usize, usize)>> {
        let mut r = vec![(0usize, 0usize); m.files.len()];
        let mut last = 0usize;
        for (i, e) in m.elements.iter().enumerate() {
            if e.file < last || e.file >= m.files.len() {
                return None;
            }
            if r[e.file].1 == 0 {
                r[e.file] = (i, i + 1);
            } else {
                r[e.file].1 = i + 1;
            }
            last = e.file;
        }
        Some(r)
    };
    let (rb, ra) = (ranges(before)?, ranges(after)?);
    let by_path: HashMap<&str, usize> = after.files.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
    let (mut fb, mut fa): (HashSet<usize>, HashSet<usize>) = ((0..before.files.len()).collect(), (0..after.files.len()).collect());
    for (i, f) in before.files.iter().enumerate() {
        let Some(&j) = by_path.get(f.path.as_str()) else { continue };
        if after.files[j].text != f.text {
            continue;
        }
        let ((b0, b1), (a0, a1)) = (rb[i], ra[j]);
        if b1 - b0 != a1 - a0 {
            return None;
        }
        // an element under a skipped owner is left out of the text census: owners are in the same file
        if (0..b1 - b0).any(|k| skip_b(&before.elements[b0 + k]) != skip_a(&after.elements[a0 + k])) {
            return None;
        }
        fb.remove(&i);
        fa.remove(&j);
    }
    Some((fb, fa))
}

/// The elements of the subtree rooted at `root`, `root` included.
fn subtree_ids(m: &Model, root: ElementId) -> HashSet<ElementId> {
    let mut set: HashSet<ElementId> = m.descendants(root).into_iter().collect();
    set.insert(root);
    set
}

/// Count of unresolved names in a file set, as the move checks count them.
fn unresolved_names(files: &[FileIn]) -> usize {
    model_of(files).diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).count()
}

thread_local! {
    /// Set while a batch of plain links and retypes runs: `run_edit` leaves the whole-model checks to `batch_checks`.
    static DEFER_CHECKS: Cell<bool> = const { Cell::new(false) };
}

struct UndeferGuard;
impl Drop for UndeferGuard {
    fn drop(&mut self) {
        DEFER_CHECKS.with(|c| c.set(false));
    }
}

/// `SYSML_EACH_CHECK=1` runs the whole-model checks after every edit of every batch, as before.
fn deferrable_checks() -> bool {
    std::env::var_os("SYSML_EACH_CHECK").is_none()
}

/// An edit that adds no name and moves or renames nothing: a relationship, or an update of an element's type, doc,
/// value or multiplicity.
fn plain_link_or_retype(e: &Value) -> bool {
    match e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) {
        Some("create_relationship") => e.get("name").map_or(true, Value::is_null) && e.get("short_name").map_or(true, Value::is_null),
        Some("update_element") => e.as_object().is_some_and(|o| o.keys().all(|k| matches!(k.as_str(), "op" | "tool" | "ref" | "typed_by" | "doc" | "value" | "multiplicity" | "direction"))),
        _ => false,
    }
}

/// An edit whose whole-model checks may wait for the end of its batch: a plain link or retype, or an `add_interface`
/// (its ports, connection and flow are new names; `batch_checks` runs the same capture, namespace, ownership and copy
/// checks on the batch, and a refusal there runs the batch again edit by edit, which names the edit).
fn deferrable(e: &Value) -> bool {
    plain_link_or_retype(e) || e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) == Some("add_interface")
}

/// The checks `run_edit` runs after each edit, run once on the text before a batch and after it.
fn batch_checks(before: &[FileIn], after: &[FileIn], events: &[IdEvent]) -> Res<()> {
    let (b, a) = (model_of(before), model_of(after));
    super::capture::refuse(&b, &a, events)?;
    super::capture::refuse_namespace_faults(&b, &a)?;
    super::ownership::refuse_new(&b, &a)?;
    super::duplicates::refuse_new(&b, &a)
}

struct DeferGuard;
impl Drop for DeferGuard {
    fn drop(&mut self) {
        DEFERRED_MOVES.with(|c| c.set(None));
    }
}

/// Edits in one batch: far above any batch an agent or an import of a real file makes (a ReqIF file of 10,000 objects is
/// about 25,000 edits and is sent in parts of this size).
pub const MAX_EDITS: usize = 20_000;

/// The refusal for a batch of the wrong size. Every edit counts toward the limit, also edits a script generated
/// (for example stand-in elements added for a statement), so a caller that adds edits must leave room.
pub fn too_many(n: usize) -> String {
    format!("edits must hold between 1 and {MAX_EDITS} edits; got {n}. The limit is {MAX_EDITS} per call, and every edit counts, including generated stand-in edits. Fix: split the batch.")
}
/// Serialized size of a whole batch, in bytes (one edit has no size limit of its own: an ICD package or a big
/// replace is one edit, and its parse is linear).
const MAX_BATCH_BYTES: usize = 64 << 20;
/// Nested `features` per create_element: total members and depth.
const MAX_FEATURES: usize = 500;
const MAX_FEATURE_DEPTH: usize = 8;
/// Names in one list argument (`specializes`, `redefines`).
const MAX_LIST: usize = 50;
/// Runs of `apply_op` one batch may cost, counting the re-runs when edits are reordered.
const MAX_BATCH_RUNS: usize = 2000;

/// Refuse an edit whose size could make the work behind it explode. Runs before any parsing.
pub fn check_edit_size(op: &Value) -> Res<()> {
    for k in ["specializes", "redefines"] {
        if let Some(Value::Array(v)) = op.get(k) {
            if v.len() > MAX_LIST {
                return Err(Fail::from(format!("refused: '{k}' lists {} names; at most {MAX_LIST}. Fix: pass fewer, or add the rest with create_relationship.", v.len())));
            }
        }
    }
    let (mut count, mut stack) = (0usize, vec![(op, 0usize)]);
    while let Some((v, depth)) = stack.pop() {
        for f in v.get("features").and_then(Value::as_array).into_iter().flatten() {
            count += 1;
            if count > MAX_FEATURES || depth + 1 > MAX_FEATURE_DEPTH {
                return Err(Fail::from(format!("refused: features[] holds more than {MAX_FEATURES} members or nests deeper than {MAX_FEATURE_DEPTH}. Fix: create the members with separate create_element calls.")));
            }
            for k in ["specializes", "redefines"] {
                if let Some(Value::Array(l)) = f.get(k) {
                    if l.len() > MAX_LIST {
                        return Err(Fail::from(format!("refused: '{k}' lists {} names; at most {MAX_LIST}.", l.len())));
                    }
                }
            }
            stack.push((f, depth + 1));
        }
    }
    Ok(())
}

thread_local! {
    /// The models of the file sets the running edit has just worked on, newest first. An edit asks for the model of the same
    /// text again and again (`step`, a read-back, the next edit of a batch); one analysis serves all of them.
    static MODELS: std::cell::RefCell<Vec<(Vec<FileIn>, Arc<Model>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Models kept; text larger than `MODEL_CACHE_BYTES` is not kept at all.
const MODEL_CACHE_SLOTS: usize = 3;
const MODEL_CACHE_BYTES: usize = 64 << 20;

fn same_text(a: &[FileIn], b: &[FileIn]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.text.len() == y.text.len() && x.path == y.path && x.text == y.text)
}

/// Drops the models kept for the edits of one call when the call ends, so a long-lived thread holds none.
pub struct ModelCache;

impl Drop for ModelCache {
    fn drop(&mut self) {
        if HELD.with(Cell::get) == 0 {
            MODELS.with(|c| c.borrow_mut().clear());
            super::capture::clear_tables();
        }
    }
}

thread_local! {
    static HELD: Cell<usize> = const { Cell::new(0) };
}

/// Keeps the models of a call's edits until it drops, for a caller that reads the model of the result after the batch (the
/// wasm build door rekeys display.json with it): the batch analysed that text already. Dropping it lets them go.
pub struct HoldModels(());

impl HoldModels {
    pub fn new() -> Self {
        HELD.with(|h| h.set(h.get() + 1));
        HoldModels(())
    }
}

impl Default for HoldModels {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HoldModels {
    fn drop(&mut self) {
        HELD.with(|h| h.set(h.get().saturating_sub(1)));
        drop(ModelCache);
    }
}

/// The model of `files`: parsed and resolved once for as long as the text stays the same.
pub fn model_of(files: &[FileIn]) -> Arc<Model> {
    let hit = MODELS.with(|c| {
        let mut c = c.borrow_mut();
        let i = c.iter().position(|(f, _)| same_text(f, files))?;
        let entry = c.remove(i);
        let m = entry.1.clone();
        c.insert(0, entry);
        Some(m)
    });
    if let Some(m) = hit {
        return m;
    }
    wave::count_analysis();
    let m = Arc::new(api::to_model(files.to_vec()));
    if files.iter().map(|f| f.text.len()).sum::<usize>() <= MODEL_CACHE_BYTES {
        MODELS.with(|c| {
            let mut c = c.borrow_mut();
            c.insert(0, (files.to_vec(), m.clone()));
            c.truncate(MODEL_CACHE_SLOTS);
        });
    }
    m
}

/// Apply one edit to a working file set.
pub fn step(files: &mut Vec<FileIn>, op: EditOp) -> Res<()> {
    let model = model_of(files);
    for c in edit::apply(&model, &op)? {
        match files.iter_mut().find(|f| f.path == c.path) {
            Some(f) => f.text = c.text,
            None => files.push(FileIn { path: c.path, text: c.text }),
        }
    }
    Ok(())
}

/// Add `text` inside `owner`, or at the end of `file` when there is no owner.
pub fn add(files: &mut Vec<FileIn>, owner: Option<&str>, file: Option<&str>, text: &str) -> Res<()> {
    step(files, EditOp::AddElement { owner: owner.map(str::to_string), file: file.map(str::to_string), text: text.to_string() })
}

fn text_arg<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// `file` and `path` name the same thing; either is accepted wherever one is taken.
fn file_arg(a: &Value) -> Option<&str> {
    text_arg(a, "file").or_else(|| text_arg(a, "path"))
}

fn flag(a: &Value, k: &str) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(false)
}

/// What an argument is, for the message when it is missing.
pub fn arg_what(k: &str) -> &'static str {
    match k {
        "kind" => "the kind, for example 'part', 'part def' or 'requirement'",
        "name" => "the element name",
        "ref" => "the qualified name of the element, for example Pkg::Part",
        "owner" => "the qualified name of the element that holds the new one",
        "source" => "the qualified name of the element the relationship starts at",
        "target" => "the qualified name of the element the relationship ends at, or of the element to change",
        "context" => "the qualified name of the part that holds the connection",
        "from" | "to" => "the qualified name of a part usage",
        "item" => "an item definition name or qualified name",
        "scope" => "the qualified name of the package or element to look at",
        "path" | "file" => "a workspace-relative file path ending in .sysml, for example vehicle.sysml",
        "text" => "the SysML v2 text",
        "new_name" => "the new element name",
        "general" => "the qualified name of the more general element",
        "doc" => "the documentation text",
        "source_path" => "the path of the project file, relative to the workspace folder",
        _ => "a required value",
    }
}

/// A required text argument; the message says which one and what it is.
pub fn req_as<'a>(a: &'a Value, k: &str, what: &str) -> Res<&'a str> {
    match a.get(k) {
        None | Some(Value::Null) => Err(Fail::from(format!("missing argument '{k}': {what}."))),
        Some(Value::String(_)) => text_arg(a, k).ok_or_else(|| Fail::from(format!("argument '{k}' is empty: it must be {what}."))),
        Some(_) => Err(Fail::from(format!("argument '{k}' must be a string: {what}."))),
    }
}

pub fn req<'a>(a: &'a Value, k: &str) -> Res<&'a str> {
    req_as(a, k, arg_what(k))
}

/// One text or a list of texts.
fn list_arg(a: &Value, k: &str) -> Res<Vec<String>> {
    let bad = || Fail::from(format!("'{k}' must be a name or a list of names"));
    match a.get(k) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(Some(s.trim()).filter(|s| !s.is_empty()).map(str::to_string).into_iter().collect()),
        Some(Value::Array(v)) => v.iter().map(|x| x.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or_else(bad)).collect(),
        Some(_) => Err(bad()),
    }
}

/// Words that start or end a clause of a transition or succession. A guard, an effect or a trigger that holds one outside
/// quotes would open another clause or statement part of its own.
const CLAUSE_WORDS: &[&str] = &["if", "do", "then", "accept"];

/// Why `scan_expr` refused a text.
enum ExprErr {
    Empty,
    /// A plain reason, for "is not a plain expression (reason)".
    Bad(String),
    /// A reserved word outside quotes.
    Word,
}

/// An expression that cannot end the statement or open a block: `5`, `12 [V]`, `mass * 2`.
fn safe_expr(what: &str, v: &str) -> Res<String> {
    let text = scan_expr(v, &[]).map_err(|e| expr_fail(what, v, e))?;
    parsed_clause(what, v, &text)?;
    Ok(text)
}

/// The second check on a text that passed `scan_expr`: it must parse as what it is written as. The characters alone do not
/// say that: `action evil : P::Act` holds only letters and a colon, and as an effect it declares an element. `what` names the
/// field; an effect is the name of an action, a send or an assign, a trigger is `after`, `at` or `when` with an expression or
/// a name, every other field is one expression. When the recogniser of the official grammar is registered (the server
/// registers it), the text is also read by it inside the statement it goes into, so what the tools save is what that
/// grammar accepts.
fn parsed_clause(what: &str, given: &str, text: &str) -> Res<()> {
    let kind = if what.starts_with("trigger") { "trigger" } else { what };
    let own = match kind {
        "effect" => super::expr::effect(text),
        "trigger" => {
            let rest = ["after", "when", "at"].iter().find_map(|w| text.strip_prefix(w).filter(|r| r.starts_with(char::is_whitespace)));
            match rest {
                Some(r) => super::expr::expression(r),
                // The name of a signal, or `Sig via port`; a payload name (`s : Sig`) is refused by `trigger_text`.
                None => super::expr::trigger(text),
            }
        }
        _ => super::expr::expression(text),
    };
    if let Err(why) = own {
        return Err(match kind {
            "effect" => Fail::from(format!("effect {} is not the name of an action, 'send X to Y' or 'assign a := b' ({why}). Fix: name an action that exists, or write the send or the assignment; an action declared inside the transition goes in with edit_model.", crate::agent::text::quote_clip(given.trim()))),
            _ => expr_fail(what, given, ExprErr::Bad(why)),
        });
    }
    let Some(check) = super::rules::official_checker() else { return Ok(()) };
    let (head, tail) = match kind {
        "effect" => ("package P { state def S { state a; state b; transition first a do ", " then b; } }"),
        "trigger" => ("package P { state def S { state a; state b; transition first a accept ", " then b; } }"),
        "guard" => ("package P { state def S { state a; state b; transition first a if ", " then b; } }"),
        "body" => ("package P { constraint c { ", " } }"),
        _ => ("package P { attribute x = ", "; }"),
    };
    match check(&format!("{head}{text}{tail}")).0 {
        super::rules::OfficialVerdict::Valid => {}
        super::rules::OfficialVerdict::Invalid { column, found, allowed, .. } => {
            let at = (column as usize).saturating_sub(head.len() + 1);
            let found = if at >= text.len() { "the end of the text".to_string() } else { crate::agent::text::quote_clip(&found) };
            return Err(Fail::from(format!(
                "{what} {} is not valid SysML v2: the official grammar does not allow {found} at character {} of it (it allows {}). Fix: write the {kind} in a form the grammar allows; nothing was written.",
                crate::agent::text::quote_clip(given.trim()),
                at.min(text.len()) + 1,
                if allowed.is_empty() { "nothing there".to_string() } else { allowed.iter().take(8).cloned().collect::<Vec<_>>().join(" ") }
            )));
        }
        // A text the recogniser did not read to the end is written: the official gate reports the file with a note.
        super::rules::OfficialVerdict::NotChecked(_) => {}
    }
    Ok(())
}

/// `safe_expr` for a guard, an effect or a trigger: it also refuses `if`, `do`, `then` and `accept` outside quotes.
fn clause_expr(what: &str, v: &str) -> Res<String> {
    let text = scan_expr(v, CLAUSE_WORDS).map_err(|e| expr_fail(what, v, e))?;
    parsed_clause(what, v, &text)?;
    Ok(text)
}

fn expr_fail(what: &str, v: &str, e: ExprErr) -> Fail {
    let v = v.trim();
    match e {
        ExprErr::Empty => Fail::from(format!("{what} is empty")),
        ExprErr::Bad(why) => Fail::from(format!(
            "{what} {} is not a plain expression ({why}). Fix: use a literal, a name or arithmetic such as '5', '12 [V]' or 'mass * 2'; other text goes in with edit_model.",
            crate::agent::text::quote_clip(v)
        )),
        ExprErr::Word => Fail::from(format!("{what} {} contains a reserved word (if, do, then, accept). Fix: rewrite the expression without it.", crate::agent::text::quote_clip(v))),
    }
}

/// The one scanner for caller text that goes into a statement as an expression. It reads the text as the lexer does,
/// through the lexer's own functions for quotes, identifiers and numbers: a quoted text or name ends at its own quote, a
/// backslash escapes the next byte, a quote of the other kind inside one is text. Brackets must match, only the characters
/// of an expression stand outside quotes, no control character stands anywhere, and an identifier outside quotes may not be
/// one of `reserved`. The reserved check is made in this loop, in the same quote state, and not on a second reading.
fn scan_expr(v: &str, reserved: &[&'static str]) -> Result<String, ExprErr> {
    let bad = |why: &str| ExprErr::Bad(why.to_string());
    let v = v.trim();
    if v.is_empty() {
        return Err(ExprErr::Empty);
    }
    if v.len() > 200 {
        return Err(bad("longer than 200 characters"));
    }
    if v.contains(['{', '}', ';']) {
        return Err(bad("braces and semicolons are not allowed"));
    }
    if v.contains("//") || v.contains("/*") || v.contains("*/") {
        return Err(bad("comment markers are not allowed"));
    }
    // A value is spliced into text that goes on after it. Ending in `/`, or starting with `/` or `*`, can make a comment
    // marker with the text beside it, and an expression has no such end or start.
    if v.ends_with('/') || v.starts_with(['/', '*']) {
        return Err(bad("an expression does not start with '/' or '*' or end with '/'"));
    }
    let b = v.as_bytes();
    let (mut round, mut square) = (0i32, 0i32);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\'' | b'"' => {
                let (end, closed) = lexer::quoted_end(b, i);
                if !closed {
                    return Err(bad("brackets or quotes do not match"));
                }
                // `i` and `end` stand on and after ASCII quotes, so they are character boundaries.
                // What stands in single quotes is a name, so it follows the rule for names.
                if v[i..end].chars().any(|x| x.is_control() || edit::char_fault(x, c == b'\'').is_some()) {
                    return Err(bad("a control character, a character that does not show or a line break inside quotes is not allowed"));
                }
                i = end;
                continue;
            }
            b'(' => round += 1,
            b')' => round -= 1,
            b'[' => square += 1,
            b']' => square -= 1,
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let end = lexer::ident_end(b, i);
                if reserved.iter().any(|w| *w == &v[i..end]) {
                    return Err(ExprErr::Word);
                }
                i = end;
                continue;
            }
            c if c.is_ascii_digit() => {
                i = lexer::number_end(b, i);
                continue;
            }
            c if b" .+-*/%<>=!,:^&|~?".contains(&c) => {}
            _ => {
                let ch = v[i..].chars().next().unwrap_or('?');
                return Err(ExprErr::Bad(format!("character '{}' is not allowed outside quotes", ch.escape_debug())));
            }
        }
        if round < 0 || square < 0 {
            return Err(bad("brackets do not match"));
        }
        i += 1;
    }
    if round != 0 || square != 0 {
        return Err(bad("brackets or quotes do not match"));
    }
    Ok(v.to_string())
}

/// `4`, `1..*`, `0..n`: the text between the brackets, parsed. The official notation (SysML 8.2.2.6.6,
/// MultiplicityBounds) is one bound, or a lower and an upper bound with `..` between them. A bound is a whole number
/// or the name of a feature; the upper bound, or the only bound, may be `*`. The text is written back from the tokens.
fn safe_multiplicity(m: &str) -> Res<String> {
    let given = m.trim();
    let bad = |why: &str| Fail::from(format!("multiplicity {} is not valid: {why}. Fix: use a count or range such as 4, 0..1, 1..* or 0..n.", crate::agent::text::quote_clip(given)));
    let m = given.strip_prefix('[').map_or(given, |r| r.strip_suffix(']').unwrap_or(r)).trim();
    if m.is_empty() || m.len() > 80 {
        return Err(bad("it is empty or longer than 80 bytes"));
    }
    let (toks, errs) = crate::lexer::lex(m);
    if !errs.is_empty() {
        return Err(bad("it has a character the notation does not read"));
    }
    let text = |t: &crate::lexer::Tok| &m[t.span.start..t.span.end];
    let toks: Vec<&crate::lexer::Tok> = toks.iter().filter(|t| t.kind != crate::lexer::TokKind::Eof).collect();
    let at = std::cell::Cell::new(0usize);
    // One bound: a whole number, `*`, or a name with `::` between its parts.
    let bound = |star: bool| -> Res<String> {
        use crate::lexer::TokKind::*;
        let Some(t) = toks.get(at.get()) else { return Err(bad("a bound is missing")) };
        at.set(at.get() + 1);
        match (t.kind, text(t)) {
            (Number, n) if n.bytes().all(|b| b.is_ascii_digit()) => Ok(n.to_string()),
            (Number, _) => Err(bad("a bound is a whole number")),
            (Punct, "*") if star => Ok("*".into()),
            (Punct, "*") => Err(bad("'*' is the upper bound only")),
            (Ident | Name, first) => {
                let mut out = String::new();
                let mut part = first;
                loop {
                    if crate::model::RESERVED.contains(&part) {
                        return Err(bad(&format!("'{part}' is a reserved word")));
                    }
                    if part.starts_with(|c: char| c.is_ascii_digit()) {
                        return Err(bad(&format!("a bound is a whole number or a name, and a name that starts with a digit goes in quotes ('{part}')")));
                    }
                    out.push_str(part);
                    match (toks.get(at.get()), toks.get(at.get() + 1)) {
                        (Some(c), Some(n)) if text(c) == "::" && matches!(n.kind, Ident | Name) => {
                            out.push_str("::");
                            part = text(n);
                            at.set(at.get() + 2);
                        }
                        _ => return Ok(out),
                    }
                }
            }
            (_, other) => Err(bad(&format!("{} is not a bound", crate::agent::text::quote_clip(other)))),
        }
    };
    let mut out = bound(true)?;
    let range = toks.get(at.get()).is_some_and(|t| text(t) == "..");
    if range {
        if out == "*" {
            return Err(bad("'*' is the upper bound only"));
        }
        at.set(at.get() + 1);
        out = format!("{out}..{}", bound(true)?);
    }
    match toks.get(at.get()) {
        Some(t) => Err(bad(&format!("{} comes after the bounds", crate::agent::text::quote_clip(text(t))))),
        None => Ok(out),
    }
}

fn safe_short_name(v: &str) -> Res<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 60 || v.chars().any(|c| edit::char_fault(c, true).is_some() || "'\\<>".contains(c)) {
        return Err(Fail::from(format!("short_name {} is not valid. Fix: use 1 to 60 characters without quotes, angle brackets or line breaks, for example R-1.", crate::agent::text::quote_clip(v))));
    }
    Ok(v.to_string())
}

/// A name a tool writes: one line, no control character, not longer than an edit may write.
pub(super) fn clean_name(what: &str, n: &str) -> Res<()> {
    if n.len() > edit::MAX_NAME_BYTES {
        return Err(format!("{what} is {} bytes, which is too long; at most {}.", n.len(), edit::MAX_NAME_BYTES).into());
    }
    match edit::name_fault(n) {
        Some(('\n' | '\r' | '\u{2028}' | '\u{2029}', _)) => Err(format!("{what} must be one line.").into()),
        Some((c, why)) if c.is_control() => Err(format!("{what} holds the control character U+{:04X} ({why}). Fix: remove it.", c as u32).into()),
        Some((c, why)) => Err(format!("{what} holds the character U+{:04X}, {why}. Fix: remove it, or use the plain space.", c as u32).into()),
        None => Ok(()),
    }
}

/// Text that goes into documentation or a comment: line breaks and tabs are kept, other control characters are refused.
fn clean_doc(what: &str, t: &str) -> Res<()> {
    edit::check_doc_text(what, t).map_err(Fail::from)
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every definition and usage that `id` is typed by or specializes, directly or not.
fn supertypes(model: &Model, id: ElementId) -> BTreeSet<ElementId> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![id];
    while let Some(x) = stack.pop() {
        for t in model.elements[x].refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes)).filter_map(|r| r.resolved()) {
            if seen.insert(t) {
                stack.push(t);
            }
        }
    }
    seen
}

pub fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Arguments each raw edit needs.
fn raw_required(op: &str) -> &'static [&'static str] {
    match op {
        "add_element" => &["text"],
        "rename" => &["target", "new_name"],
        "delete" => &["target"],
        "replace" => &["target", "text"],
        "set_doc" => &["target", "doc"],
        "add_specialization" => &["target", "general"],
        _ => &[],
    }
}

/// Refuse a raw add that gives a named element the name of a member the owner already has.
fn refuse_duplicates(before: &Model, after: &Model, owner: Option<&str>) -> Res<()> {
    let members = |m: &Model| -> Vec<(String, ElementId)> {
        let ids: Vec<ElementId> = match owner {
            Some(o) => m.find(o).map(|e| e.children.clone()).unwrap_or_default(),
            None => m.roots.clone(),
        };
        ids.into_iter().filter(|i| !m.elements[*i].implied_name).filter_map(|i| m.elements[i].name.clone().map(|n| (n, i))).collect()
    };
    let (old, new) = (members(before), members(after));
    fn count(v: &[(String, ElementId)]) -> std::collections::HashMap<&str, usize> {
        let mut m = std::collections::HashMap::new();
        for (n, _) in v {
            *m.entry(n.as_str()).or_default() += 1;
        }
        m
    }
    let (old_n, new_n) = (count(&old), count(&new));
    for (n, id) in &new {
        let (now, was) = (new_n[n.as_str()], old_n.get(n.as_str()).copied().unwrap_or(0));
        if now <= 1 || now <= was {
            continue;
        }
        let first = new.iter().find(|(x, _)| x == n).map(|(_, i)| *i).unwrap_or(*id);
        let (dup, added) = (&after.elements[first], &after.elements[*id]);
        // A package may be spread over several files.
        if dup.kind == "package" && added.kind == "package" && dup.file != added.file {
            continue;
        }
        if first == *id {
            continue;
        }
        return Err(Fail::from(format!(
            "'{}' already has a member named '{n}'. Choose another name, or change the existing one with edit_model replace {{target: '{}'}} or update_element {{ref: '{}'}}. Nothing was written.",
            owner.unwrap_or("the file"),
            dup.qualified_name,
            dup.qualified_name
        )));
    }
    Ok(())
}

/// Run one edit, given as `{op: ..., ...}`. `op` is a raw edit (`edit_model`) or an intent-level tool.
pub fn apply_op(files: &[FileIn], op: &Value) -> Res<Applied> {
    apply_op_ids(files, op, &BTreeMap::new())
}

/// Like `apply_op`, with the source ids in force: `ids` maps an id to a qualified name, so a ref may be `source_id:<id>`.
/// The reply's `info.source_id_moves` lists every id that left one element for another.
pub fn apply_op_ids(files: &[FileIn], op: &Value, ids: &BTreeMap<String, String>) -> Res<Applied> {
    let applied = apply_op_inner(files, op, ids)?;
    observe(&[op], files, &applied.files);
    Ok(applied)
}

fn apply_op_inner(files: &[FileIn], op: &Value, ids: &BTreeMap<String, String>) -> Res<Applied> {
    let _models = ModelCache;
    let mut work = files.to_vec();
    let mut track = Track::new(ids.clone());
    let (planned, placed) = place_edits(files, std::slice::from_ref(op));
    let (mut info, events) = apply_in_place(&mut work, &planned[0], &mut None, &mut track)?;
    if let Some(p) = placed.into_iter().next().flatten() {
        info["placed"] = p;
    }
    if !track.notes.is_empty() {
        info["source_id_moves"] = json!(track.notes.iter().map(|n| n.to_json()).collect::<Vec<_>>());
    }
    Ok(Applied { files: work, info, events })
}

/// Run one edit on `work`. A failed edit may leave `work` half changed: the caller drops it.
/// `cache` holds a parse of `work` that an edit which changes no file text (`set_source_id`) may reuse.
fn apply_in_place(work: &mut Vec<FileIn>, op: &Value, cache: &mut Option<Arc<Model>>, track: &mut Track) -> Res<(Value, Vec<IdEvent>)> {
    run_edit(work, op, cache, track).map_err(|mut f| {
        f.message = undouble_quotes(&f.message);
        f
    })
}

/// A message that wraps a qualified name in quotes shows `''Name''` when the name has quotes of its own
/// (`'Name'`). Quotes inside a name are written `\'`, so `''` is always a wrapper: drop the wrapper's two quotes.
pub fn undouble_quotes(msg: &str) -> String {
    let b: Vec<char> = msg.chars().collect();
    let mut out = String::with_capacity(msg.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\'' && b.get(i + 1) == Some(&'\'') && i.checked_sub(1).map(|p| b[p] != '\\').unwrap_or(true) {
            // Read the qualified name that follows: quoted or bare segments joined by `::`.
            let mut j = i + 1;
            let mut ok = false;
            loop {
                if b.get(j) == Some(&'\'') {
                    let mut k = j + 1;
                    while k < b.len() && b[k] != '\'' && k - j < 300 {
                        k += if b[k] == '\\' { 2 } else { 1 };
                    }
                    if b.get(k) != Some(&'\'') {
                        break;
                    }
                    j = k + 1;
                } else {
                    let from = j;
                    while j < b.len() && (b[j].is_alphanumeric() || b[j] == '_') {
                        j += 1;
                    }
                    if j == from {
                        break;
                    }
                }
                if b.get(j) == Some(&':') && b.get(j + 1) == Some(&':') {
                    j += 2;
                    continue;
                }
                ok = b.get(j) == Some(&'\'');
                break;
            }
            if ok {
                out.extend(&b[i + 1..j]);
                i = j + 1;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// True when a text in the op writes a unit in brackets after a number (`12 [V]`, `after 5 [s]`).
pub(super) fn op_writes_unit(op: &Value) -> bool {
    match op {
        Value::String(t) => t.contains('[') && !super::units::unit_names(t).is_empty(),
        Value::Array(v) => v.iter().any(op_writes_unit),
        Value::Object(o) => o.values().any(op_writes_unit),
        _ => false,
    }
}

/// After an edit that wrote a unit: give each unit that does not resolve the import of its library, in the package that
/// holds the element (`private import SI::*;`), and say so in `info.imports_added`. Units that did not resolve before the edit,
/// and units no library declares, are left alone.
fn add_unit_imports(work: &mut Vec<FileIn>, before: &HashSet<(String, String)>, info: &mut Value) -> Res<()> {
    let model = model_of(work);
    let mut todo: Vec<(ElementId, &'static str)> = Vec::new();
    for g in super::units::gaps(&model) {
        if before.contains(&(model.elements[g.element].qualified_name.clone(), g.unit.clone())) {
            continue;
        }
        // A unit no library declares (kWh) stays a gap whatever is imported.
        let name = g.unit.rsplit("::").next().unwrap_or(&g.unit);
        if g.unit.contains("::") || (!super::units::declares("SI", name) && !super::units::declares("USCustomaryUnits", name)) {
            continue;
        }
        let root = super::units::import_home(&model, g.element);
        let lib = super::units::library_for(&g.unit);
        if !todo.iter().any(|(r, l)| *r == root && *l == lib) {
            todo.push((root, lib));
        }
    }
    let mut added = Vec::new();
    for (root, lib) in todo {
        let r = &model.elements[root];
        let text = format!("private import {lib};");
        if r.kind == "package" {
            add(work, Some(&r.qualified_name.clone()), None, &text)?;
        } else {
            add(work, None, Some(&model.files[r.file].path.clone()), &text)?;
        }
        added.push(json!({ "in": if r.kind == "package" { r.qualified_name.clone() } else { model.files[r.file].path.clone() }, "import": lib, "library": "the unit library of the official sysml.library" }));
    }
    if !added.is_empty() {
        info["imports_added"] = json!(added);
    }
    Ok(())
}

fn run_edit(work: &mut Vec<FileIn>, op: &Value, cache: &mut Option<Arc<Model>>, track: &mut Track) -> Res<(Value, Vec<IdEvent>)> {
    check_edit_size(op)?;
    let (resolved, from_ids) = track.resolve(op)?;
    let op = &resolved;
    let name = op
        .get("op")
        .or_else(|| op.get("tool"))
        .and_then(Value::as_str)
        .ok_or_else(|| Fail::from(format!("each edit needs an 'op'. Use one of: {}, {}.", RAW_OPS.join(", "), INTENT_OPS.join(", "))))?;
    if name != "set_source_id" {
        *cache = None;
    }
    let mut events: Vec<IdEvent> = Vec::new();
    // A unit the edit writes must resolve afterwards: the units known to be unresolved now are not this edit's.
    let units_before: Option<HashSet<(String, String)>> = (matches!(name, "create_element" | "update_element" | "create_relationship" | "add_interface") && !wave::active() && op_writes_unit(op)).then(|| {
        let m = model_of(work);
        super::units::gaps(&m).into_iter().map(|g| (m.elements[g.element].qualified_name.clone(), g.unit)).collect()
    });
    // In a wave nothing is written by the edit itself: the wave rewrites names and checks the new text once, for all its edits.
    let waving = wave::active();
    let snapshot: Vec<FileIn> = if NAME_OPS.contains(&name) && !waving { work.clone() } else { Vec::new() };
    // The text as it was, for the checks that compare it with the text the edit leaves.
    let before_files: Vec<FileIn> = if name == "set_source_id" || waving { Vec::new() } else { work.clone() };
    // Edits that change one element: the model before them, so the elements around the target can be compared afterwards.
    let one_element = ONE_ELEMENT_OPS.contains(&name);
    let guard_model = if one_element { Some(model_of(work)) } else { None };
    let source_id = match op.get("source_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if SOURCE_ID_OPS.contains(&name) => Some(ids::valid_source_id(s)?),
        Some(Value::String(_)) => return Err(format!("source_id applies to {}, not {}.", SOURCE_ID_OPS.join(", "), crate::agent::text::quote_clip(name)).into()),
        Some(_) => return Err("source_id must be a string: an id from the source project, for example a Cameo xmi id.".into()),
    };
    // `source_ids`: an element with several ids in the source project takes them all in one call. The first is the one
    // reported as `source_id`.
    let mut source_id = source_id;
    let mut more_ids: Vec<String> = Vec::new();
    match op.get("source_ids") {
        None | Some(Value::Null) => {}
        Some(_) if !SOURCE_ID_OPS.contains(&name) || name == "set_source_id" => {
            return Err(format!("source_ids applies to create_element, create_relationship, add_interface, update_element and add_element, not {}. Fix: use source_id, or one set_source_id per id.", crate::agent::text::quote_clip(name)).into());
        }
        Some(Value::Array(list)) if !list.is_empty() && list.len() <= 50 => {
            for v in list {
                let Some(t) = v.as_str() else { return Err("source_ids must be a list of strings: ids from the source project.".into()) };
                let id = ids::valid_source_id(t)?;
                if source_id.is_none() {
                    source_id = Some(id);
                } else if source_id.as_deref() != Some(id.as_str()) && !more_ids.contains(&id) {
                    more_ids.push(id);
                }
            }
        }
        Some(_) => return Err("source_ids must be a list of 1 to 50 strings: ids from the source project, for example Cameo xmi ids.".into()),
    }
    // `before` / `after` / `position`: where a created member goes among the members of its owner.
    let placement = if super::order::asks_for_place(op) {
        if !PLACE_OPS.contains(&name) {
            return Err(format!("before, after and position apply to {}, not {}. Fix: use reorder_members to move members that exist.", PLACE_OPS.join(", "), crate::agent::text::quote_clip(name)).into());
        }
        super::order::placement_of(op)?
    } else if name == "create_element" && text_arg(op, "kind").map(norm_kind).as_deref() == Some("import") {
        // An import goes with the imports: after the last one its owner has, or first in the body when it has none.
        // So the members of a rebuilt namespace come out in the order of the source without a reorder.
        let m = model_of(work);
        match text_arg(op, "owner").and_then(|o| locate(&m, o).ok()) {
            Some(o) => match o.children.iter().rposition(|c| m.elements[*c].kind == "import") {
                Some(i) if i + 1 == o.children.len() => None,
                Some(i) => Some(super::order::Placement::After(m.elements[o.children[i]].qualified_name.clone())),
                None if o.children.is_empty() => None,
                None => Some(super::order::Placement::First),
            },
            None => None,
        }
    } else {
        None
    };
    let before_model = placement.as_ref().map(|_| model_of(work));
    let mut info = match name {
        n if RAW_OPS.contains(&n) => {
            let mut v = op.clone();
            v["op"] = json!(n);
            for k in raw_required(n) {
                if *k == "text" || *k == "doc" {
                    // Text may hold any characters, so check presence only.
                    if !v.get(*k).map(Value::is_string).unwrap_or(false) {
                        req(&v, k)?;
                    }
                } else {
                    req(&v, k)?;
                }
            }
            if v.get("file").is_none() {
                if let Some(p) = text_arg(op, "path") {
                    v["file"] = json!(p);
                }
            }
            let parsed: EditOp = serde_json::from_value(v).map_err(|e| Fail::from(format!("bad edit: {e}")))?;
            match &parsed {
                EditOp::Rename { target, new_name } => {
                    let m = model_of(work);
                    if let Some(e) = m.find(target) {
                        let to = match e.owner {
                            Some(o) => format!("{}::{}", m.elements[o].qualified_name, quote_name(new_name.trim())),
                            None => quote_name(new_name.trim()),
                        };
                        events.push(IdEvent::Rename { from: e.qualified_name.clone(), to });
                    }
                }
                EditOp::Delete { target } => {
                    let m = model_of(work);
                    if let Some(e) = m.find(target) {
                        let (owner, index) = ids::place_of(&m, e);
                        events.push(IdEvent::Delete { qn: e.qualified_name.clone(), owner, index });
                    }
                }
                _ => {}
            }
            if let EditOp::AddElement { owner, file, text } = parsed {
                raw_add(work, owner.filter(|o| !o.is_empty()), file, text, flag(op, "allow_duplicate"))?
            } else {
                step(work, parsed)?;
                json!({})
            }
        }
        "create_element" => create_element(work, op)?,
        "update_element" => update_element(work, op, &mut events)?,
        "delete_element" => delete_element(work, op, &mut events)?,
        "delete_relationship" => rel_ops::delete_relationship(work, op, &mut events)?,
        "create_relationship" => create_relationship(work, op)?,
        "add_interface" => connect::add_interface(work, op)?,
        "move_part" => super::move_part::move_part(work, op, &mut events)?,
        "set_source_id" => set_source_id(work, op, cache, track)?,
        "reorder_members" => super::order::reorder_members(work, op, &mut events)?,
        "fold_copy" => super::fold::fold_copy(work, op, &mut events)?,
        "mint_ids" => super::mint::mint_ids(work, op)?,
        other => return Err(format!("unknown edit {}. Use one of: {}, {}.", crate::agent::text::quote_clip(other), RAW_OPS.join(", "), INTENT_OPS.join(", ")).into()),
    };
    info["op"] = json!(name);
    // The element's id beside its name: the pointer the views, the trace and the history keep.
    if let Some(r) = info.get("ref").and_then(Value::as_str).filter(|_| info.get("id").is_none()) {
        if let Some(id) = model_of(work).find(r).and_then(|e| e.short_name.clone()) {
            info["id"] = json!(id);
        }
    }
    if !snapshot.is_empty() {
        let n = repair_names(work, &snapshot, None);
        if n > 0 {
            info["names_rewritten"] = json!(n);
        }
    }
    if let (Some(p), Some(m)) = (&placement, &before_model) {
        super::order::place_made(work, m, name, p, &mut info, &mut events)?;
    }
    if !before_files.is_empty() {
        refuse_new_controls(&before_files, work)?;
    }
    // A copy that carries the id of an element the model has (a subtree copied with its ids) gets a new id here, in this
    // write, with a note; it is never refused for it.
    if !before_files.is_empty() {
        let reminted = super::mint::remint_copies(&model_of(&before_files), work);
        if !reminted.is_empty() {
            info["reminted"] = json!(reminted);
        }
    }
    // No reference that was there before may name another element now (a new, renamed or removed name can hide or uncover one).
    // A batch of plain links and retypes runs these whole-model checks once, at its end (see `batch_checks`).
    if !before_files.is_empty() && !DEFER_CHECKS.with(Cell::get) {
        // move_part points the ends it rewires at the holder's new border ports on purpose; it checks itself that no name
        // that resolved before is left unresolved.
        if name != "move_part" {
            super::capture::refuse(&model_of(&before_files), &model_of(work), &events)?;
        }
        super::capture::refuse_namespace_faults(&model_of(&before_files), &model_of(work))?;
        super::ownership::refuse_new(&model_of(&before_files), &model_of(work))?;
        // A fold removes a copy and points its usages at the original: a copy it brings to light (a context copied around
        // the copied block, now repeating the original context) was there before, and is folded next.
        if name != "fold_copy" {
            super::duplicates::refuse_new(&model_of(&before_files), &model_of(work))?;
        }
        let similar = super::duplicates::looks_like(&model_of(&before_files), &model_of(work));
        if !similar.is_empty() {
            info["looks_like"] = json!(similar);
        }
    }
    if let Some(b) = &guard_model {
        let after = model_of(work);
        let (was, now) = element_names_of_edit(name, op, &info, &events, b, &after);
        let docs = info["changed"].as_array().map(|c| c.iter().any(|x| x == "doc")).unwrap_or(false) || name == "set_doc";
        // A bare assignment (`level = 3;` in an annotation) is named by the feature it sets, so a rename of that feature renames it.
        let sets = |e: &Element, t: Option<ElementId>| t.is_some() && e.kind == "feature" && e.refs_of(RefRole::Assign).any(|r| r.resolved() == t);
        // Moving a package into another file's package opens that package again there: a second declaration of an old name.
        let reopens = name == "update_element" && op.get("owner").is_some_and(|o| !o.is_null()) && was.is_some_and(|w| b.elements[w].kind == "package");
        // Only the packages on the new owner's path may be declared again; every other package still counts.
        let mut open_names: HashSet<String> = HashSet::new();
        if reopens {
            let mut o = now.and_then(|n| after.elements[n].owner);
            while let Some(x) = o {
                if after.elements[x].kind == "package" {
                    open_names.extend(after.elements[x].name.clone());
                }
                o = after.elements[x].owner;
            }
        }
        let opened = |e: &Element| reopens && e.kind == "package" && e.name.as_ref().is_some_and(|n| open_names.contains(n));
        let skip_b = |e: &Element| Some(e.id) == was || (docs && e.kind == "doc" && e.owner == was) || sets(e, was) || opened(e);
        let skip_a = |e: &Element| Some(e.id) == now || (docs && e.kind == "doc" && e.owner == now) || sets(e, now) || opened(e);
        // A rename or a move rewrites the references of other elements on purpose, so only the other edits compare text.
        let text = compares_text(name, op);
        // A new id rewrites the references written by the old one: those may differ by the id, and nothing else may.
        let new_id = op.get("short_name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(quote_name);
        let old_id = was.and_then(|w| b.elements[w].short_name.as_deref()).map(quote_name);
        let swap = match (&new_id, &old_id) {
            (Some(n), Some(o)) if n != o => Some((n.as_str(), o.as_str())),
            _ => None,
        };
        refuse_stray(&format!("{name} on '{}'", op.get("ref").or_else(|| op.get("target")).and_then(Value::as_str).unwrap_or("?")), b, &after, &skip_b, &skip_a, text, swap)?;
    }
    if !from_ids.is_empty() {
        info["resolved_source_ids"] = json!(from_ids);
    }
    for k in ["move", "unset"] {
        if name != "set_source_id" && op.get(k).map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("'{k}' applies only to set_source_id.").into());
        }
    }
    if let Some(id) = source_id {
        if info["created"].as_array().map(|c| c.len() > 1).unwrap_or(false) && name == "add_element" {
            return Err(format!("source_id names one element, but this add_element made {}. Fix: add one element per call, or drop source_id and use set_source_id for each.", info["created"].as_array().map_or(0, Vec::len)).into());
        }
        let made = info["ref"].as_str().or_else(|| info["connection"].as_str()).map(str::to_string);
        let Some(qn) = made else {
            return Err(format!("source_id needs {name} to make or find an element, but this call made none (a specialize or redefine only edits its source). Fix: drop source_id, or pass it to a create_element call.").into());
        };
        events.push(match (name == "set_source_id", flag(op, "unset"), flag(op, "move")) {
            (true, true, _) => IdEvent::Unset { id: id.clone(), qn },
            (true, _, true) => IdEvent::Reassign { id: id.clone(), qn },
            _ => IdEvent::Assign { id: id.clone(), qn },
        });
        info["source_id"] = json!(id);
        if !more_ids.is_empty() {
            let qn = match events.last() {
                Some(IdEvent::Assign { qn, .. }) => qn.clone(),
                _ => String::new(),
            };
            let mut all = vec![id.clone()];
            for m in &more_ids {
                events.push(IdEvent::Assign { id: m.clone(), qn: qn.clone() });
                all.push(m.clone());
            }
            info["source_ids"] = json!(all);
        }
    } else if !more_ids.is_empty() {
        return Err("source_ids needs a call that makes or finds an element. Fix: drop it, or pass it to a create_element call.".into());
    }
    // Ids given to members of `features[]`.
    for f in info["feature_ids"].as_array().cloned().unwrap_or_default() {
        if let (Some(id), Some(qn)) = (f["source_id"].as_str(), f["ref"].as_str()) {
            events.push(IdEvent::Assign { id: id.to_string(), qn: qn.to_string() });
        }
    }
    if let Some(before) = &units_before {
        add_unit_imports(work, before, &mut info)?;
    }
    // What the call did, so a batch reply can say it per edit.
    let status = if info["existed"] == true {
        Some("existed")
    } else if matches!(name, "create_element" | "create_relationship" | "add_interface") || (name == "add_element" && info["created"].as_array().map(|c| !c.is_empty()).unwrap_or(false)) {
        Some("created")
    } else if name == "set_source_id" {
        Some(if flag(op, "unset") { "unset" } else if flag(op, "move") { "moved" } else { "attached" })
    } else if name == "update_element" {
        Some("updated")
    } else {
        None
    };
    if let Some(st) = status {
        info["status"] = json!(st);
    }
    track.absorb(&events)?;
    Ok((info, events))
}

/// Edits that change one element and neither create nor delete another: after one, every element but the target is as it was.
const ONE_ELEMENT_OPS: &[&str] = &["update_element", "rename", "set_doc", "add_specialization"];

/// The target of a one-element edit in the model before (`.0`) and after (`.1`), when it can be found.
fn element_names_of_edit(name: &str, op: &Value, info: &Value, events: &[IdEvent], before: &Model, after: &Model) -> (Option<ElementId>, Option<ElementId>) {
    let from = if name == "update_element" { info["previous_ref"].as_str() } else { op.get("target").and_then(Value::as_str) };
    let was = from.and_then(|r| before.find(r)).map(|e| e.id);
    let to = match name {
        "update_element" => info["ref"].as_str().map(str::to_string),
        "rename" => events.iter().rev().find_map(|e| if let IdEvent::Rename { to, .. } = e { Some(to.clone()) } else { None }),
        _ => from.map(str::to_string),
    };
    let now = to.as_deref().and_then(|r| after.find(r)).map(|e| e.id);
    (was, now)
}

/// Edits that make a member, and so take `before`, `after` or `position`.
const PLACE_OPS: &[&str] = &["create_element", "create_relationship", "add_interface", "add_element"];

/// Ops whose text can carry a qualified name that a nearer element hides.
const NAME_OPS: &[&str] = &["create_element", "update_element", "create_relationship", "add_interface"];

/// The byte range of `new` that differs from `old`.
fn changed_region(old: &str, new: &str) -> (usize, usize) {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut p = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while p > 0 && !new.is_char_boundary(p) {
        p -= 1;
    }
    let max = a.len().min(b.len()) - p.min(a.len().min(b.len()));
    let mut s = a.iter().rev().zip(b.iter().rev()).take(max).take_while(|(x, y)| x == y).count();
    while s > 0 && !new.is_char_boundary(new.len() - s) {
        s -= 1;
    }
    (p, new.len() - s)
}

/// Name shadowing: a reference written as a qualified name whose first segment a nearer element hides does not
/// resolve. Rewrite it to the shortest name that does resolve to the element the full name means, or to `$::` plus
/// the full name when no shorter one does. Only references that are unresolved and whose full name names an
/// element are touched, inside the text the call changed (`before`), or everywhere when `before` is empty.
/// `only` limits the targets. Returns how many references were rewritten.
fn repair_names(work: &mut [FileIn], before: &[FileIn], only: Option<&dyn Fn(&Element) -> bool>) -> usize {
    let mut total = 0;
    for _round in 0..3 {
        let model = model_of(work);
        let mut queries: Vec<(ElementId, RefRole, String)> = Vec::new();
        let mut plans: Vec<(usize, Span, ElementId, std::ops::Range<usize>)> = Vec::new();
        // The changed region of each file, found once: comparing the texts per element made the cost
        // elements x file size, which is seconds on a large workspace.
        let regions: Vec<Option<(usize, usize)>> = model
            .files
            .iter()
            .map(|mf| match before.iter().find(|f| f.path == mf.path) {
                Some(b) => work.iter().find(|f| f.path == mf.path).map(|f| changed_region(&b.text, &f.text)),
                None => Some((0, usize::MAX)),
            })
            .collect();
        for e in &model.elements {
            let Some((lo, hi)) = regions[e.file] else { continue };
            for r in &e.refs {
                if matches!(r.role, RefRole::Import | RefRole::Expr | RefRole::Call | RefRole::Assign | RefRole::Meta) || crate::resolve::is_global(r) || r.span.start < lo || r.span.end > hi {
                    continue;
                }
                if r.segments.iter().all(|s| s.resolved.is_some()) {
                    continue;
                }
                let Some(t) = model.find(&r.text) else { continue };
                if t.id == e.id || t.kind == "alias" || only.map(|f| !f(t)).unwrap_or(false) {
                    continue;
                }
                let segs = crate::resolve::split_name(&r.text);
                let from = queries.len();
                for i in (0..segs.len()).rev() {
                    queries.push((e.id, r.role, segs[i..].iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("::")));
                }
                queries.push((e.id, r.role, format!("$::{}", segs.iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("::"))));
                plans.push((e.file, r.span, t.id, from..queries.len()));
            }
        }
        if plans.is_empty() {
            break;
        }
        let got = crate::resolve::resolve_texts(&model, &queries);
        let mut by_file: HashMap<usize, Vec<(Span, String)>> = HashMap::new();
        for (file, span, target, range) in plans {
            if let Some(k) = range.clone().find(|k| got[*k] == Some(target)) {
                by_file.entry(file).or_default().push((span, queries[k].2.clone()));
            }
        }
        if by_file.is_empty() {
            break;
        }
        for (file, mut edits) in by_file {
            let path = model.files[file].path.clone();
            let Some(f) = work.iter_mut().find(|f| f.path == path) else { continue };
            edits.sort_by_key(|(sp, _)| std::cmp::Reverse(sp.start));
            for (sp, t) in edits {
                if sp.end <= f.text.len() && f.text.is_char_boundary(sp.start) && f.text.is_char_boundary(sp.end) {
                    f.text.replace_range(sp.start..sp.end, &t);
                    total += 1;
                }
            }
        }
    }
    total
}

/// Attach a source id to an element that exists. Writes no model text.
///
/// An id that names another element is refused, unless `move: true`: the id then leaves that element, and the reply
/// names it (`source_id_moves`). `unset: true` takes the id off the element, which frees it. Neither is silent.
fn set_source_id(work: &mut Vec<FileIn>, a: &Value, cache: &mut Option<Arc<Model>>, track: &Track) -> Res<Value> {
    let r = req(a, "ref")?;
    let Some(id) = a.get("source_id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
        return Err(Fail::from("missing argument 'source_id': the id from the source project, for example a Cameo xmi id."));
    };
    let (mv, unset) = (flag(a, "move"), flag(a, "unset"));
    if mv && unset {
        return Err(Fail::from("move and unset contradict each other. Fix: pass one of them."));
    }
    let model = match cache.take() {
        Some(m) => m,
        None => model_of(work),
    };
    let found = locate(&model, r).map(|e| (e.qualified_name.clone(), e.keyword()));
    *cache = Some(model);
    let (qn, kw) = found?;
    let holder = track.map.get(id);
    let mut info = json!({ "ref": qn, "kind": kw });
    match holder {
        Some(h) if h != &qn && !mv && !unset => {
            return Err(Fail::with(
                format!("source id '{id}' already names '{h}'. Fix: pass move: true to take it from '{h}' and give it to '{qn}' (the reply names '{h}' as the element that lost it), or use another source_id."),
                json!({ "source_id": id, "held_by": h }),
            ))
        }
        Some(h) if h != &qn && unset => {
            return Err(Fail::with(format!("source id '{id}' names '{h}', not '{qn}', so there is nothing to unset on '{qn}'. Fix: pass ref '{h}'."), json!({ "source_id": id, "held_by": h })));
        }
        None if unset => return Err(Fail::from(format!("source id '{id}' is not recorded for any element, so there is nothing to unset."))),
        Some(h) if h != &qn && mv => info["moved_from"] = json!(h),
        Some(_) if unset => info["unset_from"] = json!(qn),
        _ => {}
    }
    Ok(info)
}

/// `file` for an add into an owner that lives in another file: the owner's own packages are written again there.
fn package_chain(model: &Model, owner: &Element) -> Res<Vec<String>> {
    let mut chain = Vec::new();
    let mut cur = Some(owner.id);
    while let Some(c) = cur {
        let e = &model.elements[c];
        if e.kind != "package" || e.is_def {
            return Err(Fail::from(format!(
                "'{}' lives in '{}', and only a package can be written again in another file; '{}' is a {}. Fix: drop 'file', or add to a package.",
                owner.qualified_name,
                model.files[owner.file].path,
                e.qualified_name,
                e.keyword()
            )));
        }
        chain.push(quote_name(e.name.as_deref().unwrap_or("")));
        cur = e.owner;
    }
    chain.reverse();
    Ok(chain)
}

/// `text` inside nested `package` blocks, one for each name of `chain`.
fn wrap_in_packages(chain: &[String], text: &str) -> String {
    let mut out = text.trim().to_string();
    for seg in chain.iter().rev() {
        let inner = out.lines().map(|l| if l.trim().is_empty() { String::new() } else { format!("    {l}") }).collect::<Vec<_>>().join("\n");
        out = format!("package {seg} {{\n{inner}\n}}");
    }
    out
}

/// Elements the text added: new names whose owner existed before (or that have no owner).
fn new_members(before: &Model, after: &Model) -> Vec<ElementId> {
    let old: HashSet<&str> = before.elements.iter().map(|e| e.qualified_name.as_str()).collect();
    after
        .elements
        .iter()
        .filter(|e| !old.contains(e.qualified_name.as_str()) && e.owner.map(|o| old.contains(after.elements[o].qualified_name.as_str())).unwrap_or(true))
        .map(|e| e.id)
        .collect()
}

/// `edit_model add_element`: add text, refuse a repeated name or relationship, and say what was made.
fn raw_add(work: &mut Vec<FileIn>, owner: Option<String>, file: Option<String>, text: String, allow_duplicate: bool) -> Res<Value> {
    let before = model_of(work);
    let (put_owner, put_file, body) = match (&owner, &file) {
        (Some(o), Some(f)) => {
            let oe = locate(&before, o)?;
            if before.files[oe.file].path == *f {
                (Some(o.clone()), None, text)
            } else {
                // The owner's packages are opened again in the other file; a package may be split over files.
                let chain = package_chain(&before, oe)?;
                (None, Some(f.clone()), wrap_in_packages(&chain, &text))
            }
        }
        _ => (owner.clone(), file.clone(), text),
    };
    step(work, EditOp::AddElement { owner: put_owner, file: put_file, text: body.clone() })?;
    let after = model_of(work);
    refuse_unrecognised(&before, &after, &body)?;
    refuse_duplicates(&before, &after, owner.as_deref())?;
    if !allow_duplicate {
        refuse_duplicate_statements(&before, &after)?;
    }
    let made = new_members(&before, &after);
    let created: Vec<Value> = made.iter().map(|i| super::brief(&after, &after.elements[*i])).collect();
    let mut info = json!({ "created": created });
    if let Some(first) = made.first() {
        info["ref"] = json!(after.elements[*first].qualified_name);
    }
    Ok(info)
}

/// Qualified names a `create_element` edit will make, as written (`Owner::name`), for ordering a batch.
fn creates(e: &Value) -> Vec<String> {
    fn walk(v: &Value, prefix: Option<&str>, out: &mut Vec<String>) {
        let Some(n) = v.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else { return };
        let qn = match prefix {
            Some(p) => format!("{p}::{}", n.trim_matches('\'')),
            None => n.trim_matches('\'').to_string(),
        };
        out.push(qn.clone());
        for f in v.get("features").and_then(Value::as_array).into_iter().flatten() {
            walk(f, Some(&qn), out);
        }
    }
    let mut out = Vec::new();
    if e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) == Some("create_element") {
        let owner = text_arg(e, "owner").map(|o| o.split("::").map(|s| s.trim().trim_matches('\'')).collect::<Vec<_>>().join("::"));
        walk(e, owner.as_deref(), &mut out);
    }
    out
}

/// The name a failed edit could not find.
fn missing_name(f: &Fail) -> Option<String> {
    f.data.get("ref").and_then(Value::as_str).map(str::to_string).or_else(|| {
        let rest = f.message.split("no element named \"").nth(1)?;
        let (mut out, mut esc) = (String::new(), false);
        for c in rest.chars() {
            match (esc, c) {
                (true, c) => {
                    out.push(c);
                    esc = false;
                }
                (false, '\\') => esc = true,
                (false, '"') => return Some(out),
                (false, c) => out.push(c),
            }
        }
        None
    })
}

/// Result of a batch.
#[derive(Debug)]
pub struct Batch {
    pub files: Vec<FileIn>,
    /// One `info` per edit, in the order the caller wrote them.
    pub results: Vec<Value>,
    /// Edits that ran earlier than written: `{edit, before}` with 1-based positions.
    pub moved: Vec<Value>,
    /// Name events of every edit, in the order they ran.
    pub events: Vec<IdEvent>,
    /// Every id that left one element for another, or was removed on request, as `{id, lost_by, now_on, how}`.
    pub source_id_moves: Vec<Value>,
    /// Copies that carried another element's minted id and were given a new one in this batch (`mint::remint_copies`).
    pub reminted: Vec<Value>,
}

/// Run edits in order. An edit that names an element a later `create_element` of the batch makes
/// is moved behind that creator; when that cannot work, the failure names the edit to move.
/// On failure returns the 0-based index of the edit as written, and the reason.
pub fn apply_batch(files: &[FileIn], edits: &[Value]) -> Result<Batch, (usize, Fail)> {
    apply_batch_ids(files, edits, &BTreeMap::new())
}

/// Like `apply_batch`, with the source ids in force. A ref `source_id:<id>` is looked up when its edit runs, so it
/// follows renames and moves of earlier edits. A positional ref (`Owner::$3`) that an earlier edit of the batch renumbers
/// is refused, because it would name another statement than the caller saw.
pub fn apply_batch_ids(files: &[FileIn], edits: &[Value], ids: &BTreeMap<String, String>) -> Result<Batch, (usize, Fail)> {
    // The count is checked before placement, which reads every edit.
    if edits.is_empty() || edits.len() > MAX_EDITS {
        return Err((0, Fail::from(too_many(edits.len()))));
    }
    let (planned, placed) = place_edits(files, edits);
    let mut b = apply_batch_inner(files, &planned, ids)?;
    for (r, p) in b.results.iter_mut().zip(placed) {
        if let (Some(p), Some(_)) = (p, r.as_object()) {
            r["placed"] = p;
        }
    }
    observe(&edits.iter().collect::<Vec<_>>(), files, &b.files);
    // Every reply names the element's id, however the batch ran (one by one, or in waves that skip the per-edit tail).
    if b.results.iter().any(|r| r.get("ref").is_some() && r.get("id").is_none()) {
        let m = model_of(&b.files);
        for r in b.results.iter_mut().filter(|r| r.get("id").is_none()) {
            if let Some(id) = r.get("ref").and_then(Value::as_str).and_then(|q| m.find(q)).and_then(|e| e.short_name.clone()) {
                r["id"] = json!(id);
            }
        }
    }
    Ok(b)
}

fn apply_batch_inner(files: &[FileIn], edits: &[Value], ids: &BTreeMap<String, String>) -> Result<Batch, (usize, Fail)> {
    // Bounds first: nothing below parses, reorders or resolves until the request is known to be small.
    if edits.is_empty() || edits.len() > MAX_EDITS {
        return Err((0, Fail::from(format!("{}", too_many(edits.len())))));
    }
    let mut total = 0usize;
    for (i, e) in edits.iter().enumerate() {
        check_edit_size(e).map_err(|f| (i, f))?;
        total += e.to_string().len();
        if total > MAX_BATCH_BYTES {
            return Err((i, Fail::from(format!("refused: the batch is larger than {} MiB. Fix: split it.", MAX_BATCH_BYTES >> 20))));
        }
    }
    let _guard = DeferGuard;
    let _models = ModelCache;
    let start_unresolved = std::cell::OnceCell::new();
    let pre: Arc<HashMap<String, String>> = Arc::new(if idrefs::any_positional(edits) { idrefs::positional_refs(&model_of(files), edits) } else { HashMap::new() });
    // Runs of independent creates share one analysis. The wave path answers only when every edit went through and every
    // read-back held; in any other case the edits run one by one below, which is the definition of the result.
    if wave::worth_trying(edits) {
        if let Some(b) = wave::run(files, edits, ids, &pre) {
            return Ok(b);
        }
    }
    // Plain retypes (a stitch making an item one def end to end): planned on one model, written and read back at once.
    if ids.is_empty() && retype_wave::worth_trying(edits) {
        if let Some(b) = retype_wave::run(files, edits) {
            return Ok(b);
        }
    }
    let mut runs = 0usize;
    let mut order: Vec<usize> = (0..edits.len()).collect();
    let mut moved: Vec<(usize, usize)> = Vec::new();
    // Links and retypes only (no new name, no rename or move): the reference, namespace, ownership and copy checks run
    // once on the whole batch instead of once per edit (each is a pass over the whole model: on a work-scale model they
    // were most of an edit's second). When they refuse, the batch runs again with the checks after every edit, which
    // names the edit and gives the same reply as before.
    let mut defer = edits.len() >= 2 && deferrable_checks() && edits.iter().all(deferrable);
    let _undefer = UndeferGuard;
    loop {
        DEFER_CHECKS.with(|c| c.set(defer));
        // One working copy for the whole run: an edit changes it in place, and a failed run drops it.
        let mut work = files.to_vec();
        DEFERRED_MOVES.with(|c| c.set(Some(0)));
        let mut cache: Option<Arc<Model>> = None;
        let mut track = Track::for_batch(ids.clone(), pre.clone());
        let mut results: Vec<Option<Value>> = vec![None; edits.len()];
        let mut events: Vec<IdEvent> = Vec::new();
        let mut failed: Option<(usize, usize, Fail)> = None;
        for (pos, &i) in order.iter().enumerate() {
            runs += 1;
            if runs > MAX_BATCH_RUNS {
                return Err((i, Fail::from(format!("refused: the batch needs more than {MAX_BATCH_RUNS} edit runs because of reordering. Fix: write the edits in dependency order, or split the batch."))));
            }
            match apply_in_place(&mut work, &edits[i], &mut cache, &mut track) {
                Ok((info, ev)) => {
                    results[i] = Some(info);
                    events.extend(ev);
                }
                Err(f) => {
                    failed = Some((pos, i, f));
                    break;
                }
            }
        }
        if failed.is_none() && defer {
            DEFER_CHECKS.with(|c| c.set(false));
            if batch_checks(files, &work, &events).is_err() {
                defer = false;
                continue;
            }
        }
        let Some((pos, i, mut f)) = failed else {
            // Moves skipped their own check: names are checked once, with every move applied.
            if DEFERRED_MOVES.with(Cell::get).unwrap_or(0) > 0 {
                let before = *start_unresolved.get_or_init(|| unresolved_names(files));
                let now = unresolved_names(&work);
                if now > before {
                    let last = (0..edits.len()).rev().find(|j| edits[*j].get("owner").is_some() && edits[*j].get("op").and_then(Value::as_str) == Some("update_element")).unwrap_or(0);
                    return Err((last, Fail::from(format!("refused: after all the edits of the batch, {} name(s) are unresolved that resolved before, because other text refers to a moved element by its old name. Fix: update those references in the same batch, or use the qualified name. Nothing was written.", now - before))));
                }
            }
            let moved = moved.iter().map(|(m, b)| json!({ "edit": m + 1, "before": b + 1 })).collect();
            let source_id_moves = track.notes.iter().map(|n| n.to_json()).collect();
            let results: Vec<Value> = results.into_iter().map(|r| r.unwrap_or(Value::Null)).collect();
            let reminted = results.iter().flat_map(|r| r["reminted"].as_array().cloned().unwrap_or_default()).collect();
            return Ok(Batch { files: work, results, moved, events, source_id_moves, reminted });
        };
        let creator = missing_name(&f).and_then(|need| {
            let norm_need = need.split("::").map(|s| s.trim().trim_matches('\'')).collect::<Vec<_>>().join("::");
            // A short name (`redefines: "mass"`) is a member some supertype declares: any later creator of that name may supply it.
            let short = (!norm_need.contains("::")).then(|| norm_need.clone());
            let makes = |j: &usize| creates(&edits[*j]).iter().any(|c| *c == norm_need || short.as_deref().map(|s| c.rsplit("::").next() == Some(s)).unwrap_or(false));
            order[pos + 1..].iter().position(makes).map(|k| (pos + 1 + k, norm_need))
        });
        let Some((cpos, need)) = creator else { return Err((i, with_other_failures(files, edits, &order, pos, ids, &pre, f))) };
        let c = order[cpos];
        if moved.iter().any(|(m, _)| *m == c) || moved.len() >= 100 {
            f.message = format!("{} Edit #{} needs '{need}', created by edit #{}: move #{} before #{}.", f.message, i + 1, c + 1, c + 1, i + 1);
            return Err((i, with_other_failures(files, edits, &order, pos, ids, &pre, f)));
        }
        order.remove(cpos);
        order.insert(pos, c);
        moved.push((c, i));
    }
}

/// The edits as they run: a new element goes to the owner its caller names. Nothing is moved, so no edit has a note.
fn place_edits(_files: &[FileIn], edits: &[Value]) -> (Vec<Value>, Vec<Option<Value>>) {
    (edits.to_vec(), vec![None; edits.len()])
}

/// The edit as a caller wrote it, in full and in one line: op, kind, and the names it carries. Used to name the first
/// failing edit of a batch, so a long qualified name is never cut.
pub fn edit_label(e: &Value) -> String {
    let get = |k: &str| e.get(k).and_then(Value::as_str).filter(|v| !v.is_empty());
    let mut parts: Vec<String> = Vec::new();
    let op = get("op").or_else(|| get("tool")).unwrap_or("?");
    parts.push(if op.len() <= 40 && op.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { op.to_string() } else { crate::agent::text::quote_claim(op) });
    for k in ["kind", "name", "ref", "owner", "source", "target", "typed_by", "target_name", "source_id"] {
        if let Some(v) = get(k) {
            parts.push(if k == "kind" { crate::agent::text::model_name(v) } else { format!("{k}: {}", crate::agent::text::quote_claim(v)) });
        }
    }
    parts.join(" ").replace('\n', " ")
}

/// Most failing edits a failed batch lists.
const MAX_FAILURES: usize = 25;

/// What two failure messages share when the same mistake hit different names: the text without quoted names and digits,
/// and without a "Did you mean" list.
fn cause_key(msg: &str) -> String {
    let msg = msg.split(" Did you mean").next().unwrap_or(msg);
    let mut out = String::new();
    let mut chars = msg.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                }
                out.push_str("'_'");
            }
            '"' => {
                let mut esc = false;
                for d in chars.by_ref() {
                    if esc {
                        esc = false;
                    } else if d == '\\' {
                        esc = true;
                    } else if d == '"' {
                        break;
                    }
                }
                out.push_str("\"_\"");
            }
            d if d.is_ascii_digit() => {}
            d => out.push(d),
        }
    }
    out
}

/// A batch stops at its first failure. Run the edits behind it, without the failed one, to list the other edits that
/// fail too, so a caller fixes them in one pass. A later failure may be a consequence of an earlier one. The list
/// is in `detail.failures[]` (`{edit, error}`, 1-based) and the message names each.
fn with_other_failures(files: &[FileIn], edits: &[Value], order: &[usize], pos: usize, ids: &BTreeMap<String, String>, pre: &Arc<HashMap<String, String>>, mut first: Fail) -> Fail {
    let mut work = files.to_vec();
    let mut cache: Option<Arc<Model>> = None;
    let mut track = Track::for_batch(ids.clone(), pre.clone());
    for &i in &order[..pos.min(order.len())] {
        if apply_in_place(&mut work, &edits[i], &mut cache, &mut track).is_err() {
            return first;
        }
    }
    let mut failures: Vec<Value> = vec![json!({ "edit": order[pos] + 1, "error": first.message })];
    for &j in &order[pos + 1..] {
        if failures.len() >= MAX_FAILURES {
            break;
        }
        let snap = work.clone();
        if let Err(f) = apply_in_place(&mut work, &edits[j], &mut cache, &mut track) {
            work = snap;
            cache = None;
            failures.push(json!({ "edit": j + 1, "error": f.message }));
        }
    }
    if failures.len() > 1 {
        // Group by cause: two edits fail for one cause when their messages differ only in the names they carry.
        let mut causes: Vec<(String, Vec<usize>, String)> = Vec::new();
        for f in &failures {
            let (edit_no, msg) = (f["edit"].as_u64().unwrap_or(0) as usize, f["error"].as_str().unwrap_or("").to_string());
            let key = cause_key(&msg);
            match causes.iter_mut().find(|c| c.0 == key) {
                Some(c) => c.1.push(edit_no),
                None => causes.push((key, vec![edit_no], msg)),
            }
        }
        let list = |v: &[usize]| v.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ");
        let cut = if failures.len() >= MAX_FAILURES { "list cut" } else { "complete list" };
        let others = failures.len() - 1;
        let tail = if causes.len() == 1 {
            format!("{others} other edit(s) fail for the same cause when this one is left out ({cut}): {}.", list(&causes[0].1[1..]))
        } else {
            let rows: Vec<String> = causes
                .iter()
                .enumerate()
                .skip(1)
                .map(|(i, c)| format!("({}) {}: {}", i + 1, list(&c.1), crate::agent::text::quote_clip(&c.2)))
                .collect();
            format!(
                "{others} other edit(s) fail too when this one is left out, for {} different causes ({cut}). Cause (1) is this one, also {}. {}.",
                causes.len(),
                if causes[0].1.len() > 1 { list(&causes[0].1[1..]) } else { "no other edit".into() },
                rows.join("; ")
            )
        };
        first.message = format!("{} {tail} A later failure may only be a consequence of an earlier one.", first.message);
        let grouped: Vec<Value> = causes.iter().map(|c| json!({ "edits": c.1, "error": c.2 })).collect();
        let mut data = if first.data.is_object() { first.data.clone() } else { json!({}) };
        data["causes"] = json!(grouped);
        first.data = data;
    }
    let mut data = if first.data.is_object() { first.data.clone() } else { json!({}) };
    data["failures"] = json!(failures);
    first.data = data;
    first
}

// ---- create_element -------------------------------------------------------

struct Spec {
    /// What an annotation is about, when it says so (`@Type about a, b;`).
    about: Vec<String>,
    kw: String,
    is_def: bool,
    /// Empty for an annotation (`@Type;`).
    name: String,
    annotation: bool,
    short: Option<String>,
    typed: Option<String>,
    specializes: Vec<String>,
    redefines: Vec<String>,
    /// `::>` targets.
    references: Vec<String>,
    /// Metadata prefixes written as `#Tag` before everything else.
    meta: Vec<String>,
    mult: Option<String>,
    value: Option<String>,
    dir: Option<String>,
    doc: Option<String>,
    /// Modifier words (`private`, `abstract`, `ref`, `ordered`, `parallel` ...), in written order.
    mods: Vec<String>,
    /// `feature = value` lines of an annotation body.
    slots: Vec<(String, String)>,
    /// Source ids given to slots: (slot name, id).
    slot_ids: Vec<(String, String)>,
    /// Source id of a member of `features[]`.
    source_id: Option<String>,
    features: Vec<Spec>,
    /// The two ends of an interface usage (`interface lnk : L connect a.p to b.q;`), as written from the owner.
    ends: Option<(String, String)>,
    /// The expression body of a `constraint` or `calc` usage (`constraint lim { v < 5 }`).
    body: Option<String>,
}

/// Text to write for a type: the qualified name of a model definition, or a library name as given.
fn type_text(model: &Model, given: &str) -> Res<String> {
    let given = given.trim();
    let (conj, bare) = match given.strip_prefix('~') {
        Some(rest) => ("~", rest.trim()),
        None => ("", given),
    };
    let first = bare.split("::").next().unwrap_or(bare).trim();
    let found = match model.find(bare) {
        Some(e) => Some(e),
        None if LIBRARY_TYPES.contains(&first) => None,
        // A short name that names one definition (`Pump` for `Demo::Pump`) is that definition.
        None => Some(locate(model, bare)?),
    };
    if let Some(e) = found {
        if !e.is_def {
            let own = e.refs_of(RefRole::Type).find_map(|r| r.resolved()).map(|t| &model.elements[t]).filter(|t| t.is_def);
            let way = match own {
                Some(d) => format!("This usage is typed by the definition '{}'. Fix: pass that definition.", d.qualified_name),
                None => format!("This usage has no definition. Fix: create one with create_element {{kind: '{} def'}} and pass it.", e.kind),
            };
            return Err(Fail::from(format!(
                "'{bare}' is a usage ({}), and SysML v2 does not allow typing by a usage: a type must be a definition. So this is refused on purpose, not a missing option. {way}",
                e.keyword()
            )));
        }
        return Ok(format!("{conj}{}", e.qualified_name));
    }
    Ok(edit::qualified_text("type", given, false)?)
}

/// The type of what a flow or a message carries. `~` marks the conjugate of a port definition as the type of a port; a
/// payload is not a port, and the official notation has no `~` after `of`.
fn payload_text(model: &Model, given: &str) -> Res<String> {
    if given.trim_start().starts_with('~') {
        return Err(Fail::from(format!("of: {} starts with '~', which marks a conjugated port type; what a flow carries is an item, not a port. Fix: pass the definition without '~'.", crate::agent::text::quote_clip(given.trim()))));
    }
    type_text(model, given)
}

/// `metadata: ["SysMLv1::Actor"]`: the tags written as `#SysMLv1::Actor` before a statement.
/// A leading `#` is accepted. A tag that names a model element is written with its qualified name.
fn meta_list(model: &Model, a: &Value) -> Res<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for m in list_arg(a, "metadata")? {
        let bare = m.trim_start_matches('#').trim();
        let Some(canon) = canonical_name(bare) else {
            return Err(format!("metadata {} is not a qualified name. Fix: pass the metadata definition, for example SysMLv1::Actor.", crate::agent::text::quote_clip(&m)).into());
        };
        let tag = model.find(&canon).map(|e| e.qualified_name.clone()).unwrap_or(canon);
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    if out.len() > MAX_LIST {
        return Err(format!("metadata lists {} names; at most {MAX_LIST}.", out.len()).into());
    }
    Ok(out)
}

/// Text for a `specializes` entry: a definition for a definition, a usage for a usage.
/// A name of the standard library (`ScalarValues::Real`) that a model without that library cannot find.
fn is_library_name(given: &str) -> bool {
    LIBRARY_TYPES.contains(&given.trim().split("::").next().unwrap_or(given).trim())
}

fn general_text(model: &Model, given: &str, want_def: bool) -> Res<String> {
    match locate_ref(model, given) {
        Ok(l) if l.el.is_def == want_def => Ok(l.text),
        Ok(l) if want_def => Err(format!("specializes: '{}' is a usage ({}); a definition specializes definitions. Fix: pass a definition.", l.el.qualified_name, l.el.keyword()).into()),
        Ok(l) => Err(format!("specializes: '{}' is a definition ({}); a usage specializes usages. Fix: pass a usage, or use typed_by for a definition.", l.el.qualified_name, l.el.keyword()).into()),
        Err(_) if want_def && is_library_name(given) => Ok(edit::qualified_text("specializes", given, false)?),
        Err(e) => Err(e),
    }
}

/// Text for a `redefines` entry: a usage that a supertype of `owner` declares.
fn redefined_text(model: &Model, owner: Option<&Element>, given: &str) -> Res<String> {
    let Some(owner) = owner else {
        return Err("redefines needs an owner: a feature redefines one that a supertype of its owner declares. Fix: pass owner, a definition or usage that specializes the type that declares it.".into());
    };
    let up = supertypes(model, owner.id);
    if !given.contains("::") {
        let hit = up.iter().filter_map(|t| model.children(*t).find(|c| c.name.as_deref() == Some(given) && !c.is_def)).next();
        if let Some(h) = hit {
            return Ok(h.qualified_name.clone());
        }
    }
    let l = locate_ref(model, given).map_err(|f| {
        Fail::with(format!("redefines: {} Name the feature by qualified name, or give a name that a supertype of '{}' declares.", f.message, owner.qualified_name), f.data)
    })?;
    if l.el.is_def {
        return Err(format!("redefines: '{}' is a definition ({}); only a usage can be redefined.", l.el.qualified_name, l.el.keyword()).into());
    }
    let holder = l.el.owner.map(|o| &model.elements[o]);
    if holder.map(|h| !up.contains(&h.id)).unwrap_or(true) {
        let h = holder.map(|h| h.qualified_name.clone()).unwrap_or_else(|| "<top level>".into());
        return Err(Fail::with(
            format!(
                "redefines: '{}' belongs to '{h}', which '{}' does not specialize or use as its type, so the redefinition would not resolve. Fix: create_relationship {{kind: 'specialize', source: '{}', target: '{h}'}} first.",
                l.el.qualified_name, owner.qualified_name, owner.qualified_name
            ),
            json!({ "ref": l.el.qualified_name }),
        ));
    }
    Ok(l.text)
}

impl Spec {
    /// Gives this element and its features an id (a short name) where the kind carries one and none is given.
    fn mint_ids(&mut self, owner_qn: &str, taken: &mut std::collections::HashSet<String>) {
        let qn = if owner_qn.is_empty() { quote_name(&self.name) } else { format!("{owner_qn}::{}", quote_name(&self.name)) };
        if self.short.is_none() && !self.annotation && !self.name.is_empty() && crate::agent::mint::is_minted_kind(&self.kw) {
            self.short = Some(crate::agent::mint::mint(&self.kw, &qn, taken));
        }
        for f in &mut self.features {
            f.mint_ids(&qn, taken);
        }
    }

    fn keyword(&self) -> String {
        if self.is_def {
            format!("{} def", self.kw)
        } else {
            self.kw.clone()
        }
    }

    /// The keyword the parser gives the element when it reads it back.
    fn read_back(&self) -> String {
        if matches!(self.kw.as_str(), "decide" | "merge" | "fork" | "join") {
            format!("{} node", self.kw)
        } else {
            self.keyword()
        }
    }

    fn parse(model: &Model, owner: Option<&Element>, nested: bool, a: &Value) -> Res<Spec> {
        let kind = norm_kind(req_as(a, "kind", "the element kind, for example 'part', 'part def' or 'requirement'")?);
        let (base, is_def) = match kind.strip_suffix(" def") {
            Some(b) => (b.trim().to_string(), true),
            None => (kind.clone(), false),
        };
        if nested && STATEMENT_KINDS.contains(&base.as_str()) && base != "comment" {
            return Err(format!("kind '{base}' is not supported inside features[]. Fix: create the {base} with its own create_element {{owner}} after the owner exists.").into());
        }
        let enum_value = base == "enum value" && !is_def;
        if !enum_value && !CREATABLE.contains(&base.as_str()) {
            return Err(format!("unknown kind '{kind}'. Use one of: {}, or a statement: {} (add ' def' for a definition).", CREATABLE.join(", "), STATEMENT_KINDS.join(", ")).into());
        }
        if base == "package" && is_def {
            return Err("a package has no definition form; use kind 'package'.".into());
        }
        if is_def && (NAMELESS_OK.contains(&base.as_str()) || matches!(base.as_str(), "perform action" | "include use case" | "feature" | "assume constraint" | "require constraint" | "exhibit state")) {
            return Err(format!("'{base}' has no definition form; use kind '{base}'.").into());
        }
        // An interface usage and an allocation usage take two ends (`connect a to b`, `allocate a to b`).
        let interface_usage = matches!(base.as_str(), "interface" | "allocation") && !is_def;
        let expr_usage = matches!(base.as_str(), "constraint" | "calc" | "assume constraint" | "require constraint");
        if matches!(base.as_str(), "assume constraint" | "require constraint") {
            // SysML 8.2.2.21.2, RequirementBody: an assumed or required constraint is a member of a requirement.
            let home = owner.map(|o| o.kind.as_str()).unwrap_or("");
            if nested || !["requirement", "concern", "viewpoint", "objective"].iter().any(|k| home.starts_with(k)) {
                return Err(format!("an {base} is a member of a requirement (or a concern, a viewpoint or an objective), not of {}. Fix: pass the requirement as owner.", if home.is_empty() { "the top of a file" } else { home }).into());
            }
        }
        if !nested {
            for (k, users) in [("source", "succession, bind and an interface usage"), ("target", "succession, alias, bind, import and an interface usage"), ("guard", "succession"), ("body", "comment, doc, textual representation, and the expression of a constraint or calc usage"), ("about", "comment"), ("language", "textual representation"), ("visibility", "import"), ("all", "import"), ("recursive", "import")] {
                let allowed_here = (interface_usage && matches!(k, "source" | "target")) || (expr_usage && k == "body") || (k == "about" && base == "metadata" && !is_def && text_arg(a, "name").is_none());
                if !allowed_here && a.get(k).map(|v| !v.is_null()).unwrap_or(false) {
                    return Err(format!("'{k}' does not apply to {kind}. Fix: drop '{k}'; it is used by {users}.").into());
                }
            }
        }
        let annotation = base == "metadata" && !is_def && text_arg(a, "name").is_none();
        let unnamed = flag(a, "unnamed");
        if unnamed && (is_def || matches!(base.as_str(), "package" | "parameter" | "enum value")) {
            return Err(format!("unnamed: true does not apply to {kind}: a {} needs a name. Fix: drop unnamed and pass name.", if is_def { "definition" } else { base.as_str() }).into());
        }
        if unnamed && text_arg(a, "name").is_some() {
            return Err("unnamed: true and name contradict each other. Fix: pass one of them.".into());
        }
        let name = if annotation
            || (NAMELESS_OK.contains(&base.as_str()) && text_arg(a, "name").is_none())
            || (NAMELESS_USAGE_OK.contains(&base.as_str()) && !is_def && text_arg(a, "name").is_none())
            || unnamed
        {
            String::new()
        } else {
            req_as(a, "name", "the element name (only a metadata usage, an actor, an include use case, a subject, a node and an entry, do or exit action may omit it; pass unnamed: true for any other usage)")?.to_string()
        };
        if nested && name.is_empty() && !annotation && !NAMELESS_OK.contains(&base.as_str()) {
            return Err(format!("a {base} without a name is not supported inside features[]. Fix: create it with its own create_element {{owner}}.").into());
        }
        clean_name("name", &name)?;
        if let Some(d) = text_arg(a, "doc") {
            clean_doc("doc", d)?;
        }
        let typed_given = list_arg(a, "typed_by")?;
        let typed = match typed_given.first() {
            Some(t) if is_def => {
                return Err(Fail::from(format!(
                    "a definition is not typed ('{t}'). To make '{name}' a kind of '{t}', pass specializes: '{t}', or create it and then use create_relationship {{kind: 'specialize', source, target}}."
                )))
            }
            Some(_) => Some(typed_given.iter().map(|t| type_text(model, t)).collect::<Res<Vec<_>>>()?.join(", ")),
            None => None,
        };
        let meta = meta_list(model, a)?;
        let dir = text_arg(a, "direction").map(str::to_lowercase);
        if let Some(d) = &dir {
            if !matches!(d.as_str(), "in" | "out" | "inout") {
                return Err(format!("direction must be in, out or inout, not '{d}'").into());
            }
        }
        if base == "parameter" {
            if is_def {
                return Err("a parameter has no definition form; use kind 'parameter'.".into());
            }
            if dir.is_none() {
                return Err("a parameter needs a direction. Fix: pass direction 'in', 'out' or 'inout'; create_element {kind: 'parameter', name: 'x', typed_by: 'T', direction: 'in'} writes `in x : T;`.".into());
            }
            if !list_arg(a, "modifiers")?.is_empty() {
                return Err("a parameter takes no modifiers: it is written as its direction and its name. Fix: drop modifiers.".into());
            }
        }
        let source_id = if nested { a.get("source_id").and_then(Value::as_str).map(ids::valid_source_id).transpose()? } else { None };
        // Modifiers go before the keyword, in a fixed order.
        let given = list_arg(a, "modifiers")?;
        let mods = check_modifiers(&given, &base, is_def)?;
        if annotation {
            if typed.is_none() {
                return Err("an annotation needs typed_by: the metadata definition, for example create_element {owner: 'Pkg::Part', kind: 'metadata', typed_by: 'Pkg::Approved'} writes `@Pkg::Approved;`. Fix: pass typed_by, or pass name to declare a metadata usage.".into());
            }
            // What the annotation is about, when that is not its owner: `@Type about a, b;`. Each name must exist; it is
            // written as the owner reaches it, the same as the `about` of a comment.
            let about_given = if nested { Vec::new() } else { list_arg(a, "about")? };
            if about_given.len() > MAX_LIST {
                return Err(format!("'about' lists {} names; at most {MAX_LIST}.", about_given.len()).into());
            }
            let about = about_given.iter().map(|r| locate_ref(model, r).map(|l| l.text)).collect::<Res<Vec<_>>>()?;
            let extra = ["specializes", "subsets", "redefines", "references", "multiplicity", "value", "short_name", "doc", "direction", "features", "modifiers", "metadata"].into_iter().find(|k| a.get(*k).map(|v| !v.is_null()).unwrap_or(false));
            if let Some(k) = extra {
                return Err(format!("an annotation (metadata without a name) is `@Type;` or `@Type {{ feature = value; }}` and takes no '{k}'. Fix: drop '{k}', or pass name to declare a metadata usage.").into());
            }
            let slots_in = match a.get("slots") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(v)) if v.len() <= MAX_FEATURES => v.iter().map(slot).collect::<Res<Vec<_>>>()?,
                Some(Value::Array(_)) => return Err(format!("slots lists more than {MAX_FEATURES} entries. Fix: split the annotation body.").into()),
                Some(_) => return Err("slots must be a list of {name, value, source_id?}".into()),
            };
            let slot_ids: Vec<(String, String)> = slots_in.iter().flat_map(|(n, _, ids)| ids.iter().map(move |i| (n.clone(), i.clone()))).collect();
            let slots: Vec<(String, String)> = slots_in.into_iter().map(|(n, v, _)| (n, v)).collect();
            return Ok(Spec { kw: base, is_def, name, annotation, short: None, typed, specializes: vec![], redefines: vec![], references: vec![], meta, mult: None, value: None, dir: None, doc: None, mods, slots, slot_ids, source_id, features: vec![], ends: None, body: None, about });
        }
        if a.get("slots").map(|v| !v.is_null()).unwrap_or(false) {
            return Err("slots applies only to an annotation (kind 'metadata' without a name). Fix: drop slots.".into());
        }
        if matches!(base.as_str(), "decide" | "merge" | "fork" | "join") {
            let extra = ["typed_by", "specializes", "subsets", "redefines", "references", "multiplicity", "value", "short_name", "doc", "direction", "features", "metadata"].into_iter().find(|k| a.get(*k).map(|v| !v.is_null()).unwrap_or(false));
            if let Some(k) = extra {
                return Err(format!("a {base} node takes only a name. Fix: drop '{k}'.").into());
            }
        }
        let mut specializes_given = list_arg(a, "specializes")?;
        specializes_given.extend(list_arg(a, "subsets")?);
        let specializes = specializes_given.iter().map(|g| general_text(model, g, is_def)).collect::<Res<Vec<_>>>()?;
        let references_given = list_arg(a, "references")?;
        if !references_given.is_empty() && is_def {
            return Err(format!("'{name}' is a definition, and a definition has no reference subsetting (::>). Fix: make it a usage, or use specializes.").into());
        }
        let references = references_given.iter().map(|g| general_text(model, g, false)).collect::<Res<Vec<_>>>()?;
        let redefines_given = list_arg(a, "redefines")?;
        if !redefines_given.is_empty() && is_def {
            return Err(format!("'{name}' is a definition, and a definition does not redefine. Fix: use specializes, or make it a usage.").into());
        }
        if !redefines_given.is_empty() && nested {
            return Err("redefines is not supported inside features[]. Fix: create the feature with its own create_element {owner, redefines}.".into());
        }
        let redefines = redefines_given.iter().map(|g| redefined_text(model, owner, g)).collect::<Res<Vec<_>>>()?;
        let mult = match a.get("multiplicity") {
            Some(Value::String(m)) if !m.trim().is_empty() => Some(safe_multiplicity(m)?),
            _ => None,
        };
        let value = match text_arg(a, "value") {
            Some(_) if is_def => return Err(format!("'{name}' is a definition and has no value. Fix: create a usage and give the value there.").into()),
            Some(v) => Some(safe_expr("value", v)?),
            None => None,
        };
        let short = text_arg(a, "short_name").map(safe_short_name).transpose()?;
        let features = match a.get("features") {
            Some(Value::Array(v)) => v.iter().map(|f| Spec::parse(model, None, true, f)).collect::<Res<Vec<_>>>()?,
            Some(Value::Null) | None => Vec::new(),
            Some(_) => return Err("features must be a list of {kind, name, typed_by?, multiplicity?, direction?, doc?, modifiers?, source_id?, features?}".into()),
        };
        // The two ends of an interface usage, written as the owner reaches them (the same text `connect` writes).
        let ends = if interface_usage && !nested {
            match (text_arg(a, "source"), text_arg(a, "target")) {
                (None, None) => None,
                (Some(s), Some(t)) => {
                    let Some(home) = owner else {
                        return Err(format!("an {base} usage writes its ends relative to the element that holds it. Fix: pass owner.").into());
                    };
                    let (sl, tl) = (locate_ref(model, s)?, locate_ref(model, t)?);
                    end_usage(model, sl.el, &base, "source")?;
                    end_usage(model, tl.el, &base, "target")?;
                    Some((end_text(model, home, &sl)?, end_text(model, home, &tl)?))
                }
                _ => return Err(format!("an {base} usage takes both ends or none. Fix: pass source and target, or drop both.").into()),
            }
        } else {
            None
        };
        // The expression of a constraint or calc: the last thing in its body, with no semicolon.
        let body = match a.get("body") {
            Some(Value::String(b)) if expr_usage && !nested => Some(safe_expr("body", b)?),
            _ => None,
        };
        Ok(Spec { kw: base, is_def, name, annotation, short, typed, specializes, redefines, references, meta, mult, value, dir, doc: text_arg(a, "doc").map(str::to_string), mods, slots: vec![], slot_ids: vec![], source_id, features, ends, body, about: vec![] })
    }

    fn render(&self) -> String {
        if self.annotation {
            let about = if self.about.is_empty() { String::new() } else { format!(" about {}", self.about.join(", ")) };
            let head = format!("@{}{about}", self.typed.clone().unwrap_or_default());
            if self.slots.is_empty() {
                return format!("{head};");
            }
            let inner = self.slots.iter().map(|(n, v)| format!("    {} = {v};", quote_name(n))).collect::<Vec<_>>().join("\n");
            return format!("{head} {{\n{inner}\n}}");
        }
        let name = if self.name.is_empty() { String::new() } else { quote_name(&self.name) };
        if self.kw == "enum value" {
            return format!("{name};");
        }
        // A bare `entry;` marks where a state starts.
        if matches!(self.kw.as_str(), "entry action" | "do action" | "exit action") && name.is_empty() && self.typed.is_none() && self.doc.is_none() && self.features.is_empty() {
            return format!("{};", self.kw.trim_end_matches(" action"));
        }
        let tail = |m: &&String| edit::TAIL_MODIFIERS.contains(&m.as_str());
        let mut head = String::new();
        for m in self.mods.iter().filter(|m| edit::VISIBILITY.contains(&m.as_str())) {
            head.push_str(&format!("{m} "));
        }
        for m in &self.meta {
            head.push_str(&format!("#{m} "));
        }
        if let Some(d) = &self.dir {
            head.push_str(d);
            head.push(' ');
        }
        for m in self.mods.iter().filter(|m| !edit::VISIBILITY.contains(&m.as_str()) && !tail(m)) {
            head.push_str(m);
            head.push(' ');
        }
        // A directed parameter has no kind keyword: `in x : T;`. A feature that only redefines or subsets has none either.
        let bare_feature = self.kw == "feature" && name.is_empty() && self.mods.is_empty() && self.dir.is_none() && !(self.specializes.is_empty() && self.redefines.is_empty() && self.references.is_empty());
        if self.kw != "parameter" && !bare_feature {
            head.push_str(&self.kw);
            if self.is_def {
                head.push_str(" def");
            }
        } else {
            head.truncate(head.trim_end().len());
        }
        if let Some(s) = &self.short {
            head.push_str(&format!(" <{}>", quote_name(s)));
        }
        if !name.is_empty() {
            head.push(' ');
            head.push_str(&name);
        }
        if let Some(t) = &self.typed {
            head.push_str(&format!(" : {t}"));
        }
        if let Some(m) = &self.mult {
            head.push_str(&format!(" [{m}]"));
        }
        for m in self.mods.iter().filter(|m| matches!(m.as_str(), "ordered" | "nonunique")) {
            head.push_str(&format!(" {m}"));
        }
        if !self.specializes.is_empty() {
            head.push_str(&format!(" :> {}", self.specializes.join(", ")));
        }
        if !self.redefines.is_empty() {
            head.push_str(&format!(" :>> {}", self.redefines.join(", ")));
        }
        if !self.references.is_empty() {
            head.push_str(&format!(" ::> {}", self.references.join(", ")));
        }
        if let Some(v) = &self.value {
            head.push_str(&format!(" = {v}"));
        }
        if let Some((s, t)) = &self.ends {
            head.push_str(&format!(" {} {s} to {t}", if self.kw == "allocation" { "allocate" } else { "connect" }));
        }
        if self.mods.iter().any(|m| m == "parallel") {
            head.push_str(" parallel");
        }
        let mut body: Vec<String> = Vec::new();
        if let Some(d) = &self.doc {
            body.push(format!("doc /* {} */", d.replace("*/", "* /")));
        }
        for f in &self.features {
            body.push(f.render());
        }
        if let Some(x) = &self.body {
            body.push(x.clone());
        }
        if body.is_empty() {
            // The grammar gives a parallel state a braced body, never `;`.
            return if self.mods.iter().any(|m| m == "parallel") { format!("{head} {{}}") } else { format!("{head};") };
        }
        let inner = body.iter().flat_map(|b| b.lines().map(|l| format!("    {l}"))).collect::<Vec<_>>().join("\n");
        format!("{head} {{\n{inner}\n}}")
    }

    /// Qualified names of the members below this spec (the spec itself is `own`), parent first,
    /// and the source ids given to them.
    fn members_below(&self, own: &str, names: &mut Vec<String>, ids: &mut Vec<Value>) {
        for f in &self.features {
            let qn = format!("{own}::{}", quote_name(&f.name));
            names.push(qn.clone());
            if let Some(id) = &f.source_id {
                ids.push(json!({ "ref": qn, "source_id": id }));
            }
            f.members_below(&qn, names, ids);
        }
    }
}

/// `{name, value, source_id?, source_ids?}` of an annotation body: the ids are those the source project gave the slot.
fn slot(v: &Value) -> Res<(String, String, Vec<String>)> {
    let Some(o) = v.as_object() else { return Err("each slot is {name, value}".into()) };
    if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "name" | "value" | "source_id" | "source_ids")) {
        return Err(format!("unknown key {} in a slot. Known: name, value, source_id, source_ids.", crate::agent::text::quote_clip(k)).into());
    }
    let mut ids: Vec<String> = Vec::new();
    match o.get("source_id") {
        None | Some(Value::Null) => {}
        Some(Value::String(t)) => ids.push(ids::valid_source_id(t)?),
        Some(_) => return Err("a slot's source_id must be a string: an id from the source project.".into()),
    }
    match o.get("source_ids") {
        None | Some(Value::Null) => {}
        Some(Value::Array(l)) if l.len() <= 50 => {
            for t in l {
                let Some(t) = t.as_str() else { return Err("a slot's source_ids must be a list of strings.".into()) };
                let id = ids::valid_source_id(t)?;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        Some(_) => return Err("a slot's source_ids must be a list of at most 50 strings.".into()),
    }
    let name = req_as(v, "name", "the feature the metadata sets")?;
    clean_name("a slot name", name)?;
    let value = safe_expr("slot value", req_as(v, "value", "the value expression")?)?;
    Ok((name.to_string(), value, ids))
}

/// The kind as the tools spell it: lower case, one name for each thing.
fn norm_kind(k: &str) -> String {
    let k = k.trim().to_lowercase();
    match k.as_str() {
        "entry" | "do" | "exit" => format!("{k} action"),
        "perform" => "perform action".into(),
        "accept" | "send" => format!("{k} action"),
        "binding" => "bind".into(),
        "rep" | "language" | "textual rep" | "textual representation" => "textual representation".into(),
        "decide node" | "merge node" | "fork node" | "join node" => k.trim_end_matches(" node").to_string(),
        _ => match k.strip_suffix(" def") {
            Some(b) if b.trim() == "binding" => "bind def".into(),
            _ => k,
        },
    }
}

/// Describe the elements a generator made, failing if the text did not produce one of them.
pub fn describe(model: &Model, created: &[String]) -> Res<Vec<Value>> {
    created
        .iter()
        .map(|qn| {
            let e = model.find(qn).ok_or_else(|| Fail::from(format!("the generated text did not produce '{qn}'; nothing was written. Report this as a generator bug.")))?;
            Ok(super::brief(model, e))
        })
        .collect()
}

/// A statement kind (`succession`, `alias`, `bind`, `comment`, `doc`) named by `kind`.
fn statement_kind(a: &Value) -> Res<Option<&'static str>> {
    let Some(k) = text_arg(a, "kind") else { return Ok(None) };
    let k = norm_kind(k);
    if let Some(b) = k.strip_suffix(" def") {
        if STATEMENT_KINDS.contains(&b.trim()) || b.trim() == "bind" {
            return Err(format!("'{}' has no definition form; use kind '{}'.", b.trim(), b.trim()).into());
        }
        return Ok(None);
    }
    Ok(STATEMENT_KINDS.iter().find(|s| **s == k).copied())
}

/// Check `owner` and `file`, add `text`, and describe what was made. `top_name` is the qualified name
/// to expect when there is no owner; with an owner the new member is the one added at the end.
/// Qualified names (and source ids) of the members `features` make below `top`.
fn feature_names(top: &str, features: &[Spec]) -> (Vec<String>, Vec<Value>) {
    let (mut names, mut ids) = (vec![top.to_string()], Vec::new());
    for f in features {
        let qn = format!("{top}::{}", quote_name(&f.name));
        names.push(qn.clone());
        if let Some(id) = &f.source_id {
            ids.push(json!({ "ref": qn, "source_id": id }));
        }
        f.members_below(&qn, &mut names, &mut ids);
    }
    (names, ids)
}

/// Write `text` into `owner_el` (or the file) and read the result back. `defer` says the caller may have the write
/// held back inside a wave of independent creates (see `wave`): it uses nothing of the text this call leaves.
fn place_new(work: &mut Vec<FileIn>, model: &Model, owner_el: Option<&Element>, a: &Value, text: &str, top_name: Option<String>, features: &[Spec], defer: bool) -> Res<Value> {
    if wave::active() {
        return wave::offer(work, model, owner_el, a, text, top_name, features, defer);
    }
    if let (Some(o), Some(f)) = (owner_el, file_arg(a)) {
        let home = model.files[o.file].path.as_str();
        if f != home {
            return Err(Fail::from(format!(
                "'{}' lives in '{home}', so its members are written there; 'file' applies only to top-level elements. Fix: drop 'file', or create a top-level package in '{f}' and import it.",
                o.qualified_name
            )));
        }
    }
    let file = file_arg(a).or_else(|| model.files.first().map(|f| f.path.as_str())).unwrap_or("model.sysml").to_string();
    let before = owner_el.map(|o| o.children.len());
    // `model` is the parse of `work` as it stands, so the edit is planned on it without a second parse.
    for c in edit::apply(model, &EditOp::AddElement { owner: owner_el.map(|o| o.qualified_name.clone()), file: Some(file.clone()), text: text.to_string() })? {
        match work.iter_mut().find(|f| f.path == c.path) {
            Some(f) => f.text = c.text,
            None => work.push(FileIn { path: c.path, text: c.text }),
        }
    }
    let after = model_of(work);
    refuse_unrecognised(model, &after, text)?;
    let top = match (owner_el, before) {
        (Some(o), Some(n)) => {
            let child = after.find(&o.qualified_name).and_then(|oe| oe.children.get(n)).map(|c| &after.elements[*c]);
            // A name was given, so the member must come back under it. A text the parser read as something else
            // (an expression `Owner::$3`) cannot be addressed by that name.
            if let (Some(c), Some(want)) = (child, top_name.as_deref()) {
                if c.name.as_deref().map(quote_name).as_deref() != Some(want) {
                    return Err(Fail::from(format!(
                        "the text written for '{want}' was read as a {} named {}, not as the element asked for; nothing was written. Fix: check the arguments (kind, modifiers, name); the text was {}.",
                        c.keyword(),
                        qshow(&c.qualified_name),
                        crate::agent::text::quote_clip(text)
                    )));
                }
            }
            child.map(|c| c.qualified_name.clone())
        }
        // No owner and no name (an import at the top of a file): the last member of that file.
        (None, _) if top_name.is_none() => after.roots.iter().rev().map(|r| &after.elements[*r]).find(|r| after.files[r.file].path == file).map(|r| r.qualified_name.clone()),
        _ => top_name,
    };
    let top = top.ok_or_else(|| Fail::from("the generated text did not produce the element; nothing was written. Report this as a generator bug."))?;
    let (names, ids) = feature_names(&top, features);
    let created = describe(&after, &names)?;
    // The text added the new element and what it holds, nothing else: no neighbour was swallowed, no extra element appeared.
    if let Some(root) = after.find(&top).map(|e| e.id) {
        let new = subtree_ids(&after, root);
        refuse_stray_elements("the new element", model, &after, &|_| false, &|e| new.contains(&e.id))?;
        // The new subtree may hold only the elements the request named: its own name and its listed features.
        let plain = |q: &str| q.replace('\'', "");
        let listed: HashSet<String> = names.iter().map(|n| plain(n)).collect();
        if let Some(e) = after.elements.iter().find(|e| new.contains(&e.id) && !e.implied_name && e.name.is_some() && !matches!(e.kind.as_str(), "doc" | "comment" | "feature") && !e.props.contains_key("accept_payload") && !listed.contains(&plain(&e.qualified_name))) {
            return Err(Fail::with(
                format!("the new element would also hold {} {}, which the request did not list; nothing was written. Fix: pass one plain name, type or value per field; statements or braces inside a field are not accepted. Add members with their own create_element call.", e.keyword(), crate::agent::text::quote_clip(&e.qualified_name)),
                json!({ "rule": "stray-elements" }),
            ));
        }
    }
    let mut info = json!({ "ref": created[0]["ref"], "created": created });
    if !ids.is_empty() {
        info["feature_ids"] = json!(ids);
    }
    Ok(info)
}

/// Fail when the text a write added holds a statement the parser does not recognise: it would become an
/// anonymous expression that nothing can address, and the warning is easy to miss in a batch.
fn refuse_unrecognised(before: &Model, after: &Model, text: &str) -> Res<()> {
    const MSG: &str = "statement is not recognized as SysML v2";
    let count = |m: &Model| m.diagnostics.iter().filter(|d| d.message == MSG).count();
    if count(after) <= count(before) {
        return Ok(());
    }
    let hit = after.diagnostics.iter().rev().find(|d| d.message == MSG);
    let at = hit.map(|d| format!(" ({}:{})", after.files[d.file].path, after.line_col(d.file, d.span.start).0)).unwrap_or_default();
    Err(Fail::from(format!(
        "the text is not recognized as SysML v2{at}; nothing was written. Fix: write a statement the grammar has (a keyword such as part, attribute, item or state, then the name), or use the create_element kinds. The text was {}.",
        crate::agent::text::quote_clip(text)
    )))
}

/// Warn when a structural usage is typed by a behavior definition or the other way round.
fn kind_warning(model: &Model, spec: &Spec) -> Option<Value> {
    let t = spec.typed.as_deref().and_then(|t| model.find(t.trim_start_matches('~')))?;
    let structural = |k: &str| matches!(k, "part" | "item" | "port" | "attribute");
    let behavioral = |k: &str| matches!(k, "action" | "state" | "calc");
    if spec.is_def || !t.is_def || !((structural(&spec.kw) && behavioral(&t.kind)) || (behavioral(&spec.kw) && structural(&t.kind))) {
        return None;
    }
    Some(json!({
        "rule": "usage-definition-kind-mismatch",
        "severity": "warning",
        "message": format!("a {} '{}' is typed by the {} '{}'; validate_model keeps reporting this as a warning.", spec.kw, spec.name, t.keyword(), t.qualified_name),
        "fix": format!("Type the {} by a {} definition, or make it an {} usage.", spec.kw, spec.kw, t.kind)
    }))
}

/// True when every general type above `e` is an element of the model, so what `e` declares and inherits is all known.
fn general_types_known(model: &Model, e: &Element) -> bool {
    let (mut seen, mut stack) = (BTreeSet::new(), vec![e.id]);
    while let Some(x) = stack.pop() {
        if !seen.insert(x) || seen.len() > 64 {
            continue;
        }
        for r in model.elements[x].refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines)) {
            match r.resolved() {
                Some(t) => stack.push(t),
                None => return false,
            }
        }
    }
    true
}

/// The named features `def` declares or inherits, in the order of the text.
fn declared_features<'a>(model: &'a Model, def: &'a Element) -> Vec<&'a str> {
    let (mut seen, mut stack, mut out) = (BTreeSet::new(), vec![def.id], Vec::new());
    while let Some(x) = stack.pop() {
        if !seen.insert(x) || seen.len() > 64 {
            continue;
        }
        out.extend(model.children(x).filter(|c| !c.implied_name && !matches!(c.kind.as_str(), "doc" | "comment" | "import")).filter_map(|c| c.name.as_deref()));
        stack.extend(model.elements[x].refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines)).filter_map(|r| r.resolved()));
    }
    out
}

fn create_element(work: &mut Vec<FileIn>, a: &Value) -> Res<Value> {
    let model = model_of(work);
    let owner = text_arg(a, "owner");
    let owner_el = owner.map(|o| locate(&model, o)).transpose()?;
    // A flow final node (a branch that ends while the others go on) has no keyword of its own in SysML v2: it is a name
    // for `done`, as the Cameo import writes it (`alias tripEnd for done;`), and a succession then reaches it by name.
    if text_arg(a, "kind").map(|k| matches!(norm_kind(k).replace(['_', '-'], " ").as_str(), "flow final" | "flow final node" | "flowfinal")).unwrap_or(false) {
        if let Some(k) = ["target", "typed_by", "doc", "specializes", "redefines", "references", "value", "multiplicity", "direction", "features", "metadata"].into_iter().find(|k| a.get(*k).map(|v| !v.is_null()).unwrap_or(false)) {
            return Err(format!("a flow final node takes only a name and an owner. Fix: drop '{k}'.").into());
        }
        if owner_el.map(|o| o.kind != "action").unwrap_or(true) {
            return Err("a flow final node belongs to an activity. Fix: pass owner, the action def or action (the method) that holds it.".into());
        }
        let mut b = a.clone();
        b["kind"] = json!("alias");
        b["target"] = json!("done");
        return create_statement(work, &model, owner_el, &b, "alias");
    }
    if let Some(k) = statement_kind(a)? {
        return create_statement(work, &model, owner_el, a, k);
    }
    let mut spec = Spec::parse(&model, owner_el, false, a)?;
    spec.mint_ids(owner_el.map(|o| o.qualified_name.as_str()).unwrap_or(""), &mut crate::agent::mint::taken(&model));
    let if_absent = flag(a, "if_absent");
    let siblings: Vec<&Element> = match owner_el {
        Some(o) => model.children(o.id).collect(),
        None => model.roots.iter().map(|r| &model.elements[*r]).collect(),
    };
    if let Some(o) = owner_el.filter(|o| o.kind == "alias") {
        let stands = o.first_resolved(RefRole::Target).map(|t| model.elements[t].qualified_name.clone());
        return Err(format!(
            "'{}' is an alias, and an alias holds no members. Fix: create the member in {}.",
            o.qualified_name,
            stands.map(|q| format!("'{q}', the element the alias stands for")).unwrap_or_else(|| "the element the alias stands for".into())
        )
        .into());
    }
    if let Some(short) = spec.short.as_deref() {
        if let Some(c) = siblings.iter().find(|c| c.name.as_deref() == Some(short) || c.short_name.as_deref() == Some(short)) {
            return Err(format!(
                "the short name '{short}' is already the name or short name of '{}' beside it, so a reference to '{short}' would be ambiguous. Fix: choose another short name, or drop short_name.",
                c.qualified_name
            )
            .into());
        }
    }
    let home = owner_el.map(|o| o.qualified_name.as_str()).unwrap_or("the file");
    if spec.annotation {
        let Some(o) = owner_el else {
            return Err("an annotation (metadata without a name) goes inside an element. Fix: pass owner.".into());
        };
        let typed = spec.typed.as_deref().and_then(|t| model.find(t.trim_start_matches('~'))).map(|t| t.id);
        // A slot sets a feature of the metadata definition. When the definition and all its general types are in the
        // model, a slot it does not declare is refused; a definition from a library that is not loaded cannot be checked.
        if let Some(def) = typed.map(|t| &model.elements[t]).filter(|d| general_types_known(&model, d)) {
            if let Some((n, _)) = spec.slots.iter().find(|(n, _)| member_named(&model, def, n, &mut BTreeSet::new()).is_none()) {
                let mut names: Vec<&str> = declared_features(&model, def);
                names.truncate(12);
                return Err(Fail::with(
                    format!(
                        "the slot {} is no feature of the metadata definition {}: {}. Nothing was written. Fix: use a feature the definition declares, or add the feature to the definition first (create_element {{owner: {}, kind: 'attribute', name: {}}}).",
                        crate::agent::text::quote_clip(n),
                        qshow(&def.qualified_name),
                        if names.is_empty() { "it declares no feature".to_string() } else { format!("it declares {}", names.join(", ")) },
                        qshow(&def.qualified_name),
                        crate::agent::text::quote_clip(n)
                    ),
                    json!({ "rule": "unresolved-reference", "definition": def.qualified_name, "slot": n, "features": names }),
                ));
            }
        }
        let allow_dup = flag(a, "allow_duplicate");
        if allow_dup && if_absent {
            return Err("allow_duplicate and if_absent contradict each other. Fix: pass one of them.".into());
        }
        // Two annotations of one type are the same thing only when they are about the same elements.
        let mut about_ids: Vec<Option<ElementId>> = list_arg(a, "about")?.iter().map(|r| locate_ref(&model, r).ok().map(|l| l.el.id)).collect();
        about_ids.sort();
        let about_of = |c: &Element| {
            let mut v: Vec<Option<ElementId>> = c.refs_of(RefRole::Other).map(|r| r.resolved()).collect();
            v.sort();
            v
        };
        let dup = siblings.iter().find(|c| c.kind == "metadata" && c.name.is_none() && typed.is_some() && c.first_resolved(RefRole::Type) == typed && about_of(c) == about_ids).filter(|_| !allow_dup);
        if let Some(d) = dup {
            if if_absent {
                return Ok(json!({ "ref": d.qualified_name, "created": [], "existed": true }));
            }
            return Err(Fail::with(
                format!("{} already carries @{}. Fix: nothing to do, or pass allow_duplicate: true when the source model repeats the annotation (the bodies may differ).", qshow(&o.qualified_name), spec.typed.clone().unwrap_or_default()),
                json!({ "declared_at": super::graph::declared_at(&model, d.id) }),
            ));
        }
        let mut info = place_new(work, &model, owner_el, a, &spec.render(), None, &[], true)?;
        // Ids the source project gave to the slots: each one names the feature the slot makes.
        if !spec.slot_ids.is_empty() {
            let top = info["ref"].as_str().unwrap_or("").to_string();
            // In a wave the text is not written yet: the wave's read-back looks for the slot features.
            let after = (!wave::active()).then(|| model_of(work));
            let mut ids = Vec::new();
            for (n, id) in &spec.slot_ids {
                let qn = format!("{top}::{}", quote_name(n));
                match &after {
                    Some(m) if m.find(&qn).is_none() => {
                        return Err(Fail::from(format!("the slot '{n}' did not read back as a member of the annotation; nothing was written. Report this as a generator bug.")));
                    }
                    Some(_) => {}
                    None => wave::require(&qn),
                }
                ids.push(json!({ "ref": qn, "source_id": id }));
            }
            info["feature_ids"] = json!(ids);
        }
        return Ok(info);
    }
    if !spec.name.is_empty() {
        if let Some(dup) = siblings.iter().find(|c| !c.implied_name && c.name.as_deref() == Some(spec.name.as_str())) {
            if if_absent {
                if dup.keyword() != spec.read_back() {
                    return Err(Fail::from(format!(
                        "'{}' already has a member named '{}', but it is a {}, not a {}. Fix: choose another name, or change it with update_element {{ref: '{}'}}.",
                        home,
                        spec.name,
                        dup.keyword(),
                        spec.keyword(),
                        dup.qualified_name
                    )));
                }
                return Ok(json!({ "ref": dup.qualified_name, "created": [], "existed": true }));
            }
            return Err(Fail::from(format!(
                "'{home}' already has a member named '{}'. Choose another name, change the existing one with update_element {{ref: '{}'}}, or pass if_absent: true to reuse it.",
                spec.name, dup.qualified_name
            )));
        }
    } else if owner_el.is_none() {
        return Err(format!("a {} without a name goes inside an element. Fix: pass owner, or pass name.", spec.kw).into());
    }
    let top_name = (!spec.name.is_empty()).then(|| quote_name(&spec.name));
    let mut info = place_new(work, &model, owner_el, a, &spec.render(), top_name, &spec.features, true)?;
    if let Some(w) = kind_warning(&model, &spec) {
        info["warnings"] = json!([w]);
    }
    Ok(info)
}

/// `succession`, `alias`, `bind`, `comment` and `doc`: statements with ends or a body, not declarations.
fn create_statement(work: &mut Vec<FileIn>, model: &Model, owner_el: Option<&Element>, a: &Value, kind: &str) -> Res<Value> {
    // export_facts prints the text of a comment as `doc`: it is the same text as `body`.
    let renamed;
    let a = if matches!(kind, "comment" | "doc") && a.get("doc").map(|v| !v.is_null()).unwrap_or(false) {
        if a.get("body").map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("a {kind} takes its text as body, and doc is the same thing. Fix: pass one of them.").into());
        }
        renamed = {
            let mut b = a.clone();
            b["body"] = b["doc"].take();
            b.as_object_mut().map(|o| o.remove("doc"));
            b
        };
        &renamed
    } else {
        a
    };
    let allowed: &[&str] = match kind {
        "succession" => &["source", "target", "name", "guard"],
        "alias" => &["name", "target"],
        "bind" => &["source", "target", "name"],
        "doc" => &["name", "body"],
        "message" => &["name", "source", "target", "of", "doc", "metadata"],
        "import" => &["target", "visibility", "all", "recursive", "modifiers"],
        "textual representation" => &["name", "language", "body"],
        "accept action" | "send action" => &["name", "payload", "via", "to", "doc"],
        _ => &["name", "body", "about"],
    };
    for k in ["source", "target", "guard", "body", "about", "of", "typed_by", "specializes", "subsets", "redefines", "references", "metadata", "unnamed", "value", "short_name", "doc", "multiplicity", "direction", "features", "modifiers", "slots", "language", "visibility", "all", "recursive"] {
        if !allowed.contains(&k) && a.get(k).map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("'{k}' does not apply to {kind}. Fix: drop '{k}'; {kind} takes {}.", allowed.join(", ")).into());
        }
    }
    let if_absent = flag(a, "if_absent");
    let name = text_arg(a, "name");
    if let Some(n) = name {
        clean_name("name", n)?;
    }
    for k in ["body", "doc"] {
        if let Some(Value::String(b)) = a.get(k) {
            clean_doc(k, b)?;
        }
    }
    if kind == "import" && name.is_some() {
        return Err("an import has no name. Fix: drop name; the import is addressed by its target.".into());
    }
    let siblings: Vec<&Element> = match owner_el {
        Some(o) => model.children(o.id).collect(),
        None => model.roots.iter().map(|r| &model.elements[*r]).collect(),
    };
    let home = owner_el.map(|o| o.qualified_name.as_str()).unwrap_or("the file");
    if kind == "succession" {
        let mut args = json!({ "kind": "succession", "source": req(a, "source")?, "target": req(a, "target")?, "if_absent": if_absent });
        for (k, v) in [("owner", owner_el.map(|o| o.qualified_name.clone())), ("name", name.map(str::to_string)), ("guard", text_arg(a, "guard").map(str::to_string))] {
            if let Some(v) = v {
                args[k] = json!(v);
            }
        }
        let mut info = create_relationship(work, &args)?;
        if info["existed"] != true {
            let after = model_of(work);
            let r = info["ref"].as_str().unwrap_or("").to_string();
            info["created"] = json!(describe(&after, &[r])?);
        } else {
            info["created"] = json!([]);
        }
        return Ok(info);
    }
    // Named statements follow the same rule as other named members.
    if let Some(n) = name {
        if let Some(dup) = siblings.iter().find(|c| !c.implied_name && c.name.as_deref() == Some(n)) {
            if if_absent && dup.kind == kind.replace("bind", "binding") {
                return Ok(json!({ "ref": dup.qualified_name, "created": [], "existed": true }));
            }
            return Err(Fail::from(format!(
                "'{home}' already has a member named '{n}'. Choose another name, change the existing one with update_element {{ref: '{}'}}, or pass if_absent: true to reuse it.",
                dup.qualified_name
            )));
        }
    }
    if kind == "import" {
        return create_import(work, model, owner_el, a, &siblings, home);
    }
    let end_owner = |what: &str| -> Res<&Element> { owner_el.ok_or_else(|| Fail::from(format!("a {what} writes its ends relative to the element that holds it. Fix: pass owner."))) };
    let text = match kind {
        // `accept goal : Goal via goalIn;` or `action receive accept goal : Goal via goalIn;`; `send status via statusOut;`.
        "accept action" | "send action" => {
            end_owner(kind)?;
            let payload = req_as(a, "payload", if kind == "accept action" { "what is accepted: 'name : Item' or 'Item'" } else { "the expression sent, for example an item usage or 'new Item()'" })?;
            let payload = if kind == "accept action" {
                match split_payload(payload) {
                    Some((n, t)) => {
                        clean_name("payload name", n)?;
                        format!("{} : {}", quote_name(n), payload_text(model, t)?)
                    }
                    None => payload_text(model, payload)?,
                }
            } else {
                safe_expr("payload", payload)?
            };
            let via = text_arg(a, "via").map(|v| safe_expr("via", v)).transpose()?.map(|v| format!(" via {v}")).unwrap_or_default();
            if kind == "accept action" && a.get("to").map(|v| !v.is_null()).unwrap_or(false) {
                return Err("'to' does not apply to an accept action. Fix: drop 'to'; it names where a send action sends.".into());
            }
            let to = text_arg(a, "to").map(|v| safe_expr("to", v)).transpose()?.map(|v| format!(" to {v}")).unwrap_or_default();
            let head = format!("{}{} {payload}{via}{to}", name.map(|n| format!("action {} ", quote_name(n))).unwrap_or_default(), kind.trim_end_matches(" action"));
            match text_arg(a, "doc") {
                Some(d) => format!("{head} {{\n    doc /* {} */\n}}", d.replace("*/", "* /")),
                None => format!("{head};"),
            }
        }
        "textual representation" => {
            let lang = req_as(a, "language", "the language of the text, for example \"StructuredExpression\"")?;
            if lang.contains(['"', '\n', '\\']) || lang.chars().any(|c| edit::char_fault(c, true).is_some()) {
                return Err("language must be one line without quotes or backslashes. Fix: pass the language name, for example StructuredExpression.".into());
            }
            let Some(Value::String(body)) = a.get("body") else {
                return Err(Fail::from("missing argument 'body': the text between /* and */ (export_facts prints it as body)."));
            };
            let body = body.replace("*/", "* /");
            let comment = if body.contains('\n') || body.trim_start().starts_with('*') {
                let lines = body.lines().map(|l| format!(" * {l}")).collect::<Vec<_>>().join("\n");
                format!("/*\n{lines}\n */")
            } else {
                format!("/* {} */", body.trim())
            };
            let n = name.map(|n| format!("rep {} ", quote_name(n))).unwrap_or_default();
            format!("{n}language \"{lang}\" {comment}")
        }
        "alias" => {
            let n = name.ok_or_else(|| Fail::from("missing argument 'name': the alias name."))?;
            let given = req(a, "target")?;
            // `start` and `done` are the ends of a behavior, not elements of the model.
            let target = if MARKERS.contains(&given) { given.to_string() } else { locate_ref(model, given)?.text };
            format!("alias {} for {};", quote_name(n), target)
        }
        "message" => {
            let home_el = end_owner("message")?;
            // A message may have no ends (`message StartVehicle { ... }`); with one end it is incomplete.
            let ends = match (text_arg(a, "source"), text_arg(a, "target")) {
                (None, None) => String::new(),
                (Some(_), Some(_)) => {
                    let (s, t) = (locate_ref(model, req(a, "source")?)?, locate_ref(model, req(a, "target")?)?);
                    format!(" from {} to {}", end_text(model, home_el, &s)?, end_text(model, home_el, &t)?)
                }
                _ => return Err("a message takes both ends or none. Fix: pass source and target, or drop both.".into()),
            };
            let of = text_arg(a, "of").map(|t| payload_text(model, t)).transpose()?.map(|t| format!(" of {t}")).unwrap_or_default();
            let n = name.map(|n| format!(" {}", quote_name(n))).unwrap_or_default();
            // A combined fragment the message belongs to (`#SysMLv1::alt`, `#SysMLv1::par`): the sequence view frames the
            // run of messages that carry one tag, an operand per guard (the doc in brackets).
            let mprefix: String = meta_list(model, a)?.iter().map(|m| format!("#{m} ")).collect();
            let head = format!("{mprefix}message{n}{of}{ends}");
            match text_arg(a, "doc") {
                Some(d) => format!("{head} {{\n    doc /* {} */\n}}", d.replace("*/", "* /")),
                None => format!("{head};"),
            }
        }
        "bind" => {
            let home_el = end_owner("bind")?;
            // An end may be written relative to the owner, as the statement reads (`goal`, `receive.goal`).
            let rel = |key: &str| -> Res<Located> {
                let given = req(a, key)?;
                locate_ref(model, given).or_else(|fail| {
                    let path = given.trim().replace('.', "::");
                    locate_ref(model, &format!("{}::{path}", home_el.qualified_name)).map_err(|_| fail)
                })
            };
            let (s, t) = (rel("source")?, rel("target")?);
            if s.el.id == t.el.id && s.text == t.text {
                return Err("source and target are the same element".into());
            }
            let (st, tt) = (end_text(model, home_el, &s)?, end_text(model, home_el, &t)?);
            let key = stmt_key_of("bind", &end_path(model, home_el.id, &s), &end_path(model, home_el.id, &t), &name_extra(name), Some(home_el.qualified_name.clone()));
            if !flag(a, "allow_duplicate") {
                need_whole_model(model)?;
            }
            if let Some(at) = find_duplicate(model, "bind", &key).filter(|_| !flag(a, "allow_duplicate")) {
                if if_absent {
                    return Ok(json!({ "ref": at["element"], "created": [], "existed": true, "declared_at": at }));
                }
                return Err(Fail::with("the bind between those ends already exists. Fix: nothing to do, pass if_absent: true to make the call a no-op, or pass allow_duplicate: true when the source model really has a second one.", json!({ "declared_at": at })));
            }
            match name {
                Some(n) => format!("binding {} bind {st} = {tt};", quote_name(n)),
                None => format!("bind {st} = {tt};"),
            }
        }
        _ => {
            // A comment may have an empty body (`comment about X /*  */`); a doc may not.
            let body = match a.get("body") {
                Some(Value::String(b)) if !b.trim().is_empty() => b.trim().replace("*/", "* /"),
                Some(Value::String(_)) if kind == "comment" => " ".to_string(),
                _ => return Err(Fail::from("missing argument 'body': the comment text (an empty string is accepted for a comment).")),
            };
            let about = if kind == "comment" {
                let refs = list_arg(a, "about")?;
                if refs.len() > MAX_LIST {
                    return Err(format!("'about' lists {} names; at most {MAX_LIST}.", refs.len()).into());
                }
                let texts = refs.iter().map(|r| locate_ref(model, r).map(|l| l.text)).collect::<Res<Vec<_>>>()?;
                if texts.is_empty() { String::new() } else { format!(" about {}", texts.join(", ")) }
            } else {
                String::new()
            };
            let n = name.map(|n| format!(" {}", quote_name(n))).unwrap_or_default();
            if owner_el.is_none() && name.is_none() {
                return Err(format!("a {kind} without a name goes inside an element. Fix: pass owner, or pass name.").into());
            }
            format!("{kind}{n}{about} /* {body} */")
        }
    };
    let top_name = name.map(quote_name);
    place_new(work, model, owner_el, a, &text, top_name, &[], true)
}

/// `import Pkg::*;`, `private import Pkg::Sub::**;`, `import all Pkg;`: the target as `export_facts` prints it.
fn create_import(work: &mut Vec<FileIn>, model: &Model, owner_el: Option<&Element>, a: &Value, siblings: &[&Element], home: &str) -> Res<Value> {
    let given = req_as(a, "target", "the namespace to import, for example 'Pkg::Sub::*' (export_facts prints it as target)")?.trim();
    let (base, mut wild) = if let Some(b) = given.strip_suffix("::**") {
        (b, "**")
    } else if let Some(b) = given.strip_suffix("::*") {
        (b, "*")
    } else {
        (given, "")
    };
    if flag(a, "recursive") {
        if wild == "*" {
            return Err("recursive: true and a target that ends in ::* contradict each other. Fix: end the target in ::** or drop recursive.".into());
        }
        wild = "**";
    }
    let first = split_first(base);
    // A target written as the owner reaches it (`Sub::Units` inside `Pkg`) is read as the parser reads it there.
    let from_owner = || owner_el.and_then(|o| crate::resolve::resolve_texts(model, &[(o.id, RefRole::Source, base.to_string())]).into_iter().next().flatten()).map(|t| model.elements[t].qualified_name.clone());
    let target = match locate_ref(model, base) {
        Ok(l) => l.text,
        Err(f) => match from_owner() {
            Some(q) => q,
            None if LIBRARY_TYPES.contains(&first.as_str()) => edit::qualified_text("import target", base, false)?,
            None => return Err(f),
        },
    };
    let mut words = list_arg(a, "modifiers")?;
    if let Some(v) = text_arg(a, "visibility") {
        words.push(v.to_string());
    }
    let mut vis: Option<String> = None;
    for w in words {
        let w = w.trim().to_lowercase();
        if !edit::VISIBILITY.contains(&w.as_str()) {
            return Err(format!("an import takes a visibility word ({}), not {}. Fix: pass visibility.", edit::VISIBILITY.join(", "), crate::agent::text::quote_clip(&w)).into());
        }
        if vis.as_ref().is_some_and(|v| *v != w) {
            return Err("an import takes one visibility word.".into());
        }
        vis = Some(w);
    }
    let all = flag(a, "all");
    let same = |c: &&&Element| {
        c.kind == "import"
            && c.props.get("wildcard").map(String::as_str).unwrap_or("") == wild
            && (c.props.get("all").map(String::as_str) == Some("true")) == all
            // The same target: the element it resolves to, or, for a library that is not loaded, the same text. (An
            // import that does not resolve used to count as the same as any target.)
            && c.refs_of(RefRole::Import).next().is_some_and(|r| match r.resolved() {
                Some(t) => model.elements[t].qualified_name == target,
                None => {
                    let plain = |t: &str| {
                        let t = t.trim().trim_end_matches("::**").trim_end_matches("::*").trim();
                        crate::model::canonical_name(t).unwrap_or_else(|| t.to_string())
                    };
                    plain(&r.text) == plain(&target)
                }
            })
    };
    let allow_dup = flag(a, "allow_duplicate");
    if allow_dup && flag(a, "if_absent") {
        return Err("allow_duplicate and if_absent contradict each other. Fix: pass one of them.".into());
    }
    if let (Some(d), false) = (siblings.iter().find(same), allow_dup) {
        if flag(a, "if_absent") {
            return Ok(json!({ "ref": d.qualified_name, "created": [], "existed": true }));
        }
        return Err(Fail::with(
            format!("'{home}' already imports '{given}'. Fix: nothing to do, pass if_absent: true to make the call a no-op, or pass allow_duplicate: true when the source model repeats the import."),
            json!({ "declared_at": super::graph::declared_at(model, d.id) }),
        ));
    }
    let v = format!("{} ", vis.as_deref().unwrap_or("private"));
    let text = format!("{v}import {}{target}{};", if all { "all " } else { "" }, if wild.is_empty() { String::new() } else { format!("::{wild}") });
    place_new(work, model, owner_el, a, &text, None, &[], false)
}

/// The first segment of a qualified name, without quotes.
fn split_first(q: &str) -> String {
    name_segs(q).into_iter().next().unwrap_or_default()
}

// ---- update_element -------------------------------------------------------

fn update_element(work: &mut Vec<FileIn>, a: &Value, events: &mut Vec<IdEvent>) -> Res<Value> {
    let model = model_of(work);
    let target = locate(&model, req(a, "ref")?)?;
    let qn = target.qualified_name.clone();
    let present = |k: &str| a.get(k).map(|v| !v.is_null()).unwrap_or(false);
    let fields = ["rename", "typed_by", "doc", "multiplicity", "value", "short_name", "specializes", "subsets", "remove_specializes", "redefines", "references", "metadata", "modifiers", "direction"];
    // Only a source id: nothing in the text changes, and the element may be an unnamed statement.
    if present("source_id") && !present("owner") && !fields.iter().any(|k| present(k)) {
        return Ok(json!({ "ref": qn, "previous_ref": qn, "changed": [], "element": super::brief(&model, target) }));
    }
    // Metadata prefixes alone: any element or statement, named or not.
    if present("metadata") && !present("owner") && !fields.iter().filter(|k| **k != "metadata").any(|k| present(k)) {
        let tags = meta_list(&model, a)?;
        step(work, EditOp::SetMetadata { target: qn.clone(), tags })?;
        let after = model_of(work);
        let e = after.find(&qn).ok_or_else(|| Fail::from(format!("after the edit '{qn}' was not found; nothing was written")))?;
        return Ok(json!({ "ref": qn, "previous_ref": qn, "changed": ["metadata"], "element": super::brief(&after, e) }));
    }
    if present("owner") {
        if let Some(k) = fields.iter().find(|k| present(k)) {
            return Err(format!("owner moves the element and cannot be combined with '{k}'. Fix: call update_element twice.").into());
        }
        let new_owner = req(a, "owner")?;
        if target.kind == "package" && !target.is_def && target.owner.is_none() {
            return move_package(work, &model, target, new_owner, events);
        }
        return move_element(work, &model, target, new_owner, events);
    }
    if matches!(a.get("rename"), Some(Value::String(s)) if s.trim().is_empty()) {
        if let Some(k) = fields.iter().skip(1).find(|k| present(k)) {
            return Err(format!("rename '' clears the name and cannot be combined with '{k}'. Fix: call update_element twice.").into());
        }
        return clear_name(work, &model, target, events);
    }
    if target.name.is_none() || target.implied_name {
        if let Some(n) = text_arg(a, "rename").filter(|n| !n.is_empty()) {
            if let Some(k) = fields.iter().skip(1).find(|k| present(k)) {
                return Err(format!("giving a statement a name cannot be combined with '{k}'. Fix: call update_element twice.").into());
            }
            return name_statement(work, &model, target, n, events);
        }
        // An unnamed statement (`Owner::$n`) takes its documentation, metadata, modifiers and short name; what names or
        // types it is written in the statement itself.
        if let Some(k) = fields.iter().find(|k| present(k) && !matches!(**k, "doc" | "metadata" | "modifiers" | "short_name" | "direction")) {
            return Err(format!("'{qn}' has no declared name, so update_element cannot change its {k}. It takes doc, metadata, modifiers, short_name, rename (to give it a name) and owner. Fix: delete it (delete_relationship by its ends) and create it again, or use edit_model replace.").into());
        }
    }
    let text_or_empty = |k: &str| -> Res<Option<String>> {
        match a.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.trim().to_string())),
            Some(_) => Err(Fail::from(format!("'{k}' must be a string; an empty string removes it."))),
        }
    };
    let rename = text_arg(a, "rename");
    let typed = match a.get("typed_by") {
        Some(Value::Array(_)) => Some(list_arg(a, "typed_by")?.join(", ")).filter(|t| !t.is_empty()),
        _ => text_or_empty("typed_by")?,
    };
    let doc = text_or_empty("doc")?;
    let value = text_or_empty("value")?;
    let short = text_or_empty("short_name")?;
    let mult = a.get("multiplicity").and_then(Value::as_str);
    let mut add_gen = list_arg(a, "specializes")?;
    add_gen.extend(list_arg(a, "subsets")?);
    let refs_to = if present("references") { Some(list_arg(a, "references")?) } else { None };
    let tags = if present("metadata") { Some(meta_list(&model, a)?) } else { None };
    let rem_gen = list_arg(a, "remove_specializes")?;
    let redefs = if present("redefines") { Some(list_arg(a, "redefines")?) } else { None };
    // Modifier words (the entries of prefixes that do not start with #) and the direction; [] and "" remove them.
    let mods = if present("modifiers") { Some(check_modifiers(&list_arg(a, "modifiers")?, &target.kind, target.is_def)?) } else { None };
    let dir = match a.get("direction") {
        None | Some(Value::Null) => None,
        Some(Value::String(d)) => {
            let d = d.trim().to_lowercase();
            if !d.is_empty() && !matches!(d.as_str(), "in" | "out" | "inout") {
                return Err(format!("direction must be in, out or inout, not '{d}'; an empty string removes it.").into());
            }
            Some(d)
        }
        Some(_) => return Err("direction must be a string: in, out or inout; an empty string removes it.".into()),
    };
    if rename.is_none() && typed.is_none() && doc.is_none() && mult.is_none() && value.is_none() && short.is_none() && add_gen.is_empty() && rem_gen.is_empty() && redefs.is_none() && refs_to.is_none() && tags.is_none() && mods.is_none() && dir.is_none() {
        return Err("nothing to change: give at least one of rename, typed_by, multiplicity, value, specializes, remove_specializes, redefines, references, metadata, modifiers, direction, short_name, doc, owner, source_id".into());
    }
    let usage_only = |what: &str| -> Res<()> {
        if target.is_def {
            return Err(format!("'{qn}' is a definition; {what}.").into());
        }
        Ok(())
    };
    let mut changed = Vec::new();
    if let Some(tags) = tags {
        step(work, EditOp::SetMetadata { target: qn.clone(), tags })?;
        changed.push("metadata");
    }
    if mods.is_some() || dir.is_some() {
        if target.kind == "parameter" && mods.as_ref().map(|m| !m.is_empty()).unwrap_or(false) {
            return Err("a parameter takes no modifiers: it is written as its direction and its name. Fix: drop modifiers.".into());
        }
        let m = model_of(work);
        let before = m.find(&qn).map(|e| (e.prefixes.clone(), e.direction.clone()));
        step(work, EditOp::SetPrefixes { target: qn.clone(), direction: dir.clone(), words: mods.clone() })?;
        let m = model_of(work);
        let now = m.find(&qn).map(|e| (e.prefixes.clone(), e.direction.clone()));
        if before != now {
            if mods.is_some() {
                changed.push("modifiers");
            }
            if dir.is_some() {
                changed.push("direction");
            }
        }
        let want: Vec<String> = mods.clone().unwrap_or_default();
        if let (Some(_), Some((p, _))) = (&mods, &now) {
            let have: Vec<&String> = p.iter().filter(|x| !x.starts_with('#')).collect();
            if let Some(w) = want.iter().find(|w| !have.iter().any(|h| h == w)) {
                return Err(format!("the modifier '{w}' did not read back on '{qn}'; nothing was written.").into());
            }
        }
    }
    for g in &rem_gen {
        let m = model_of(work);
        let e = m.find(&qn).ok_or("element vanished")?;
        // A library name has no element in the model: it is found by its text.
        let idx = match locate_ref(&m, g) {
            Ok(l) => e.refs_of(RefRole::Specializes).position(|r| r.resolved() == Some(l.el.id)),
            Err(_) if is_library_name(g) => e.refs_of(RefRole::Specializes).position(|r| r.text.trim() == g.trim()),
            Err(f) => return Err(f),
        };
        let Some(index) = idx else {
            return Err(format!("'{qn}' does not specialize '{g}'. Fix: pass a name from its specializations: {}.", e.refs_of(RefRole::Specializes).map(|r| r.text.clone()).collect::<Vec<_>>().join(", ")).into());
        };
        step(work, EditOp::RemoveSpecialization { target: qn.clone(), index })?;
        changed.push("remove_specializes");
    }
    for g in &add_gen {
        let text = general_text(&model, g, target.is_def)?;
        let m = model_of(work);
        let e = m.find(&qn).ok_or("element vanished")?;
        let held = match locate_ref(&m, g) {
            Ok(l) => e.refs_of(RefRole::Specializes).any(|r| r.resolved() == Some(l.el.id)),
            // `general_text` let a library name through, so it has no element: it is held when its text is.
            Err(_) => e.refs_of(RefRole::Specializes).any(|r| r.text.trim() == text.trim()),
        };
        if held {
            continue;
        }
        step(work, EditOp::AddSpecialization { target: qn.clone(), general: text })?;
        changed.push("specializes");
    }
    if let Some(t) = typed {
        if target.is_def {
            return Err(format!("'{qn}' is a definition and has no type. To change what it specializes use create_relationship {{kind: 'specialize'}}.").into());
        }
        if t.is_empty() {
            step(work, EditOp::ClearType { target: qn.clone() })?;
        } else {
            let text = t.split(", ").map(|x| type_text(&model, x)).collect::<Res<Vec<_>>>()?.join(", ");
            step(work, EditOp::SetType { target: qn.clone(), type_ref: text })?;
        }
        changed.push("typed_by");
    }
    if let Some(m) = mult {
        usage_only("only usages have a multiplicity")?;
        let m = if m.trim().is_empty() { String::new() } else { safe_multiplicity(m)? };
        step(work, EditOp::SetMultiplicity { target: qn.clone(), multiplicity: m })?;
        changed.push("multiplicity");
    }
    if let Some(v) = value {
        usage_only("a definition has no value. Fix: set the value on a usage")?;
        let v = if v.is_empty() { v } else { safe_expr("value", &v)? };
        step(work, EditOp::SetValue { target: qn.clone(), value: v })?;
        changed.push("value");
    }
    if let Some(list) = redefs {
        usage_only("a definition does not redefine. Fix: use specializes")?;
        let names = list.iter().map(|g| redefined_text(&model, target.owner.map(|o| &model.elements[o]), g)).collect::<Res<Vec<_>>>()?;
        step(work, EditOp::SetRedefines { target: qn.clone(), names })?;
        changed.push("redefines");
    }
    if let Some(list) = refs_to {
        usage_only("a definition has no reference subsetting. Fix: use specializes")?;
        let names = list.iter().map(|g| general_text(&model, g, false)).collect::<Res<Vec<_>>>()?;
        step(work, EditOp::SetReferences { target: qn.clone(), names })?;
        changed.push("references");
    }
    if let Some(sn) = short {
        let sn = if sn.is_empty() { sn } else { safe_short_name(&sn)? };
        step(work, EditOp::SetShortName { target: qn.clone(), short: sn })?;
        changed.push("short_name");
    }
    if let Some(d) = doc {
        if d.is_empty() {
            let m = model_of(work);
            if let Some(c) = m.find(&qn).and_then(|e| m.children(e.id).find(|c| c.kind == "doc")) {
                step(work, EditOp::Delete { target: c.qualified_name.clone() })?;
            }
        } else {
            step(work, EditOp::SetDoc { target: qn.clone(), doc: d })?;
        }
        changed.push("doc");
    }
    let mut new_ref = qn.clone();
    if let Some(n) = rename {
        let owner_children: Vec<&Element> = match target.owner {
            Some(o) => model.children(o).collect(),
            None => model.roots.iter().map(|r| &model.elements[*r]).collect(),
        };
        if owner_children.iter().any(|c| c.id != target.id && !c.implied_name && c.name.as_deref() == Some(n)) {
            return Err(format!("the owner already has a member named '{n}'. Pick another name.").into());
        }
        // References a nearer name hid do not resolve, so the rename would skip them: make them resolve first.
        let (own, prefix) = (qn.clone(), format!("{qn}::"));
        let n_fixed = repair_names(work, &[], Some(&|t: &Element| t.qualified_name == own || t.qualified_name.starts_with(&prefix)));
        if n_fixed > 0 {
            changed.push("shadowed names made explicit");
        }
        step(work, EditOp::Rename { target: qn.clone(), new_name: n.to_string() })?;
        new_ref = match target.owner {
            Some(o) => format!("{}::{}", model.elements[o].qualified_name, quote_name(n)),
            None => quote_name(n),
        };
        events.push(IdEvent::Rename { from: qn.clone(), to: new_ref.clone() });
        changed.push("rename");
    }
    let after = model_of(work);
    let e = after.find(&new_ref).ok_or_else(|| Fail::from(format!("after the edit '{new_ref}' was not found; nothing was written")))?;
    Ok(json!({ "ref": new_ref, "previous_ref": qn, "changed": changed, "element": super::brief(&after, e) }))
}

/// Give an unnamed connector, flow, transition, succession, binding, allocation, dependency, message or comment a
/// name: the name goes where the statement's grammar puts it. Its ref changes from `Owner::$n` to `Owner::name`.
fn name_statement(work: &mut Vec<FileIn>, model: &Model, t: &Element, name: &str, events: &mut Vec<IdEvent>) -> Res<Value> {
    let qn = t.qualified_name.clone();
    if t.is_def || t.implied_name || !CLEARABLE.contains(&t.kind.as_str()) {
        return Err(format!("'{qn}' is a {} with no name, and only these may be named after the fact: {}. Fix: use edit_model replace.", t.keyword(), CLEARABLE.join(", ")).into());
    }
    clean_name("name", name)?;
    if let Some(d) = t.owner.map(|o| model.children(o)).into_iter().flatten().find(|c| c.id != t.id && !c.implied_name && c.name.as_deref() == Some(name)) {
        return Err(format!("the owner already has a member named '{name}' ('{}'). Fix: pass another name.", d.qualified_name).into());
    }
    let path = model.files[t.file].path.clone();
    let file = work.iter_mut().find(|f| f.path == path).ok_or("file vanished")?;
    let text = file.text.get(t.span.start..t.span.end).ok_or("statement text not found")?.to_string();
    // Skip prefixes such as `#SysMLv1::ItemFlow ` and modifiers, then find the keyword that opens the statement.
    let mut at = 0;
    let (mut head, mut tail_kw): (usize, &str) = (0, "");
    let words = ["connection", "flow", "transition", "succession", "dependency", "message", "interface", "allocation", "comment", "binding"];
    let mut found = false;
    for (i, _) in text.char_indices() {
        if i < at {
            continue;
        }
        let rest = &text[i..];
        let boundary = i == 0 || !text[..i].chars().next_back().map(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '\'').unwrap_or(false);
        if !boundary {
            continue;
        }
        if let Some(w) = words.iter().find(|w| rest.starts_with(**w) && !rest[w.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == ':')) {
            head = i + w.len();
            tail_kw = w;
            found = true;
            break;
        }
        for (bare, long) in [("bind", "binding"), ("allocate", "allocation"), ("first", "succession"), ("connect", "connection")] {
            if rest.starts_with(bare) && !rest[bare.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == ':') {
                head = i;
                tail_kw = long;
                found = true;
                break;
            }
        }
        if found {
            break;
        }
        at = i + 1;
    }
    if !found {
        return Err(format!("could not find where the name goes in the statement of '{qn}'. Fix: use edit_model replace.").into());
    }
    let n = quote_name(name);
    let (start, ins) = if matches!(tail_kw, "binding" | "allocation" | "succession" | "connection") && !text[head..].starts_with(tail_kw) && !text[..head].ends_with(tail_kw) {
        // The statement is written without its long keyword (`bind a = b;`, `allocate a to b;`, `first a then b;`,
        // `connect a to b;`): the keyword and the name go before it (`connection c connect a to b;`).
        let bare = &text[head..];
        let word = match tail_kw {
            "binding" => "bind",
            "allocation" => "allocate",
            "connection" => "connect",
            _ => "first",
        };
        debug_assert!(bare.starts_with(word));
        (head, format!("{tail_kw} {n} "))
    } else {
        (head, format!(" {n}"))
    };
    let abs = t.span.start + start;
    file.text.insert_str(abs, &ins);
    let new_ref = match t.owner {
        Some(o) => format!("{}::{}", model.elements[o].qualified_name, n),
        None => n.clone(),
    };
    events.push(IdEvent::Rename { from: qn.clone(), to: new_ref.clone() });
    let after = model_of(work);
    let e = after.find(&new_ref).ok_or_else(|| Fail::from(format!("after the edit '{new_ref}' was not found; nothing was written. The statement now reads: {}", ins.trim())))?;
    Ok(json!({ "ref": new_ref, "previous_ref": qn, "changed": ["name"], "element": super::brief(&after, e) }))
}

/// Statements and members that may be anonymous: `update_element {rename: ""}` removes their name.
const CLEARABLE: &[&str] = &["connection", "interface", "flow", "message", "allocation", "binding", "succession", "transition", "dependency", "comment"];

/// Remove the name of a connector, flow, allocation or other member that may be anonymous.
/// Its ref becomes the positional `Owner::$n`.
fn clear_name(work: &mut Vec<FileIn>, model: &Model, t: &Element, events: &mut Vec<IdEvent>) -> Res<Value> {
    let qn = t.qualified_name.clone();
    let (Some(ns), false) = (t.name_span, t.name.is_none() || t.implied_name) else {
        return Err(format!("'{qn}' has no declared name to clear.").into());
    };
    if t.is_def || !CLEARABLE.contains(&t.kind.as_str()) {
        return Err(format!("a {} needs its name. Only these may be anonymous: {}. Fix: pass another name with rename.", t.keyword(), CLEARABLE.join(", ")).into());
    }
    let refs = referrers(model, t.id, false);
    if !refs.is_empty() {
        let names = refs.iter().take(5).map(|(o, _)| model.elements[*o].qualified_name.clone()).collect::<Vec<_>>().join(", ");
        return Err(Fail::with(format!("refused: {} element(s) refer to '{qn}' by name: {names}. Change them first.", refs.len()), json!({ "referrers": refs.len() })));
    }
    let path = model.files[t.file].path.clone();
    let f = work.iter_mut().find(|f| f.path == path).ok_or("file vanished")?;
    let text = &f.text;
    let (from, to) = if text.get(ns.end..).map(|r| r.starts_with(' ')).unwrap_or(false) {
        (ns.start, ns.end + 1)
    } else if text.get(..ns.start).map(|r| r.ends_with(' ')).unwrap_or(false) {
        (ns.start - 1, ns.end)
    } else {
        (ns.start, ns.end)
    };
    if text.get(from..to).is_none() {
        return Err(Fail::from("the name is not inside the file text; read the model again and retry"));
    }
    f.text.replace_range(from..to, "");
    let (owner, index) = ids::place_of(model, t);
    let new_ref = match &owner {
        Some(o) => format!("{o}::${}", index + 1),
        None => format!("${}", index + 1),
    };
    events.push(IdEvent::Rename { from: qn.clone(), to: new_ref.clone() });
    let after = model_of(work);
    let e = after.find(&new_ref).filter(|e| e.kind == t.kind && e.name.is_none()).ok_or_else(|| Fail::from(format!("after the edit '{new_ref}' was not found as an anonymous {}; nothing was written", t.kind)))?;
    Ok(json!({ "ref": new_ref, "previous_ref": qn, "changed": ["rename"], "element": super::brief(&after, e) }))
}

/// Move any element or statement, named or unnamed, under another owner: the text is cut out and written at the end of
/// the new owner, in one write. The element keeps its source ids and so does everything inside it. A move that would leave
/// a name unresolved (other text refers to the element by the old qualified name, or the element refers to names only the old
/// owner sees) is refused and writes nothing.
pub(crate) fn move_element(work: &mut Vec<FileIn>, model: &Model, t: &Element, new_owner: &str, events: &mut Vec<IdEvent>) -> Res<Value> {
    let qn = t.qualified_name.clone();
    let o = locate(model, new_owner)?;
    if t.owner == Some(o.id) {
        return Ok(json!({ "ref": qn, "previous_ref": qn, "changed": [], "how": "already in that owner", "element": super::brief(model, t) }));
    }
    if std::iter::successors(Some(o.id), |i| model.elements[*i].owner).any(|i| i == t.id) {
        return Err(format!("'{}' is '{qn}' or lives inside it, so '{qn}' cannot move there.", o.qualified_name).into());
    }
    if matches!(t.kind.as_str(), "doc" | "import") {
        return Err(format!("a {} is part of its owner and cannot move alone. Fix: change it with update_element {{doc}} or edit_model.", t.kind).into());
    }
    // Cameo refuses a drop on an owner that has no feature for the element (a port on a package); so does the move.
    if let Some((rule, message, fix)) = super::ownership::misplaced_in(model, t, Some(o.id)).filter(|(r, _, _)| super::ownership::is_error(r)) {
        return Err(Fail::with(format!("refused: after the move, {}{message} Nothing was written. Fix: {fix}", message.get(..1).map(str::to_lowercase).unwrap_or_default(), message = message.get(1..).unwrap_or("")), json!({ "rule": rule, "element": qn, "owner": o.qualified_name })));
    }
    if let Some(n) = t.name.as_deref().filter(|_| !t.implied_name) {
        if let Some(d) = model.children(o.id).find(|c| !c.implied_name && c.name.as_deref() == Some(n)) {
            return Err(format!("'{}' already has a member named '{n}' ('{}'). Fix: rename one of them first.", o.qualified_name, d.qualified_name).into());
        }
    }
    let src_text = model.files[t.file].text.get(t.span.start..t.span.end).ok_or("the element is not inside the file text; read the model again and retry")?.to_string();
    // The lines after the first keep their place relative to the first: the text goes in at the new owner's depth.
    let src_text = dedent_after_first(&src_text, &model.files[t.file].text[..t.span.start]);
    let (old_owner, old_index) = ids::place_of(model, t);
    // The new owner may sit after the element among unnamed siblings and be renumbered by the removal.
    let gone = IdEvent::Delete { qn: qn.clone(), owner: old_owner.clone(), index: old_index };
    let owner_after = ids::step(&o.qualified_name, &gone).ok_or("the new owner vanished")?;
    let snapshot = work.clone();
    let unresolved = |m: &Model| m.diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).count();
    step(work, EditOp::Delete { target: qn.clone() })?;
    step(work, EditOp::AddElement { owner: Some(owner_after.clone()), file: None, text: src_text })?;
    repair_names(work, &snapshot, None);
    let after = model_of(work);
    let moved = after
        .find(&owner_after)
        .and_then(|oa| oa.children.last())
        .map(|c| &after.elements[*c])
        .filter(|m| m.kind == t.kind && m.is_def == t.is_def && m.name == t.name)
        .ok_or_else(|| Fail::from(format!("after the move the element was not found at the end of '{}'; nothing was written", o.qualified_name)))?;
    if after.descendants(moved.id).len() != model.descendants(t.id).len() {
        return Err(Fail::from(format!("the moved text of '{qn}' did not read back with the same members; nothing was written")));
    }
    // Cameo's references point at the element, so a move changes nothing else; here the names that pointed at it follow.
    let moved_qn = moved.qualified_name.clone();
    let followed = follow_moved(work, model, t.id, &gone, &moved_qn);
    let after = if followed > 0 { model_of(work) } else { after };
    let moved = after.find(&moved_qn).ok_or_else(|| Fail::from(format!("after the move '{moved_qn}' was not found; nothing was written")))?;
    if let Some(n) = DEFERRED_MOVES.with(Cell::get) {
        DEFERRED_MOVES.with(|c| c.set(Some(n + 1)));
    } else if unresolved(&after) > unresolved(model) {
        return Err(format!(
            "refused: moving '{qn}' under '{}' would leave {} name(s) unresolved, because other text refers to it by '{qn}', or its own text uses names only its old owner sees. Fix: update those references first (the element is '{}' after the move). Nothing was written.",
            o.qualified_name,
            unresolved(&after) - unresolved(model),
            moved.qualified_name
        )
        .into());
    }
    let to = moved.qualified_name.clone();
    events.push(IdEvent::Move { from: qn.clone(), to: to.clone(), owner: old_owner, index: old_index });
    let mut info = json!({ "ref": to, "previous_ref": qn, "changed": ["owner"], "how": "moved to the end of the new owner", "element": super::brief(&after, moved) });
    if followed > 0 {
        info["references_followed"] = json!(followed);
    }
    Ok(info)
}

/// `text` starts at a declaration; `before` is the file up to it. The indentation of the declaration's line is taken
/// off every later line that has it, so the text can be indented again where it goes.
pub(crate) fn dedent_after_first(text: &str, before: &str) -> String {
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let lead: String = before[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    if lead.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for (i, l) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(l.strip_prefix(lead.as_str()).unwrap_or(l));
        } else {
            out.push_str(l);
        }
    }
    out
}

/// After a move: every reference from outside the moved element to it, or to an element inside it, that no longer names
/// it is rewritten to the shortest name that does from where it stands. `gone` is the removal of the element from its
/// old owner (positional names of unnamed siblings shift); `to` its qualified name now. Returns the count rewritten.
fn follow_moved(work: &mut [FileIn], before: &Model, root: ElementId, gone: &IdEvent, to: &str) -> usize {
    let inside = subtree_ids(before, root);
    let old_root = before.elements[root].qualified_name.clone();
    // (referrer after the move, ref index, text, target after the move)
    let mut recs: Vec<(String, usize, String, String)> = Vec::new();
    for e in before.elements.iter().filter(|e| !inside.contains(&e.id)) {
        for (i, r) in e.refs.iter().enumerate() {
            let Some(t) = r.resolved().filter(|t| inside.contains(t)) else { continue };
            if r.text.contains('.') || matches!(r.role, RefRole::Expr | RefRole::Call | RefRole::Assign) {
                continue;
            }
            let tq = &before.elements[t].qualified_name;
            let Some(rest) = tq.strip_prefix(old_root.as_str()) else { continue };
            let Some(referrer) = ids::step(&e.qualified_name, gone) else { continue };
            recs.push((referrer, i, r.text.clone(), format!("{to}{rest}")));
        }
    }
    if recs.is_empty() {
        return 0;
    }
    let after = model_of(work);
    let mut queries: Vec<(ElementId, RefRole, String)> = Vec::new();
    let mut plans: Vec<(usize, Span, ElementId, std::ops::Range<usize>)> = Vec::new();
    for (referrer, i, text, target) in &recs {
        let (Some(e), Some(t)) = (after.find(referrer), after.find(target)) else { continue };
        let Some(r) = e.refs.get(*i).filter(|r| r.text == *text) else { continue };
        if r.resolved() == Some(t.id) && r.segments.iter().all(|s| s.resolved.is_some()) {
            continue;
        }
        let segs = crate::resolve::split_name(&t.qualified_name);
        let from = queries.len();
        for k in (0..segs.len()).rev() {
            queries.push((e.id, r.role, segs[k..].iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("::")));
        }
        plans.push((e.file, r.span, t.id, from..queries.len()));
    }
    if plans.is_empty() {
        return 0;
    }
    let got = crate::resolve::resolve_texts(&after, &queries);
    let mut by_file: HashMap<usize, Vec<(Span, String)>> = HashMap::new();
    for (file, span, target, range) in plans {
        if let Some(k) = range.clone().find(|k| got[*k] == Some(target)) {
            by_file.entry(file).or_default().push((span, queries[k].2.clone()));
        }
    }
    let mut n = 0;
    for (file, mut edits) in by_file {
        let path = after.files[file].path.clone();
        let Some(f) = work.iter_mut().find(|f| f.path == path) else { continue };
        edits.sort_by_key(|(sp, _)| std::cmp::Reverse(sp.start));
        edits.dedup_by_key(|(sp, _)| sp.start);
        for (sp, t) in edits {
            if sp.end <= f.text.len() && f.text.is_char_boundary(sp.start) && f.text.is_char_boundary(sp.end) {
                f.text.replace_range(sp.start..sp.end, &t);
                n += 1;
            }
        }
    }
    n
}

/// Move a top-level package under another package. In the same file the text moves into the owner. In
/// another file the package stays there and is wrapped in the owner's packages (a package may be split
/// over files), so no file is rewritten except the one that holds it.
fn move_package(work: &mut Vec<FileIn>, model: &Model, t: &Element, new_owner: &str, events: &mut Vec<IdEvent>) -> Res<Value> {
    let qn = t.qualified_name.clone();
    if t.kind != "package" || t.is_def || t.owner.is_some() {
        return Err(format!("owner moves a top-level package, but '{qn}' is a {}{}. Fix: only a top-level package can change owner.", t.keyword(), if t.owner.is_some() { " inside another element" } else { "" }).into());
    }
    let o = locate(model, new_owner)?;
    let inside = std::iter::successors(Some(o.id), |i| model.elements[*i].owner).any(|i| i == t.id);
    if inside {
        return Err(format!("'{}' is '{qn}' or lives inside it, so the package cannot move there.", o.qualified_name).into());
    }
    let chain = package_chain(model, o).map_err(|f| Fail::from(format!("the new owner must be a package (or packages inside packages). {}", f.message)))?;
    let name = t.name.clone().unwrap_or_default();
    let to = format!("{}::{}", o.qualified_name, quote_name(&name));
    if model.find(&to).is_some() {
        return Err(format!("'{}' already has a member named '{name}'. Fix: rename one of them first.", o.qualified_name).into());
    }
    let src_text = model.files[t.file].text.get(t.span.start..t.span.end).ok_or("the package is not inside the file text; read the model again and retry")?.to_string();
    let unresolved = |m: &Model| m.diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).count();
    let mut info_note = "wrapped in place";
    if t.file == o.file {
        step(work, EditOp::Delete { target: qn.clone() })?;
        step(work, EditOp::AddElement { owner: Some(o.qualified_name.clone()), file: None, text: src_text })?;
        info_note = "moved into the owner";
    } else {
        let path = model.files[t.file].path.clone();
        let f = work.iter_mut().find(|f| f.path == path).ok_or("file vanished")?;
        if f.text.get(t.span.start..t.span.end).is_none() {
            return Err(Fail::from("the package is not inside the file text; read the model again and retry"));
        }
        f.text.replace_range(t.span.start..t.span.end, &wrap_in_packages(&chain, &src_text));
    }
    let after = model_of(work);
    let e = after.find(&to).filter(|e| e.kind == "package").ok_or_else(|| Fail::from(format!("after the move '{to}' was not found; nothing was written")))?;
    if let Some(n) = DEFERRED_MOVES.with(Cell::get) {
        DEFERRED_MOVES.with(|c| c.set(Some(n + 1)));
    } else if unresolved(&after) > unresolved(model) {
        return Err(format!("refused: moving '{qn}' under '{}' would leave {} name(s) unresolved, because other text refers to it as '{qn}'. Fix: update those references first (they now need the qualified name '{to}'). Nothing was written.", o.qualified_name, unresolved(&after) - unresolved(model)).into());
    }
    events.push(IdEvent::Rename { from: qn.clone(), to: to.clone() });
    Ok(json!({ "ref": to, "previous_ref": qn, "changed": ["owner"], "how": info_note, "element": super::brief(&after, e) }))
}

// ---- delete_element -------------------------------------------------------

/// A statement that says how two elements relate: when an end of it goes, it dangles.
fn is_relationship_statement(e: &Element) -> bool {
    if e.is_def {
        return false;
    }
    matches!(e.kind.as_str(), "connection" | "interface" | "flow" | "message" | "binding" | "succession" | "transition" | "allocation" | "dependency")
        || e.kind.starts_with("satisfy")
        || e.kind.starts_with("verify")
        || matches!(e.kind.as_str(), "perform" | "exhibit" | "include" | "alias")
}

/// What would dangle once `doomed` (and everything inside) is gone, to a fixed point: relationship statements
/// that name a deleted element, and flows whose connection is deleted. Each with the reason.
fn dangling_after(model: &Model, doomed: &BTreeSet<ElementId>, keep: &BTreeSet<ElementId>) -> Vec<(ElementId, String)> {
    let mut gone: HashSet<ElementId> = HashSet::new();
    for d in doomed {
        gone.insert(*d);
        gone.extend(model.descendants(*d));
    }
    let carriers = connect::flow_carriers(model);
    let mut out: Vec<(ElementId, String)> = Vec::new();
    for _ in 0..8 {
        let mut more: Vec<(ElementId, String)> = Vec::new();
        for e in model.elements.iter().filter(|e| !gone.contains(&e.id) && !keep.contains(&e.id)) {
            if let Some(c) = carriers.get(&e.id).filter(|c| gone.contains(c)) {
                more.push((e.id, format!("flow on the deleted connection {}", qshow(&model.elements[*c].qualified_name))));
                continue;
            }
            if !is_relationship_statement(e) {
                continue;
            }
            let hit = e.refs.iter().flat_map(|r| r.segments.iter().filter_map(|s| s.resolved)).find(|x| gone.contains(x));
            if let Some(x) = hit {
                more.push((e.id, format!("names the deleted {}", qshow(&model.elements[x].qualified_name))));
            }
        }
        if more.is_empty() {
            break;
        }
        for (id, _) in &more {
            gone.insert(*id);
            gone.extend(model.descendants(*id));
        }
        out.extend(more);
    }
    out
}

fn delete_element(work: &mut Vec<FileIn>, a: &Value, events: &mut Vec<IdEvent>) -> Res<Value> {
    let model = model_of(work);
    let target = locate(&model, req(a, "ref")?)?;
    let cascade = a.get("cascade").and_then(Value::as_bool).unwrap_or(false);
    let refs = referrers(&model, target.id, false);
    let ancestors: BTreeSet<ElementId> = std::iter::successors(target.owner, |o| model.elements[*o].owner).collect();
    // A step turned into a call keeps its name for the pins of the called activity: when the owner's type or general
    // has a member of the same name, a reference by name moves to it after the delete, so it does not block.
    let twin = target.name.as_deref().and_then(|n| {
        let owner = target.owner?;
        model.elements[owner]
            .refs
            .iter()
            .filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes))
            .filter_map(|r| r.resolved())
            .flat_map(|t| crate::agent::text::all_members(&model, t))
            .find(|m| *m != target.id && model.elements[*m].name.as_deref() == Some(n) && !model.elements[*m].is_def)
    });
    let mut blocking: Vec<(ElementId, RefRole)> = refs.iter().copied().filter(|(o, role)| !ancestors.contains(o) && !(twin.is_some() && *role != RefRole::Type)).collect();
    blocking.sort_by_key(|(o, _)| *o);
    let list: Vec<Value> = blocking
        .iter()
        .map(|(o, role)| {
            let mut b = super::brief(&model, &model.elements[*o]);
            b["role"] = json!(role);
            b
        })
        .collect();
    // Cameo keeps a property typed by a deleted block and clears its type, so a cascade does too: a named usage that
    // refers to the element only as its type, and holds nothing that names a member of it, loses the type and stays.
    let tree: HashSet<ElementId> = std::iter::once(target.id).chain(model.descendants(target.id)).collect();
    let untype: BTreeSet<ElementId> = blocking
        .iter()
        .map(|(o, _)| *o)
        .filter(|o| {
            let e = &model.elements[*o];
            let only_type = blocking.iter().filter(|(x, _)| x == o).all(|(_, r)| *r == RefRole::Type);
            let inner = std::iter::once(*o).chain(model.descendants(*o)).flat_map(|d| model.elements[d].refs.iter()).filter(|r| !(r.role == RefRole::Type && r.resolved() == Some(target.id))).flat_map(|r| r.segments.iter().filter_map(|s| s.resolved)).any(|x| tree.contains(&x));
            only_type && !inner && !e.is_def && e.name.is_some() && !e.implied_name && !is_relationship_statement(e)
        })
        .collect();
    // What a cascade removes besides the element: every other referrer, then what would dangle after those are gone
    // (a flow on a deleted connection, a satisfy or allocate that names a deleted usage).
    let mut first: BTreeSet<ElementId> = std::iter::once(target.id).chain(blocking.iter().map(|(o, _)| *o).filter(|o| !untype.contains(o))).collect();
    // An operation's method is the action def beside it that types it. A cascade takes the method with the
    // operation (re-creating the operation would find the old method's name taken), when only the operation and
    // relationship statements refer to it.
    let method = (cascade && target.kind == "action" && !target.is_def)
        .then(|| target.first_resolved(RefRole::Type))
        .flatten()
        .filter(|d| model.elements[*d].kind == "action" && model.elements[*d].is_def && model.elements[*d].owner == target.owner)
        .filter(|d| {
            let inside: HashSet<ElementId> = std::iter::once(*d).chain(model.descendants(*d)).collect();
            referrers(&model, *d, false).iter().all(|(o, _)| *o == target.id || inside.contains(o) || is_relationship_statement(&model.elements[*o]))
        });
    if let Some(d) = method {
        first.insert(d);
    }
    let extra = dangling_after(&model, &first, &ancestors);
    let mut seen: BTreeSet<ElementId> = BTreeSet::new();
    let mut would: Vec<Value> = blocking
        .iter()
        .filter(|(o, _)| !untype.contains(o) && seen.insert(*o))
        .map(|(o, _)| {
            let e = &model.elements[*o];
            json!({ "ref": e.qualified_name, "kind": e.keyword(), "why": format!("refers to {}", qshow(&target.qualified_name)) })
        })
        .collect();
    let untyped: Vec<Value> = untype.iter().map(|o| json!({ "ref": model.elements[*o].qualified_name, "kind": model.elements[*o].keyword(), "why": format!("typed by {}: keeps its place and loses the type, as in Cameo", qshow(&target.qualified_name)) })).collect();
    if let Some(d) = method {
        would.push(json!({ "ref": model.elements[d].qualified_name, "kind": model.elements[d].keyword(), "why": format!("the method of {}", qshow(&target.qualified_name)) }));
    }
    would.extend(extra.iter().map(|(o, why)| json!({ "ref": model.elements[*o].qualified_name, "kind": model.elements[*o].keyword(), "why": why })));
    if !blocking.is_empty() && !cascade {
        let names = blocking.iter().take(10).map(|(o, _)| format!("{} [{}]", model.elements[*o].qualified_name, model.elements[*o].keyword())).collect::<Vec<_>>().join("; ");
        return Err(Fail::with(
            format!(
                "refused: {} element(s) still refer to '{}': {names}{}. Delete or change them first, or pass cascade: true ({} elements and statements are deleted with it, detail.cascade_would_remove lists them; {} usages typed by it stay and lose the type, as in Cameo, detail.cascade_would_untype lists them).",
                blocking.len(),
                target.qualified_name,
                if blocking.len() > 10 { "; ..." } else { "" },
                would.len(),
                untyped.len()
            ),
            json!({ "ref": target.qualified_name, "referrers": list, "cascade_would_remove": would, "cascade_would_untype": untyped, "cascade_count": would.len() }),
        ));
    }
    if would.len() > 1000 {
        return Err(Fail::from(format!("refused: {} elements refer to '{}' or would dangle; a cascade deletes at most 1000. Fix: delete them in groups, or change what refers to it.", would.len(), target.qualified_name)));
    }
    // Delete later text first so the numbering of unnamed siblings stays valid.
    let mut doomed: Vec<ElementId> = first.iter().copied().collect();
    if cascade {
        doomed.extend(extra.iter().map(|(o, _)| *o));
    }
    doomed.sort();
    doomed.dedup();
    let set: BTreeSet<ElementId> = doomed.iter().copied().collect();
    doomed.retain(|d| !std::iter::successors(model.elements[*d].owner, |o| model.elements[*o].owner).any(|o| set.contains(&o)));
    doomed.sort_by_key(|d| (model.elements[*d].file, std::cmp::Reverse(model.elements[*d].span.start)));
    let names: Vec<String> = doomed.iter().map(|d| model.elements[*d].qualified_name.clone()).collect();
    for d in &doomed {
        let (owner, index) = ids::place_of(&model, &model.elements[*d]);
        events.push(IdEvent::Delete { qn: model.elements[*d].qualified_name.clone(), owner, index });
    }
    if names.len() > 1 {
        // One pass over the text: reading the model again after each of many deletes costs a parse per delete.
        let targets: Vec<&str> = names.iter().map(String::as_str).collect();
        for c in edit::delete_many(&model, &targets)? {
            if let Some(f) = work.iter_mut().find(|f| f.path == c.path) {
                f.text = c.text;
            }
        }
    } else {
        for n in &names {
            step(work, EditOp::Delete { target: n.clone() })?;
        }
    }
    // The untyped usages are named, so their qualified names survive the deletes before them.
    let kept: Vec<String> = untype.iter().filter(|o| !std::iter::successors(model.elements[**o].owner, |x| model.elements[*x].owner).any(|x| set.contains(&x))).map(|o| model.elements[*o].qualified_name.clone()).collect();
    for k in &kept {
        step(work, EditOp::ClearType { target: k.clone() })?;
    }
    let mut info = json!({ "deleted": names, "referrers": list });
    if cascade {
        info["cascaded"] = json!(would);
        info["untyped"] = json!(untyped);
    } else {
        // Nothing refers to the element by name, yet something is left without what it ran on.
        info["cascade_would_remove"] = json!(would);
        if !would.is_empty() {
            let rows: Vec<String> = would.iter().take(5).map(|w| format!("{} ({})", w["ref"].as_str().unwrap_or(""), w["why"].as_str().unwrap_or(""))).collect();
            info["warnings"] = json!([{
                "rule": "dangling-after-delete",
                "message": format!("{} statement(s) are left dangling: {}{}.", would.len(), rows.join("; "), if would.len() > 5 { "; ..." } else { "" }),
                "fix": "Pass cascade: true to delete them in the same write, or delete them yourself. change_report.dangling lists them.",
            }]);
        }
    }
    Ok(info)
}

// ---- ends written relative to their owner, and duplicate statements ----------

/// The segments of a qualified name, with quotes removed. `::` inside quotes does not split.
fn name_segs(q: &str) -> Vec<String> {
    crate::resolve::split_name(q)
}

/// How a statement that sits in `home` writes an end: the path below `home` with dots (`a.p`), which is what
/// the views draw. The first step may also be a member `home` inherits through its types and supertypes
/// (`Pump::suction::suction` written in a specialization of `Pump` is `suction.suction`). An end that is no
/// feature chain and does not live in `home` keeps its qualified name. A feature chain that `home` cannot reach
/// is refused, because the statement would not draw.
pub(super) fn end_text(model: &Model, home: &Element, l: &Located) -> Res<String> {
    end_text_opt(model, home, l, false).map(|(t, _)| t)
}

/// `end_text`, and with `unreachable_ok` a feature chain that `home` cannot reach is written as given: the
/// shortest qualified name that resolves from `home` to the element the chain starts at, then the steps with
/// dots, which is what `export_facts` prints in `ends.*.text`. The flag in the reply is true for such an end.
fn end_text_opt(model: &Model, home: &Element, l: &Located, unreachable_ok: bool) -> Res<(String, bool)> {
    let (segs, hs) = (name_segs(&l.text), name_segs(&home.qualified_name));
    if segs.len() > hs.len() && segs[..hs.len()] == hs[..] {
        return Ok((segs[hs.len()..].iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("."), false));
    }
    // The shortest prefix that names a member of `home` (declared or inherited), then the rest as steps.
    for k in 1..=segs.len() {
        let prefix = segs[..k].iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("::");
        let Some(p) = model.find(&prefix) else { continue };
        let Some(name) = p.name.as_deref().filter(|_| !p.implied_name) else { continue };
        if p.owner.is_some() && p.id != home.id && member_named(model, home, name, &mut BTreeSet::new()).map(|m| m.id) == Some(p.id) {
            return Ok((std::iter::once(quote_name(name)).chain(segs[k..].iter().map(|s| quote_name(s))).collect::<Vec<_>>().join("."), false));
        }
    }
    // A member the owner inherits but its own same-named member hides: the qualified name of the inherited one, then the steps.
    let up = supertypes(model, home.id);
    for k in 1..segs.len() {
        let prefix = segs[..k].iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("::");
        if model.find(&prefix).is_some_and(|p| p.owner.map(|o| up.contains(&o)).unwrap_or(false)) {
            return Ok((format!("{prefix}.{}", segs[k..].iter().map(|s| quote_name(s)).collect::<Vec<_>>().join(".")), false));
        }
    }
    if l.anchor.id != l.el.id && unreachable_ok {
        let anchor_segs = name_segs(&l.anchor.qualified_name);
        let rest = segs.get(anchor_segs.len()..).unwrap_or(&[]);
        let quoted: Vec<String> = anchor_segs.iter().map(|s| quote_name(s)).collect();
        // The shortest suffix of the anchor's name that still resolves from `home` to the anchor.
        let short = (0..quoted.len())
            .rev()
            .map(|k| quoted[k..].join("::"))
            .find(|cand| crate::resolve::resolve_texts(model, &[(home.id, RefRole::Expr, cand.clone())]).first().copied().flatten() == Some(l.anchor.id))
            .unwrap_or_else(|| quoted.join("::"));
        let text = std::iter::once(short).chain(rest.iter().map(|s| quote_name(s))).collect::<Vec<_>>().join(".");
        return Ok((text, true));
    }
    if l.anchor.id != l.el.id {
        return Err(Fail::with(
            format!(
                "the end '{}' is a feature chain that '{}' cannot reach: its first element is not a member of '{}', declared or inherited through its types and supertypes, so a connector there would not draw. Fix: pass owner, an element that holds or inherits '{}', or name the end from there. For connect only: pass allow_unreachable_ends: true to write the ends as given; the interconnection view will not draw that connector.",
                l.text,
                home.qualified_name,
                home.qualified_name,
                l.anchor.qualified_name
            ),
            json!({ "ref": l.text, "owner": home.qualified_name }),
        ));
    }
    Ok((segs.iter().map(|s| quote_name(s)).collect::<Vec<_>>().join("::"), false))
}

/// `locate_ref`, and with `lenient` also the text `export_facts` prints in `ends.*.text`: dots between the steps of a
/// chain, and a name written relative to the owner (`'HSUV Instance Values'::T::testVehicle1.bk` seen from the
/// element that holds the statement, or from any element above it).
fn locate_end<'a>(model: &'a Model, text: &str, scope: Option<&Element>, lenient: bool) -> Res<Located<'a>> {
    // The qualified name first, then (lenient) the name seen from the scope outward, and only then an id or a printed
    // form: a name written relative to the scope keeps the meaning it had before those forms were read.
    match locate_end_named(model, text, scope, lenient) {
        Ok(l) => Ok(l),
        Err(fail) => match super::locate_end_ref(model, text) {
            Ok(l) => Ok(l),
            Err(f) if f.data["ambiguous"] == true => Err(f),
            Err(_) => Err(fail),
        },
    }
}

/// An end by its qualified name, and (lenient) by the name seen from the scope outward. No id and no printed form.
fn locate_end_named<'a>(model: &'a Model, text: &str, scope: Option<&Element>, lenient: bool) -> Res<Located<'a>> {
    let fail = match super::locate_strict(model, text) {
        Ok(l) => return Ok(l),
        Err(f) => f,
    };
    if !lenient {
        return Err(fail);
    }
    let (mut norm, mut quoted) = (String::new(), false);
    for ch in text.trim().chars() {
        match ch {
            '\'' => {
                quoted = !quoted;
                norm.push(ch);
            }
            '.' if !quoted => norm.push_str("::"),
            _ => norm.push(ch),
        }
    }
    let mut tries = vec![norm.clone()];
    if let Some(sc) = scope {
        tries.extend(std::iter::successors(Some(sc.id), |i| model.elements[*i].owner).map(|i| format!("{}::{norm}", model.elements[i].qualified_name)));
    }
    for t in tries {
        if let Ok(l) = super::locate_strict(model, &t) {
            return Ok(l);
        }
    }
    Err(fail)
}

/// `locate_end`, and when `owner` holds the statement also the name as the parser reads it there: from `owner` outward,
/// through the imports and aliases of each scope on the way (`final`, `initial`, a name written relative to a package).
/// The order is: the qualified name, the name from the scope outward, the name as the parser reads it at the owner, and
/// last an id or a printed form.
fn locate_end_from<'a>(model: &'a Model, text: &str, scope: Option<&Element>, lenient: bool, owner: Option<&Element>) -> Res<Located<'a>> {
    let Some(owner) = owner else { return locate_end(model, text, scope, lenient) };
    let fail = match locate_end_named(model, text, scope, lenient) {
        Ok(l) => return Ok(l),
        Err(f) => f,
    };
    let text = text.trim();
    let hit = crate::resolve::resolve_texts(model, &[(owner.id, RefRole::Source, text.to_string())]).into_iter().next().flatten();
    if let Some(id) = hit {
        let el = &model.elements[id];
        return Ok(Located { el, text: el.qualified_name.clone(), anchor: el });
    }
    match super::locate_end_ref(model, text) {
        Ok(l) => Ok(l),
        Err(f) if f.data["ambiguous"] == true => Err(f),
        Err(_) => Err(Fail::with(
            format!("{} It was also looked up from '{}' outward, as the parser reads a name written there, and not found.", fail.message.trim_end(), owner.qualified_name),
            fail.data.clone(),
        )),
    }
}

/// An end written as given must be a name the parser reads as one: identifiers and quoted names joined by `::` or `.`.
fn valid_end_text(t: &str) -> Result<(), &'static str> {
    if t.is_empty() || t.len() > 2000 {
        return Err("empty or longer than 2000 characters");
    }
    // The lexer's own reading of a quoted name, so the guard and the lexer cannot disagree about where it ends.
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' {
            let (end, closed) = lexer::quoted_end(b, i);
            if !closed {
                return Err(if t[i..].contains(['\n', '\r']) { "a control character or line break inside a quoted name" } else { "a quote is not closed" });
            }
            // A control character counts after a backslash too.
            if edit::name_fault(&t[i..end]).is_some() {
                return Err("a control character or line break inside a quoted name");
            }
            i = end;
        } else if c.is_ascii_alphanumeric() || b"_:.$".contains(&c) {
            i += 1;
        } else {
            return Err("a character outside a quoted name that a name cannot hold, such as a space");
        }
    }
    Ok(())
}

/// Where an end sits below `home`: the usages on the way, so two ends are equal when they name the same
/// usages, not merely the same definition feature.
fn end_path(model: &Model, home: ElementId, l: &Located) -> Vec<String> {
    let mut home_up: HashSet<ElementId> = std::iter::successors(Some(home), |i| model.elements[*i].owner).collect();
    // What `home` inherits is reached without naming its definition: `Pump::suction` seen from a specialization of Pump.
    home_up.extend(supertypes(model, home));
    let mut up: Vec<ElementId> = std::iter::successors(Some(l.anchor.id), |i| model.elements[*i].owner).collect();
    up.reverse();
    let mut path: Vec<String> = up.into_iter().skip_while(|i| home_up.contains(i)).map(|i| model.elements[i].qualified_name.clone()).collect();
    if l.anchor.id != l.el.id {
        let segs = name_segs(&l.text);
        let rest: Vec<&str> = segs.iter().skip(name_segs(&l.anchor.qualified_name).len()).map(String::as_str).collect();
        path.extend(chain_steps(model, l.anchor, &rest).into_iter().map(|m| m.qualified_name.clone()));
    }
    path
}

/// The same path for an end of a statement that is already written.
fn ref_path(model: &Model, via: &Element, role: RefRole, text: &str) -> Option<Vec<String>> {
    let r = via.refs_of(role).find(|r| r.text == text).or_else(|| via.refs_of(role).next())?;
    // `start` and `done` are the ends of a behavior, whatever an element of that name hides them with.
    if r.segments.len() == 1 && MARKERS.contains(&r.text.trim()) {
        return Some(vec![format!("~{}", r.text.trim())]);
    }
    let up: HashSet<ElementId> = via.owner.map(|o| std::iter::successors(Some(o), |i| model.elements[*i].owner).collect()).unwrap_or_default();
    let mut path = Vec::new();
    let mut leading = true;
    for s in &r.segments {
        match s.resolved {
            Some(id) if leading && up.contains(&id) => {}
            Some(id) => {
                leading = false;
                path.push(model.elements[id].qualified_name.clone());
            }
            None => {
                leading = false;
                path.push(format!("~{}", s.name));
            }
        }
    }
    Some(path)
}

fn norm_prop(v: &Element, k: &str) -> String {
    norm(v.props.get(k).map(String::as_str).unwrap_or(""))
}

/// What besides the ends tells two statements of one kind apart.
fn extra_of(model: &Model, kind: &str, v: &Element) -> String {
    let named = name_extra(v.name.as_deref().filter(|_| !v.implied_name));
    format!("{}{named}", extra_of_base(model, kind, v))
}

/// A name the statement declares tells it apart from another statement with the same ends; one rule for every
/// kind. Same name, or no name on both, is the same statement.
fn name_extra(name: Option<&str>) -> String {
    name.map(|n| format!("|n:{n}")).unwrap_or_default()
}

fn extra_of_base(model: &Model, kind: &str, v: &Element) -> String {
    match kind {
        "flow" => format!("of:{}", v.first_resolved(RefRole::Type).map(|t| model.elements[t].qualified_name.clone()).unwrap_or_default()),
        "transition" => format!("t:{}|g:{}|e:{}", norm_prop(v, "trigger"), norm_prop(v, "guard"), norm_prop(v, "effect")),
        "succession" => format!("g:{}", norm_prop(v, "guard")),
        _ => {
            let _ = model;
            String::new()
        }
    }
}

/// `owner` joins the key when an end is a name that resolves to nothing (`start`, `done`, a misspelling): the
/// same words in two owners are two statements.
fn stmt_key_of(kind: &str, s: &[String], t: &[String], extra: &str, owner: Option<String>) -> String {
    let open = s.iter().chain(t).any(|x| x.starts_with('~'));
    let at = if open { format!("@{owner:?}") } else { String::new() };
    format!("{kind}|{s:?}|{t:?}|{extra}{at}")
}

const CHAIN_KINDS: &[&str] = &["connect", "flow", "transition", "succession", "bind"];

/// A key that is equal for two statements that say the same thing. Connectors, flows, transitions, successions
/// and bindings compare the usages their ends name; the other kinds compare the elements they resolve to.
fn dup_key(model: &Model, s: &Stmt) -> Option<String> {
    let via = &model.elements[s.via];
    if via.is_def {
        return None;
    }
    if CHAIN_KINDS.contains(&s.kind) {
        let sp = ref_path(model, via, RefRole::Source, &s.source.text)?;
        let tp = ref_path(model, via, RefRole::Target, &s.target.text)?;
        return Some(stmt_key_of(s.kind, &sp, &tp, &extra_of(model, s.kind, via), via.owner.map(|o| model.elements[o].qualified_name.clone())));
    }
    // Names, not element numbers: adding an element elsewhere shifts the numbers, which made old repeats look new.
    let qn = |id: ElementId| model.elements[id].qualified_name.clone();
    // A name the statement declares tells it apart from another statement with the same ends.
    let named = via.name.as_deref().filter(|_| !via.implied_name).map(|n| format!("|n:{n}")).unwrap_or_default();
    Some(format!("{}|{}|{}{named}", s.kind, qn(s.source.id?), qn(s.target.id?)))
}

/// Refuse a repeat check on a model whose pairs were cut, by the limit for the model or by the limit for one statement:
/// the check would read part of the pairs and pass. Only a write that makes this check calls it.
fn need_whole_model(model: &Model) -> Res<()> {
    let (_, cuts) = graph::statements_cut(model);
    match graph::check_notice(&cuts) {
        None => Ok(()),
        Some(why) => Err(format!("{why} Nothing was written.").into()),
    }
}

/// The declaring statement of a statement of `kind` whose chain key is `key`.
fn find_duplicate(model: &Model, kind: &str, key: &str) -> Option<Value> {
    graph::statements(model).into_iter().filter(|s| s.kind == kind).find(|s| dup_key(model, s).as_deref() == Some(key)).map(|s| graph::declared_at(model, s.via))
}

/// Refuse text that adds a relationship statement `create_relationship` would refuse as already existing.
fn refuse_duplicate_statements(before: &Model, after: &Model) -> Res<()> {
    // Text that adds no relationship statement repeats none, so it needs no list of them and is written on a cut model too.
    let stated = |m: &Model| m.elements.iter().filter(|e| graph::is_statement(e)).count();
    if stated(after) <= stated(before) {
        return Ok(());
    }
    need_whole_model(after)?;
    let count = |m: &Model| -> HashMap<String, usize> {
        let mut c = HashMap::new();
        for s in graph::statements(m) {
            if let Some(k) = dup_key(m, &s) {
                *c.entry(k).or_default() += 1;
            }
        }
        c
    };
    let (old, new) = (count(before), count(after));
    for s in graph::statements(after) {
        let Some(k) = dup_key(after, &s) else { continue };
        if new[&k] > old.get(&k).copied().unwrap_or(0) && new[&k] > 1 {
            let at = graph::declared_at(after, s.via);
            return Err(Fail::with(
                format!(
                    "the {} from '{}' to '{}' already exists ({}:{}). Nothing was written. Fix: drop the repeated statement, or pass allow_duplicate: true when the source model has parallel statements with the same ends.",
                    s.kind,
                    s.source.text.escape_debug(),
                    s.target.text.escape_debug(),
                    at["file"].as_str().unwrap_or(""),
                    at["line"]
                ),
                json!({ "declared_at": at }),
            ));
        }
    }
    Ok(())
}

// ---- create_relationship --------------------------------------------------

fn usage_operand(model: &Model, e: &Element, kind: &str, role: &str) -> Res<()> {
    if e.is_def {
        let owner = e.owner.map(|o| model.elements[o].qualified_name.clone()).unwrap_or_else(|| "<owner>".into());
        return Err(Fail::with(
            format!(
                "{kind} needs a usage as the {role}, but {} is a definition ({}). Fix: create a usage of it first, for example create_element {{owner: {}, kind: '{}', name: '{}1', typed_by: {}}}, then pass that usage.",
                qshow(&e.qualified_name),
                e.keyword(),
                qshow(&owner),
                e.kind,
                lower_first(e.name.as_deref().unwrap_or("item")),
                qshow(&e.qualified_name)
            ),
            json!({ "rule": "trace-operand-definition", "ref": e.qualified_name }),
        ));
    }
    if e.kind == "package" {
        return Err(format!("{kind} cannot use the package {} as the {role}; name a usage inside it.", qshow(&e.qualified_name)).into());
    }
    Ok(())
}

/// An end of a connector: a usage, never a definition (the semantic gate rejects those).
fn end_usage(model: &Model, e: &Element, kind: &str, role: &str) -> Res<()> {
    if e.is_def {
        let owner = e.owner.map(|o| model.elements[o].qualified_name.clone()).unwrap_or_else(|| "<context>".into());
        return Err(Fail::with(
            format!(
                "{kind} joins usages, but the {role} {} is a definition ({}). Fix: pass the usage inside the context, for example create_element {{owner: {}, kind: '{}', name: '{}1', typed_by: {}}} and then that usage. A {kind} between definitions is rejected by the semantic gate.",
                qshow(&e.qualified_name),
                e.keyword(),
                qshow(&owner),
                e.kind,
                lower_first(e.name.as_deref().unwrap_or("item")),
                qshow(&e.qualified_name)
            ),
            json!({ "rule": "connection-between-definitions", "ref": e.qualified_name }),
        ));
    }
    if e.kind == "package" {
        return Err(format!("{kind} cannot use the package {} as the {role}; name a usage inside it.", qshow(&e.qualified_name)).into());
    }
    Ok(())
}

fn requirement_usage(model: &Model, e: &Element, kind: &str) -> Res<()> {
    usage_operand(model, e, kind, "requirement")?;
    if e.kind != "requirement" {
        return Err(format!("the {kind} target must be a requirement usage, but '{}' is a {}.", e.qualified_name, e.keyword()).into());
    }
    Ok(())
}

/// The statement of `kind` from `a` to `b` whose declaring element passes `same`.
fn exists_where(model: &Model, kind: &str, a: ElementId, b: ElementId, same: impl Fn(&Element) -> bool) -> Option<Value> {
    graph::statements(model)
        .into_iter()
        .chain(graph::structural(model))
        .find(|s| s.kind == kind && s.source.id == Some(a) && s.target.id == Some(b) && same(&model.elements[s.via]))
        .map(|s| graph::declared_at(model, s.via))
}

/// Make sure `package SysMLv1 { metadata def <tag>; }` exists, the way the importer writes it.
fn ensure_tag(work: &mut Vec<FileIn>, tag: &str, near: ElementId) -> Res<()> {
    let model = model_of(work);
    let pkg = model.elements.iter().find(|e| e.kind == "package" && e.name.as_deref() == Some("SysMLv1"));
    if let Some(p) = pkg {
        if model.children(p.id).any(|c| c.kind == "metadata" && c.name.as_deref() == Some(tag)) {
            return Ok(());
        }
        return add(work, Some(&p.qualified_name.clone()), None, &format!("metadata def {tag};"));
    }
    let mut root = near;
    while let Some(o) = model.elements[root].owner {
        root = o;
    }
    let text = format!("package SysMLv1 {{\n    metadata def {tag};\n}}");
    let r = &model.elements[root];
    if r.kind == "package" {
        add(work, Some(&r.qualified_name.clone()), None, &text)
    } else {
        add(work, None, Some(&model.files[r.file].path.clone()), &text)
    }
}

fn place(work: &mut Vec<FileIn>, model: &Model, near: &Element, text: &str) -> Res<String> {
    match near.owner {
        Some(o) => {
            let q = model.elements[o].qualified_name.clone();
            add(work, Some(&q), None, text)?;
            Ok(q)
        }
        None => {
            let path = model.files[near.file].path.clone();
            add(work, None, Some(&path), text)?;
            Ok(path)
        }
    }
}

/// The package that holds `e` (its own owner when that is a package), or `None` when `e` stands at the top of a file
/// or the nearest package is its own owner already, so writing beside `e` is the same place.
fn nearest_package<'a>(model: &'a Model, e: &Element) -> Option<&'a Element> {
    let first = e.owner?;
    let p = std::iter::successors(Some(first), |o| model.elements[*o].owner).map(|o| &model.elements[o]).find(|o| o.kind == "package")?;
    (p.id != first).then_some(p)
}

/// Write `text` into `owner` when the caller named one, else beside `near`. Returns where it went.
fn put(work: &mut Vec<FileIn>, model: &Model, near: &Element, owner: Option<&Element>, text: &str) -> Res<String> {
    match owner {
        Some(o) => {
            add(work, Some(&o.qualified_name), None, text)?;
            Ok(o.qualified_name.clone())
        }
        None => place(work, model, near, text),
    }
}

/// Where a connector or transition goes: `owner` when given, else the nearest owner both ends share.
fn home_of<'a>(model: &'a Model, a: &Value, x: &Element, y: &Element, kind: &str) -> Res<&'a Element> {
    let up = |e: &Element| std::iter::successors(e.owner, |o| model.elements[*o].owner).collect::<Vec<_>>();
    let ys = up(y);
    let home = match text_arg(a, "owner") {
        Some(o) => locate(model, o)?,
        None => up(x)
            .into_iter()
            .find(|o| ys.contains(o))
            .map(|o| &model.elements[o])
            .ok_or_else(|| Fail::from(format!("'{}' and '{}' share no owner, so the {kind} has nowhere to go. Fix: pass owner, the element that holds the {kind}.", x.qualified_name, y.qualified_name)))?,
    };
    if kind == "transition" && !home.kind.contains("state") {
        return Err(format!("a transition lives in a state, but the nearest owner of both ends is the {} '{}'. Fix: pass owner, the state (def or usage) that holds both.", home.keyword(), home.qualified_name).into());
    }
    if kind != "transition" && kind != "flow" && home.kind == "package" {
        let holder = if kind == "succession" { "action" } else { "part" };
        return Err(format!("a {kind} needs {} {holder} to live in, but the nearest owner of both ends is the package '{}'. Fix: pass owner, the {holder} (def or usage) that holds both ends.", if holder == "action" { "an" } else { "a" }, home.qualified_name).into());
    }
    Ok(home)
}

/// Trigger text after `accept`: `after N [unit]`, `when <expression>` or the name of a signal. A name is looked up the
/// way the parser looks up a reference inside the transition: from `home` (the state that holds it) outward, so the
/// text `export_facts` prints for a trigger is accepted as printed.
fn trigger_text(model: &Model, home: Option<&Element>, t: &str) -> Res<String> {
    let t = t.trim();
    let lower = t.to_lowercase();
    for word in ["after", "when"] {
        if lower.strip_prefix(word).map(|r| r.starts_with(char::is_whitespace)).unwrap_or(false) {
            // `lower` matched the ASCII word at the start, so `t` has it at the same width; stay safe if it did not.
            let rest = scan_expr(t.get(word.len()..).unwrap_or(""), CLAUSE_WORDS).map_err(|e| match e {
                ExprErr::Word => Fail::from(format!("trigger {} contains a reserved word (if, do, then, accept). Fix: rewrite the expression without it.", crate::agent::text::quote_clip(t))),
                e => expr_fail(&format!("trigger '{word} ...'"), t.get(word.len()..).unwrap_or(""), e),
            })?;
            let text = format!("{word} {rest}");
            parsed_clause("trigger", t, &text)?;
            return Ok(text);
        }
    }
    // `x : Sig` and `x : Sig via port`: a payload name, then the signal (and port) as below (`accept x : Item via port`).
    if let Some((name, rest)) = split_payload(t) {
        let rest = trigger_text(model, home, rest)?;
        let text = format!("{name} : {rest}");
        parsed_clause("trigger", t, &text)?;
        return Ok(text);
    }
    // `Sig via port`: the signal as below, then the port the signal comes in through, which must resolve from the same place.
    if let Some((sig, port)) = split_via(t) {
        let sig = trigger_text(model, home, sig)?;
        let port = scan_expr(port, CLAUSE_WORDS).map_err(|e| expr_fail("trigger port (after 'via')", port, e))?;
        let first = super::expr::names("value", &port).and_then(|n| n.names.first().filter(|x| !x.soft).map(|x| lexer::unquote(&port[x.segs[0].start..x.segs[0].end])));
        let Some(first) = first else {
            return Err(Fail::from(format!("trigger port {} (after 'via') is not the name of a port. Fix: name a port, for example 'Sig via cmdPort' or 'Sig via ctl.cmdPort'.", crate::agent::text::quote_clip(&port))));
        };
        if let Some(h) = home {
            if crate::resolve::resolve_texts(model, &[(h.id, RefRole::Expr, quote_name(&first))]).first().copied().flatten().is_none() {
                return Err(Fail::from(format!("trigger port {} (after 'via') does not resolve from {}. Fix: name a port that the state's owner has, or create the port first.", crate::agent::text::quote_clip(&first), qshow(&h.qualified_name))));
            }
        }
        let text = format!("{sig} via {port}");
        parsed_clause("trigger", t, &text)?;
        return Ok(text);
    }
    if let Some(h) = home {
        if crate::resolve::resolve_texts(model, &[(h.id, RefRole::Expr, t.to_string())]).first().copied().flatten().is_some() {
            return clause_expr("trigger", t);
        }
    }
    locate_ref(model, t).map(|l| l.text).map_err(|f| {
        Fail::with(format!("trigger {} is not 'after N [unit]', 'when <expression>' or the name of a signal that resolves from {}. {}", crate::agent::text::quote_clip(t), home.map(|h| qshow(&h.qualified_name)).unwrap_or_else(|| "the transition's owner".into()), f.message), f.data)
    })
}

/// `x : Sig ...` as (`x`, `Sig ...`): a payload name before a single colon. `None` when the text does not start so.
fn split_payload(t: &str) -> Option<(&str, &str)> {
    let (toks, errs) = lexer::lex(t);
    if !errs.is_empty() || toks.len() < 3 {
        return None;
    }
    let word = |k: &lexer::Tok| &t[k.span.start..k.span.end];
    if !matches!(toks[0].kind, lexer::TokKind::Ident) || matches!(word(&toks[0]), "after" | "at" | "when") || word(&toks[1]) != ":" {
        return None;
    }
    Some((word(&toks[0]), t[toks[1].span.end..].trim()))
}

/// `Sig via port` as (`Sig`, `port`): the two sides of the word `via` that stands outside quotes and brackets. `None` for
/// a trigger with no such word, and for a timed or a change trigger (`after`, `at`, `when`).
fn split_via(t: &str) -> Option<(&str, &str)> {
    let (toks, errs) = lexer::lex(t);
    if !errs.is_empty() {
        return None;
    }
    let word = |k: &lexer::Tok| &t[k.span.start..k.span.end];
    if toks.first().map(|k| matches!(word(k), "after" | "at" | "when")).unwrap_or(true) {
        return None;
    }
    let mut depth = 0i32;
    for k in &toks {
        match (k.kind, word(k)) {
            (lexer::TokKind::Punct, "(" | "[") => depth += 1,
            (lexer::TokKind::Punct, ")" | "]") => depth -= 1,
            (lexer::TokKind::Ident, "via") if depth == 0 => return Some((t[..k.span.start].trim(), t[k.span.end..].trim())),
            _ => {}
        }
    }
    None
}

fn clause(what: &str, v: Option<&str>) -> Res<Option<String>> {
    v.map(|v| {
        clause_expr(what, v)
    })
    .transpose()
}

const REL_KINDS: &str = "specialize, satisfy, verify, allocate, derive, refine, dependency, trace, redefine, connect, flow, succession flow, perform, transition or succession";

fn create_relationship(work: &mut Vec<FileIn>, a: &Value) -> Res<Value> {
    let model = model_of(work);
    let kind = req_as(a, "kind", &format!("the relationship kind: {REL_KINDS}"))?.to_lowercase();
    // The kind words export_facts prints: `allocation` is an allocate, `connection` a connect, `binding` a bind.
    let kind = if kind == "allocation" { "allocate".to_string() } else { kind };
    // An object flow between steps (SysML v2 and Cameo): `succession flow of X from a.p to b.q;`, a flow that is also a
    // succession. It is written and read as a flow, with the word succession before it.
    let succession_flow = matches!(kind.trim(), "succession flow" | "succession_flow" | "successionflow");
    let kind = if succession_flow { "flow".to_string() } else { kind };
    if matches!(kind.as_str(), "bind" | "binding") {
        let mut b = a.clone();
        b["kind"] = json!("bind");
        return create_element(work, &b);
    }
    if kind == "succession" {
        return create_succession(work, &model, a);
    }
    // `allocate start to X;` names the start or done of a behavior. When no element of that name is in scope, the
    // bare word is the marker, as export_facts prints it.
    if kind == "allocate" {
        let marked = |role: &str| text_arg(a, role).filter(|t| MARKERS.contains(t) && locate_ref(&model, t).is_err()).is_some();
        if marked("source") || marked("target") {
            return create_allocate_marked(work, &model, a);
        }
    }
    if kind == "succession" {
        return create_succession(work, &model, a);
    }
    if kind == "transition" && [text_arg(a, "source"), text_arg(a, "target")].iter().flatten().any(|t| MARKERS.contains(t)) {
        return create_transition_marked(work, &model, a);
    }
    // `Level :> ScalarValues::Real`: the general is a name of the standard library, which the model does not hold.
    if matches!(kind.as_str(), "specialize" | "specialization") {
        if let Some(t) = text_arg(a, "target").filter(|t| is_library_name(t) && locate_ref(&model, t).is_err()) {
            return create_specialize_library(work, &model, a, t);
        }
    }
    let lenient = flag(a, "allow_unreachable_ends");
    if lenient && !matches!(kind.as_str(), "connect" | "connection") {
        return Err(format!("allow_unreachable_ends applies only to connect. Fix: drop it; {kind} names its ends relative to its owner.").into());
    }
    let scope = if lenient { text_arg(a, "owner").and_then(|o| locate(&model, o).ok()) } else { None };
    // satisfy, allocate, derive, refine, trace and dependency write their ends inside `owner`, so a short end means what the
    // parser makes of it from there outward, aliases of the scope included.
    let inside = matches!(kind.as_str(), "satisfy" | "allocate" | "derive" | "refine" | "trace" | "dependency");
    let keep_text = flag(a, "keep_text");
    if keep_text && !inside {
        return Err(format!("keep_text applies to satisfy, allocate, derive, refine, trace and dependency. Fix: drop it; {kind} writes its ends as the tool names them.").into());
    }
    let mut end_owner: Option<&Element> = if inside { text_arg(a, "owner").map(|o| locate(&model, o)).transpose()? } else { None };
    let mut src_l = locate_end_from(&model, req(a, "source")?, scope, lenient, end_owner)?;
    let mut tgt_l = locate_end_from(&model, req(a, "target")?, scope, lenient, end_owner)?;
    // Cameo puts a new satisfy, allocate or dependency in the nearest package of its client, never in a block or an
    // activity, so with no owner given the statement goes there and names its ends from there.
    let default_home: Option<&Element> = if inside && end_owner.is_none() && !keep_text { nearest_package(&model, src_l.anchor) } else { None };
    if let Some(pkg) = default_home {
        end_owner = Some(pkg);
        src_l = locate_end_from(&model, req(a, "source")?, scope, lenient, end_owner)?;
        tgt_l = locate_end_from(&model, req(a, "target")?, scope, lenient, end_owner)?;
    }
    if keep_text {
        for (l, role) in [(&mut src_l, "source"), (&mut tgt_l, "target")] {
            let given = req(a, role)?.trim();
            valid_end_text(given).map_err(|why| Fail::from(format!("keep_text: the {role} {} cannot be written as given ({why}). Fix: quote each name that has spaces, as in 'Pkg Name'::Part, or drop keep_text.", crate::agent::text::quote_clip(given))))?;
            l.text = given.to_string();
        }
    }
    let (src, tgt) = (src_l.el, tgt_l.el);
    // Two chains through different usages of one definition end on the same definition member: they are two ends.
    let one_chain = src_l.text == tgt_l.text && src.id == tgt.id;
    // A state may leave itself and come back (`transition first S then S;`), so only a transition may join an element to itself.
    if src.id == tgt.id && kind != "transition" && (one_chain || !matches!(kind.as_str(), "connect" | "connection" | "flow" | "bind")) {
        return Err("source and target are the same element".into());
    }
    let (sq, tq) = (src_l.text.clone(), tgt_l.text.clone());
    let (sqn, tqn) = (src.qualified_name.clone(), tgt.qualified_name.clone());
    let stmt_kind = match kind.as_str() {
        "specialize" | "specialization" => "specialization",
        "satisfy" => "satisfy",
        "verify" => "verify",
        "allocate" => "allocate",
        "derive" => "derive",
        "refine" => "refine",
        "dependency" => "dependency",
        "trace" => "trace",
        "redefine" | "redefinition" => "redefinition",
        "connect" | "connection" => "connect",
        "flow" => "flow",
        "perform" => "perform",
        "transition" => "transition",
        other => return Err(format!("unknown relationship kind '{other}'. Use {REL_KINDS}.").into()),
    };
    let allowed: &[&str] = match stmt_kind {
        "flow" => &["of", "name", "owner", "metadata"],
        "connect" => &["name", "typed_by", "owner", "metadata"],
        "transition" => &["name", "trigger", "guard", "effect", "owner", "metadata", "doc"],
        "perform" => &["name"],
        "dependency" => &["owner", "name", "metadata"],
        "allocate" => &["owner", "metadata"],
        "satisfy" | "derive" | "refine" | "trace" => &["owner"],
        _ => &[],
    };
    if !allowed.contains(&"metadata") && a.get("metadata").map(|v| !v.is_null()).unwrap_or(false) {
        return Err(format!("'metadata' does not apply to {stmt_kind}. Fix: drop 'metadata'; it is used by dependency, allocate, connect, flow and transition (derive, refine and trace write their own tag).").into());
    }
    let meta = meta_list(&model, a)?;
    let mprefix: String = meta.iter().map(|m| format!("#{m} ")).collect();
    // A dependency tagged `Verify`, `Satisfy` or the like reads back as that kind of edge.
    let edge_kind: &str = if stmt_kind == "dependency" { meta.iter().find_map(|m| graph::tag_edge_kind(m)).unwrap_or("dependency") } else { stmt_kind };
    let same_meta = |v: &Element| -> bool {
        let have: Vec<String> = v.prefixes.iter().filter_map(|p| p.strip_prefix('#')).map(|p| canonical_name(p).unwrap_or_else(|| p.to_string())).collect();
        stmt_kind != "dependency" || have == meta
    };
    let allow_defs = flag(a, "allow_definitions");
    if allow_defs && !matches!(stmt_kind, "allocate" | "satisfy") {
        return Err(format!("allow_definitions applies only to allocate and satisfy. Fix: drop it; {stmt_kind} needs usages (create one with create_element {{typed_by}}).").into());
    }
    for (o, users) in [("of", "flow"), ("name", "flow, connect, transition, dependency and perform"), ("typed_by", "connect"), ("trigger", "transition"), ("guard", "transition"), ("effect", "transition"), ("owner", "satisfy, allocate, derive, refine, dependency, trace, flow, connect, transition and succession")] {
        if !allowed.contains(&o) && text_arg(a, o).is_some() {
            return Err(format!("'{o}' does not apply to {stmt_kind}. Fix: drop '{o}'; it is used by {users}.").into());
        }
    }
    if !allowed.contains(&"doc") && text_arg(a, "doc").is_some() {
        return Err(format!("'doc' does not apply to {stmt_kind}. Fix: drop 'doc' and document the statement with update_element {{ref, doc}} on the ref this call returns.").into());
    }
    let name = text_arg(a, "name");
    if let Some(n) = name {
        clean_name("name", n)?;
    }
    let of = if stmt_kind == "flow" { text_arg(a, "of").map(|t| payload_text(&model, t)).transpose()? } else { None };
    let of_id = of.as_deref().and_then(|t| model.find(t).map(|e| e.id));
    let guard = clause("guard", text_arg(a, "guard"))?;
    let effect = clause("effect", text_arg(a, "effect"))?;
    let conn_def = match (stmt_kind, text_arg(a, "typed_by")) {
        ("connect", Some(t)) => {
            let d = locate_ref(&model, t)?;
            if !d.el.is_def || d.el.kind != "connection" {
                return Err(format!("typed_by must be a connection definition, but '{}' is a {}. Fix: create one with create_element {{kind: 'connection def', name: '...'}}.", d.el.qualified_name, d.el.keyword()).into());
            }
            Some(d.text)
        }
        _ => None,
    };
    // Connectors, flows and transitions go where both ends are visible, and name their ends relative to it.
    let chain_home: Option<&Element> = if matches!(stmt_kind, "connect" | "flow" | "transition") {
        for (role, e) in [("source", src), ("target", tgt)] {
            end_usage(&model, e, stmt_kind, role)?;
            if stmt_kind == "transition" && !e.kind.contains("state") {
                return Err(format!("a transition joins states, but the {role} '{}' is a {}. Fix: pass a state usage, or create one with create_element {{kind: 'state', ...}}.", e.qualified_name, e.keyword()).into());
            }
        }
        Some(home_of(&model, a, src_l.anchor, tgt_l.anchor, stmt_kind)?)
    } else {
        None
    };
    let trigger = if stmt_kind == "transition" { text_arg(a, "trigger").map(|t| trigger_text(&model, chain_home, t)).transpose()? } else { None };
    let owner_el: Option<&Element> = if matches!(stmt_kind, "satisfy" | "allocate" | "derive" | "refine" | "dependency" | "trace") { text_arg(a, "owner").map(|o| locate(&model, o)).transpose()?.or(default_home) } else { None };
    let dup_ok = flag(a, "allow_duplicate");
    if dup_ok && flag(a, "if_absent") {
        return Err("allow_duplicate and if_absent contradict each other. Fix: pass one of them.".into());
    }
    if !dup_ok && stmt_kind != "perform" {
        need_whole_model(&model)?;
    }
    let at = if dup_ok {
        None
    } else if stmt_kind == "perform" {
        src.children.iter().map(|c| &model.elements[*c]).find(|c| c.kind.starts_with("perform") && c.refs.iter().any(|r| r.resolved() == Some(tgt.id))).map(|c| graph::declared_at(&model, c.id))
    } else if let Some(home) = chain_home {
        let extra = match stmt_kind {
            "flow" => format!("of:{}", of_id.map(|i| model.elements[i].qualified_name.clone()).unwrap_or_default()),
            "transition" => format!("t:{}|g:{}|e:{}", norm(trigger.as_deref().map(|t| t.strip_prefix("accept ").unwrap_or(t)).unwrap_or("")), norm(guard.as_deref().unwrap_or("")), norm(effect.as_deref().unwrap_or(""))),
            _ => String::new(),
        } + &name_extra(name);
        let key = stmt_key_of(stmt_kind, &end_path(&model, home.id, &src_l), &end_path(&model, home.id, &tgt_l), &extra, Some(home.qualified_name.clone()));
        find_duplicate(&model, stmt_kind, &key)
    } else {
        exists_where(&model, edge_kind, src.id, tgt.id, |v| same_meta(v) && match stmt_kind {
            "flow" => v.first_resolved(RefRole::Type) == of_id,
            // Two dependencies with the same ends and different names are two statements.
            "dependency" => v.name.as_deref().filter(|_| !v.implied_name) == name,
            "transition" => {
                let have = |k: &str| norm(v.props.get(k).map(String::as_str).unwrap_or(""));
                have("trigger") == norm(trigger.as_deref().map(|t| t.strip_prefix("accept ").unwrap_or(t)).unwrap_or(""))
                    && have("guard") == norm(guard.as_deref().unwrap_or(""))
                    && have("effect") == norm(effect.as_deref().unwrap_or(""))
            }
            _ => true,
        })
    };
    if let Some(at) = at {
        if flag(a, "if_absent") {
            return Ok(json!({ "kind": stmt_kind, "source": sq, "target": tq, "existed": true, "ref": at["element"], "declared_at": at }));
        }
        return Err(Fail::with(format!("the {stmt_kind} from '{sq}' to '{tq}' already exists. Fix: nothing to do, pass if_absent: true to make the call a no-op, or pass allow_duplicate: true when the source model really has a second one."), json!({ "declared_at": at })));
    }
    let taken = |home: &Element, n: &str| -> Res<()> {
        match model.children(home.id).find(|c| !c.implied_name && c.name.as_deref() == Some(n)) {
            Some(d) => Err(format!("'{}' already has a member named '{n}' ('{}'). Fix: pass another name.", home.qualified_name, d.qualified_name).into()),
            None => Ok(()),
        }
    };
    let mut unreachable_used: Vec<String> = Vec::new();
    // The model the statement is added to; for derive, refine and trace the one with their tag definition in it.
    let mut base = model.clone();
    let (declared_in, text) = match stmt_kind {
        "specialization" => {
            if src.is_def != tgt.is_def {
                return Err(format!("'{sq}' is a {} and '{tq}' is a {}; both must be definitions or both usages.", src.keyword(), tgt.keyword()).into());
            }
            // Cameo's customization of Requirement disallows Generalization (Requirement[4]): one requirement reuses another
            // through derive or by containment, never by specializing it.
            if let Some(r) = [src, tgt].into_iter().find(|e| e.is_def && matches!(e.kind.as_str(), "requirement" | "concern")) {
                return Err(Fail::with(
                    format!("'{}' is a {}, and Cameo does not let a requirement take part in a generalization (Requirement[4]). Fix: link the two with create_relationship {{kind: 'derive'}}, or nest one in the other.", r.qualified_name, r.keyword()),
                    json!({ "rule": "cameo-requirement-4" }),
                ));
            }
            let mut up = BTreeSet::new();
            let mut stack = vec![tgt.id];
            while let Some(x) = stack.pop() {
                if up.insert(x) {
                    stack.extend(model.elements[x].refs_of(RefRole::Specializes).filter_map(|r| r.resolved()));
                }
            }
            if up.contains(&src.id) {
                return Err(format!("'{tq}' already specializes '{sq}'; this would make a cycle.").into());
            }
            step(work, EditOp::AddSpecialization { target: sqn.clone(), general: tq.clone() })?;
            (model.files[src.file].path.clone(), format!("{sqn} :> {tq}"))
        }
        "satisfy" => {
            // A Cameo import satisfies a requirement by an activity (an action definition): `satisfy R by ActionDef;`.
            if !(allow_defs && src.is_def && src.kind != "package") {
                usage_operand(&model, src, "satisfy", "satisfying element").map_err(|mut f| {
                    if src.is_def {
                        f.message.push_str(" To satisfy by a definition as a Cameo import does, pass allow_definitions: true; validate_model then reports the rule trace-operand-definition as a warning.");
                    }
                    f
                })?;
            }
            requirement_usage(&model, tgt, "satisfy")?;
            let text = format!("satisfy {tq} by {sq};");
            (put(work, &model, src_l.anchor, owner_el, &text)?, text)
        }
        "allocate" => {
            for (e, role) in [(src, "source"), (tgt, "target")] {
                if e.kind == "package" && !e.is_def && !(allow_defs && role == "target") {
                    let hint = if role == "target" { "to allocate to the package itself, as a Cameo import does, pass allow_definitions: true; validate_model then reports the rule trace-operand-definition as a warning" } else { "a package can only be the target of an allocate" };
                    return Err(format!("allocate cannot use the package {} as the {role}; name a usage inside it. Fix: {hint}.", qshow(&e.qualified_name)).into());
                }
                if allow_defs && (e.is_def || e.kind == "package") {
                    continue;
                }
                usage_operand(&model, e, "allocate", role).map_err(|mut f| {
                    f.message.push_str(" To allocate between definitions as a Cameo import does, pass allow_definitions: true; validate_model then reports the rule trace-operand-definition as a warning.");
                    f
                })?;
            }
            let text = format!("{mprefix}allocate {sq} to {tq};");
            (put(work, &model, src_l.anchor, owner_el, &text)?, text)
        }
        "verify" => {
            requirement_usage(&model, tgt, "verify")?;
            if !src.kind.starts_with("verification") {
                return Err(Fail::with(
                    format!(
                        "verify is only legal inside the objective of a verification case, and '{sq}' is a {}. Fix: create one, for example create_element {{owner: '<package>', kind: 'verification def', name: 'Verify{}'}}, then pass it as source.",
                        src.keyword(),
                        tgt.name.clone().unwrap_or_default().replace(' ', "")
                    ),
                    json!({ "rule": "verify-outside-objective" }),
                ));
            }
            let stmt = format!("verify {tq};");
            match model.children(src.id).find(|c| c.kind == "objective") {
                Some(obj) => {
                    add(work, Some(&obj.qualified_name.clone()), None, &stmt)?;
                    (obj.qualified_name.clone(), stmt)
                }
                None => {
                    add(work, Some(&sqn), None, &format!("objective {{\n    {stmt}\n}}"))?;
                    (format!("{sqn} objective"), stmt)
                }
            }
        }
        "derive" | "refine" | "trace" => {
            let tag = match stmt_kind {
                "derive" => "DeriveReqt",
                "refine" => "Refine",
                _ => "Trace",
            };
            if stmt_kind == "derive" {
                requirement_any(src, "derive", "derived requirement")?;
            }
            // Cameo 2022x Refresh2 restricts both ends of a derive (its customization's typesForSource and typesForTarget are
            // AbstractRequirement) and no end of a refine or a trace.
            if stmt_kind == "derive" {
                requirement_any(tgt, stmt_kind, "requirement")?;
            }
            ensure_tag(work, tag, owner_el.map(|o| o.id).unwrap_or(src.id))?;
            let model = model_of(work);
            base = model.clone();
            let src = model.find(&sqn).ok_or("source vanished")?;
            let text = format!("#SysMLv1::{tag} dependency from {sq} to {tq};");
            (put(work, &model, src, owner_el, &text)?, text)
        }
        "redefinition" => {
            usage_operand(&model, src, "redefine", "redefining feature")?;
            usage_operand(&model, tgt, "redefine", "redefined feature")?;
            let Some(name_span) = src.name_span.filter(|_| !src.implied_name) else {
                return Err(format!("'{sqn}' has no declared name, so redefine cannot extend it. Fix: use edit_model replace on its statement.").into());
            };
            let holder = tgt.owner.map(|o| &model.elements[o]);
            let supers = src.owner.map(|o| supertypes(&model, o)).unwrap_or_default();
            if holder.map(|h| !supers.contains(&h.id)).unwrap_or(true) {
                let h = holder.map(|h| h.qualified_name.clone()).unwrap_or_else(|| "<top level>".into());
                let o = src.owner.map(|o| model.elements[o].qualified_name.clone()).unwrap_or_else(|| "<top level>".into());
                return Err(Fail::with(
                    format!("'{sqn}' sits in '{o}', which does not specialize or use as its type '{h}', where '{tqn}' is declared, so the redefinition would not resolve. Fix: create_relationship {{kind: 'specialize', source: '{o}', target: '{h}'}} first."),
                    json!({ "ref": tqn }),
                ));
            }
            let (at, add_text) = match src.refs_of(RefRole::Redefines).last() {
                Some(r) => (r.span.end, format!(", {tq}")),
                None => {
                    let end = src.refs.iter().filter(|r| matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::References) && r.span.start >= name_span.end).map(|r| r.span.end).fold(name_span.end, usize::max);
                    (end, format!(" :>> {tq}"))
                }
            };
            let path = model.files[src.file].path.clone();
            let f = work.iter_mut().find(|f| f.path == path).ok_or("source file vanished")?;
            if !f.text.is_char_boundary(at) {
                return Err(Fail::from("the redefinition point is not inside the file text; read the model again and retry"));
            }
            f.text.insert_str(at, &add_text);
            (path, format!("{sqn} :>> {tq}"))
        }
        "connect" => {
            let home = chain_home.ok_or("connect has no home")?;
            if let Some(n) = name {
                taken(home, n)?;
            }
            let ((se, su), (te, tu)) = (end_text_opt(&model, home, &src_l, lenient)?, end_text_opt(&model, home, &tgt_l, lenient)?);
            unreachable_used = [(su, &src_l.text), (tu, &tgt_l.text)].into_iter().filter(|(u, _)| *u).map(|(_, t)| t.clone()).collect();
            let text = if name.is_some() || conn_def.is_some() {
                let n = name.map(|n| format!(" {}", quote_name(n))).unwrap_or_default();
                let t = conn_def.as_ref().map(|t| format!(" : {t}")).unwrap_or_default();
                format!("{mprefix}connection{n}{t} connect {se} to {te};")
            } else {
                format!("{mprefix}connect {se} to {te};")
            };
            add(work, Some(&home.qualified_name.clone()), None, &text)?;
            (home.qualified_name.clone(), text)
        }
        "flow" => {
            let home = chain_home.ok_or("flow has no home")?;
            if let Some(n) = name {
                taken(home, n)?;
            }
            let n = name.map(|n| format!(" {}", quote_name(n))).unwrap_or_default();
            let o = of.as_ref().map(|t| format!(" of {t}")).unwrap_or_default();
            let text = format!("{mprefix}{}flow{n}{o} from {} to {};", if succession_flow { "succession " } else { "" }, end_text(&model, home, &src_l)?, end_text(&model, home, &tgt_l)?);
            add(work, Some(&home.qualified_name.clone()), None, &text)?;
            (home.qualified_name.clone(), text)
        }
        "transition" => {
            let home = chain_home.ok_or("transition has no home")?;
            if let Some(n) = name {
                taken(home, n)?;
            }
            let n = name.map(|n| format!(" {}", quote_name(n))).unwrap_or_default();
            let acc = trigger.as_ref().map(|t| format!(" accept {t}")).unwrap_or_default();
            let g = guard.as_ref().map(|g| format!(" if {g}")).unwrap_or_default();
            let d = effect.as_ref().map(|e| format!(" do {e}")).unwrap_or_default();
            let text = format!("{mprefix}transition{n} first {}{acc}{g}{d} then {}{}", end_text(&model, home, &src_l)?, end_text(&model, home, &tgt_l)?, transition_tail(a)?);
            add(work, Some(&home.qualified_name.clone()), None, &text)?;
            (home.qualified_name.clone(), text)
        }
        "perform" => {
            if matches!(src.kind.as_str(), "package" | "metadata" | "attribute" | "enum") {
                return Err(format!("perform needs a part or action as the performer, but '{sqn}' is a {}. Fix: pass the part (def or usage) that performs the action.", src.keyword()).into());
            }
            if tgt.kind != "action" {
                return Err(format!("perform needs an action as the target, but '{tqn}' is a {}. Fix: create one with create_element {{kind: 'action', ...}} and pass it.", tgt.keyword()).into());
            }
            let text = if tgt.is_def {
                let n = name.map(str::to_string).unwrap_or_else(|| lower_first(tgt.name.as_deref().unwrap_or("action")));
                taken(src, &n)?;
                format!("perform action {} : {tq};", quote_name(&n))
            } else {
                if name.is_some() {
                    return Err("name applies only when the target is an action definition (it names the new performed action). Fix: drop name.".into());
                }
                format!("perform {tq};")
            };
            add(work, Some(&sqn), None, &text)?;
            (sqn.clone(), text)
        }
        _ => {
            let text = match name {
                Some(n) => format!("{mprefix}dependency {} from {sq} to {tq};", quote_name(n)),
                None => format!("{mprefix}dependency from {sq} to {tq};"),
            };
            (put(work, &model, src_l.anchor, owner_el, &text)?, text)
        }
    };
    let after = model_of(work);
    // A statement's end is the element an alias stands for, so the ends to look for are unaliased too.
    let un = |i: Option<ElementId>| i.map(|x| graph::unalias(&after, x));
    let (sid, tid) = (un(after.find(&sqn).map(|e| e.id)), un(after.find(&tqn).map(|e| e.id)));
    // The statement the write made: the last member of the place it went to. A verify sits in the objective of its source,
    // and a new objective is the new element.
    let made: Option<ElementId> = match stmt_kind {
        "specialization" | "redefinition" => None,
        "verify" => after.find(&sqn).and_then(|s| after.children(s.id).filter(|c| c.kind == "objective").last()).and_then(|o| o.children.last().copied()),
        _ => new_member_ref(&after, &declared_in).and_then(|r| after.find(&r)).map(|e| e.id),
    };
    let new_root: Option<ElementId> = match (stmt_kind, made) {
        ("verify", Some(v)) if base.children(src.id).all(|c| c.kind != "objective") => after.elements[v].owner,
        (_, m) => m,
    };
    let unreadable = |why: &str| Fail::from(format!("the generated statement '{text}' {why}; nothing was written. Check that both references are reachable."));
    let ok = match stmt_kind {
        "specialization" | "redefinition" => {
            // The source gained one reference of its role, it is the target, and no element came or went.
            let role = if stmt_kind == "specialization" { RefRole::Specializes } else { RefRole::Redefines };
            let had = base.find(&sqn).map(|e| e.refs_of(role).count());
            let now = after.find(&sqn);
            let last = now.and_then(|e| e.refs_of(role).last()).and_then(|r| un(r.resolved()));
            had.is_some() && now.map(|e| e.refs_of(role).count()) == had.map(|h| h + 1) && last.is_some() && last == tid
        }
        "perform" => made.map(|m| &after.elements[m]).is_some_and(|c| c.kind.starts_with("perform") && c.owner == after.find(&sqn).map(|s| s.id) && c.refs.iter().any(|r| un(r.resolved()) == tid)),
        _ => made.is_some_and(|m| {
            let stated = graph::statements_of(&after, m);
            stated.len() == 1 && stated.iter().all(|s| s.kind == edge_kind && un(s.source.id) == sid && un(s.target.id) == tid && sid.is_some() && tid.is_some())
        }),
    };
    if !ok {
        return Err(unreadable("does not resolve to the two elements, or states more than one relationship"));
    }
    if let (true, Some(m)) = (stmt_kind == "transition", made) {
        check_clauses(&after, &after.elements[m], trigger.as_deref(), guard.as_deref(), effect.as_deref())?;
    }
    let new_ids = new_root.map(|r| subtree_ids(&after, r)).unwrap_or_default();
    // A specialize or redefine edits its source in place, so only the other kinds compare text.
    let compare_text = !matches!(stmt_kind, "specialization" | "redefinition");
    refuse_stray(&format!("the {stmt_kind}"), &base, &after, &|_| false, &|e| new_ids.contains(&e.id), compare_text, None)?;
    let mut info = json!({ "kind": stmt_kind, "source": sq, "target": tq, "statement": text, "declared_in": declared_in });
    if let Some(home) = chain_home {
        info["owner_used"] = json!(home.qualified_name);
        info["owner_how"] = json!(if text_arg(a, "owner").is_some() { "given by the caller" } else { "nearest owner that holds both ends" });
    }
    if let Some(r) = new_member_ref(&after, &declared_in) {
        info["ref"] = json!(r);
    }
    if !unreachable_used.is_empty() {
        info["warnings"] = json!([{
            "rule": "unreachable-ends",
            "severity": "warning",
            "message": format!("the end(s) {} are not members of '{}', declared or inherited, so the interconnection view will not draw this connector. The statement was written with the ends as given: {text}", unreachable_used.iter().map(|t| format!("'{t}'")).collect::<Vec<_>>().join(", "), chain_home.map(|h| h.qualified_name.as_str()).unwrap_or("")),
            "fix": "Pass owner as an element that holds or inherits the first element of each end when the view should draw it."
        }]);
    }
    if allow_defs && (src.is_def || tgt.is_def || src.kind == "package" || tgt.kind == "package") {
        info["warnings"] = json!([{
            "rule": "trace-operand-definition",
            "severity": "warning",
            "message": format!("{stmt_kind} relates a definition or package ({}). SysML v2 relates usages here; validate_model keeps reporting this as a warning.", if src.is_def || src.kind == "package" { &sq } else { &tq }),
            "fix": "Create usages typed by the definitions and relate those, when the model should pass the semantic rule cleanly."
        }]);
    }
    Ok(info)
}

/// A definition that specializes a name of the standard library, as `create_element {specializes}` writes it.
fn create_specialize_library(work: &mut Vec<FileIn>, model: &Model, a: &Value, given: &str) -> Res<Value> {
    for k in ["of", "typed_by", "trigger", "guard", "effect", "name", "owner", "metadata", "allow_definitions", "keep_text"] {
        if a.get(k).map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("'{k}' does not apply to specialize. Fix: drop '{k}'.").into());
        }
    }
    let src = locate_ref(model, req(a, "source")?)?.el;
    if !src.is_def {
        return Err(format!("'{}' is a {} and '{}' is a definition of the library; a usage cannot specialize it. Fix: pass a definition as source.", src.qualified_name, src.keyword(), given.trim()).into());
    }
    let tq = edit::qualified_text("specialize target", given, false)?;
    let sqn = src.qualified_name.clone();
    if let Some(r) = src.refs_of(RefRole::Specializes).find(|r| r.text.trim() == tq) {
        let at = json!({ "file": model.files[src.file].path, "line": super::line_of(model, src), "text": r.text });
        if flag(a, "if_absent") {
            return Ok(json!({ "kind": "specialization", "source": sqn, "target": tq, "existed": true, "ref": sqn, "declared_at": at }));
        }
        return Err(Fail::with(format!("the specialization from '{sqn}' to '{tq}' already exists. Fix: nothing to do, or pass if_absent: true to make the call a no-op."), json!({ "declared_at": at })));
    }
    step(work, EditOp::AddSpecialization { target: sqn.clone(), general: tq.clone() })?;
    let after = model_of(work);
    if !after.find(&sqn).map(|e| e.refs_of(RefRole::Specializes).any(|r| r.text.trim() == tq)).unwrap_or(false) {
        return Err(format!("the specialization of '{sqn}' by '{tq}' did not read back; nothing was written. Report this as a generator bug.").into());
    }
    let declared_in = model.files[src.file].path.clone();
    Ok(json!({ "kind": "specialization", "source": sqn, "target": tq, "statement": format!("{sqn} :> {tq}"), "declared_in": declared_in }))
}

/// `allocate start to X;` and `allocate X to done;`: an allocate whose end is the start or done of the behavior.
fn create_allocate_marked(work: &mut Vec<FileIn>, model: &Model, a: &Value) -> Res<Value> {
    for k in ["of", "typed_by", "trigger", "guard", "effect", "name", "allow_definitions"] {
        if a.get(k).map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("'{k}' does not apply to an allocate with `start` or `done`. Fix: drop '{k}'.").into());
        }
    }
    // The real end is read as in an allocate without a marker: by its qualified name, or, when `owner` is given, as the
    // parser reads a short name written inside that owner. `keep_text` writes it as given.
    let end_owner: Option<&Element> = text_arg(a, "owner").map(|o| locate(model, o)).transpose()?;
    let keep_text = flag(a, "keep_text");
    let mut ends: Vec<(String, Option<ElementId>)> = Vec::new();
    for role in ["source", "target"] {
        let t = req(a, role)?;
        if MARKERS.contains(&t) && locate_ref(model, t).is_err() {
            ends.push((t.to_string(), None));
        } else {
            let l = locate_end_from(model, t, None, false, end_owner)?;
            usage_operand(model, l.el, "allocate", role)?;
            let text = if keep_text {
                let given = t.trim();
                valid_end_text(given).map_err(|why| Fail::from(format!("keep_text: the {role} {} cannot be written as given ({why}). Fix: quote each name that has spaces, as in 'Pkg Name'::Part, or drop keep_text.", crate::agent::text::quote_clip(given))))?;
                given.to_string()
            } else {
                l.text.clone()
            };
            ends.push((text, Some(l.el.id)));
        }
    }
    if ends.iter().all(|e| e.1.is_none()) {
        return Err("an allocate needs one real element besides `start` or `done`. Fix: name a usage as the other end.".into());
    }
    let home: &Element = match end_owner {
        Some(o) => o,
        None => ends.iter().find_map(|e| e.1).and_then(|id| model.elements[id].owner).map(|o| &model.elements[o]).ok_or_else(|| Fail::from("start and done belong to the behavior that holds the allocate. Fix: pass owner."))?,
    };
    let meta = meta_list(model, a)?;
    let mprefix: String = meta.iter().map(|m| format!("#{m} ")).collect();
    let (sq, tq) = (ends[0].0.clone(), ends[1].0.clone());
    if !flag(a, "allow_duplicate") {
        let same = |r: Option<&Reference>, end: &(String, Option<ElementId>)| r.map(|r| match end.1 {
            Some(id) => r.resolved() == Some(id),
            None => r.text.trim() == end.0,
        });
        let dup = model.children(home.id).find(|c| c.kind == "allocation" && same(c.refs_of(RefRole::Source).next(), &ends[0]) == Some(true) && same(c.refs_of(RefRole::Target).next(), &ends[1]) == Some(true));
        if let Some(d) = dup {
            let at = graph::declared_at(model, d.id);
            if flag(a, "if_absent") {
                return Ok(json!({ "kind": "allocate", "source": sq, "target": tq, "existed": true, "ref": d.qualified_name, "declared_at": at }));
            }
            return Err(Fail::with(format!("the allocate from '{sq}' to '{tq}' already exists. Fix: nothing to do, pass if_absent: true to make the call a no-op, or pass allow_duplicate: true when the source model really has a second one."), json!({ "declared_at": at })));
        }
    }
    let text = format!("{mprefix}allocate {sq} to {tq};");
    add(work, Some(&home.qualified_name), None, &text)?;
    let after = model_of(work);
    let r = new_member_ref(&after, &home.qualified_name);
    if !r.as_deref().and_then(|r| after.find(r)).map(|e| e.kind == "allocation").unwrap_or(false) {
        return Err(format!("the generated statement '{text}' did not read back as an allocate; nothing was written. Report this as a generator bug.").into());
    }
    // The end as written must name the element that was meant, read from where the statement stands.
    let made = r.as_deref().and_then(|r| after.find(r));
    for (role, end) in [(RefRole::Source, &ends[0]), (RefRole::Target, &ends[1])] {
        let Some(want) = end.1.map(|id| model.elements[id].qualified_name.as_str()) else { continue };
        let got = made.and_then(|m| m.refs_of(role).next()).and_then(|x| x.resolved()).map(|t| after.elements[graph::unalias(&after, t)].qualified_name.as_str());
        if got != Some(want) {
            return Err(Fail::from(format!(
                "refused: written in '{}', the end '{}' would name {} and not '{want}'. Nothing was written. Fix: pass the qualified name of the end, written $::A::b when a nearer name hides it.",
                home.qualified_name,
                end.0,
                got.map(|g| format!("'{g}'")).unwrap_or_else(|| "nothing".into())
            )));
        }
    }
    Ok(json!({ "kind": "allocate", "source": sq, "target": tq, "statement": text, "declared_in": home.qualified_name, "owner_used": home.qualified_name, "ref": r }))
}

/// `transition first start ... then X;` and `... then done;`: a transition whose source is `start` or whose target is `done`.
/// How a transition statement ends: `;`, or a body holding its documentation when the call passes `doc`.
fn transition_tail(a: &Value) -> Res<String> {
    match text_arg(a, "doc") {
        Some(d) => {
            clean_doc("doc", d)?;
            Ok(format!(" {{\n    doc /* {} */\n}}", d.replace("*/", "* /")))
        }
        None => Ok(";".to_string()),
    }
}

fn create_transition_marked(work: &mut Vec<FileIn>, model: &Model, a: &Value) -> Res<Value> {
    for k in ["of", "typed_by", "allow_definitions"] {
        if a.get(k).map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("'{k}' does not apply to transition. Fix: drop '{k}'.").into());
        }
    }
    let end = |role: &str| -> Res<SuccEnd> {
        match req(a, role)? {
            "start" if role == "source" => Ok(SuccEnd::Marker("start")),
            "done" if role == "target" => Ok(SuccEnd::Marker("done")),
            "start" | "done" => Err(format!("a transition cannot use `{}` as its {role}. Fix: `start` is a source and `done` is a target.", req(a, role)?).into()),
            t => {
                let l = locate_ref(model, t)?;
                end_usage(model, l.el, "transition", role)?;
                if !l.el.kind.contains("state") {
                    return Err(format!("a transition joins states, but the {role} {} is a {}. Fix: pass a state usage, or `start` / `done`.", qshow(&l.el.qualified_name), l.el.keyword()).into());
                }
                Ok(SuccEnd::Real(l))
            }
        }
    };
    let (s, t) = (end("source")?, end("target")?);
    let home: &Element = match (&s, &t) {
        (SuccEnd::Real(x), SuccEnd::Real(y)) => home_of(model, a, x.anchor, y.anchor, "transition")?,
        (SuccEnd::Real(x), _) | (_, SuccEnd::Real(x)) => match text_arg(a, "owner") {
            Some(o) => locate(model, o)?,
            None => x.anchor.owner.map(|o| &model.elements[o]).ok_or_else(|| Fail::from("the end has no owner. Fix: pass owner, the state that holds the transition."))?,
        },
        _ => match text_arg(a, "owner") {
            Some(o) => locate(model, o)?,
            None => return Err("start and done belong to the state that holds the transition. Fix: pass owner.".into()),
        },
    };
    if !home.kind.contains("state") {
        return Err(format!("a transition lives in a state, but the owner is the {} {}. Fix: pass owner, the state (def or usage) that holds the transition.", home.keyword(), qshow(&home.qualified_name)).into());
    }
    let trigger = text_arg(a, "trigger").map(|t| trigger_text(model, Some(home), t)).transpose()?;
    let guard = clause("guard", text_arg(a, "guard"))?;
    let effect = clause("effect", text_arg(a, "effect"))?;
    let name = text_arg(a, "name");
    if let Some(n) = name {
        clean_name("name", n)?;
        if let Some(d) = model.children(home.id).find(|c| !c.implied_name && c.name.as_deref() == Some(n)) {
            return Err(format!("{} already has a member named '{n}' ({}). Fix: pass another name.", qshow(&home.qualified_name), qshow(&d.qualified_name)).into());
        }
    }
    let meta = meta_list(model, a)?;
    let mprefix: String = meta.iter().map(|m| format!("#{m} ")).collect();
    let shown = |e: &SuccEnd| match e {
        SuccEnd::Marker(w) => w.to_string(),
        SuccEnd::Real(l) => l.text.clone(),
    };
    let written = |e: &SuccEnd| -> Res<String> {
        match e {
            SuccEnd::Marker(w) => Ok(w.to_string()),
            SuccEnd::Real(l) => end_text(model, home, l),
        }
    };
    let path = |e: &SuccEnd| match e {
        SuccEnd::Marker(w) => vec![format!("~{w}")],
        SuccEnd::Real(l) => end_path(model, home.id, l),
    };
    let (sq, tq) = (shown(&s), shown(&t));
    let extra = format!("t:{}|g:{}|e:{}", norm(trigger.as_deref().map(|t| t.strip_prefix("accept ").unwrap_or(t)).unwrap_or("")), norm(guard.as_deref().unwrap_or("")), norm(effect.as_deref().unwrap_or(""))) + &name_extra(name);
    if flag(a, "allow_duplicate") && flag(a, "if_absent") {
        return Err("allow_duplicate and if_absent contradict each other. Fix: pass one of them.".into());
    }
    if !flag(a, "allow_duplicate") {
        let key = stmt_key_of("transition", &path(&s), &path(&t), &extra, Some(home.qualified_name.clone()));
        need_whole_model(model)?;
        if let Some(at) = find_duplicate(model, "transition", &key) {
            if flag(a, "if_absent") {
                return Ok(json!({ "kind": "transition", "source": sq, "target": tq, "existed": true, "ref": at["element"], "declared_at": at }));
            }
            return Err(Fail::with(format!("the transition from '{sq}' to '{tq}' already exists. Fix: nothing to do, pass if_absent: true to make the call a no-op, or pass allow_duplicate: true when the source model really has a second one."), json!({ "declared_at": at })));
        }
    }
    let n = name.map(|n| format!(" {}", quote_name(n))).unwrap_or_default();
    let acc = trigger.as_ref().map(|t| format!(" accept {t}")).unwrap_or_default();
    let g = guard.as_ref().map(|g| format!(" if {g}")).unwrap_or_default();
    let d = effect.as_ref().map(|e| format!(" do {e}")).unwrap_or_default();
    let text = format!("{mprefix}transition{n} first {}{acc}{g}{d} then {}{}", written(&s)?, written(&t)?, transition_tail(a)?);
    add(work, Some(&home.qualified_name), None, &text)?;
    let after = model_of(work);
    let r = new_member_ref(&after, &home.qualified_name);
    let Some(made) = r.as_deref().and_then(|r| after.find(r)).filter(|e| e.kind == "transition") else {
        return Err(format!("the generated statement '{text}' did not read back as a transition; nothing was written. Report this as a generator bug.").into());
    };
    check_ends(&after, made, &text, [(RefRole::Source, &s), (RefRole::Target, &t)])?;
    check_clauses(&after, made, trigger.as_deref(), guard.as_deref(), effect.as_deref())?;
    let new = subtree_ids(&after, made.id);
    refuse_stray_elements("the transition", model, &after, &|_| false, &|e| new.contains(&e.id))?;
    Ok(json!({ "kind": "transition", "source": sq, "target": tq, "statement": text, "declared_in": home.qualified_name, "owner_used": home.qualified_name, "ref": r }))
}

/// A transition or succession as read back must carry the guard, effect and trigger that were asked for, and no others: text
/// that closes a clause early (`then`, `if`, `do`) would show up here as a clause the caller never wrote.
fn check_clauses(after: &Model, made: &Element, trigger: Option<&str>, guard: Option<&str>, effect: Option<&str>) -> Res<()> {
    let want = |v: Option<&str>| norm(v.map(|t| t.strip_prefix("accept ").unwrap_or(t)).unwrap_or(""));
    for (k, v) in [("trigger", trigger), ("guard", guard), ("effect", effect)] {
        let have = norm_prop(made, k);
        if have != want(v) {
            let _ = after;
            return Err(Fail::from(format!(
                "the {} was read back with {k} {}, not {}; nothing was written. Fix: a guard, effect or trigger is one expression, without if, do, then or accept outside quotes.",
                made.kind,
                crate::agent::text::quote_clip(&have),
                crate::agent::text::quote_clip(&want(v))
            )));
        }
    }
    Ok(())
}

/// Each end of a succession or transition is read back as exactly the one element, or the one marker, that was asked for.
fn check_ends(after: &Model, made: &Element, text: &str, ends: [(RefRole, &SuccEnd); 2]) -> Res<()> {
    for (role, end) in ends {
        let refs: Vec<&Reference> = made.refs_of(role).collect();
        let fine = refs.len() == 1
            && match end {
                SuccEnd::Marker(w) => refs[0].segments.len() == 1 && refs[0].text.trim() == *w,
                SuccEnd::Real(l) => {
                    let want = after.find(&l.el.qualified_name).map(|e| graph::unalias(after, e.id));
                    want.is_some() && refs[0].resolved().map(|r| graph::unalias(after, r)) == want
                }
            };
        if !fine {
            return Err(Fail::from(format!(
                "the generated statement '{text}' did not read back with one {} end as asked ({} found); nothing was written.",
                if role == RefRole::Source { "source" } else { "target" },
                refs.len()
            )));
        }
    }
    Ok(())
}

enum SuccEnd<'a> {
    /// `start` or `done`: the ends of the behavior that holds the succession.
    Marker(&'static str),
    Real(Located<'a>),
}

/// `succession`: `first A [if G] then B;` in the action or state that holds both ends. `start` and `done`
/// name the ends of that behavior.
fn create_succession(work: &mut Vec<FileIn>, model: &Model, a: &Value) -> Res<Value> {
    for k in ["of", "typed_by", "trigger", "effect", "allow_definitions", "metadata"] {
        if a.get(k).map(|v| !v.is_null()).unwrap_or(false) {
            return Err(format!("'{k}' does not apply to succession. Fix: drop '{k}'; a succession takes source, target, name, guard, owner and if_absent.").into());
        }
    }
    let end = |role: &str| -> Res<SuccEnd> {
        match req(a, role)? {
            "start" => Ok(SuccEnd::Marker("start")),
            "done" => Ok(SuccEnd::Marker("done")),
            t => {
                let l = locate_ref(model, t)?;
                if l.el.is_def || l.el.kind == "package" {
                    return Err(format!("a succession joins action usages, but the {role} '{}' is a {}. Fix: pass the usage inside the action, or `start` / `done`.", l.el.qualified_name, l.el.keyword()).into());
                }
                if l.el.name.is_none() || l.el.implied_name {
                    return Err(format!("the {role} '{}' has no declared name, and a succession names its ends. Fix: give it a name first with update_element {{ref, rename}}.", l.el.qualified_name).into());
                }
                Ok(SuccEnd::Real(l))
            }
        }
    };
    let (s, t) = (end("source")?, end("target")?);
    match (&s, &t) {
        (SuccEnd::Marker("done"), _) => return Err("a succession cannot start at `done`. Fix: use `start` or an action as the source.".into()),
        (_, SuccEnd::Marker("start")) => return Err("a succession cannot end at `start`. Fix: use `done` or an action as the target.".into()),
        (SuccEnd::Real(x), SuccEnd::Real(y)) if x.el.id == y.el.id => return Err("source and target are the same element".into()),
        _ => {}
    }
    let home: &Element = match (&s, &t) {
        (SuccEnd::Real(x), SuccEnd::Real(y)) => home_of(model, a, x.anchor, y.anchor, "succession")?,
        (SuccEnd::Real(x), _) | (_, SuccEnd::Real(x)) => match text_arg(a, "owner") {
            Some(o) => locate(model, o)?,
            None => x.anchor.owner.map(|o| &model.elements[o]).ok_or_else(|| Fail::from("the end has no owner. Fix: pass owner, the action that holds the succession."))?,
        },
        _ => match text_arg(a, "owner") {
            Some(o) => locate(model, o)?,
            None => return Err("start and done belong to the action that holds the succession. Fix: pass owner.".into()),
        },
    };
    if home.kind == "package" {
        return Err(format!("a succession needs an action to live in, but the owner is the package '{}'. Fix: pass owner, the action (def or usage) that holds both ends.", home.qualified_name).into());
    }
    let guard = clause("guard", text_arg(a, "guard"))?;
    let name = text_arg(a, "name");
    if let Some(n) = name {
        clean_name("name", n)?;
        if let Some(d) = model.children(home.id).find(|c| !c.implied_name && c.name.as_deref() == Some(n)) {
            return Err(format!("'{}' already has a member named '{n}' ('{}'). Fix: pass another name.", home.qualified_name, d.qualified_name).into());
        }
    }
    let shown = |e: &SuccEnd| match e {
        SuccEnd::Marker(w) => w.to_string(),
        SuccEnd::Real(l) => l.text.clone(),
    };
    let written = |e: &SuccEnd| -> Res<String> {
        match e {
            SuccEnd::Marker(w) => Ok(w.to_string()),
            SuccEnd::Real(l) => end_text(model, home, l),
        }
    };
    let path = |e: &SuccEnd| match e {
        SuccEnd::Marker(w) => vec![format!("~{w}")],
        SuccEnd::Real(l) => end_path(model, home.id, l),
    };
    let (sq, tq) = (shown(&s), shown(&t));
    if !flag(a, "allow_duplicate") {
        let key = stmt_key_of("succession", &path(&s), &path(&t), &(format!("g:{}", norm(guard.as_deref().unwrap_or(""))) + &name_extra(name)), Some(home.qualified_name.clone()));
        need_whole_model(model)?;
        if let Some(at) = find_duplicate(model, "succession", &key) {
            if flag(a, "if_absent") {
                return Ok(json!({ "kind": "succession", "source": sq, "target": tq, "existed": true, "ref": at["element"], "declared_at": at }));
            }
            return Err(Fail::with(format!("the succession from '{sq}' to '{tq}' already exists. Fix: nothing to do, pass if_absent: true to make the call a no-op, or pass allow_duplicate: true when the source model really has a second one."), json!({ "declared_at": at })));
        }
    } else if flag(a, "if_absent") {
        return Err("allow_duplicate and if_absent contradict each other. Fix: pass one of them.".into());
    }
    let g = guard.as_ref().map(|g| format!(" if {g}")).unwrap_or_default();
    let text = match name {
        Some(n) => format!("succession {} first {}{g} then {};", quote_name(n), written(&s)?, written(&t)?),
        None => format!("first {}{g} then {};", written(&s)?, written(&t)?),
    };
    add(work, Some(&home.qualified_name), None, &text)?;
    let after = model_of(work);
    let r = new_member_ref(&after, &home.qualified_name);
    let made = r.as_deref().and_then(|r| after.find(r)).filter(|e| e.kind == "succession");
    let Some(made) = made else {
        return Err(format!("the generated statement '{text}' did not read back as a succession; nothing was written. Report this as a generator bug.").into());
    };
    // Exactly the two ends that were asked for, the guard that was asked for, and no other element or edge in the model.
    check_ends(&after, made, &text, [(RefRole::Source, &s), (RefRole::Target, &t)])?;
    check_clauses(&after, made, None, guard.as_deref(), None)?;
    let new = subtree_ids(&after, made.id);
    refuse_stray_elements("the succession", model, &after, &|_| false, &|e| new.contains(&e.id))?;
    Ok(json!({ "kind": "succession", "source": sq, "target": tq, "statement": text, "declared_in": home.qualified_name, "ref": r }))
}

/// The member a statement just added: the last member of the element it was written into.
fn new_member_ref(after: &Model, declared_in: &str) -> Option<String> {
    let last = |e: &Element| e.children.last().map(|c| &after.elements[*c]);
    if let Some(owner) = declared_in.strip_suffix(" objective") {
        let o = after.find(owner)?;
        let obj = o.children.iter().map(|c| &after.elements[*c]).rev().find(|c| c.kind == "objective")?;
        return last(obj).map(|m| m.qualified_name.clone());
    }
    if let Some(o) = after.find(declared_in) {
        return last(o).map(|m| m.qualified_name.clone());
    }
    after.roots.iter().rev().map(|r| &after.elements[*r]).find(|r| after.files[r.file].path == declared_in).map(|r| r.qualified_name.clone())
}

fn requirement_any(e: &Element, kind: &str, role: &str) -> Res<()> {
    if e.kind != "requirement" {
        return Err(format!("{kind}: the {role} must be a requirement, but '{}' is a {}.", e.qualified_name, e.keyword()).into());
    }
    Ok(())
}

#[cfg(test)]
mod r14_tests {
    use super::*;

    fn files(text: &str) -> Vec<FileIn> {
        vec![FileIn { path: "m.sysml".into(), text: text.into() }]
    }

    const BASE: &str = "package P {\n    action def Act {\n        action s1;\n        action s2;\n        action s3;\n    }\n}\n";

    // The statement the old guard let through: the text after `\"` was read as outside quotes by the guard and inside quotes
    // by the lexer. Read back, it has three ends and a guard that is not the one asked for.
    #[test]
    fn a_succession_that_reads_back_with_an_extra_end_is_refused() {
        let before = model_of(&files(BASE));
        let bad = BASE.replace("        action s3;\n", "        action s3;\n        first s1 if \"\\\"\" then P::Act::s3 then s2;\n");
        let after = model_of(&files(&bad));
        let made = after.find("P::Act::$4").or_else(|| after.elements.iter().find(|e| e.kind == "succession").map(|e| e.id).map(|i| &after.elements[i])).unwrap();
        let s = SuccEnd::Real(locate_ref(&before, "P::Act::s1").unwrap());
        let t = SuccEnd::Real(locate_ref(&before, "P::Act::s2").unwrap());
        let err = check_ends(&after, made, "first s1 if ... then s2;", [(RefRole::Source, &s), (RefRole::Target, &t)]);
        let clause = check_clauses(&after, made, None, Some("\"\\\"\" then P::Act::s3"), None);
        assert!(err.is_err() || clause.is_err(), "the injected statement read back as asked: {:?}", made.props);
        // Asked for the two ends and no guard, the statement holds a guard, so it is refused too.
        assert!(check_clauses(&after, made, None, None, None).is_err());
    }

    #[test]
    fn a_transition_that_reads_back_with_an_extra_clause_is_refused() {
        let text = "package P {\n    state def St {\n        state a;\n        state b;\n        transition first a if \"\\\"\" do evil then b;\n    }\n}\n";
        let m = model_of(&files(text));
        let made = m.elements.iter().find(|e| e.kind == "transition").unwrap();
        let err = check_clauses(&m, made, None, Some("\"\\\"\" do evil"), None);
        assert!(err.is_err(), "guard and effect were not told apart: {:?}", made.props);
    }

    #[test]
    fn one_scanner_reads_quotes_for_the_guard_and_the_lexer() {
        // Each of these holds a clause word outside quotes, after a quote the old guard read differently.
        for v in ["\"\\\"\" then x", "\"\\\"\" do x", "\"\\\"\" if x", "\"\\\\\" then x", "'a\\'b' then x", "5then x", "1e5do x", "x if y"] {
            assert!(clause_expr("guard", v).is_err(), "{v} was accepted");
        }
        // Inside quotes, and as part of a longer name, the words are text.
        for v in ["\"if then do accept\" == name", "'then' > 1", "a_if_b > 1", "doing > 1", "\"it's\" == s", "'x\\'then' == s", "level > 3"] {
            assert!(clause_expr("guard", v).is_ok(), "{v} was refused: {:?}", clause_expr("guard", v).err().map(|f| f.message));
        }
    }

    #[test]
    fn a_control_character_after_a_backslash_is_refused_everywhere() {
        for v in ["\"a\\\u{0}b\"", "\"a\\\u{7}b\"", "'a\\\u{1b}b'", "\"a\u{7}b\"", "x\u{0}"] {
            assert!(safe_expr("value", v).is_err(), "{v:?} was accepted");
        }
        for t in ["'a\\\u{7}b'", "'a\u{7}b'", "P::'a\\\u{0}'", "P::\u{7}x"] {
            assert!(valid_end_text(t).is_err(), "{t:?} was accepted");
        }
        assert!(valid_end_text("P::'a\\'b'::x").is_ok());
    }

    // The reviewer's text: `attribute x = 5 ///* c */;` reads as a line note, the `;` goes with it and `y` is swallowed.
    /// F13 of the third security review. `gMPflVXtwGDXbIhP73TX` and `LtHf1prlU1bCeYZEdqWf` have the same 64-bit FNV-1a
    /// hash. The check on the elements beside an edit keyed each element by that hash of its text, so one of these texts in
    /// the place of the other went unseen. It compares the text itself now.
    #[test]
    fn r17_a_neighbour_whose_text_changed_to_one_with_the_same_fnv_hash_is_found() {
        let fnv = |s: &str| s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        assert_eq!(fnv("gMPflVXtwGDXbIhP73TX"), fnv("LtHf1prlU1bCeYZEdqWf"), "the pair must collide for the test to mean anything");
        let before = model_of(&files("package P { constraint def C { gMPflVXtwGDXbIhP73TX } part x; }"));
        let after = model_of(&files("package P { constraint def C { LtHf1prlU1bCeYZEdqWf } part x; }"));
        let none = |_: &Element| false;
        assert_ne!(text_census(&before, &none), text_census(&after, &none));
        let e = refuse_stray("an edit of 'P::x'", &before, &after, &none, &none, true, None).err().expect("the changed neighbour must be found");
        assert!(e.message.contains("the text of an element beside it changed"), "{}", e.message);
        assert!(refuse_stray("an edit of 'P::x'", &before, &before, &none, &none, true, None).is_ok());
    }

    /// F13, second part. An update that sets a short name changes its own element only, so the check compares the text of
    /// the others, as for every update but a rename and a move. Before, `short_name` switched that comparison off.
    #[test]
    fn r17_a_short_name_update_compares_the_text_of_the_other_elements() {
        assert!(compares_text("update_element", &json!({"ref": "P::a", "short_name": "A1"})));
        assert!(compares_text("update_element", &json!({"ref": "P::a", "value": "1"})));
        assert!(!compares_text("update_element", &json!({"ref": "P::a", "rename": "b"})));
        assert!(!compares_text("update_element", &json!({"ref": "P::a", "owner": "P::Q", "short_name": "A1"})));
        assert!(compares_text("set_doc", &json!({})) && !compares_text("create_element", &json!({})));
    }

    #[test]
    fn a_write_that_swallows_a_neighbour_is_refused() {
        let before = model_of(&files("package P {\n    attribute x = 1//* c */;\n    attribute y = 2;\n}\n"));
        let after = model_of(&files("package P {\n    attribute x = 5 ///* c */;\n    attribute y = 2;\n}\n"));
        assert!(after.find("P::y").is_none(), "the fixture no longer shows the swallow");
        let x_b = before.find("P::x").map(|e| e.id);
        let x_a = after.find("P::x").map(|e| e.id);
        let r = refuse_stray_elements("update_element on 'P::x'", &before, &after, &|e| Some(e.id) == x_b, &|e| Some(e.id) == x_a);
        let msg = r.err().expect("the swallow was not noticed").message;
        assert!(msg.contains("attribute \"y\""), "{msg}");
    }
}
