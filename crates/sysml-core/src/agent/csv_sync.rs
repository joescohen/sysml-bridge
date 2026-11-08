//! The spreadsheet round trip as a proposal: `export_table_csv` writes the rows of a generated table with a key column (the id of
//! the element), and `propose_from_csv` compares a file with the model by that key. A row is New (no key), Updated (a mapped cell
//! differs), Unchanged, or refused with the reason; an element under the scope that the file no longer has is Obsolete (listed, never
//! deleted). The plan is edits a caller stores as a proposal; this module writes nothing. The file is untrusted: a cell is data, it
//! is the id, the name or the text of a requirement and nothing else, and it reaches the model only through the checks of the write path.

use super::numbering::id_attr;
use super::reqimport::{name_from_id, parse_csv};
use super::text::id_of;
use super::text::{csv_all_quoted, quote_clip, strip_formula_guard};
use super::{Fail, Res};
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Limits, tested before the file is read: bytes, rows, columns, one cell, mapped columns, the length of a name.
pub const MAX_BYTES: usize = 2 << 20;
pub const MAX_ROWS: usize = 5_000;
pub const MAX_COLUMNS: usize = 32;
pub const MAX_CELL: usize = 8_000;
pub const MAX_MAP: usize = 8;
pub const MAX_NAME: usize = 120;
/// Most rows listed in a reply; the counts are full.
pub const MAX_LISTED: usize = 500;
/// The properties a column can be mapped to.
pub const PROPERTIES: &[&str] = &["Id", "Name", "Text"];
/// The header of the key column the export writes.
pub const KEY_HEADER: &str = "Key";

#[derive(Default)]
pub struct Opts {
    pub csv: String,
    /// The header of the key column (default `Key`).
    pub key_column: Option<String>,
    /// Header of a column -> property (Id, Name or Text). Empty: columns named like a property are mapped to it.
    pub map: Vec<(String, String)>,
    /// The package that receives a New row.
    pub owner: Option<String>,
    /// Requirements under this element are compared with the file for Obsolete.
    pub scope: Option<String>,
}

/// The key of an element: the id the importer or a tool set for it, else its qualified name.
pub fn key_of(rev: &HashMap<&str, &str>, e: &Element) -> String {
    rev.get(e.qualified_name.as_str()).map(|s| s.to_string()).unwrap_or_else(|| e.qualified_name.clone())
}

/// Qualified name -> the smallest id set for it (the map is ordered, so the choice is stable).
pub fn reverse_ids(ids: &BTreeMap<String, String>) -> HashMap<&str, &str> {
    let mut rev: HashMap<&str, &str> = HashMap::new();
    for (id, q) in ids {
        rev.entry(q.as_str()).or_insert(id.as_str());
    }
    rev
}

/// The CSV of a generated table: a key column first, then the columns that are text. `table` is the reply of `table_gen::table`
/// (heading rows are left out). Every field is quoted and passes the formula guard.
pub fn export(m: &Model, ids: &BTreeMap<String, String>, table: &Value) -> Value {
    let rev = reverse_ids(ids);
    let cols: Vec<(usize, String)> = table["columns"].as_array().into_iter().flatten().enumerate().filter(|(_, c)| c["kind"] != "number").map(|(i, c)| (i, c["title"].as_str().unwrap_or("").to_string())).collect();
    let mut headers = vec![KEY_HEADER.to_string()];
    headers.extend(cols.iter().map(|(_, t)| t.clone()));
    let mut rows: Vec<Vec<String>> = Vec::new();
    for r in table["rows"].as_array().into_iter().flatten().filter(|r| r["heading"] != true) {
        let key = r["ref"].as_str().and_then(|q| m.find(q)).map(|e| key_of(&rev, e)).unwrap_or_default();
        let mut row = vec![key];
        row.extend(cols.iter().map(|(i, _)| r["cells"][*i]["text"].as_str().unwrap_or("").to_string()));
        rows.push(row);
    }
    json!({ "csv": csv_all_quoted(&headers, &rows), "rows": rows.len(), "columns": headers })
}

#[derive(Clone, Copy, PartialEq)]
enum Prop {
    Id,
    Name,
    Text,
}

fn prop_of(word: &str) -> Option<Prop> {
    match word.trim().to_ascii_lowercase().as_str() {
        "id" => Some(Prop::Id),
        "name" => Some(Prop::Name),
        "text" | "documentation" => Some(Prop::Text),
        _ => None,
    }
}

fn same_text(stored: &str, cell: &str) -> bool {
    let squeeze = |s: &str| s.replace("*/", "* /").chars().filter(|c| !c.is_whitespace()).collect::<String>();
    squeeze(stored.trim()) == squeeze(cell.trim())
}

/// Any name the model can hold: the text quotes it when SysML v2 needs it (`'1st Stage'`).
fn usable_name(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_NAME && !s.chars().any(char::is_control)
}

fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 60 && !s.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
}

/// What the file would do to the model, row by row. Limits come first; then the header; then each row once.
pub fn plan(m: &Model, ids: &BTreeMap<String, String>, o: &Opts) -> Res<Value> {
    if o.csv.len() > MAX_BYTES {
        return Err(Fail::from(format!("refused: the csv is {} bytes; at most {} ({} MiB). Fix: split the file.", o.csv.len(), MAX_BYTES, MAX_BYTES >> 20)));
    }
    if o.map.len() > MAX_MAP || o.map.iter().any(|(c, p)| c.len() > 200 || p.len() > 40) {
        return Err(Fail::from(format!("the column map names at most {MAX_MAP} columns, each at most 200 characters.")));
    }
    if o.owner.iter().chain(o.scope.iter()).chain(o.key_column.iter()).any(|t| t.len() > 500) {
        return Err(Fail::from("owner, scope and key_column are at most 500 characters.".to_string()));
    }
    let rows = parse_csv(&o.csv).map_err(|e| Fail::from(format!("the csv cannot be read: {e}")))?;
    let Some(header) = rows.first() else { return Err(Fail::from("the csv is empty: the first row is the header (Key, then Id, Name, Text).".to_string())) };
    if rows.len() - 1 > MAX_ROWS {
        return Err(Fail::from(format!("refused: the csv has {} rows; at most {MAX_ROWS} per call. Fix: split the file.", rows.len() - 1)));
    }
    if header.len() > MAX_COLUMNS {
        return Err(Fail::from(format!("refused: the csv has {} columns; at most {MAX_COLUMNS}.", header.len())));
    }
    let heads: Vec<String> = header.iter().map(|h| strip_formula_guard(h.trim()).trim().to_string()).collect();
    let lower: Vec<String> = heads.iter().map(|h| h.to_lowercase()).collect();
    // The map: a column header to a property. Without a map, a column named like a property is that property.
    let mut mapped: Vec<(usize, Prop, String)> = Vec::new();
    if o.map.is_empty() {
        for (i, h) in lower.iter().enumerate() {
            if let Some(p) = prop_of(h) {
                mapped.push((i, p, heads[i].clone()));
            }
        }
    } else {
        for (col, prop) in &o.map {
            let Some(i) = lower.iter().position(|h| *h == col.trim().to_lowercase()) else {
                return Err(Fail::from(format!("column map: the csv has no column {}. Its columns: {}.", quote_clip(col), heads.iter().take(MAX_COLUMNS).map(|h| quote_clip(h)).collect::<Vec<_>>().join(", "))));
            };
            let Some(p) = prop_of(prop) else { return Err(Fail::from(format!("column map: the property is one of {}, not {}.", PROPERTIES.join(", "), quote_clip(prop)))) };
            mapped.push((i, p, heads[i].clone()));
        }
    }
    if mapped.is_empty() {
        return Err(Fail::from(format!("no column is mapped: name a column Id, Name or Text, or give the column map (a column to one of {}).", PROPERTIES.join(", "))));
    }
    let key_header = o.key_column.as_deref().unwrap_or(KEY_HEADER).trim().to_lowercase();
    let c_key = lower.iter().position(|h| *h == key_header);
    let owner = match o.owner.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => Some(m.find(q).ok_or_else(|| Fail::from(format!("owner {} is not an element of the model.", quote_clip(q))))?),
        None => None,
    };
    let scope = match o.scope.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => Some(m.find(q).ok_or_else(|| Fail::from(format!("scope {} is not an element of the model.", quote_clip(q))))?),
        None => None,
    };
    let rev = reverse_ids(ids);
    let cell = |row: &Vec<String>, i: usize| -> String { strip_formula_guard(row.get(i).map(String::as_str).unwrap_or("").trim()).trim().to_string() };

    let mut listed: Vec<Value> = Vec::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::from([("new", 0), ("updated", 0), ("unchanged", 0), ("obsolete", 0), ("refused", 0)]);
    let list = |listed: &mut Vec<Value>, counts: &mut BTreeMap<&str, usize>, status: &'static str, v: Value| {
        *counts.entry(status).or_default() += 1;
        if listed.len() < MAX_LISTED {
            listed.push(v);
        }
    };
    let mut edits: Vec<Value> = Vec::new();
    let mut seen_keys: HashMap<String, usize> = HashMap::new();
    let mut taken_names: HashSet<String> = owner.map(|o| m.children(o.id).filter_map(|c| c.name.clone()).collect()).unwrap_or_default();
    let mut not_proposed = 0usize;
    for (n, row) in rows.iter().enumerate().skip(1) {
        let line = n + 1;
        let refuse = |why: String| json!({ "line": line, "status": "refused", "reason": why });
        // A row refused for its shape still names its element: the element is not Obsolete because its row was refused.
        let key = c_key.map(|i| cell(row, i)).unwrap_or_default();
        if row.len() > header.len() {
            if !key.is_empty() {
                seen_keys.entry(key.clone()).or_insert(line);
            }
            list(&mut listed, &mut counts, "refused", refuse(format!("the row has {} cells and the header {}", row.len(), header.len())));
            continue;
        }
        if let Some(i) = row.iter().position(|c| c.len() > MAX_CELL) {
            if !key.is_empty() {
                seen_keys.entry(key.clone()).or_insert(line);
            }
            list(&mut listed, &mut counts, "refused", refuse(format!("the cell in column {} is longer than {MAX_CELL} bytes", i + 1)));
            continue;
        }
        if !key.is_empty() {
            if let Some(first) = seen_keys.get(&key) {
                list(&mut listed, &mut counts, "refused", refuse(format!("the key {} is also on line {first}", quote_clip(&key))));
                continue;
            }
            seen_keys.insert(key.clone(), line);
            let found = ids.get(&key).and_then(|q| m.find(q)).or_else(|| m.find(&key));
            let Some(e) = found else {
                list(&mut listed, &mut counts, "refused", refuse(format!("the key {} is not an element of the model", quote_clip(&key))));
                continue;
            };
            if e.kind != "requirement" || e.is_def {
                list(&mut listed, &mut counts, "refused", refuse(format!("the key {} is not a requirement", quote_clip(&key))));
                continue;
            }
            let mut changes: Vec<Value> = Vec::new();
            let mut row_edits: Vec<Value> = Vec::new();
            let mut bad: Option<String> = None;
            for (i, p, title) in &mapped {
                let v = cell(row, *i);
                match p {
                    Prop::Text => {
                        let before = e.doc.clone().map(|d| d.trim().to_string()).unwrap_or_default();
                        if !same_text(&before, &v) {
                            changes.push(json!({ "property": title, "before": before, "after": v }));
                            row_edits.push(json!({ "op": "update_element", "ref": e.qualified_name, "doc": v }));
                        }
                    }
                    Prop::Name => {
                        let before = e.name.clone().unwrap_or_default();
                        if v != before {
                            if !usable_name(&v) {
                                bad = Some(format!("a new name is 1 to {MAX_NAME} characters without line breaks"));
                                break;
                            }
                            changes.push(json!({ "property": title, "before": before, "after": v }));
                            row_edits.push(json!({ "op": "update_element", "ref": e.qualified_name, "rename": v }));
                        }
                    }
                    Prop::Id => {
                        let before = id_of(m, e);
                        if v != before {
                            if !safe_id(&v) {
                                bad = Some("an id is 1 to 60 characters without quotes, angle brackets, backslash or line breaks".into());
                                break;
                            }
                            changes.push(json!({ "property": title, "before": before, "after": v }));
                            row_edits.push(match id_attr(m, e) {
                                Some(a) => json!({ "op": "update_element", "ref": m.elements[a].qualified_name, "value": format!("\"{v}\"") }),
                                None => json!({ "op": "update_element", "ref": e.qualified_name, "short_name": v }),
                            });
                        }
                    }
                }
            }
            if let Some(why) = bad {
                list(&mut listed, &mut counts, "refused", refuse(why));
            } else if changes.is_empty() {
                list(&mut listed, &mut counts, "unchanged", json!({ "line": line, "status": "unchanged", "key": key, "ref": e.qualified_name }));
            } else if edits.len() + row_edits.len() > super::intent::MAX_EDITS {
                not_proposed += 1;
                list(&mut listed, &mut counts, "updated", json!({ "line": line, "status": "updated", "key": key, "ref": e.qualified_name, "changes": changes, "not_proposed": format!("the proposal holds at most {} edits; split the file", super::intent::MAX_EDITS) }));
            } else {
                edits.extend(row_edits);
                list(&mut listed, &mut counts, "updated", json!({ "line": line, "status": "updated", "key": key, "ref": e.qualified_name, "changes": changes }));
            }
            continue;
        }
        // No key: a New requirement, in the owner given.
        let Some(own) = owner else {
            list(&mut listed, &mut counts, "refused", refuse("the row has no key and no owner is given for New rows".into()));
            continue;
        };
        let get = |p: Prop| mapped.iter().find(|(_, q, _)| *q == p).map(|(i, _, _)| cell(row, *i)).unwrap_or_default();
        let (id, name_cell, text) = (get(Prop::Id), get(Prop::Name), get(Prop::Text));
        if id.is_empty() && name_cell.is_empty() {
            list(&mut listed, &mut counts, "refused", refuse("the row has no key, no Id and no Name".into()));
            continue;
        }
        if !id.is_empty() && !safe_id(&id) {
            list(&mut listed, &mut counts, "refused", refuse("an id is 1 to 60 characters without quotes, angle brackets, backslash or line breaks".into()));
            continue;
        }
        let base = if !name_cell.is_empty() { name_from_id(&name_cell) } else { name_from_id(&id) };
        let mut name = base.chars().take(MAX_NAME).collect::<String>();
        let mut k = 2;
        while taken_names.contains(&name) {
            name = format!("{}_{k}", base.chars().take(MAX_NAME - 6).collect::<String>());
            k += 1;
        }
        let mut edit = json!({ "op": "create_element", "kind": "requirement", "owner": own.qualified_name, "name": name });
        if !id.is_empty() {
            edit["short_name"] = json!(id);
        }
        if !text.is_empty() {
            edit["doc"] = json!(text);
        }
        if edits.len() + 1 > super::intent::MAX_EDITS {
            not_proposed += 1;
            list(&mut listed, &mut counts, "new", json!({ "line": line, "status": "new", "name": name, "not_proposed": format!("the proposal holds at most {} edits; split the file", super::intent::MAX_EDITS) }));
        } else {
            taken_names.insert(name.clone());
            edits.push(edit);
            list(&mut listed, &mut counts, "new", json!({ "line": line, "status": "new", "name": name, "id": id }));
        }
    }
    // Obsolete: requirements under the scope whose key the file does not have. Listed, never deleted.
    if let Some(sc) = scope {
        let mut under: HashSet<ElementId> = m.descendants(sc.id).into_iter().collect();
        under.insert(sc.id);
        let mut have: HashSet<&str> = seen_keys.keys().map(String::as_str).collect();
        have.insert("");
        for e in m.elements.iter().filter(|e| e.kind == "requirement" && !e.is_def && under.contains(&e.id)) {
            let k = key_of(&rev, e);
            if !have.contains(k.as_str()) && !have.contains(e.qualified_name.as_str()) {
                list(&mut listed, &mut counts, "obsolete", json!({ "status": "obsolete", "key": k, "ref": e.qualified_name, "note": "in the model under the scope, not in the file; it is not deleted" }));
            }
        }
    }
    let mut bounds: Vec<String> = Vec::new();
    if counts.values().sum::<usize>() > listed.len() {
        bounds.push(format!("rows: the first {MAX_LISTED} are listed; the counts are full"));
    }
    if not_proposed > 0 {
        bounds.push(format!("edits: a proposal holds at most {} edits; {not_proposed} row(s) are not proposed. Fix: split the file.", super::intent::MAX_EDITS));
    }
    Ok(json!({
        "counts": counts,
        "rows": listed,
        "edits": edits,
        "mapped": mapped.iter().map(|(_, p, t)| json!({ "column": t, "property": match p { Prop::Id => "Id", Prop::Name => "Name", Prop::Text => "Text" } })).collect::<Vec<_>>(),
        "key_column": if c_key.is_some() { json!(key_header) } else { Value::Null },
        "truncated": !bounds.is_empty(),
        "bounds": bounds,
        "limits": { "bytes": MAX_BYTES, "rows": MAX_ROWS, "columns": MAX_COLUMNS, "cell_bytes": MAX_CELL, "edits": super::intent::MAX_EDITS },
    }))
}
