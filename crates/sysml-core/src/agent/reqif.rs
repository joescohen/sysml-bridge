//! Bounded ReqIF requirement exchange. XML and XHTML are data, never model syntax.
use super::numbering::id_attr;
use super::reqimport::name_from_id;
use super::text::id_of;
use super::{Fail, Res};
use crate::model::{Element, ElementId, Model};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Far above any file a requirements tool exports (a DOORS module of 10,000 objects is a few tens of MiB).
pub const MAX_BYTES: usize = 256 << 20;
pub const MAX_ELEMENTS: usize = 50_000_000;
const MAX_OBJECTS: usize = 1_000_000;
const MAX_TEXT: usize = 8_000;
const MAX_LISTED: usize = 500;
const DEPRECATED: &str = "[Deprecated in ReqIF]";

fn xml(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if !matches!(c, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}') =>
                {}
            c => out.push(c),
        }
    }
    out
}

// FNV-1a over a qualified name is deterministic across processes and model parses.
fn stable_id(prefix: &str, q: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in q.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{h:016x}")
}
/// What a requirement's IDENTIFIER is made from: its element id (short name) when it has one, so a rename or a move
/// of the requirement keeps its IDENTIFIER and a tool on the other side sees the same object; else its qualified name.
fn object_key(e: &Element) -> String {
    match &e.short_name {
        Some(id) => format!("id:{id}"),
        None => e.qualified_name.clone(),
    }
}
fn is_req(e: &Element) -> bool {
    e.kind == "requirement" && !e.is_def
}
fn value_for(m: &Model, e: &Element, col: &str) -> String {
    match col.to_ascii_lowercase().as_str() {
        "id" | "identifier" => id_of(m, e),
        "name" => e.name.clone().unwrap_or_default(),
        "text" => e.doc.clone().unwrap_or_default(),
        _ => m
            .children(e.id)
            .find(|c| c.kind == "attribute" && c.name.as_deref() == Some(col))
            .and_then(|c| c.value.clone())
            .map(|v| v.trim_matches('"').to_string())
            .unwrap_or_default(),
    }
}
fn package_path(m: &Model, e: &Element, root: ElementId) -> String {
    let mut packages = Vec::new();
    let mut at = e.owner;
    while let Some(id) = at {
        if id == root || is_req(&m.elements[id]) {
            break;
        }
        let parent = &m.elements[id];
        if parent.kind == "package" {
            if let Some(name) = &parent.name {
                packages.push(name.clone());
            }
        }
        at = parent.owner;
    }
    packages.reverse();
    packages.join("::")
}
fn hierarchy(m: &Model, id: ElementId, root: ElementId, level: usize, out: &mut String) {
    for child in m.children(id) {
        if is_req(child) {
            let pad = "  ".repeat(level);
            let path = package_path(m, child, root);
            let owner_attr = if path.is_empty() {
                String::new()
            } else {
                format!(" LONG-NAME=\"sysml-owner:{}\"", xml(&path))
            };
            out.push_str(&format!("{pad}<SPEC-HIERARCHY IDENTIFIER=\"{}\"{owner_attr}><OBJECT><SPEC-OBJECT-REF>{}</SPEC-OBJECT-REF></OBJECT>", stable_id("h", &object_key(child)), stable_id("o", &object_key(child))));
            if m.descendants(child.id)
                .into_iter()
                .any(|id| is_req(&m.elements[id]))
            {
                out.push_str("<CHILDREN>");
                hierarchy(m, child.id, root, level + 1, out);
                out.push_str("</CHILDREN>");
            }
            out.push_str("</SPEC-HIERARCHY>\n");
        } else if child.kind == "package" {
            hierarchy(m, child.id, root, level, out);
        }
    }
}
/// Export one package as one specification. Ordering follows model order and is byte-stable.
pub fn export(m: &Model, scope: &str, columns: &[String]) -> Res<Value> {
    let root = m
        .find(scope)
        .ok_or_else(|| Fail::from(format!("unknown scope {scope}")))?;
    if root.kind != "package" {
        return Err(Fail::from("scope must be a package"));
    }
    if columns.len() > 24
        || columns
            .iter()
            .any(|c| c.is_empty() || c.len() > 120 || c.chars().any(char::is_control))
    {
        return Err(Fail::from(
            "columns must name at most 24 attributes, each 1 to 120 characters",
        ));
    }
    let mut cols = vec![
        "Identifier".to_string(),
        "Name".to_string(),
        "Text".to_string(),
    ];
    for c in columns {
        if !cols.iter().any(|x| x.eq_ignore_ascii_case(c)) {
            cols.push(c.clone());
        }
    }
    let reqs: Vec<&Element> = m
        .descendants(root.id)
        .into_iter()
        .map(|id| &m.elements[id])
        .filter(|e| is_req(e))
        .collect();
    if reqs.len() > MAX_OBJECTS {
        return Err(Fail::from(format!(
            "at most {MAX_OBJECTS} requirements per ReqIF file"
        )));
    }
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<REQ-IF xmlns=\"http://www.omg.org/spec/ReqIF/20110401/reqif.xsd\" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n<THE-HEADER><REQ-IF-HEADER IDENTIFIER=\"header\"><CREATION-TIME>2000-01-01T00:00:00Z</CREATION-TIME><REQ-IF-TOOL-ID>sysml-bridge</REQ-IF-TOOL-ID><REQ-IF-VERSION>1.2</REQ-IF-VERSION><SOURCE-TOOL-ID>sysml-bridge</SOURCE-TOOL-ID><TITLE>Requirements</TITLE></REQ-IF-HEADER></THE-HEADER>\n<CORE-CONTENT><REQ-IF-CONTENT>\n<DATATYPES><DATATYPE-DEFINITION-STRING IDENTIFIER=\"dt-string\" MAX-LENGTH=\"8000\"/><DATATYPE-DEFINITION-XHTML IDENTIFIER=\"dt-xhtml\"/></DATATYPES>\n<SPEC-TYPES><SPEC-OBJECT-TYPE IDENTIFIER=\"type-requirement\" LONG-NAME=\"Requirement\"><SPEC-ATTRIBUTES>");
    for (i, c) in cols.iter().enumerate() {
        s.push_str(&format!("<ATTRIBUTE-DEFINITION-{} IDENTIFIER=\"a-{i}\" LONG-NAME=\"{}\"><TYPE><DATATYPE-DEFINITION-{}-REF>dt-{}</DATATYPE-DEFINITION-{}-REF></TYPE></ATTRIBUTE-DEFINITION-{}>", if c == "Text" { "XHTML" } else { "STRING" }, xml(c), if c == "Text" { "XHTML" } else { "STRING" }, if c == "Text" { "xhtml" } else { "string" }, if c == "Text" { "XHTML" } else { "STRING" }, if c == "Text" { "XHTML" } else { "STRING" }));
    }
    s.push_str("</SPEC-ATTRIBUTES></SPEC-OBJECT-TYPE><SPECIFICATION-TYPE IDENTIFIER=\"type-spec\"/></SPEC-TYPES>\n<SPEC-OBJECTS>\n");
    for e in &reqs {
        s.push_str(&format!("<SPEC-OBJECT IDENTIFIER=\"{}\" LONG-NAME=\"{}\"><TYPE><SPEC-OBJECT-TYPE-REF>type-requirement</SPEC-OBJECT-TYPE-REF></TYPE><VALUES>", stable_id("o", &object_key(e)), xml(e.name.as_deref().unwrap_or(""))));
        for (i, c) in cols.iter().enumerate() {
            if c == "Text" {
                s.push_str(&format!("<ATTRIBUTE-VALUE-XHTML><THE-VALUE><xhtml:div><xhtml:p>{}</xhtml:p></xhtml:div></THE-VALUE><DEFINITION><ATTRIBUTE-DEFINITION-XHTML-REF>a-{i}</ATTRIBUTE-DEFINITION-XHTML-REF></DEFINITION></ATTRIBUTE-VALUE-XHTML>", xml(&value_for(m,e,c))));
            } else {
                s.push_str(&format!("<ATTRIBUTE-VALUE-STRING THE-VALUE=\"{}\"><DEFINITION><ATTRIBUTE-DEFINITION-STRING-REF>a-{i}</ATTRIBUTE-DEFINITION-STRING-REF></DEFINITION></ATTRIBUTE-VALUE-STRING>", xml(&value_for(m,e,c))));
            }
        }
        s.push_str("</VALUES></SPEC-OBJECT>\n");
    }
    s.push_str("</SPEC-OBJECTS><SPECIFICATIONS><SPECIFICATION IDENTIFIER=\"");
    s.push_str(&stable_id("s", scope));
    s.push_str("\" LONG-NAME=\"");
    s.push_str(&xml(scope));
    s.push_str(
        "\"><TYPE><SPECIFICATION-TYPE-REF>type-spec</SPECIFICATION-TYPE-REF></TYPE><CHILDREN>\n",
    );
    hierarchy(m, root.id, root.id, 0, &mut s);
    s.push_str(
        "</CHILDREN></SPECIFICATION></SPECIFICATIONS></REQ-IF-CONTENT></CORE-CONTENT></REQ-IF>\n",
    );
    if s.len() > MAX_BYTES {
        return Err(Fail::from(format!(
            "ReqIF export exceeds {MAX_BYTES} bytes; use a smaller package"
        )));
    }
    Ok(json!({"reqif": s, "objects": reqs.len(), "scope": scope, "columns": cols}))
}

fn child<'a, 'input>(
    n: roxmltree::Node<'a, 'input>,
    local: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == local)
}
fn descendants<'a, 'input>(
    n: roxmltree::Node<'a, 'input>,
    local: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    n.descendants()
        .filter(move |c| c.is_element() && c.tag_name().name() == local)
}
fn plain(n: roxmltree::Node<'_, '_>) -> String {
    let mut s = String::new();
    for d in n.descendants() {
        if d.is_text() {
            s.push_str(d.text().unwrap_or(""));
        } else if d.is_element()
            && matches!(d.tag_name().name(), "p" | "div" | "br" | "li")
            && !s.ends_with('\n')
            && !s.is_empty()
        {
            s.push('\n');
        }
    }
    s.trim().to_string()
}
fn attr_value(n: roxmltree::Node<'_, '_>) -> String {
    if let Some(v) = n.attribute("THE-VALUE") {
        v.to_string()
    } else {
        child(n, "THE-VALUE").map(plain).unwrap_or_default()
    }
}
#[derive(Clone)]
struct Object {
    source: String,
    id: String,
    custom_id: String,
    name: String,
    text: String,
    parent: Option<String>,
    packages: Vec<String>,
}
/// A row for an object (or a part of the file) that is left out of the import: the rest of the file is imported.
fn skip(identifier: &str, reason: &str) -> Value {
    json!({ "status": "skipped", "identifier": identifier, "reason": format!("skipped: {reason}") })
}

/// The objects of the file in hierarchy order, and a row for each object or part of the file left out. Only a file that
/// is not ReqIF at all (not XML, another root, a DOCTYPE or ENTITY declaration, no SPECIFICATION) is refused whole.
fn parse(input: &str) -> Res<(Vec<Object>, Vec<Value>)> {
    let mut skipped: Vec<Value> = Vec::new();
    if input.len() > MAX_BYTES {
        return Err(Fail::from(format!(
            "refused: ReqIF is longer than {MAX_BYTES} bytes"
        )));
    }
    // Reject the declarations before the XML parser sees them, including mixed case. Built-in XML entities remain valid.
    let upper = input.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err(Fail::from(
            "refused: ReqIF DOCTYPE and ENTITY declarations are forbidden",
        ));
    }
    let doc = roxmltree::Document::parse(input)
        .map_err(|e| Fail::from(format!("invalid ReqIF XML: {e}")))?;
    if doc.descendants().count() > MAX_ELEMENTS {
        return Err(Fail::from(format!(
            "refused: ReqIF exceeds {MAX_ELEMENTS} elements"
        )));
    }
    if !matches!(doc.root_element().tag_name().name(), "REQ-IF" | "RIF") {
        return Err(Fail::from(
            "expected a ReqIF REQ-IF (1.2) or RIF (1.0) root",
        ));
    }
    let mut defs = HashMap::new();
    for n in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name().starts_with("ATTRIBUTE-DEFINITION-"))
    {
        if let (Some(id), Some(name)) = (n.attribute("IDENTIFIER"), n.attribute("LONG-NAME")) {
            defs.insert(id.to_string(), name.to_string());
        }
    }
    let specs: Vec<roxmltree::Node<'_, '_>> = descendants(doc.root_element(), "SPECIFICATION").collect();
    if specs.is_empty() {
        return Err(Fail::from("ReqIF has no SPECIFICATION"));
    }
    let mut objects = HashMap::new();
    for n in descendants(doc.root_element(), "SPEC-OBJECT") {
        let Some(source) = n.attribute("IDENTIFIER").map(str::to_string) else {
            skipped.push(skip(n.attribute("LONG-NAME").unwrap_or(""), "SPEC-OBJECT lacks IDENTIFIER"));
            continue;
        };
        if source.is_empty() || source.len() > 240 || !source.bytes().all(|b| b.is_ascii_graphic())
        {
            skipped.push(skip(
                &source.chars().filter(|c| c.is_ascii_graphic()).take(60).collect::<String>(),
                "SPEC-OBJECT IDENTIFIER must be 1 to 240 printable ASCII characters without spaces",
            ));
            continue;
        }
        let mut o = Object {
            source: source.clone(),
            id: String::new(),
            custom_id: String::new(),
            name: n.attribute("LONG-NAME").unwrap_or("").to_string(),
            text: String::new(),
            parent: None,
            packages: Vec::new(),
        };
        if let Some(values) = child(n, "VALUES") {
            for v in values
                .children()
                .filter(|c| c.is_element() && c.tag_name().name().starts_with("ATTRIBUTE-VALUE-"))
            {
                let def = descendants(v, "ATTRIBUTE-DEFINITION-STRING-REF")
                    .next()
                    .or_else(|| descendants(v, "ATTRIBUTE-DEFINITION-XHTML-REF").next());
                let Some(name) = def.and_then(|d| defs.get(d.text().unwrap_or(""))) else {
                    continue;
                };
                let value = attr_value(v);
                let key = name.to_ascii_lowercase();
                let key = key.strip_prefix("reqif.").unwrap_or(&key);
                match key {
                    "identifier" | "id" => o.id = value,
                    "custom id" | "customid" => o.custom_id = value,
                    "name" => o.name = value,
                    "text" | "description" => o.text = value,
                    _ => {}
                }
            }
        }
        if objects.contains_key(&source) {
            skipped.push(skip(&source, "duplicate SPEC-OBJECT IDENTIFIER (the first is imported)"));
            continue;
        }
        objects.insert(source, o);
        if objects.len() > MAX_OBJECTS {
            return Err(Fail::from(format!(
                "refused: ReqIF exceeds {MAX_OBJECTS} SPEC-OBJECTs"
            )));
        }
    }
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    #[allow(clippy::too_many_arguments)]
    fn walk(
        n: roxmltree::Node<'_, '_>,
        parent: Option<String>,
        depth: usize,
        spec_pkg: Option<&str>,
        objects: &mut HashMap<String, Object>,
        order: &mut Vec<String>,
        seen: &mut HashSet<String>,
        skipped: &mut Vec<Value>,
    ) -> Res<()> {
        if depth > 128 {
            return Err(Fail::from(
                "refused: ReqIF hierarchy is deeper than 128 levels",
            ));
        }
        for h in n
            .children()
            .filter(|c| c.is_element() && c.tag_name().name() == "SPEC-HIERARCHY")
        {
            let Some(source) = child(h, "OBJECT")
                .and_then(|o| descendants(o, "SPEC-OBJECT-REF").next())
                .and_then(|r| r.text())
                .map(str::to_string)
            else {
                skipped.push(skip(h.attribute("IDENTIFIER").unwrap_or(""), "SPEC-HIERARCHY has no SPEC-OBJECT-REF"));
                continue;
            };
            if !seen.insert(source.clone()) {
                skipped.push(skip(&source, "SPEC-OBJECT appears twice in the hierarchy (its first place is imported)"));
                continue;
            }
            let Some(obj) = objects.get_mut(&source) else {
                skipped.push(skip(&source, "SPEC-HIERARCHY references an unknown or skipped SPEC-OBJECT"));
                continue;
            };
            obj.parent = parent.clone();
            // With several SPECIFICATIONs, each goes under a package of its own name.
            if parent.is_none() {
                obj.packages = spec_pkg.map(|p| vec![p.to_string()]).unwrap_or_default();
            }
            if let Some(path) = h
                .attribute("LONG-NAME")
                .and_then(|s| s.strip_prefix("sysml-owner:"))
            {
                if path.len() > 500
                    || path
                        .split("::")
                        .any(|part| part.is_empty() || part.len() > 120)
                {
                    skipped.push(skip(&source, "invalid sysml-owner package path in ReqIF hierarchy (the object is imported without it)"));
                } else {
                    obj.packages.extend(path.split("::").map(str::to_string));
                }
            }
            order.push(source.clone());
            if let Some(children) = child(h, "CHILDREN") {
                walk(
                    children,
                    Some(source.clone()),
                    depth + 1,
                    spec_pkg,
                    objects,
                    order,
                    seen,
                    skipped,
                )?;
            }
            // Export writes nested hierarchies directly; accept that common spelling too.
            walk(h, Some(source), depth + 1, spec_pkg, objects, order, seen, skipped)?;
        }
        Ok(())
    }
    for (i, spec) in specs.iter().enumerate() {
        // One SPECIFICATION is imported under the owner itself; several each get a package named after the specification.
        let pkg = (specs.len() > 1).then(|| {
            let n = spec.attribute("LONG-NAME").or_else(|| spec.attribute("IDENTIFIER")).unwrap_or("").trim();
            let n: String = n.chars().filter(|c| !c.is_control()).take(120).collect();
            if n.is_empty() { format!("Specification_{}", i + 1) } else { n }
        });
        if let Some(children) = child(*spec, "CHILDREN") {
            walk(children, None, 0, pkg.as_deref(), &mut objects, &mut order, &mut seen, &mut skipped)?;
        }
    }
    if order.is_empty() && skipped.is_empty() {
        return Err(Fail::from(
            "ReqIF SPECIFICATION has no requirement hierarchy",
        ));
    }
    Ok((order
        .into_iter()
        .filter_map(|id| objects.remove(&id))
        .collect(), skipped))
}

pub struct Opts<'a> {
    pub content: &'a str,
    pub owner: &'a str,
    pub fill_empty_name: bool,
    pub set_custom_id_as_id: bool,
}
/// Compare the specification with a package, returning proposal edits and bounded status rows.
pub fn plan(m: &Model, ids: &BTreeMap<String, String>, o: &Opts<'_>) -> Res<Value> {
    let owner = m
        .find(o.owner)
        .ok_or_else(|| Fail::from(format!("unknown owner {}", o.owner)))?;
    if owner.kind != "package" {
        return Err(Fail::from("owner must be a package"));
    }
    let (objects, mut skipped) = parse(o.content)?;
    let under: HashSet<ElementId> = m.descendants(owner.id).into_iter().collect();
    let mut by_id: HashMap<String, Vec<&Element>> = HashMap::new();
    for e in m
        .elements
        .iter()
        .filter(|e| under.contains(&e.id) && is_req(e))
    {
        by_id.entry(id_of(m, e)).or_default().push(e);
    }
    let mut edits = Vec::new();
    let mut prelude = Vec::new();
    let mut rows = Vec::new();
    let mut counts = BTreeMap::from([
        ("new", 0usize),
        ("updated", 0),
        ("unchanged", 0),
        ("obsolete", 0),
    ]);
    let mut matched = HashSet::new();
    let mut source_ref = HashMap::new();
    let mut display_ref = HashMap::new();
    let mut package_ref: HashMap<String, String> = HashMap::new();
    let mut names: HashSet<(String, String)> = m
        .elements
        .iter()
        .filter_map(|e| Some((m.elements[e.owner?].qualified_name.clone(), e.name.clone()?)))
        .collect();
    for obj in &objects {
        let target_id = if o.set_custom_id_as_id && !obj.custom_id.is_empty() {
            &obj.custom_id
        } else {
            &obj.id
        };
        // A skipped object that names a requirement the model has keeps it from being marked obsolete: the file still has it.
        let keep_known = |matched: &mut HashSet<ElementId>| {
            if let Some(e) = ids.get(&obj.source).and_then(|q| m.find(q)) {
                matched.insert(e.id);
            }
        };
        if obj.text.len() > MAX_TEXT || obj.name.len() > 120 || target_id.len() > 60 {
            keep_known(&mut matched);
            skipped.push(skip(&obj.source, "ReqIF name, id or text exceeds a field limit (120, 60, 8000 bytes)"));
            continue;
        }
        if target_id
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
        {
            keep_known(&mut matched);
            skipped.push(skip(&obj.source, "ReqIF requirement id contains an unsafe character"));
            continue;
        }
        let from_source = ids.get(&obj.source).and_then(|q| m.find(q));
        if from_source.is_none()
            && !target_id.is_empty()
            && by_id.get(target_id).is_some_and(|v| v.len() > 1)
        {
            matched.extend(by_id.get(target_id).into_iter().flatten().map(|e| e.id));
            skipped.push(skip(&obj.source, &format!("ReqIF requirement id {target_id} matches more than one requirement under owner")));
            continue;
        }
        let by_number = by_id
            .get(target_id)
            .filter(|_| !target_id.is_empty())
            .and_then(|v| if v.len() == 1 { Some(v[0]) } else { None });
        let existing = from_source.or(by_number);
        if let Some(e) = existing {
            if !under.contains(&e.id) || !is_req(e) {
                skipped.push(skip(&obj.source, &format!("ReqIF identifier {} names an element outside owner or not a requirement", obj.source)));
                continue;
            }
            if matched.contains(&e.id) {
                skipped.push(skip(&obj.source, &format!("two SPEC-OBJECTs match the same requirement {} (the first is imported)", e.qualified_name)));
                continue;
            }
            matched.insert(e.id);
        }
        let mut name = obj.name.clone();
        if name.is_empty() && o.fill_empty_name {
            name = obj
                .text
                .split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ");
        }
        if name.is_empty() {
            name = target_id.clone();
        }
        name = name_from_id(&name);
        let mut parent_ref = obj
            .parent
            .as_ref()
            .and_then(|p| source_ref.get(p))
            .cloned()
            .unwrap_or_else(|| owner.qualified_name.clone());
        let mut parent_display = obj
            .parent
            .as_ref()
            .and_then(|p| display_ref.get(p))
            .cloned()
            .unwrap_or_else(|| owner.qualified_name.clone());
        let mut bad_package: Option<String> = None;
        for package in &obj.packages {
            let safe_name = name_from_id(package);
            let q = format!("{parent_display}::{safe_name}");
            if let Some(reference) = package_ref.get(&q) {
                parent_ref = reference.clone();
                parent_display = q;
                continue;
            }
            let sid = stable_id("pkg", &q);
            if let Some(e) = m.find(&q) {
                if e.kind != "package" {
                    bad_package = Some(format!("ReqIF package path {q} names a non-package"));
                    break;
                }
                if let Some(other) = ids.get(&sid) {
                    if other != &q {
                        bad_package = Some(format!("ReqIF package source id {sid} already belongs to another element"));
                        break;
                    }
                } else {
                    prelude.push(json!({"op":"set_source_id","ref":q,"source_id":sid}));
                }
            } else {
                edits.push(json!({"op":"create_element","kind":"package","owner":parent_ref,"name":safe_name,"source_id":sid}));
                names.insert((parent_display.clone(), safe_name.clone()));
            }
            parent_ref = format!("source_id:{sid}");
            package_ref.insert(q.clone(), parent_ref.clone());
            parent_display = q;
        }
        if let Some(why) = bad_package {
            skipped.push(skip(&obj.source, &why));
            continue;
        }
        let mut changes = Vec::new();
        if let Some(e) = existing {
            source_ref.insert(obj.source.clone(), format!("source_id:{}", obj.source));
            display_ref.insert(obj.source.clone(), e.qualified_name.clone());
            if from_source.is_none() {
                edits.push(
                    json!({"op":"set_source_id","ref":e.qualified_name,"source_id":obj.source}),
                );
                changes.push("Source ID");
            }
            let mut update =
                json!({"op":"update_element","ref":format!("source_id:{}",obj.source)});
            let mut has_update = false;
            if e.owner.map(|id| m.elements[id].qualified_name.as_str())
                != Some(parent_display.as_str())
            {
                update["owner"] = json!(parent_ref);
                has_update = true;
                changes.push("Owner");
            }
            if e.doc
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .collect::<String>()
                != obj.text.split_whitespace().collect::<String>()
            {
                update["doc"] = json!(obj.text);
                has_update = true;
                changes.push("Text");
            }
            if !target_id.is_empty() && id_of(m, e) != *target_id {
                if let Some(a) = id_attr(m, e) {
                    edits.push(json!({"op":"update_element","ref":m.elements[a].qualified_name,"value":format!("\"{}\"",target_id)}));
                } else {
                    update["short_name"] = json!(target_id);
                    has_update = true;
                }
                changes.push("Id");
            }
            if !obj.name.is_empty() && e.name.as_deref() != Some(name.as_str()) {
                update["rename"] = json!(name);
                has_update = true;
                changes.push("Name");
            }
            if has_update {
                edits.push(update);
            }
            let status = if changes.is_empty() {
                "unchanged"
            } else {
                "updated"
            };
            *counts.get_mut(status).unwrap() += 1;
            if rows.len() < MAX_LISTED {
                rows.push(json!({"status":status,"ref":e.qualified_name,"identifier":obj.source,"changes":changes}));
            }
        } else {
            let base = name_from_id(&name);
            let mut unique = base.clone();
            let mut n = 2;
            while names.contains(&(parent_display.clone(), unique.clone())) {
                unique = format!("{base}_{n}");
                n += 1;
            }
            names.insert((parent_display.clone(), unique.clone()));
            let q = format!("{parent_display}::{unique}");
            source_ref.insert(obj.source.clone(), format!("source_id:{}", obj.source));
            display_ref.insert(obj.source.clone(), q.clone());
            let mut edit = json!({"op":"create_element","kind":"requirement","owner":parent_ref,"name":unique,"source_id":obj.source});
            if !target_id.is_empty() {
                edit["short_name"] = json!(target_id);
            }
            if !obj.text.is_empty() {
                edit["doc"] = json!(obj.text);
            }
            edits.push(edit);
            *counts.get_mut("new").unwrap() += 1;
            if rows.len() < MAX_LISTED {
                rows.push(json!({"status":"new","ref":q,"identifier":obj.source}));
            }
        }
    }
    for e in m
        .elements
        .iter()
        .filter(|e| under.contains(&e.id) && is_req(e) && !matched.contains(&e.id))
    {
        *counts.get_mut("obsolete").unwrap() += 1;
        if rows.len() < MAX_LISTED {
            rows.push(json!({"status":"obsolete","ref":e.qualified_name}));
        }
        if !e.doc.as_deref().unwrap_or("").contains(DEPRECATED) {
            let doc = format!("{}\n{}", e.doc.as_deref().unwrap_or("").trim(), DEPRECATED)
                .trim()
                .to_string();
            edits.push(json!({"op":"update_element","ref":e.qualified_name,"doc":doc}));
        }
    }
    edits.splice(0..0, prelude);
    // Every skipped object is listed, whatever the size of the file.
    counts.insert("skipped", skipped.len());
    let listed = rows.len() + skipped.len();
    rows.extend(skipped.iter().cloned());
    Ok(
        json!({"counts":counts,"rows":rows,"skipped":skipped,"edits":edits,"truncated":listed<counts.values().sum::<usize>()}),
    )
}
