//! Suspect links, derived and never stored in the model text.
//!
//! The public feature: a satisfy, verify, refine or derive link whose tracked end changed since the link was last reviewed is
//! "suspect" until a person clears it. Here a link is suspect when the tracked end differs between a revision (a baseline, or
//! the model before a proposal) and now. "Differs" is decided by comparing the end's own text facts one by one: its name, its
//! text (documentation), the values it states and its type. A hash decides nothing about a change. The default tracked end is the
//! requirement side; the other end, or both, can be asked for, and so can a subset of the fields.
//!
//! Ends and links are matched across the two revisions by the identity the change package uses (`change::pair`: a rename is a
//! rename, a moved element is the same element), never by a name. The walk fails closed: a current link whose tracked end has no
//! match before is listed ("end new, renamed or moved: not matched in <revision>"), an older link with no match now is listed
//! ("link removed or its end removed"), and a walk that a bound cut says "not checked".
//!
//! Clearing is a person's act and is not here: the review store (`suspect-reviews.jsonl` in the workspace folder, not the model)
//! holds who cleared which fingerprint of which link. This module only reads it: a link is cleared when a review holds the
//! fingerprint the end has now, so a later change of the end makes the link suspect again.

use super::{change, graph, line_of};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The fields of the fingerprint, in the order they are shown.
pub const FIELDS: [&str; 4] = ["name", "text", "values", "type"];

/// Every field a fingerprint can hold, in canonical order. `id` and `stereotypes` are asked for by name, so the default four
/// and every stored review keep their bytes.
pub const ALL_FIELDS: [&str; 6] = ["name", "text", "values", "type", "id", "stereotypes"];

/// Most links one reply lists; the count is always the full one.
pub const MAX_LISTED: usize = 500;
/// Longest before or after value one entry shows.
const MAX_VALUE: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    /// The requirement side of the link (for a derive link, the supplier): the default.
    Requirement,
    /// The other end.
    Other,
    Both,
}

impl Track {
    pub fn parse(s: &str) -> Option<Track> {
        match s {
            "requirement" => Some(Track::Requirement),
            "other" => Some(Track::Other),
            "both" => Some(Track::Both),
            _ => None,
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Track::Requirement => "requirement",
            Track::Other => "other",
            Track::Both => "both",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub track: Track,
    pub fields: Vec<&'static str>,
    /// How the older revision is called in a reason ("baseline \"B1\"", "the model before the proposal").
    pub since: String,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { track: Track::Requirement, fields: FIELDS.to_vec(), since: "the older revision".to_string() }
    }
}

/// One link that is watched.
#[derive(Clone, Debug)]
pub struct Link {
    pub kind: &'static str,
    /// `kind|client|supplier` by qualified names: the same link has the same key in every revision.
    pub key: String,
    pub via: ElementId,
    pub source: ElementId,
    pub target: ElementId,
    /// Set when a trace link is read as a satisfy link.
    pub as_satisfy: bool,
}

impl Link {
    /// The requirement side: the target of satisfy, verify, refine and derive (the supplier); for a trace link read as satisfy,
    /// the requirement end.
    fn requirement_end(&self, m: &Model) -> ElementId {
        if self.as_satisfy && m.elements[self.source].kind == "requirement" && m.elements[self.target].kind != "requirement" {
            self.source
        } else {
            self.target
        }
    }
    fn other_end(&self, m: &Model) -> ElementId {
        let r = self.requirement_end(m);
        if r == self.target {
            self.source
        } else {
            self.target
        }
    }
}

/// The links watched in a model: satisfy, verify, refine and derive, and a trace link that joins a requirement and an element
/// that is not one (this model family writes satisfy that way).
pub fn links(m: &Model) -> Vec<Link> {
    links_cut(m).0
}

/// `links`, and the relationship statements the reading bound cut.
pub fn links_cut(m: &Model) -> (Vec<Link>, Vec<graph::Cut>) {
    let (stmts, cuts) = graph::statements_cut(m);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for s in &stmts {
        let (Some(a), Some(b)) = (s.source.id, s.target.id) else { continue };
        let (kind, as_satisfy) = match s.kind {
            "satisfy" => ("satisfy", false),
            "verify" => ("verify", false),
            "refine" => ("refine", false),
            "derive" => ("derive", false),
            "trace" => {
                let req = |x: ElementId| m.elements[x].kind == "requirement";
                if req(a) != req(b) {
                    ("trace", true)
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        let key = format!("{kind}|{}|{}", m.elements[a].qualified_name, m.elements[b].qualified_name);
        if seen.insert(key.clone()) {
            out.push(Link { kind, key, via: s.via, source: a, target: b, as_satisfy });
        }
    }
    (out, cuts)
}

/// The text of one field of an element.
fn field_text(m: &Model, e: &Element, field: &str) -> String {
    match field {
        "name" => e.name.clone().unwrap_or_default(),
        "text" => super::text::doc_of(m, e).unwrap_or_default(),
        "values" => {
            let mut v: Vec<String> = super::text::values(m, e).into_iter().map(|(n, val, _)| format!("{n} = {val}")).collect();
            if let Some(own) = &e.value {
                v.push(format!("= {}", own.trim()));
            }
            v.sort();
            v.join("; ")
        }
        "type" => {
            let mut v: Vec<String> = e.refs_of(RefRole::Type).map(|r| r.resolved().map(|t| m.elements[t].qualified_name.clone()).unwrap_or_else(|| r.text.clone())).collect();
            v.sort();
            v.join("; ")
        }
        "id" => e.short_name.clone().unwrap_or_default(),
        "stereotypes" => {
            // The kind keyword, the prefixes and the metadata applied to the element.
            let mut v: Vec<String> = e.prefixes.clone();
            for c in &e.children {
                let ce = &m.elements[*c];
                if ce.kind == "metadata" {
                    v.extend(ce.refs_of(RefRole::Type).map(|r| format!("#{}", r.resolved().map(|t| m.elements[t].qualified_name.clone()).unwrap_or_else(|| r.text.clone()))));
                }
            }
            v.sort();
            v.insert(0, e.keyword().to_string());
            v.join("; ")
        }
        _ => String::new(),
    }
}

// ---- SHA-256 (FIPS 180-4), for the fingerprint of a review. No crate in the lock file offers it.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The SHA-256 of `data` as 64 lower-case hex digits.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// The version tag of the canonical form. A change of the form is a change of the tag.
pub const FORM: &str = "sysml-bridge-suspect-v1";

fn put(out: &mut Vec<u8>, name: &str, bytes: &[u8]) {
    out.extend_from_slice(&(name.len() as u64).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Length-prefixed parts (name, byte length, bytes), under a version tag: the form every hash of this module is taken over.
pub fn frame(tag: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, "form", tag.as_bytes());
    for (n, b) in parts {
        put(&mut out, n, b);
    }
    out
}

/// The canonical form of the tracked ends: the version tag, then for each end (in the order given) its side, and for each
/// field in the fixed order of `ALL_FIELDS` its name and its value. Every name and value is written with its byte length first, so
/// no value can look like a field boundary.
pub fn canonical(ends: &[(&str, Vec<(&'static str, String)>)]) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, "form", FORM.as_bytes());
    for (side, fields) in ends {
        put(&mut out, "end", side.as_bytes());
        for f in ALL_FIELDS.iter() {
            if let Some((_, v)) = fields.iter().find(|(k, _)| k == f) {
                put(&mut out, f, v.as_bytes());
            }
        }
    }
    out
}

/// The text of every asked field of an element.
fn values_of(m: &Model, id: ElementId, fields: &[&'static str]) -> Vec<(&'static str, String)> {
    let e = &m.elements[id];
    fields.iter().map(|f| (*f, field_text(m, e, f))).collect()
}

/// The fingerprint of the tracked ends and the hash of each field: what a review stores. `hash` makes the fingerprint (SHA-256);
/// the per-field hashes are always SHA-256.
pub type Hasher = fn(&[u8]) -> String;

fn stamp_of(ends: &[(&str, Vec<(&'static str, String)>)], hash: Hasher) -> (String, Value) {
    let fp = hash(&canonical(ends));
    let mut per: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (side, fields) in ends {
        let e = per.entry(side.to_string()).or_default();
        for (k, v) in fields {
            let mut b = Vec::new();
            put(&mut b, k, v.as_bytes());
            e.insert(k.to_string(), sha256_hex(&b));
        }
    }
    (fp, json!(per))
}

/// The fields of an end and their fingerprint (kept for callers that ask for one end).
pub fn fingerprint(m: &Model, id: ElementId, fields: &[&'static str]) -> (String, BTreeMap<&'static str, String>) {
    let vals = values_of(m, id, fields);
    let fp = sha256_hex(&canonical(&[("end", vals.clone())]));
    (fp, vals.into_iter().collect())
}

fn clip(s: &str) -> String {
    if s.chars().count() > MAX_VALUE {
        format!("{}...", s.chars().take(MAX_VALUE).collect::<String>())
    } else {
        s.to_string()
    }
}

fn end_json(m: &Model, id: ElementId) -> Value {
    let e = &m.elements[id];
    json!({ "ref": e.qualified_name, "name": e.name, "kind": e.keyword(), "file": m.files[e.file].path, "line": line_of(m, e) })
}

/// What the review store says: for a link key, the reviews in the order they were written.
pub type Reviews = HashMap<String, Vec<Value>>;

fn ends_of(l: &Link, m: &Model, track: Track) -> Vec<(&'static str, ElementId)> {
    let (rq, ot) = (l.requirement_end(m), l.other_end(m));
    match track {
        Track::Requirement => vec![("requirement", rq)],
        Track::Other => vec![("other", ot)],
        Track::Both => vec![("requirement", rq), ("other", ot)],
    }
}

/// What a review of the link `key` in `m` stores: its key, the way of tracking, the fingerprint (SHA-256 over the canonical form)
/// and the hash of each field of each tracked end, so that a later read can say which field changed.
pub fn review_stamp(m: &Model, key: &str, opts: &Opts) -> Option<Value> {
    let l = links(m).into_iter().find(|l| l.key == key)?;
    let ends: Vec<(&str, Vec<(&'static str, String)>)> = ends_of(&l, m, opts.track).into_iter().map(|(s, id)| (s, values_of(m, id, &opts.fields))).collect();
    let (fp, per) = stamp_of(&ends, sha256_hex);
    Some(json!({ "key": l.key, "track": opts.track.word(), "fields": opts.fields, "form": FORM, "fingerprint": fp, "field_hashes": per }))
}

/// The fingerprint a review clears for the link `key` in `m`.
pub fn current_fingerprint(m: &Model, key: &str, opts: &Opts) -> Option<String> {
    review_stamp(m, key, opts).and_then(|s| s["fingerprint"].as_str().map(str::to_string))
}

/// Whether a stored review holds exactly what the link has now: the same way of tracking, the same fields, the same fingerprint
/// after, the same fingerprint before (a review made against another `since` does not carry over; a review with none never holds)
/// and the same hash of every field. Both the fingerprint and the field hashes must agree.
fn review_holds(r: &Value, now: &Value) -> bool {
    r["track"] == now["track"] && r["fields"] == now["fields"] && r["form"] == now["form"] && r["fingerprint"] == now["fingerprint"] && r["before_fingerprint"] == now["before_fingerprint"] && r["field_hashes"] == now["field_hashes"]
}

/// The fields whose hash differs between a review and now.
fn fields_since_review(r: &Value, now: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for (side, f) in now["field_hashes"].as_object().into_iter().flatten() {
        for (k, h) in f.as_object().into_iter().flatten() {
            if r["field_hashes"][side][k] != *h {
                out.push(format!("{side} {k}"));
            }
        }
    }
    out
}

/// The watched links whose tracked end differs between `before` and `now`, and every one that cannot be matched. `reviews` is the
/// review store. Each entry says which fields changed (before and after), why it is listed, and whether a review clears it.
pub fn list(before: &Model, now: &Model, opts: &Opts, reviews: &Reviews) -> Value {
    list_with(before, now, opts, reviews, sha256_hex)
}

/// `list` with the hash of the fingerprint given. A test hands in a hash that collides; the answer must not change, because a
/// change is found by comparing the values and a review is held to the hash of every field as well.
pub fn list_with(before: &Model, now: &Model, opts: &Opts, reviews: &Reviews, hash: Hasher) -> Value {
    let no_ids = BTreeMap::new();
    let p = change::pair(before, now, &no_ids, &no_ids, false);
    let ((links_b, cuts_b), (links_n, cuts_n)) = (links_cut(before), links_cut(now));
    let mut rows: Vec<Value> = Vec::new();
    let (mut total, mut cleared_n, mut unmatched_n, mut removed_n, mut changed_n) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut now_keys: BTreeSet<(&'static str, ElementId, ElementId)> = BTreeSet::new();
    for l in &links_n {
        now_keys.insert((l.kind, l.source, l.target));
    }
    // The links the older revision had, by the elements after that stand for their ends.
    let mut was: BTreeSet<(&'static str, ElementId, ElementId)> = BTreeSet::new();
    for l in &links_b {
        if let (Some(s), Some(t)) = (p.pair_b.get(&l.source), p.pair_b.get(&l.target)) {
            was.insert((l.kind, *s, *t));
        }
    }
    for l in &links_n {
        let ends = ends_of(l, now, opts.track);
        let mut changed: Vec<Value> = Vec::new();
        let mut tracked: Vec<Value> = Vec::new();
        let mut unmatched = false;
        let mut now_ends: Vec<(&str, Vec<(&'static str, String)>)> = Vec::new();
        let mut before_ends: Vec<(&str, Vec<(&'static str, String)>)> = Vec::new();
        for (side, x) in &ends {
            let va = values_of(now, *x, &opts.fields);
            now_ends.push((side, va.clone()));
            match p.pair_a.get(x) {
                None => {
                    before_ends.push((side, Vec::new()));
                    unmatched = true;
                    tracked.push(json!({ "end": side, "element": end_json(now, *x) }));
                }
                Some(y) => {
                    let vb = values_of(before, *y, &opts.fields);
                    before_ends.push((side, vb.clone()));
                    let mut differs = false;
                    for ((f, a), (_, b)) in va.iter().zip(vb.iter()) {
                        if a != b {
                            differs = true;
                            changed.push(json!({ "end": side, "field": f, "before": clip(b), "after": clip(a) }));
                        }
                    }
                    if differs {
                        tracked.push(json!({ "end": side, "element": end_json(now, *x) }));
                    }
                }
            }
        }
        // A link drawn since, to an end that was there before, relied on nothing the older revision said.
        if changed.is_empty() && !unmatched || !unmatched && !was.contains(&(l.kind, l.source, l.target)) {
            continue;
        }
        total += 1;
        if unmatched {
            unmatched_n += 1;
        } else {
            changed_n += 1;
        }
        let (fp_now, per_now) = stamp_of(&now_ends, hash);
        let (fp_before, _) = stamp_of(&before_ends, hash);
        let now_stamp = json!({ "track": opts.track.word(), "fields": opts.fields, "form": FORM, "fingerprint": fp_now, "before_fingerprint": fp_before, "field_hashes": per_now });
        let hist = reviews.get(&l.key);
        let review = hist.and_then(|v| v.iter().rev().find(|r| review_holds(r, &now_stamp))).cloned();
        let since_review = if review.is_none() { hist.and_then(|v| v.last()).map(|r| fields_since_review(r, &now_stamp)).unwrap_or_default() } else { Vec::new() };
        if review.is_some() {
            cleared_n += 1;
        }
        if rows.len() >= MAX_LISTED {
            continue;
        }
        let via = &now.elements[l.via];
        let other = l.other_end(now);
        rows.push(json!({
            "key": l.key,
            "reason": if unmatched { format!("end new, renamed or moved: not matched in {}", opts.since) } else { "changed".to_string() },
            "link": { "kind": l.kind, "read_as": if l.as_satisfy { "satisfy" } else { l.kind }, "ref": via.qualified_name, "file": now.files[via.file].path, "line": line_of(now, via) },
            "client": end_json(now, l.source),
            "supplier": end_json(now, l.target),
            "other_end": end_json(now, other),
            "tracked": tracked,
            "changed": changed,
            "fingerprint": { "before": fp_before, "after": fp_now, "form": FORM },
            "cleared": review.is_some(),
            "review": review,
            "earlier_reviews": hist.map(|v| v.len()).unwrap_or(0),
            "changed_since_last_review": since_review,
        }));
    }
    // Older links that have no match now: the link or an end of it is gone (or cannot be matched).
    for l in &links_b {
        let (Some(s), Some(t)) = (p.pair_b.get(&l.source), p.pair_b.get(&l.target)) else {
            removed_n += 1;
            total += 1;
            removed_row(before, l, opts, &mut rows);
            continue;
        };
        if now_keys.contains(&(l.kind, *s, *t)) {
            continue;
        }
        removed_n += 1;
        total += 1;
        removed_row(before, l, opts, &mut rows);
    }
    let mut not_checked: Vec<String> = Vec::new();
    for (label, cuts) in [("the older revision", &cuts_b), ("the newer revision", &cuts_n)] {
        if !cuts.is_empty() {
            not_checked.push(format!("not checked: {label} has {} relationship statement(s) cut by the reading bound, so links among them were not compared", cuts.len()));
        }
    }
    json!({
        "tracked_end": opts.track.word(),
        "fields": opts.fields,
        "total": total,
        "listed": rows.len(),
        "cleared": cleared_n,
        "suspect": total - cleared_n,
        "by_reason": { "changed": changed_n, "end_not_matched": unmatched_n, "link_or_end_removed": removed_n },
        "not_checked": not_checked,
        "links": rows,
        "rule": "A link is suspect when its tracked end (name, text, stated values, type; the fields asked for) differs between the older revision and now, found by comparing the values. Ends and links are matched by the identity the change package uses, not by name. A current link whose tracked end has no match before is listed too, and so is an older link with no match now. A review clears a link only for the exact values it names (the fingerprint and the hash of every field): when the end changes again, the link is suspect again.",
    })
}

fn removed_row(before: &Model, l: &Link, opts: &Opts, rows: &mut Vec<Value>) {
    if rows.len() >= MAX_LISTED {
        return;
    }
    let via = &before.elements[l.via];
    rows.push(json!({
        "key": l.key,
        "reason": "link removed or its end removed",
        "link": { "kind": l.kind, "read_as": if l.as_satisfy { "satisfy" } else { l.kind }, "ref": via.qualified_name, "file": before.files[via.file].path, "line": line_of(before, via), "in": opts.since },
        "client": end_json(before, l.source),
        "supplier": end_json(before, l.target),
        "other_end": end_json(before, l.other_end(before)),
        "tracked": [],
        "changed": [],
        "fingerprint": { "after": Value::Null, "form": FORM },
        "cleared": false,
        "review": Value::Null,
        "earlier_reviews": 0,
        "changed_since_last_review": [],
    }));
}

/// Links a change makes suspect, for a proposal: the links whose tracked requirement end the edits change, read as the model
/// before the edits against the model with them.
pub fn changed_by(before: &Model, after: &Model, reviews: &Reviews) -> Value {
    list(before, after, &Opts::default(), reviews)
}

/// The elements at the other end of the suspect links of a listing: the elements whose verdict a change must give.
pub fn other_ends(listing: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for l in listing["links"].as_array().into_iter().flatten() {
        if let Some(r) = l["other_end"]["ref"].as_str() {
            if !out.contains(&r.to_string()) {
                out.push(r.to_string());
            }
        }
    }
    out
}
