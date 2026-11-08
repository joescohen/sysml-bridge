//! Hyperlinks on an element and element references inside its text, as `get_element` returns them. The importer keeps the
//! hyperlinks of a project as `@SysMLv1::HyperlinkOwner` metadata (model links by element, addresses as http or https text) and
//! writes an element reference in text as `(element <id>)`. Both are read here as data; an address is never fetched or followed.

use crate::model::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Most links and most references one reply lists.
pub const MAX_LINKS: usize = 100;
pub const MAX_TEXT_REFS: usize = 50;

fn is_owner(c: &Element) -> bool {
    c.kind == "metadata" && !c.is_def && c.refs.iter().any(|r| r.role == RefRole::Type && (r.text == "HyperlinkOwner" || r.text.ends_with("::HyperlinkOwner")))
}

fn tag_of(c: &Element) -> String {
    c.refs.iter().find(|r| r.role == RefRole::Assign).map(|r| r.text.clone()).or_else(|| c.name.clone()).unwrap_or_default().trim_matches('\'').to_string()
}

/// The attribute that the metadata definition declares for a tag (the redefinition target), not a value.
fn declared_tag(m: &Model, t: ElementId) -> bool {
    m.elements[t].owner.map(|o| m.elements[o].kind == "metadata" && m.elements[o].is_def).unwrap_or(false)
}

fn safe_address(a: &str) -> bool {
    (a.starts_with("https://") || a.starts_with("http://")) && a.len() <= 2048 && !a.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The line that ends the prose of a generated element and starts its provenance lines.
pub const PROVENANCE_MARKER: &str = "@@ generated provenance @@";
/// Most provenance lines one element reports, and the longest text of one.
pub const MAX_PROVENANCE: usize = 200;
const MAX_PROVENANCE_TEXT: usize = 500;

/// One provenance line, read as data: `kind` is the tag (`source`, `decision`, `decision-list`, `edge`, `conflict`), `text` what
/// follows it, and for a source the `path` and `anchor` on either side of the last `#`. Nothing here is followed or fetched.
pub struct Provenance {
    pub kind: &'static str,
    pub id: Option<String>,
    pub text: String,
    pub path: Option<String>,
    pub anchor: Option<String>,
}

fn plain_text(s: &str, n: usize) -> String {
    s.chars().take(n).map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// The provenance lines of a documentation text, or `None` when it has no marker. The provenance is what follows the LAST marker
/// line: the generator writes it after the prose of the element, so a marker or a `SOURCE:` line that the prose itself prints
/// comes before it and is prose. Lines after the marker that are not one of the known tags are not provenance. The second
/// value is the number of provenance lines (reported lines are capped at `MAX_PROVENANCE`).
pub fn provenance(doc: &str) -> Option<(Vec<Provenance>, usize)> {
    let lines: Vec<&str> = doc.lines().collect();
    let at = lines.iter().rposition(|l| l.trim() == PROVENANCE_MARKER)?;
    let mut out = Vec::new();
    let mut total = 0usize;
    for l in &lines[at + 1..] {
        let l = l.trim();
        // The longest tag first: DECISION-LIST before DECISION.
        let Some((kind, tag)) = [("decision-list", "DECISION-LIST"), ("decision", "DECISION"), ("source", "SOURCE"), ("edge", "EDGE"), ("conflict", "CONFLICT")].into_iter().find(|(_, t)| l.strip_prefix(*t).map(|r| r.starts_with(':') || r.starts_with(" <")).unwrap_or(false)) else { continue };
        total += 1;
        if out.len() >= MAX_PROVENANCE {
            continue;
        }
        let mut rest = &l[tag.len()..];
        // An id in angle brackets and quotes: <'id'>, with a backslash before a quote or a backslash inside it.
        let mut id = None;
        if let Some(r) = rest.strip_prefix(" <'") {
            let mut name = String::new();
            let mut it = r.char_indices();
            let mut end = None;
            while let Some((i, c)) = it.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = it.next() {
                            name.push(n);
                        }
                    }
                    '\'' if r[i + 1..].starts_with('>') => {
                        end = Some(i + 2);
                        break;
                    }
                    c => name.push(c),
                }
            }
            if let Some(end) = end {
                id = Some(plain_text(&name, 120));
                rest = &r[end..];
            }
        }
        let text = rest.strip_prefix(':').unwrap_or(rest).trim();
        let (path, anchor) = if kind == "source" {
            match text.rsplit_once('#') {
                Some((p, a)) if !p.is_empty() => (Some(plain_text(p, MAX_PROVENANCE_TEXT)), Some(plain_text(a, 200))),
                _ => (Some(plain_text(text, MAX_PROVENANCE_TEXT)), None),
            }
        } else {
            (None, None)
        };
        out.push(Provenance { kind, id, text: plain_text(text, MAX_PROVENANCE_TEXT), path, anchor });
    }
    Some((out, total))
}

pub fn attach(m: &Model, ids: &BTreeMap<String, String>, e: &Element, reply: &mut Value) {
    // Provenance lines the generator wrote after its marker: data only, capped, never followed.
    let prov = e.doc.as_deref().and_then(provenance);
    reply["provenance"] = json!(prov.as_ref().map(|(v, _)| v.iter().map(|p| json!({ "kind": p.kind, "id": p.id, "text": p.text, "path": p.path, "anchor": p.anchor })).collect::<Vec<_>>()).unwrap_or_default());
    reply["provenance_total"] = json!(prov.as_ref().map(|(_, t)| *t).unwrap_or(0));
    reply["provenance_marker"] = json!(prov.is_some());
    let mut owners: Vec<&Element> = e.children.iter().map(|c| &m.elements[*c]).filter(|c| is_owner(c)).collect();
    owners.extend(m.elements.iter().filter(|a| is_owner(a) && a.owner != Some(e.id) && a.refs.iter().any(|r| r.role == RefRole::Other && r.resolved() == Some(e.id))));
    let mut links: Vec<Value> = Vec::new();
    let mut total = 0usize;
    for o in owners {
        let attrs: Vec<&Element> = o.children.iter().map(|c| &m.elements[*c]).collect();
        let active_models: Vec<ElementId> = attrs.iter().filter(|c| tag_of(c) == "hyperlinkModelActive").flat_map(|c| c.refs.iter().filter_map(|r| r.resolved())).filter(|t| !declared_tag(m, *t)).collect();
        let active_text: Vec<String> = attrs.iter().filter(|c| tag_of(c) == "hyperlinkTextActive").filter_map(|c| c.value.as_deref()).map(|v| crate::lexer::unquote(v.trim())).collect();
        for c in &attrs {
            let tag = tag_of(c);
            if tag == "hyperlinkModel" {
                for t in c.refs.iter().filter_map(|r| r.resolved()).filter(|t| !declared_tag(m, *t)) {
                    total += 1;
                    if links.len() < MAX_LINKS {
                        let te = &m.elements[t];
                        links.push(json!({ "kind": "element", "ref": te.qualified_name, "name": te.name, "element_kind": te.keyword(), "active": active_models.contains(&t) }));
                    }
                }
            } else if tag == "hyperlinkText" {
                // One value or a list: every quoted text in the value.
                let raw = c.value.as_deref().unwrap_or("");
                for part in raw.split("\", \"") {
                    let a = crate::lexer::unquote(&format!("\"{}\"", part.trim().trim_matches(|ch| ch == '"' || ch == '(' || ch == ')')));
                    if a.is_empty() {
                        continue;
                    }
                    total += 1;
                    if links.len() < MAX_LINKS {
                        links.push(json!({ "kind": "web", "address": a, "followable": safe_address(&a), "active": active_text.contains(&a) }));
                    }
                }
            }
        }
    }
    reply["hyperlinks"] = json!(links);
    reply["hyperlinks_total"] = json!(total);
    // References inside the text: `(element <id>)`.
    let mut refs: Vec<Value> = Vec::new();
    let mut refs_total = 0usize;
    if let Some(doc) = e.doc.as_deref() {
        let mut from = 0usize;
        while let Some(i) = doc[from..].find("(element ") {
            let start = from + i;
            let Some(len) = doc[start..].find(')') else { break };
            let name = doc[start + 9..start + len].trim();
            refs_total += 1;
            if refs.len() < MAX_TEXT_REFS {
                let resolved = ids.get(name).and_then(|q| m.find(q)).or_else(|| m.find(name));
                refs.push(json!({ "text": name, "offset": doc[..start].chars().count(), "length": doc[start..=start + len].chars().count(), "ref": resolved.map(|r| r.qualified_name.clone()), "resolved": resolved.is_some() }));
            }
            from = start + len + 1;
        }
    }
    reply["text_refs"] = json!(refs);
    reply["text_refs_total"] = json!(refs_total);
}

/// Most text references read in the whole model, and in one text.
pub const MAX_MODEL_TEXT_REFS: usize = 50_000;
const MAX_REFS_PER_TEXT: usize = 100;

/// One `(element <id>)` reference in the text of an element: the owner of the text, what it names (None when it names nothing in the
/// model), and the text of the reference.
pub struct Mention {
    pub owner: ElementId,
    pub target: Option<ElementId>,
    pub text: String,
}

/// Every reference in text, in model order. The scan reads each text once; it stops at `MAX_MODEL_TEXT_REFS` references and says so.
pub fn mentions(m: &Model, ids: &BTreeMap<String, String>) -> (Vec<Mention>, bool) {
    let mut out = Vec::new();
    let mut cut = false;
    for e in &m.elements {
        let Some(doc) = e.doc.as_deref() else { continue };
        // The text of a `doc` is held by the doc element and by its owner; it is read once, for the owner.
        if e.owner.map(|o| m.elements[o].doc.as_deref() == Some(doc)).unwrap_or(false) {
            continue;
        }
        let mut from = 0usize;
        let mut n = 0usize;
        while let Some(i) = doc[from..].find("(element ") {
            let start = from + i;
            let Some(len) = doc[start..].find(')') else { break };
            let name = doc[start + 9..start + len].trim();
            from = start + len + 1;
            n += 1;
            if n > MAX_REFS_PER_TEXT {
                // References past the per-text cap are not read: the scan is incomplete, and says so.
                cut = true;
                break;
            }
            if out.len() >= MAX_MODEL_TEXT_REFS {
                return (out, true);
            }
            let target = ids.get(name).and_then(|q| m.find(q)).or_else(|| m.find(name)).map(|t| t.id);
            out.push(Mention { owner: e.id, target, text: name.chars().take(120).collect() });
        }
    }
    (out, cut)
}
