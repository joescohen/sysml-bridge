//! Quick find over element names: prefixes, globs and camel-case humps, with word initials, exact short ids,
//! qualified paths and paging.

use super::line_of;
use super::Res;
use crate::model::*;
use serde_json::{json, Value};

pub const PAGE: usize = 100;
pub const MAX_PAGE: usize = 1000;
/// Longest query read.
pub const MAX_QUERY: usize = 200;

pub struct Opts {
    pub text: String,
    pub scope: String,
    pub match_anywhere: bool,
    pub use_camel_case: bool,
    pub offset: usize,
    pub limit: usize,
}

/// The words of a name: split at spaces, underscores, hyphens, digits-to-letters and lower-to-upper changes.
fn words(name: &str, camel: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        let split = !c.is_alphanumeric() || prev.map(|p| (camel && p.is_lowercase() && c.is_uppercase()) || (p.is_alphabetic() && c.is_ascii_digit())).unwrap_or(false);
        if split && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if c.is_alphanumeric() {
            cur.push(c);
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whole-string glob matching, bounded by pattern length times candidate length.
fn wild(pattern: &[char], text: &[char]) -> bool {
    let p = pattern;
    let m = p.len();
    let close = |dp: &mut Vec<bool>| {
        for j in 0..m {
            if p[j] == '*' && dp[j] {
                dp[j + 1] = true;
            }
        }
    };
    let mut dp = vec![false; m + 1];
    dp[0] = true;
    close(&mut dp);
    for &t in text {
        let mut next = vec![false; m + 1];
        for j in 0..m {
            if p[j] == '*' {
                next[j] |= dp[j];
            } else if dp[j] && (p[j] == '?' || p[j] == t) {
                next[j + 1] = true;
            }
        }
        close(&mut next);
        dp = next;
    }
    dp[m]
}

enum Kind {
    Id,
    Exact,
    Prefix,
    Initials,
    Contains,
    /// The words of a requirement's text (its doc), not its name.
    Text,
}

impl Kind {
    fn word(&self) -> &'static str {
        match self {
            Kind::Id => "id",
            Kind::Exact => "exact",
            Kind::Prefix => "prefix",
            Kind::Initials => "initials",
            Kind::Contains => "contains",
            Kind::Text => "text",
        }
    }
}

struct Matcher {
    q: String,
    qc: Vec<char>,
    wildcard: bool,
    path: bool,
    camel: String,
    anywhere: bool,
    use_camel_case: bool,
}

impl Matcher {
    fn new(text: &str, anywhere: bool, use_camel_case: bool) -> Matcher {
        let text = normalize(text);
        let q = text.to_lowercase();
        let anywhere = anywhere || q.starts_with('*');
        let pattern = format!("{}{}*", if anywhere { "*" } else { "" }, q.trim_start_matches('*'));
        let camel = if text.chars().any(char::is_lowercase) && !text.chars().any(char::is_uppercase) { text.to_uppercase() } else { text };
        Matcher { qc: pattern.chars().collect(), wildcard: q.contains('*') || q.contains('?'), path: q.contains("::"), q, camel, anywhere, use_camel_case }
    }

    fn matches(&self, name: &str, id: Option<&str>, qualified: &str) -> Option<Kind> {
        if self.q.is_empty() || self.q.chars().all(|c| c == '*') {
            return Some(Kind::Prefix);
        }
        if let Some(i) = id {
            if normalize(i).to_lowercase() == self.q {
                return Some(Kind::Id);
            }
        }
        let original = normalize(if self.path { qualified } else { name });
        if original.is_empty() {
            return None;
        }
        let subject = original.to_lowercase();
        if wild(&self.qc, &subject.chars().collect::<Vec<_>>()) {
            return Some(if self.wildcard {
                Kind::Contains
            } else if subject == self.q {
                Kind::Exact
            } else if subject.starts_with(&self.q) {
                Kind::Prefix
            } else {
                Kind::Contains
            });
        }
        if self.use_camel_case && camel_matches(&self.camel, &original, self.anywhere) {
            return Some(Kind::Initials);
        }
        if !self.path && self.q.chars().all(|c| c.is_alphabetic()) && self.q.len() >= 2 {
            let initials: String = words(name, self.use_camel_case).iter().filter_map(|w| w.chars().next()).flat_map(|c| c.to_lowercase()).collect();
            if initials.starts_with(&self.q) || initials.contains(&self.q) {
                return Some(Kind::Initials);
            }
        }
        None
    }
}

/// The path that tells two elements of one name apart: the last two segments of the owner path.
fn telling(model: &Model, e: &Element) -> String {
    let owner = e.owner.map(|o| model.elements[o].qualified_name.clone()).unwrap_or_default();
    let segs: Vec<&str> = owner.split("::").collect();
    segs[segs.len().saturating_sub(2)..].join("::")
}

fn is_type(e: &Element) -> bool {
    e.is_def || e.kind == "requirement"
}

pub fn find(model: &Model, o: &Opts) -> Res<Value> {
    let text = o.text.trim();
    if text.chars().count() > MAX_QUERY {
        return Err(format!("text is longer than {MAX_QUERY} characters.").into());
    }
    if !matches!(o.scope.as_str(), "type" | "any" | "") {
        return Err(format!("scope is type or any, not {}.", super::text::quote_clip(&o.scope)).into());
    }
    let mode = if o.scope.is_empty() { "any" } else { o.scope.as_str() };
    let mt = Matcher::new(text, o.match_anywhere, o.use_camel_case);
    let limit = if o.limit == 0 { PAGE } else { o.limit.min(MAX_PAGE) };
    // A requirement is also found by its text: "4 m/s" finds the requirement that states it (three characters or more, no glob).
    let words_q = text.to_lowercase();
    let by_text = words_q.chars().count() >= 3 && !words_q.contains('*') && !words_q.contains("::");
    let text_hit = |e: &Element| by_text && e.kind.starts_with("requirement") && e.doc.as_deref().is_some_and(|d| d.to_lowercase().contains(&words_q));
    let elem_hits = |mt: &Matcher, types_only: bool| -> Vec<(Kind, String, &Element)> {
        model
            .elements
            .iter()
            .filter(|e| e.name.is_some() && !e.implied_name && (!types_only || is_type(e)))
            .filter_map(|e| {
                let name = e.name.as_deref().unwrap_or("");
                let id = e.short_name.as_deref();
                mt.matches(name, id, &e.qualified_name).or_else(|| text_hit(e).then_some(Kind::Text)).map(|k| (k, e.qualified_name.clone(), e))
            })
            .collect()
    };
    let counts = json!({ "type": elem_hits(&mt, true).len(), "any": elem_hits(&mt, false).len() });
    let mut rows: Vec<(bool, bool, String, Value)> = Vec::new();
    {
        for (k, q, e) in elem_hits(&mt, mode == "type") {
            let mut r = super::brief(model, e);
            r["path"] = json!(telling(model, e));
            r["match"] = json!(k.word());
            if let Some(s) = &e.short_name {
                r["id"] = json!(s);
            }
            r["line"] = json!(line_of(model, e));
            if matches!(k, Kind::Text) {
                r["text"] = json!(snippet(e.doc.as_deref().unwrap_or(""), &words_q));
            }
            rows.push((low_priority(e) || matches!(k, Kind::Text), !matches!(k, Kind::Id), q, r));
        }
    }
    rows.sort_by(|a, b| {
        let an = a.3["name"].as_str().unwrap_or("");
        let bn = b.3["name"].as_str().unwrap_or("");
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| if mt.anywhere { positions(&mt.q, an).cmp(&positions(&mt.q, bn)) } else { std::cmp::Ordering::Equal })
            .then_with(|| name_class(an).cmp(&name_class(bn)))
            .then_with(|| super::text::natural_cmp(an, bn))
            .then_with(|| a.2.cmp(&b.2))
    });
    let total = rows.len();
    let hits: Vec<Value> = rows.into_iter().skip(o.offset).take(limit).map(|r| r.3).collect();
    let shown = o.offset + hits.len();
    Ok(json!({
        "text": text,
        "scope": mode,
        "total": total,
        "returned": hits.len(),
        "shown": shown,
        "more": total > shown,
        "next_offset": if total > shown { json!(shown) } else { Value::Null },
        "counts_by_scope": counts,
        "hits": hits,
        "rank_rule": "ordinary elements before relationships, comments and metadata/value-like kinds; exact short IDs first within each priority; in contains mode earlier literal-piece positions first; then natural name order (symbol-first and empty names last), then path. Prefix matching by default; match_anywhere or leading * enables contains. Camel humps default on; word initials and :: paths are also supported.",
    }))
}

/// The words around the first match of `q` (lower case) in `doc`: about 40 characters each side, on one line.
fn snippet(doc: &str, q: &str) -> String {
    let flat: String = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let low = flat.to_lowercase();
    let Some(at) = low.find(q) else { return flat.chars().take(90).collect() };
    let chars: Vec<(usize, char)> = flat.char_indices().collect();
    let i = chars.iter().position(|(b, _)| *b >= at).unwrap_or(0);
    let (a, b) = (i.saturating_sub(40), (i + q.chars().count() + 40).min(chars.len()));
    let body: String = chars[a..b].iter().map(|(_, c)| c).collect();
    format!("{}{}{}", if a > 0 { "…" } else { "" }, body, if b < chars.len() { "…" } else { "" })
}

fn normalize(s: &str) -> String {
    s.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect()
}

/// A small state machine for hump jumps; no regex syntax is interpreted.
fn camel_matches(pattern: &str, name: &str, anywhere: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let mut states = vec![false; n.len() + 1];
    states[0] = true;
    if anywhere {
        states.fill(true);
    }
    for (i, &c) in p.iter().enumerate() {
        let mut next = vec![false; n.len() + 1];
        let jump_upper = i > 0 && c.is_uppercase();
        let jump_digit = i > 0 && c.is_numeric() && !p[i - 1].is_numeric();
        for j in 0..=n.len() {
            if c == '*' {
                if states[j] || (j > 0 && next[j - 1]) {
                    next[j] = true;
                }
            } else if j < n.len() {
                if states[j] && (n[j] == c || (i == 0 && j == 0 && c.is_uppercase() && n[j].to_uppercase().collect::<String>() == c.to_string())) {
                    next[j + 1] = true;
                }
                if states[j] && ((jump_upper && !n[j].is_uppercase()) || (jump_digit && !n[j].is_numeric())) {
                    states[j + 1] = true;
                }
            }
        }
        states = next;
    }
    states.into_iter().any(|s| s)
}

fn low_priority(e: &Element) -> bool {
    matches!(
        e.kind.as_str(),
        "dependency"
            | "connection"
            | "binding"
            | "succession"
            | "flow"
            | "message"
            | "allocation"
            | "allocate"
            | "satisfy"
            | "verify"
            | "assoc"
            | "association"
            | "generalization"
            | "relationship"
            | "transition"
            | "comment"
            | "doc"
            | "metadata"
            | "slot"
            | "instance"
            | "instance specification"
            | "enum value"
            | "value"
            | "value specification"
            | "literal"
            | "expression"
            | "customization"
    )
}

fn positions(pattern: &str, name: &str) -> Vec<usize> {
    let name = normalize(name).to_lowercase();
    pattern.split(['*', '?']).filter(|p| !p.is_empty()).map(|p| name.find(p).map(|i| name[..i].chars().count()).unwrap_or(usize::MAX)).collect()
}

fn name_class(name: &str) -> u8 {
    match name.trim_start().chars().next() {
        None => 2,
        Some(c) if ":+-([{\\/<>*=%$&".contains(c) => 1,
        _ => 0,
    }
}
