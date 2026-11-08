//! Requirements from a spreadsheet: a CSV file read into the edits that create them.
//!
//! This module reads and plans; it writes nothing. `plan` turns the rows into the same edits `apply_edits` takes
//! (`create_element`, `update_element`, `create_relationship`), so every name, text and value of the file passes the
//! checks of the write path like any other caller's text. The file is untrusted: nothing in it is run or trusted, a cell
//! is only ever the id, the name, the documentation or the kind of a requirement.

use super::{Fail, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Limits checked before any work.
pub const MAX_ROWS: usize = 5_000;
pub const MAX_BYTES: usize = 2 << 20;

/// The requirement kinds a row may state: the word in the file and the name of the mark.
pub const KINDS: [(&str, &str); 7] = [
    ("functional", "functionalRequirement"),
    ("performance", "performanceRequirement"),
    ("interface", "interfaceRequirement"),
    ("physical", "physicalRequirement"),
    ("design", "designConstraint"),
    ("business", "businessRequirement"),
    ("extended", "extendedRequirement"),
];

/// The package that holds the marks when the model has none: `<root package>::SysMLv1`, where the tools already put
/// the marks of derive, refine and trace.
pub const MARK_PACKAGE: &str = "SysMLv1";

/// CSV text as rows of cells (RFC 4180): a cell in double quotes may hold commas, line breaks and `""` for a quote;
/// lines end with CR LF or LF. A line with no cell at all is skipped. A byte order mark at the start is dropped.
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let (mut quoted, mut was_quoted, mut line, mut opened) = (false, false, 1usize, 1usize);
    let mut chars = text.chars().peekable();
    let end_row = |row: &mut Vec<String>, rows: &mut Vec<Vec<String>>| {
        if !(row.len() == 1 && row[0].is_empty()) {
            rows.push(std::mem::take(row));
        } else {
            row.clear();
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                _ => {
                    if c == '\n' {
                        line += 1;
                    }
                    cell.push(c);
                }
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
                opened = line;
            }
            '"' => return Err(format!("line {line}: a double quote inside a cell that is not in quotes. Fix: put the whole cell in double quotes and write each quote twice (RFC 4180).")),
            ',' => {
                row.push(std::mem::take(&mut cell));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut cell));
                was_quoted = false;
                end_row(&mut row, &mut rows);
                line += 1;
            }
            _ if was_quoted => return Err(format!("line {line}: text after the closing quote of a cell. Fix: write a quote inside a quoted cell twice (RFC 4180).")),
            _ => cell.push(c),
        }
    }
    if quoted {
        return Err(format!("line {opened}: a cell opens with a double quote that is never closed."));
    }
    if !cell.is_empty() || was_quoted || !row.is_empty() {
        row.push(cell);
        end_row(&mut row, &mut rows);
    }
    Ok(rows)
}

/// The mark of a kind cell: `functional`, `Functional requirement`, `functionalRequirement`, `design constraint`.
pub fn mark_of(cell: &str) -> Option<&'static str> {
    let k: String = cell.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let k = k.strip_suffix("requirement").or_else(|| k.strip_suffix("constraint")).filter(|s| !s.is_empty()).unwrap_or(&k).to_string();
    KINDS.iter().find(|(word, _)| *word == k).map(|(_, mark)| *mark)
}

/// A name made from an id when the file gives none: `R-4.1` gives `R_4_1`.
pub fn name_from_id(id: &str) -> String {
    let mut n: String = id.trim().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    while n.contains("__") {
        n = n.replace("__", "_");
    }
    let n = n.trim_matches('_').to_string();
    match n.chars().next() {
        None => "requirement".into(),
        Some(c) if c.is_alphabetic() => n,
        Some(_) => format!("R_{n}"),
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when the documentation in the model is the text of the cell. The write path stores a text with its line breaks
/// indented and with `*/` opened to `* /`, so the two are compared without regard to white space.
fn same_text(stored: &str, cell: &str) -> bool {
    let squeeze = |s: &str| s.replace("*/", "* /").chars().filter(|c| !c.is_whitespace()).collect::<String>();
    squeeze(stored) == squeeze(cell)
}

/// What an import would do, and the edits that do it.
pub struct Plan {
    /// The edits, in the order they run: the owner and the marks, the new requirements, the updates, the derive links.
    pub edits: Vec<Value>,
    /// For each edit, the line of the file it comes from (the header is line 1); `None` for the owner and the marks.
    pub rows_of: Vec<Option<usize>>,
    /// `created`, `existing`, `changed`, `skipped`, `links`, `links_skipped`, `columns`, `counts`.
    pub report: Value,
}

/// `import_requirements {csv, owner, id_column?, text_column?, name_column?, parent_column?, kind_column?, update?}`:
/// the plan of the import on the model as it is.
pub fn plan(m: &Model, a: &Value) -> Res<Plan> {
    let text_arg = |k: &str| a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let csv = match a.get("csv") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err("missing argument 'csv': the content of the CSV file as one string (the first row is the header).".into()),
    };
    if csv.len() > MAX_BYTES {
        return Err(format!("refused: the csv is {} bytes; at most {} ({} MiB). Fix: split the file.", csv.len(), MAX_BYTES, MAX_BYTES >> 20).into());
    }
    let owner_arg = text_arg("owner").ok_or_else(|| Fail::from("missing argument 'owner': the qualified name of the package that receives the requirements (a top-level package is made when it does not exist)."))?;
    let rows = parse_csv(csv).map_err(|e| Fail::from(format!("the csv cannot be read: {e}")))?;
    let Some(header) = rows.first() else { return Err("the csv is empty: the first row must be the header (id, text, and optionally name, parent, kind).".into()) };
    if rows.len() - 1 > MAX_ROWS {
        return Err(format!("refused: the csv has {} rows; at most {MAX_ROWS} per call. Fix: split the file.", rows.len() - 1).into());
    }
    let heads: Vec<String> = header.iter().map(|h| h.trim().to_lowercase()).collect();
    let column = |arg: &str, defaults: &[&str], required: bool| -> Res<Option<usize>> {
        if let Some(want) = text_arg(arg) {
            return match heads.iter().position(|h| *h == want.to_lowercase()) {
                Some(i) => Ok(Some(i)),
                None => Err(format!("{arg}: the csv has no column {}. Its columns: {}.", crate::agent::text::quote_clip(want), header.iter().map(|h| crate::agent::text::quote_clip(h.trim())).collect::<Vec<_>>().join(", ")).into()),
            };
        }
        match defaults.iter().find_map(|d| heads.iter().position(|h| h == d)) {
            Some(i) => Ok(Some(i)),
            None if required => Err(format!("the csv has no column for the {} (looked for: {}). Its columns: {}. Fix: name the column with {arg} (--{}).", arg.trim_end_matches("_column"), defaults.join(", "), header.iter().map(|h| crate::agent::text::quote_clip(h.trim())).collect::<Vec<_>>().join(", "), arg.replace('_', "-")).into()),
            None => Ok(None),
        }
    };
    let c_id = column("id_column", &["id", "requirement id", "req id", "identifier", "number"], true)?.unwrap_or(0);
    // The text: a header named so, else the first other header holding one of those words ("Requirement",
    // "Requirement statement", "Text (shall)").
    let c_text = match column("text_column", &["text", "description", "requirement text", "statement", "requirement"], false)? {
        Some(i) => i,
        None => heads
            .iter()
            .enumerate()
            .position(|(i, h)| i != c_id && ["requirement", "text", "statement", "description"].iter().any(|w| h.contains(w)) && !h.contains("id"))
            .map(Ok)
            .unwrap_or_else(|| column("text_column", &["text", "description", "requirement text", "statement", "requirement"], true).map(|c| c.unwrap_or(0)))?,
    };
    let c_name = column("name_column", &["name", "title"], false)?;
    let c_parent = column("parent_column", &["parent", "parent id", "derived from"], false)?;
    let c_kind = column("kind_column", &["kind", "type"], false)?;
    let update = a.get("update").and_then(Value::as_bool).unwrap_or(false);

    // ---- the owner, and what the model holds already
    let owner_el = m.find(owner_arg);
    let owner_qn = match owner_el {
        Some(o) => o.qualified_name.clone(),
        None if !owner_arg.contains("::") => quote_name(owner_arg.trim_matches('\'')),
        None => return Err(format!("no element named {}. Fix: pass a package that exists, or the name of a new top-level package.", crate::agent::text::quote_clip(owner_arg)).into()),
    };
    let root_qn = owner_el.map(|o| std::iter::successors(Some(o), |e| e.owner.map(|i| &m.elements[i])).last().map(|e| e.qualified_name.clone()).unwrap_or_default()).unwrap_or_else(|| owner_qn.clone());
    let mut by_id: HashMap<&str, &Element> = HashMap::new();
    for r in m.elements.iter().filter(|e| e.kind == "requirement" && !e.is_def) {
        if let Some(id) = r.short_name.as_deref() {
            by_id.entry(id).or_insert(r);
        }
    }
    let mut taken: HashSet<String> = owner_el.map(|o| m.children(o.id).filter_map(|c| c.name.clone()).collect()).unwrap_or_default();

    let (mut edits, mut rows_of): (Vec<Value>, Vec<Option<usize>>) = (Vec::new(), Vec::new());
    let (mut created, mut existing, mut changed, mut skipped, mut links, mut links_skipped) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut creates: Vec<(Value, usize)> = Vec::new();
    let mut updates: Vec<(Value, usize)> = Vec::new();
    let mut marks: Vec<&'static str> = Vec::new();
    // id -> (the ref it has or will have, the line it came from)
    let mut refs: HashMap<String, String> = by_id.iter().map(|(id, e)| (id.to_string(), e.qualified_name.clone())).collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut wants_parent: Vec<(usize, String, String, String)> = Vec::new();
    let mark_ref = |mark: &str| -> String {
        match m.elements.iter().find(|e| e.kind == "metadata" && e.is_def && e.name.as_deref() == Some(mark)) {
            Some(d) => d.qualified_name.clone(),
            None => format!("{root_qn}::{MARK_PACKAGE}::{mark}"),
        }
    };
    for (i, row) in rows.iter().enumerate().skip(1) {
        let line = i + 1;
        let cell = |c: Option<usize>| c.and_then(|c| row.get(c)).map(|s| s.trim()).unwrap_or("");
        let (id, text, name_cell, parent, kind) = (cell(Some(c_id)), cell(Some(c_text)), cell(c_name), cell(c_parent), cell(c_kind));
        if id.is_empty() {
            skipped.push(json!({ "row": line, "id": "", "reason": "the row has no id" }));
            continue;
        }
        if let Some(first) = seen.get(id) {
            skipped.push(json!({ "row": line, "id": id, "reason": format!("the id repeats the id of row {first}") }));
            continue;
        }
        seen.insert(id.to_string(), line);
        if let Some(e) = by_id.get(id) {
            let stored = super::text::doc_of(m, e).unwrap_or_default();
            if same_text(&stored, text) {
                existing.push(json!({ "row": line, "id": id, "ref": e.qualified_name }));
            } else {
                changed.push(json!({ "row": line, "id": id, "ref": e.qualified_name, "before": one_line(&stored), "after": one_line(text), "updated": update }));
                if update {
                    updates.push((json!({ "op": "update_element", "ref": e.qualified_name, "doc": text }), line));
                }
            }
            continue;
        }
        let mark = if kind.is_empty() {
            None
        } else {
            match mark_of(kind) {
                Some(k) => Some(k),
                None => {
                    skipped.push(json!({ "row": line, "id": id, "reason": format!("the kind {} is not a requirement kind ({})", crate::agent::text::quote_clip(kind), KINDS.iter().map(|k| k.0).collect::<Vec<_>>().join(", ")) }));
                    seen.remove(id);
                    continue;
                }
            }
        };
        let name = if name_cell.is_empty() { name_from_id(id) } else { name_cell.to_string() };
        if !taken.insert(name.clone()) {
            skipped.push(json!({ "row": line, "id": id, "reason": format!("the name {} is taken in {owner_qn}", crate::agent::text::quote_clip(&name)) }));
            seen.remove(id);
            continue;
        }
        let reference = format!("{owner_qn}::{}", quote_name(&name));
        let mut e = json!({ "op": "create_element", "kind": "requirement", "owner": owner_qn, "name": name, "short_name": id });
        if !text.is_empty() {
            e["doc"] = json!(text);
        }
        if let Some(k) = mark {
            e["metadata"] = json!([mark_ref(k)]);
            if !marks.contains(&k) {
                marks.push(k);
            }
        }
        creates.push((e, line));
        refs.insert(id.to_string(), reference.clone());
        created.push(json!({ "row": line, "id": id, "name": name, "ref": reference, "kind": mark, "parent": if parent.is_empty() { Value::Null } else { json!(parent) }, "text": one_line(text) }));
        if !parent.is_empty() {
            wants_parent.push((line, id.to_string(), parent.to_string(), reference));
        }
    }

    // ---- the edits, in the order they run
    if !creates.is_empty() && owner_el.is_none() {
        edits.push(json!({ "op": "create_element", "kind": "package", "name": owner_qn.trim_matches('\''), "if_absent": true }));
        rows_of.push(None);
    }
    let missing: Vec<&str> = marks.iter().copied().filter(|k| !m.elements.iter().any(|e| e.kind == "metadata" && e.is_def && e.name.as_deref() == Some(*k))).collect();
    if !missing.is_empty() {
        edits.push(json!({ "op": "create_element", "kind": "package", "owner": root_qn, "name": MARK_PACKAGE, "if_absent": true }));
        rows_of.push(None);
        for k in missing {
            edits.push(json!({ "op": "create_element", "kind": "metadata def", "owner": format!("{root_qn}::{MARK_PACKAGE}"), "name": k, "if_absent": true }));
            rows_of.push(None);
        }
    }
    for (e, line) in creates.into_iter().chain(updates) {
        edits.push(e);
        rows_of.push(Some(line));
    }
    for (line, id, parent, source) in wants_parent {
        match refs.get(&parent) {
            Some(target) if *target != source => {
                edits.push(json!({ "op": "create_relationship", "kind": "derive", "source": source, "target": target, "if_absent": true }));
                rows_of.push(Some(line));
                links.push(json!({ "row": line, "id": id, "parent": parent, "source": source, "target": target }));
            }
            Some(_) => links_skipped.push(json!({ "row": line, "id": id, "parent": parent, "reason": "a requirement cannot be derived from itself" })),
            None => links_skipped.push(json!({ "row": line, "id": id, "parent": parent, "reason": format!("no requirement with the id {} in the file or in the model: the requirement is made, the derive link is not", crate::agent::text::quote_clip(&parent)) })),
        }
    }
    let name = |c: Option<usize>| c.map(|c| header[c].trim().to_string());
    let report = json!({
        "owner": owner_qn,
        "rows": rows.len() - 1,
        "columns": { "id": name(Some(c_id)), "text": name(Some(c_text)), "name": name(c_name), "parent": name(c_parent), "kind": name(c_kind) },
        "update": update,
        "counts": { "created": created.len(), "existing": existing.len(), "changed": changed.len(), "skipped": skipped.len(), "links": links.len(), "links_skipped": links_skipped.len() },
        "created": created,
        "existing": existing,
        "changed": changed,
        "skipped": skipped,
        "links": links,
        "links_skipped": links_skipped,
    });
    Ok(Plan { edits, rows_of, report })
}

/// The report in sentences.
pub fn report_text(r: &Value, dry_run: bool) -> String {
    let c = &r["counts"];
    let verb = if dry_run { "would be created" } else { "created" };
    let mut t = format!("{} row(s): {} {verb} in {}, {} existing and unchanged, {} existing with another text ({}), {} skipped; {} derive link(s){}.\n", r["rows"], c["created"], r["owner"].as_str().unwrap_or(""), c["existing"], c["changed"], if r["update"] == true { if dry_run { "would be updated" } else { "updated" } } else { "not overwritten; pass update: true to take the text of the file" }, c["skipped"], c["links"], if c["links_skipped"].as_u64().unwrap_or(0) > 0 { format!(", {} not made", c["links_skipped"]) } else { String::new() });
    for x in r["changed"].as_array().into_iter().flatten().take(20) {
        t.push_str(&format!("changed row {} {}: model \"{}\" / file \"{}\"\n", x["row"], x["id"].as_str().unwrap_or(""), x["before"].as_str().unwrap_or(""), x["after"].as_str().unwrap_or("")));
    }
    for x in r["skipped"].as_array().into_iter().flatten().take(20) {
        t.push_str(&format!("skipped row {} {}: {}\n", x["row"], x["id"].as_str().unwrap_or(""), x["reason"].as_str().unwrap_or("")));
    }
    for x in r["links_skipped"].as_array().into_iter().flatten().take(20) {
        t.push_str(&format!("no link for row {} {}: {}\n", x["row"], x["id"].as_str().unwrap_or(""), x["reason"].as_str().unwrap_or("")));
    }
    t
}
