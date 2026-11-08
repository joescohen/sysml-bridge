//! Name capture: an edit that makes existing text mean another element.
//!
//! A new member named `r` inside `Car` hides the outer `r`, so `satisfy r by s;` inside `Car`, which no one touched, now
//! names the new member. The same happens through a rename, a short name, an alias, a move, an import, a specialization
//! (its members are inherited, and an inherited name is nearer than an outer one), a redefinition without a name, a
//! change in the order of two imports, and (the other way round) when the element that hid a name is deleted. The text
//! checks do not see it: the reference reads the same before and after.
//!
//! `refuse` does not list those cases. It compares every reference that was there before the edit with the same
//! reference after it, and refuses the edit when one names another element. "The same reference" is the reference of the
//! same holder (the element that carries it, by its qualified name, followed through the renames and moves of the edit),
//! in the same role, with the same text. A reference whose text the edit rewrote (a rename rewrites the name in every
//! reference to the element) is the same reference too: it must still name the element it named.
//!
//! What is not a capture:
//! - a reference that did not resolve before and resolves now, or the reverse (a model may be written in any order; the
//!   semantic gate reports a name that no longer resolves);
//! - a reference the edit itself gave another text (`typed_by` on an update): that is what the edit asked for;
//! - a later step of a chain (`a.b`: the `b`) that changes because the edit gave `a` another type or general type. The
//!   step is a capture when the edit brought in or took away a member of that name, or changed an import;
//! - a reference that moves between a feature and its redefinition: a redefinition stands for the feature it redefines.
//!
//! The cost is one pass over the elements and the references of both models for each edit, and for each wave of a batch.
//! The key of each element is worked out once for a model and kept while the edit runs (`take_work_steps` counts the
//! references read, for a test).

use super::ids::IdEvent;
use super::{Fail, Res};
use crate::model::*;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::sync::{Arc, OnceLock};

/// Most references a refusal lists.
const MAX_SHOWN: usize = 10;

thread_local! {
    /// References and elements read by `refuse` on this thread.
    static WORK: Cell<u64> = const { Cell::new(0) };
    /// The tables of the models the running edit compares, newest first. The model an edit leaves is the model the
    /// next edit of a batch starts from, so each model is read once.
    static TABLES: RefCell<Vec<(Arc<Model>, Arc<Table>)>> = const { RefCell::new(Vec::new()) };
}

fn work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n as u64)));
}

/// The count of elements and references `refuse` read on this thread since the last call; the count starts again at zero.
pub fn take_work_steps() -> u64 {
    WORK.with(|w| w.replace(0))
}

/// Drop the tables kept for the edits of one call.
pub(super) fn clear_tables() {
    TABLES.with(|t| t.borrow_mut().clear());
}

/// The qualified name `q` with the position of each unnamed member left out (`P::$3::x` is `P::$::x`): an edit that adds
/// a member moves the positions of the unnamed ones after it, and that is no change of the element.
fn norm_q(q: &str) -> String {
    if !q.contains('$') {
        return q.to_string();
    }
    crate::resolve::split_name(q).iter().map(|s| if s.starts_with('$') && s[1..].bytes().all(|b| b.is_ascii_digit()) { "$".to_string() } else { quote_name(s) }).collect::<Vec<_>>().join("::")
}

fn key(m: &Model, id: ElementId) -> String {
    norm_q(&m.elements[id].qualified_name)
}

/// `k` as the edit renamed it: `from` and everything inside it is `to`.
fn moved(mut k: String, renames: &[(String, String)]) -> String {
    for (from, to) in renames {
        if k == *from {
            k = to.clone();
        } else if k.len() > from.len() + 2 && k.starts_with(from.as_str()) && k[from.len()..].starts_with("::") {
            k = format!("{to}{}", &k[from.len()..]);
        }
    }
    k
}

/// An element across two models: 128 bits of its key, from two hashers with keys drawn when the process starts. The
/// names are the model author's, so a hash with a known key would let a crafted name stand for another element.
type Key = u128;

fn key_of(s: &str) -> Key {
    static HASHERS: OnceLock<(RandomState, RandomState)> = OnceLock::new();
    let (a, b) = HASHERS.get_or_init(|| (RandomState::new(), RandomState::new()));
    ((a.hash_one(s) as u128) << 64) | b.hash_one(s) as u128
}

/// What `refuse` needs of one model, worked out once.
struct Table {
    /// The key of each element.
    keys: Vec<Key>,
    /// The elements that hold a reference, sorted by key and then in the order of the model.
    holders: Vec<(Key, ElementId)>,
}

fn sorted_holders(m: &Model, keys: &[Key]) -> Vec<(Key, ElementId)> {
    let mut holders: Vec<(Key, ElementId)> = m.elements.iter().filter(|e| !e.refs.is_empty()).map(|e| (keys[e.id], e.id)).collect();
    holders.sort_unstable();
    holders
}

fn table_of(m: &Arc<Model>) -> Arc<Table> {
    if let Some(t) = TABLES.with(|t| t.borrow().iter().find(|(x, _)| Arc::ptr_eq(x, m)).map(|(_, t)| t.clone())) {
        return t;
    }
    work(m.elements.len());
    let keys: Vec<Key> = m.elements.iter().map(|e| if e.qualified_name.contains('$') { key_of(&norm_q(&e.qualified_name)) } else { key_of(&e.qualified_name) }).collect();
    let holders = sorted_holders(m, &keys);
    let t = Arc::new(Table { keys, holders });
    TABLES.with(|c| {
        let mut c = c.borrow_mut();
        c.insert(0, (m.clone(), t.clone()));
        c.truncate(3);
    });
    t
}

/// How often each name and short name is declared. An implied name (`perform act;`, `:>> bs`) is not a declared name.
fn name_counts(m: &Model) -> HashMap<&str, i64> {
    let mut c: HashMap<&str, i64> = HashMap::new();
    for e in &m.elements {
        if !e.implied_name {
            if let Some(n) = &e.name {
                *c.entry(n.as_str()).or_default() += 1;
            }
        }
        if let Some(s) = &e.short_name {
            *c.entry(s.as_str()).or_default() += 1;
        }
    }
    c
}

/// The imports of each namespace, in the order written: the namespace, then (imported name, wildcard) of each.
fn imports(m: &Model, fix: &dyn Fn(String) -> String) -> HashMap<String, Vec<(String, String)>> {
    let mut c: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for e in m.elements.iter().filter(|e| matches!(e.kind.as_str(), "import" | "expose")) {
        let home = e.owner.map(|o| fix(key(m, o))).unwrap_or_default();
        let text = e.refs.first().map(|r| r.text.clone()).unwrap_or_default();
        c.entry(home).or_default().push((text, e.props.get("wildcard").cloned().unwrap_or_default()));
    }
    c
}

/// One side of the comparison: a model, the key of each of its elements as the other side knows it, and the plain keys.
struct Side<'a> {
    m: &'a Model,
    keys: &'a [Key],
    /// The keys before the renames of the edit were applied (the same slice for the model after the edit).
    plain: &'a [Key],
}

impl Side<'_> {
    fn row(&self, r: &Reference) -> Vec<Option<Key>> {
        r.segments.iter().map(|s| s.resolved.map(|t| self.keys[t])).collect()
    }

    /// True when `id` redefines, directly or through other redefinitions, the element with key `other`.
    fn redefines(&self, id: ElementId, other: Key) -> bool {
        let mut cur = vec![id];
        for _ in 0..8 {
            let mut next = Vec::new();
            for x in cur {
                for t in self.m.elements[x].refs_of(RefRole::Redefines).filter_map(|r| r.resolved()) {
                    if self.keys[t] == other {
                        return true;
                    }
                    next.push(t);
                }
            }
            if next.is_empty() || next.len() > 64 {
                return false;
            }
            cur = next;
        }
        false
    }
}

/// The names the edit brought in or took away and whether it changed an import, worked out when the first later step of a
/// chain differs (most edits never ask).
struct Deep<'a> {
    before: &'a Model,
    after: &'a Model,
    renames: &'a [(String, String)],
    known: Option<(HashSet<String>, bool)>,
}

impl Deep<'_> {
    fn changed(&mut self, name: &str) -> bool {
        let (before, after, renames) = (self.before, self.after, self.renames);
        let (names, imports_changed) = self.known.get_or_insert_with(|| {
            work(before.elements.len() + after.elements.len());
            let (nb, na) = (name_counts(before), name_counts(after));
            let mut names: HashSet<String> = HashSet::new();
            for (n, c) in &na {
                if nb.get(n) != Some(c) {
                    names.insert(n.to_string());
                }
            }
            for n in nb.keys().filter(|n| !na.contains_key(*n)) {
                names.insert(n.to_string());
            }
            for (from, to) in renames {
                for q in [from, to] {
                    names.extend(crate::resolve::split_name(q).pop());
                }
            }
            (names, imports(before, &|k| moved(k, renames)) != imports(after, &|k| k))
        });
        *imports_changed || names.contains(name)
    }

    /// True when the edit removed or changed an import and the element `named` (its key before the edit, as the model
    /// after knows it) is still in the model: a reference that named it and names nothing now lost it to the import.
    fn lost_to_an_import(&mut self, named: &str, name: &str) -> bool {
        self.changed(name);
        self.known.as_ref().is_some_and(|(_, imports)| *imports) && self.after.find(named).is_some()
    }
}

/// A reference that names another element than before: where the two rows part.
struct Hit {
    text: String,
    holder: String,
    named: String,
    would_name: String,
}

/// Compare one reference before (`o` in `was`) with the same reference after (`n` in `now`).
fn compare(was: &Side, now: &Side, o: &Reference, n: &Reference, holder: &Element, deep: &mut Deep) -> Option<Hit> {
    for i in 0..o.segments.len().min(n.segments.len()) {
        let (a, b) = (o.segments[i].resolved, n.segments[i].resolved);
        if a.map(|t| was.keys[t]) == b.map(|t| now.keys[t]) {
            continue;
        }
        // A first name that resolved and resolves to nothing now, while its element is still there: the edit took away
        // the import that gave the name.
        if let (0, Some(a), None) = (i, a, b) {
            let named = moved(key(was.m, a), deep.renames);
            if deep.lost_to_an_import(&named, &n.segments[0].name) {
                return Some(Hit { text: o.text.clone(), holder: holder.qualified_name.clone(), named, would_name: "nothing (the import that gave the name would be gone)".into() });
            }
        }
        // Else only where both resolve: a name that resolves now and did not before, or the reverse, is no capture.
        let (Some(a), Some(b)) = (a, b) else { return None };
        // A redefinition stands for the feature it redefines.
        if now.redefines(b, was.keys[a]) || was.redefines(a, now.keys[b]) {
            return None;
        }
        // A deleted feature whose name now reaches the inherited feature of the same name (a step turned into a call
        // dropping its own pins): the reference keeps its meaning through the type, nothing else took the name.
        let (ae, be) = (&was.m.elements[a], &now.m.elements[b]);
        if i > 0 && ae.name.is_some() && ae.name == be.name && !now.m.elements.iter().any(|e| e.qualified_name == ae.qualified_name) {
            return None;
        }
        // The first name is looked up from the place of the reference outward: any change there is a capture. A later
        // step is looked up in the element before it; it may change because the edit gave that element another type.
        if i > 0 && !deep.changed(&n.segments[i].name) {
            return None;
        }
        return Some(Hit { text: o.text.clone(), holder: holder.qualified_name.clone(), named: moved(key(was.m, a), deep.renames), would_name: key(now.m, b) });
    }
    None
}

/// Refuse the edit that turned `before` into `after` when it re-points a reference that was there before.
/// `events` are the renames and moves the edit reported, so an element that only changed its name is the same element.
pub(super) fn refuse(before: &Arc<Model>, after: &Arc<Model>, events: &[IdEvent]) -> Res<()> {
    if Arc::ptr_eq(before, after) {
        return Ok(());
    }
    let mut raw_from: Vec<&str> = Vec::new();
    let mut renames: Vec<(String, String)> = Vec::new();
    for ev in events {
        if let IdEvent::Rename { from, to } | IdEvent::Move { from, to, .. } = ev {
            raw_from.push(from.as_str());
            renames.push((norm_q(from), norm_q(to)));
        }
    }
    let (tb, ta) = (table_of(before), table_of(after));
    // The keys of the model before, as the model after knows the same elements: what the edit renamed or moved, and
    // everything inside it, has its new name.
    let mut renamed_keys: Option<(Vec<Key>, Vec<(Key, ElementId)>)> = None;
    if !renames.is_empty() {
        let inside = |q: &str| raw_from.iter().any(|f| q == *f || (q.len() > f.len() + 2 && q.starts_with(f) && q[f.len()..].starts_with("::")));
        let mut keys = tb.keys.clone();
        let mut any = false;
        for e in before.elements.iter().filter(|e| inside(&e.qualified_name)) {
            keys[e.id] = key_of(&moved(key(before, e.id), &renames));
            any = true;
        }
        if any {
            let holders = sorted_holders(before, &keys);
            renamed_keys = Some((keys, holders));
        }
    }
    let (kb, hb): (&[Key], &[(Key, ElementId)]) = match &renamed_keys {
        Some((k, h)) => (k, h),
        None => (&tb.keys, &tb.holders),
    };
    let was = Side { m: before, keys: kb, plain: &tb.keys };
    let now = Side { m: after, keys: &ta.keys, plain: &ta.keys };
    let mut deep = Deep { before, after, renames: &renames, known: None };
    let mut hits: Vec<Hit> = Vec::new();
    let mut total = 0usize;
    let mut hit = |h: Hit| {
        total += 1;
        if hits.len() < MAX_SHOWN {
            hits.push(h);
        }
    };
    // Walk the two sorted lists of holders together: the holders of one key before, and of the same key after.
    let (mut i, mut j) = (0usize, 0usize);
    while i < hb.len() && j < ta.holders.len() {
        let k = hb[i].0.min(ta.holders[j].0);
        let (i0, j0) = (i, j);
        while i < hb.len() && hb[i].0 == k {
            i += 1;
        }
        while j < ta.holders.len() && ta.holders[j].0 == k {
            j += 1;
        }
        let (old, new) = (&hb[i0..i], &ta.holders[j0..j]);
        if old.is_empty() || new.is_empty() {
            continue;
        }
        // One holder on each side whose references read the same, one for one: the case of nearly every element.
        if let ([(_, o)], [(_, n)]) = (old, new) {
            let (oe, ne) = (&before.elements[*o], &after.elements[*n]);
            if oe.refs.len() == ne.refs.len() && oe.refs.iter().zip(&ne.refs).all(|(x, y)| x.role == y.role && x.text == y.text) {
                work(oe.refs.len());
                for (x, y) in oe.refs.iter().zip(&ne.refs) {
                    if let Some(h) = compare(&was, &now, x, y, ne, &mut deep) {
                        hit(h);
                    }
                }
                continue;
            }
        }
        group(&was, &now, old, new, &mut deep, &mut hit);
    }
    drop(hit);
    if total == 0 {
        return Ok(());
    }
    hits.sort_by(|a, b| (&a.text, &a.holder, &a.named, &a.would_name).cmp(&(&b.text, &b.holder, &b.named, &b.would_name)));
    let lines: Vec<String> = hits.iter().map(|h| format!("'{}' in {} named {} and would name {}", h.text, h.holder, h.named, h.would_name)).collect();
    let more = if total > hits.len() { format!(" (and {} more)", total - hits.len()) } else { String::new() };
    Err(Fail::with(
        format!(
            "refused: this edit would change what {total} existing reference(s) mean, because the name it adds, changes or removes is nearer to them than the element they named: {}{more}. Nothing was written. Fix: use a name that no reference in that scope already uses, or first rewrite those references so they stay with their element (update_element with the qualified name, written $::A::b when a nearer name hides it), then run this edit again.",
            lines.join("; ")
        ),
        json!({
            "repointed_total": total,
            "repointed": hits.iter().map(|h| json!({ "reference": h.text, "in": h.holder, "named": h.named, "would_name": h.would_name })).collect::<Vec<_>>(),
        }),
    ))
}

/// The references of the holders of one key, when they do not read the same one for one: holders without a name share a
/// key, and the edit may have added, removed or rewritten a reference.
fn group(was: &Side, now: &Side, old: &[(Key, ElementId)], new: &[(Key, ElementId)], deep: &mut Deep, hit: &mut dyn FnMut(Hit)) {
    let refs = |side: &Side, list: &[(Key, ElementId)]| -> Vec<(ElementId, usize)> { list.iter().flat_map(|(_, e)| (0..side.m.elements[*e].refs.len()).map(|r| (*e, r))).collect() };
    let (olds, news) = (refs(was, old), refs(now, new));
    work(olds.len() + news.len());
    fn ref_of<'m>(side: &Side<'m>, at: (ElementId, usize)) -> &'m Reference {
        &side.m.elements[at.0].refs[at.1]
    }
    // References with the same role and text, by the order their holders have in the model.
    let mut buckets: HashMap<(u8, &str), (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (x, at) in olds.iter().enumerate() {
        let r = &was.m.elements[at.0].refs[at.1];
        buckets.entry((r.role as u8, r.text.as_str())).or_default().0.push(x);
    }
    for (x, at) in news.iter().enumerate() {
        let r = &now.m.elements[at.0].refs[at.1];
        buckets.entry((r.role as u8, r.text.as_str())).or_default().1.push(x);
    }
    // What is left on each side when the references that read and resolve the same are taken out.
    let (mut left_old, mut left_new): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut order: Vec<(&(u8, &str), &(Vec<usize>, Vec<usize>))> = buckets.iter().collect();
    order.sort_by_key(|(_, (o, n))| (o.first().copied().unwrap_or(usize::MAX), n.first().copied().unwrap_or(usize::MAX)));
    for (_, (o, n)) in order {
        if o.is_empty() || n.is_empty() {
            left_old.extend(o);
            left_new.extend(n);
            continue;
        }
        let rows_old: Vec<Vec<Option<Key>>> = o.iter().map(|x| was.row(ref_of(was, olds[*x]))).collect();
        let rows_new: Vec<Vec<Option<Key>>> = n.iter().map(|x| now.row(ref_of(now, news[*x]))).collect();
        let mut left: HashMap<&Vec<Option<Key>>, i64> = HashMap::new();
        for r in &rows_old {
            *left.entry(r).or_default() += 1;
        }
        let mut changed_new: Vec<usize> = Vec::new();
        for (x, r) in n.iter().zip(&rows_new) {
            match left.get_mut(r) {
                Some(c) if *c > 0 => *c -= 1,
                _ => changed_new.push(*x),
            }
        }
        let mut changed_old: Vec<usize> = Vec::new();
        for (x, r) in o.iter().zip(&rows_old) {
            match left.get_mut(r) {
                Some(c) if *c > 0 => {
                    *c -= 1;
                    changed_old.push(*x);
                }
                _ => {}
            }
        }
        // The same text in the same scope: the ones that resolve another way than before, in their order.
        for (x, y) in changed_old.iter().zip(&changed_new) {
            if let Some(h) = compare(was, now, ref_of(was, olds[*x]), ref_of(now, news[*y]), &now.m.elements[news[*y].0], deep) {
                hit(h);
            }
        }
        let paired = changed_old.len().min(changed_new.len());
        left_old.extend(&changed_old[paired..]);
        left_new.extend(&changed_new[paired..]);
    }
    if left_old.is_empty() || left_new.is_empty() {
        return;
    }
    // A reference whose text is gone was removed, given another text by the edit, or rewritten because the edit renamed
    // or moved what it names. A rewritten one must still name the same element: it is looked for among the new texts by
    // what it resolves to (the whole row, or the last step when the rewrite changed the count of steps).
    left_old.sort_unstable();
    left_new.sort_unstable();
    let mut by_row: HashMap<(u8, Vec<Option<Key>>), Vec<usize>> = HashMap::new();
    let mut by_last: HashMap<(u8, usize, Key), Vec<usize>> = HashMap::new();
    for y in left_new.iter().rev() {
        let r = ref_of(now, news[*y]);
        let row = now.row(r);
        if let Some(Some(last)) = row.last() {
            by_last.entry((r.role as u8, row.len(), *last)).or_default().push(*y);
        }
        by_row.entry((r.role as u8, row)).or_default().push(*y);
    }
    let mut used: HashSet<usize> = HashSet::new();
    let mut lost: Vec<usize> = Vec::new();
    for x in &left_old {
        let r = ref_of(was, olds[*x]);
        let row = was.row(r);
        let mut take = |list: Option<&mut Vec<usize>>| -> bool {
            let Some(list) = list else { return false };
            while let Some(y) = list.pop() {
                if used.insert(y) {
                    return true;
                }
            }
            false
        };
        if take(by_row.get_mut(&(r.role as u8, row.clone()))) {
            continue;
        }
        // Another count of steps (`Lib::User` is now `User`): the element named last decides.
        let Some(Some(last)) = row.last().copied() else { continue };
        let other_len = by_last.iter_mut().filter(|((role, len, k), _)| *role == r.role as u8 && *len != row.len() && *k == last).map(|(_, l)| l).next();
        if take(other_len) {
            continue;
        }
        // Only a reference to something the edit renamed or moved is rewritten by the tool; any other text that is gone
        // was changed by the caller.
        if r.segments.iter().any(|s| s.resolved.is_some_and(|t| was.keys[t] != was.plain[t])) {
            lost.push(*x);
        }
    }
    if lost.is_empty() {
        return;
    }
    // The rewritten references that name another element now, each with the new text in its place: the first text of the
    // same role that no reference before accounts for.
    let mut free: Vec<usize> = left_new.iter().copied().filter(|y| !used.contains(y)).collect();
    for x in lost {
        let o = ref_of(was, olds[x]);
        let Some(at) = free.iter().position(|y| ref_of(now, news[*y]).role == o.role) else { continue };
        let y = free.remove(at);
        let n = ref_of(now, news[y]);
        let (Some(a), Some(b)) = (o.resolved(), n.resolved()) else { continue };
        if now.redefines(b, was.keys[a]) {
            continue;
        }
        hit(Hit { text: o.text.clone(), holder: now.m.elements[news[y].0].qualified_name.clone(), named: moved(key(was.m, a), deep.renames), would_name: key(now.m, b) });
    }
}

/// Two faults of a namespace that no write may bring in, whatever tool made it (a create, an update, raw text, a statement
/// with an owner, a move, a batch): a member inside an alias, and a short name that is also the name or the short name of
/// a member beside it. `create_element` refuses both before it writes; this is the check on what was written, so the
/// other paths cannot differ. A fault that `before` already had is not the edit's and stays.
pub(super) fn refuse_namespace_faults(before: &Model, after: &Model) -> Res<()> {
    let in_alias = |m: &Model| -> HashMap<String, usize> { m.elements.iter().filter(|e| e.kind == "alias" && !e.children.is_empty()).map(|e| (key(m, e.id), e.children.len())).collect() };
    let now = in_alias(after);
    if !now.is_empty() {
        let was = in_alias(before);
        if let Some((alias, _)) = now.iter().find(|(k, n)| was.get(*k).copied().unwrap_or(0) < **n) {
            return Err(Fail::with(
                format!("refused: this edit would put a member inside '{alias}', which is an alias, and an alias holds no members. Nothing was written. Fix: put the member in the element the alias stands for."),
                json!({ "rule": "member-under-alias", "alias": alias }),
            ));
        }
    }
    let clashes = |m: &Model| -> HashSet<(String, String)> {
        // Only the namespaces that hold a short name are read.
        let owners: HashSet<Option<ElementId>> = m.elements.iter().filter(|e| e.short_name.is_some()).map(|e| e.owner).collect();
        let mut out = HashSet::new();
        for o in owners {
            let kids: &[ElementId] = match o {
                Some(o) => &m.elements[o].children,
                None => &m.roots,
            };
            let (mut names, mut shorts): (HashMap<&str, usize>, HashMap<&str, usize>) = (HashMap::new(), HashMap::new());
            for c in kids.iter().map(|c| &m.elements[*c]) {
                if let (Some(n), false) = (&c.name, c.implied_name) {
                    *names.entry(n.as_str()).or_default() += 1;
                }
                if let Some(s) = &c.short_name {
                    *shorts.entry(s.as_str()).or_default() += 1;
                }
            }
            for c in kids.iter().map(|c| &m.elements[*c]) {
                let Some(s) = c.short_name.as_deref() else { continue };
                // Its own name may repeat the short name (`part <a> a;`); a member beside it may not.
                let own = usize::from(!c.implied_name && c.name.as_deref() == Some(s));
                if shorts[s] > 1 || names.get(s).copied().unwrap_or(0) > own {
                    out.insert((o.map(|o| key(m, o)).unwrap_or_default(), s.to_string()));
                }
            }
        }
        out
    };
    let now = clashes(after);
    if !now.is_empty() {
        let was = clashes(before);
        if let Some((owner, short)) = now.iter().filter(|c| !was.contains(*c)).min() {
            let place = if owner.is_empty() { "the top of the model".to_string() } else { format!("'{owner}'") };
            return Err(Fail::with(
                format!("refused: after this edit the short name '{short}' would also be the name or the short name of another member of {place}, so a reference to '{short}' would be ambiguous. Nothing was written. Fix: choose a short name or a name that no member beside it has."),
                json!({ "rule": "short-name-clash", "owner": owner, "short_name": short }),
            ));
        }
    }
    Ok(())
}
