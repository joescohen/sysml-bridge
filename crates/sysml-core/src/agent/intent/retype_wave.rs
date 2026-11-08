//! Retype waves: a batch of plain retypes (`update_element {ref, typed_by}`) planned on one model and written at once.
//!
//! One edit costs a parse and resolve of the whole workspace; on a work-scale model that is most of a second. A stitch
//! that makes an item one def end to end retypes dozens of pins, none of which changes what another one names. Each
//! retype is planned against the same model, the type texts are patched in together, and one analysis reads the result
//! back. Like the create waves (`wave.rs`) this is a fast path only: when anything is in doubt it answers `None` and the
//! batch runs one edit at a time, which gives the replies and the failure messages.
//!
//! The read-back: the same elements by qualified name as before; each target is typed by the definition its edit named;
//! no more names that do not resolve than before; and the whole-model checks of a batch of links and retypes
//! (`batch_checks`) pass. `SYSML_NO_WAVES=1` turns this path off too.

use super::*;

const MARGIN: usize = 2;

fn plain_retype(e: &Value) -> Option<(&str, &str)> {
    let o = e.as_object()?;
    if e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) != Some("update_element") {
        return None;
    }
    if !o.keys().all(|k| matches!(k.as_str(), "op" | "tool" | "ref" | "typed_by")) {
        return None;
    }
    let r = o.get("ref")?.as_str()?.trim();
    let t = o.get("typed_by")?.as_str()?.trim();
    // a ref by source id or position, a list of types or a conjugated one: one by one
    if r.is_empty() || t.is_empty() || r.contains('$') || r.contains("source_id:") || t.contains(',') || t.starts_with('~') {
        return None;
    }
    Some((r, t))
}

/// A `replace {target, text}` of a statement (the `flow of OLD` a retype rewrites to `flow of NEW`).
fn plain_replace(e: &Value) -> Option<(&str, &str)> {
    let o = e.as_object()?;
    if e.get("op").or_else(|| e.get("tool")).and_then(Value::as_str) != Some("replace") || !o.keys().all(|k| matches!(k.as_str(), "op" | "tool" | "target" | "text")) {
        return None;
    }
    let t = o.get("target")?.as_str()?.trim();
    let x = o.get("text")?.as_str()?;
    (!t.is_empty() && !t.contains("source_id:") && !x.trim().is_empty() && !x.contains('{') && !x.contains("/*")).then_some((t, x))
}

fn plain(e: &Value) -> bool {
    plain_retype(e).is_some() || plain_replace(e).is_some()
}

pub(super) fn worth_trying(edits: &[Value]) -> bool {
    if std::env::var_os("SYSML_WAVE_DEBUG").is_some() {
        eprintln!("retype wave? {} {:?}", edits.len(), edits.iter().position(|e| !plain(e)).map(|i| edits[i].to_string()));
    }
    edits.len() >= 2 && super::wave::enabled() && deferrable_checks() && edits.iter().all(plain) && edits.iter().any(|e| plain_retype(e).is_some())
}

pub(super) fn run(files: &[FileIn], edits: &[Value]) -> Option<Batch> {
    let r = run_inner(files, edits);
    if r.is_err() && std::env::var_os("SYSML_WAVE_DEBUG").is_some() {
        eprintln!("retype wave: {:?}", r.as_ref().err());
    }
    r.ok()
}

fn run_inner(files: &[FileIn], edits: &[Value]) -> Result<Batch, &'static str> {
    let model = model_of(files);
    let mut patches: Vec<(String, usize, usize, String)> = Vec::new();
    let mut plan: Vec<(String, String)> = Vec::new(); // (target qn, wanted def qn)
    // a pin two retypes of one batch both name (two chains that meet): the same type twice is one patch
    let mut seen: HashMap<ElementId, String> = HashMap::new();
    let mut replaced: HashMap<ElementId, String> = HashMap::new();
    for e in edits {
        if let Some((t, x)) = plain_replace(e) {
            let el = model.find(t).ok_or("replace target")?;
            if el.is_def {
                return Err("replace target kind");
            }
            match replaced.get(&el.id) {
                Some(prev) if prev == x => {
                    plan.push((String::new(), String::new()));
                    continue;
                }
                Some(_) => return Err("replace twice"),
                None => {
                    replaced.insert(el.id, x.to_string());
                }
            }
            let changes = crate::edit::apply(&model, &crate::edit::EditOp::Replace { target: t.to_string(), text: x.to_string() }).map_err(|_| "replace apply")?;
            let [c] = changes.as_slice() else { return Err("changes") };
            let old = files.iter().find(|f| f.path == c.path).ok_or("file")?;
            let (p, q) = changed_region(&old.text, &c.text);
            let end = old.text.len().checked_sub(c.text.len() - q).filter(|x| *x >= p).ok_or("region")?;
            let piece = c.text.get(p..q).ok_or("piece")?.to_string();
            if patches.iter().any(|(f, s, en, _)| *f == c.path && p < en + MARGIN && *s < end + MARGIN) {
                return Err("overlap");
            }
            patches.push((c.path.clone(), p, end, piece));
            plan.push((String::new(), String::new()));
            continue;
        }
        let (r, t) = plain_retype(e).ok_or("plain")?;
        let target = locate(&model, r).map_err(|_| "locate")?;
        if target.is_def || target.name.is_none() || target.implied_name {
            return Err("target");
        }
        let def = model.find(t).filter(|d| d.is_def).ok_or("def")?;
        match seen.get(&target.id) {
            Some(prev) if *prev == def.qualified_name => {
                plan.push((target.qualified_name.clone(), def.qualified_name.clone()));
                continue;
            }
            Some(_) => return Err("retyped twice"),
            None => {
                seen.insert(target.id, def.qualified_name.clone());
            }
        }
        let text = type_text(&model, t).map_err(|_| "type_text")?;
        let changes = crate::edit::apply(&model, &crate::edit::EditOp::SetType { target: target.qualified_name.clone(), type_ref: text }).map_err(|_| "apply")?;
        let [c] = changes.as_slice() else { return Err("changes") };
        let old = files.iter().find(|f| f.path == c.path).ok_or("file")?;
        let (p, q) = changed_region(&old.text, &c.text);
        let end = old.text.len().checked_sub(c.text.len() - q).filter(|x| *x >= p).ok_or("region")?;
        let piece = c.text.get(p..q).ok_or("piece")?.to_string();
        if patches.iter().any(|(f, s, en, _)| *f == c.path && p < en + MARGIN && *s < end + MARGIN) {
            return Err("overlap");
        }
        patches.push((c.path.clone(), p, end, piece));
        plan.push((target.qualified_name.clone(), def.qualified_name.clone()));
    }
    let mut out = files.to_vec();
    patches.sort_by(|a, b| (a.0.as_str(), std::cmp::Reverse(a.1)).cmp(&(b.0.as_str(), std::cmp::Reverse(b.1))));
    for (path, s, en, piece) in &patches {
        let f = out.iter_mut().find(|f| &f.path == path).ok_or("file2")?;
        f.text.replace_range(*s..*en, piece);
    }
    let after = model_of(&out);
    // the same elements, and each target typed by its definition
    if after.elements.len() != model.elements.len() {
        return Err("count");
    }
    let names = |m: &Model| m.elements.iter().map(|e| e.qualified_name.clone()).collect::<HashSet<_>>();
    if names(&model) != names(&after) {
        return Err("names");
    }
    let mut results = Vec::new();
    for (qn, want) in &plan {
        if qn.is_empty() {
            results.push(json!({ "op": "replace" }));
            continue;
        }
        let e = after.find(qn).ok_or("find")?;
        let got = e.refs_of(RefRole::Type).filter_map(|r| r.resolved()).map(|t| after.elements[t].qualified_name.as_str()).collect::<Vec<_>>();
        if got != [want.as_str()] {
            return Err("typed");
        }
        results.push(json!({ "ref": qn, "previous_ref": qn, "changed": ["typed_by"], "element": super::super::brief(&after, e), "op": "update_element", "status": "updated" }));
    }
    let unresolved = |m: &Model| m.diagnostics.iter().filter(|d| d.message.starts_with("cannot resolve")).count();
    if unresolved(&after) > unresolved(&model) {
        return Err("unresolved");
    }
    batch_checks(files, &out, &[]).map_err(|_| "checks")?;
    Ok(Batch { files: out, results, moved: Vec::new(), events: Vec::new(), source_id_moves: Vec::new(), reminted: Vec::new() })
}
