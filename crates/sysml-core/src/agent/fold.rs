//! `fold_copy`: put a copied definition back into the one it copies. A copy made only to show fewer ports is a second
//! definition of one thing; folding it keeps one.
//!
//! Every usage typed by the copy is typed by the original instead, so each keeps its name, links and connections (their
//! ends name ports the original has too); the copy is deleted; and the reply lists the ports the original has and the
//! copy left out.
//! The copy must hold nothing the original lacks, and nothing but typed usages may name it, or nothing is written and the
//! reply says what to move first.

use super::ids::IdEvent;
use super::intent::{model_of, step};
use super::{locate, qshow, Res};
use crate::api::FileIn;
use crate::edit::EditOp;
use crate::model::*;
use serde_json::{json, Value};

/// A member as a copy repeats it: keyword, name as read, and its source text with the names of its types left out
/// (a usage typed by the copy's own copies reads the same as one typed by the originals).
fn member_key(model: &Model, c: &Element) -> String {
    let name = norm(c.name.as_deref().unwrap_or(""));
    if !name.is_empty() {
        let types: Vec<String> = c.refs_of(RefRole::Type).map(|r| r.resolved().map(|d| model.elements[d].qualified_name.clone()).unwrap_or_else(|| r.text.clone())).collect();
        return format!("{} {name} : {}", c.keyword(), types.join(","));
    }
    let src = model.files.get(c.file).and_then(|f| f.text.get(c.span.start..c.span.end)).unwrap_or("");
    format!("{} {}", c.keyword(), src.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn norm(n: &str) -> String {
    n.chars().filter(|x| x.is_alphanumeric()).flat_map(|x| x.to_lowercase()).collect()
}

pub fn fold_copy(work: &mut Vec<FileIn>, a: &Value, events: &mut Vec<IdEvent>) -> Res<Value> {
    let model = model_of(work);
    let copy = locate(&model, super::intent::req(a, "copy")?)?;
    let into = locate(&model, super::intent::req(a, "into")?)?;
    let (cq, iq) = (copy.qualified_name.clone(), into.qualified_name.clone());
    if copy.id == into.id {
        return Err(format!("{} is {}: name the copy and the definition it copies.", qshow(&cq), qshow(&iq)).into());
    }
    if !copy.is_def || !into.is_def || copy.kind != into.kind {
        return Err(format!("{} ({}) and {} ({}) are not two definitions of one kind: only a copied definition folds back into its original.", qshow(&cq), copy.keyword(), qshow(&iq), into.keyword()).into());
    }
    if model.descendants(into.id).contains(&copy.id) || model.descendants(copy.id).contains(&into.id) {
        return Err(format!("{} and {} hold one another: fold a copy that stands apart from its original.", qshow(&cq), qshow(&iq)).into());
    }
    // Nothing of the copy's own may be lost: each member must be one the original has.
    let theirs: std::collections::HashSet<String> = into.children.iter().map(|c| member_key(&model, &model.elements[*c])).collect();
    let own: Vec<String> = copy
        .children
        .iter()
        .map(|c| &model.elements[*c])
        .filter(|c| !matches!(c.kind.as_str(), "doc" | "comment" | "import") && !theirs.contains(&member_key(&model, c)))
        .map(|c| c.name.clone().map(|n| format!("{} {n}", c.keyword())).unwrap_or_else(|| c.keyword()))
        .collect();
    if !own.is_empty() {
        return Err(format!(
            "{} holds what {} does not: {}. Folding it would lose them. Fix: add them to {} (or, when they are other members under the same names, rename them there), then fold again.",
            qshow(&cq),
            qshow(&iq),
            own.join(", "),
            qshow(&iq)
        )
        .into());
    }
    let kept: std::collections::BTreeSet<String> = copy.children.iter().filter_map(|c| model.elements[*c].name.as_deref()).map(norm).collect();
    let left_out = |kind: &str| -> Vec<String> {
        into.children.iter().map(|c| &model.elements[*c]).filter(|c| c.kind == kind && c.name.as_deref().is_some_and(|n| !kept.contains(&norm(n)))).map(|c| c.qualified_name.clone()).collect()
    };
    let (hidden, parts) = (left_out("port"), left_out("part"));
    // Every usage typed by the copy is typed by the original.
    let mut retyped: Vec<String> = Vec::new();
    for e in model.elements.iter().filter(|e| !e.is_def && e.refs_of(RefRole::Type).any(|r| r.resolved() == Some(copy.id))) {
        let text = e
            .refs_of(RefRole::Type)
            .map(|r| {
                let name = if r.resolved() == Some(copy.id) { iq.clone() } else { r.text.clone() };
                format!("{}{name}", if r.conjugated { "~" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(", ");
        retyped.push(e.qualified_name.clone());
        step(work, EditOp::SetType { target: e.qualified_name.clone(), type_ref: text })?;
    }
    // What still names the copy (a specialization, a satisfy, a dependency) would dangle once it is gone.
    let now = model_of(work);
    let still: Vec<String> = now
        .find(&cq)
        .map(|c| super::referrers(&now, c.id, false))
        .unwrap_or_default()
        .iter()
        .map(|(o, role)| format!("{} ({role:?})", qshow(&now.elements[*o].qualified_name)))
        .collect();
    if !still.is_empty() {
        return Err(format!("{} is still named by {}. Fix: point those at {} (or delete them), then fold again.", qshow(&cq), still.join(", "), qshow(&iq)).into());
    }
    if let Some(e) = now.find(&cq) {
        // Each member of the copy is the original's member of that name from now on: a connection to `bus.imageIn`
        // that reached the copy's port reaches the original's, and an id that named the one names the other.
        for c in e.children.iter().map(|c| &now.elements[*c]) {
            let Some(n) = c.name.as_deref() else { continue };
            if let Some(o) = now.find(&iq).into_iter().flat_map(|o| o.children.iter()).map(|o| &now.elements[*o]).find(|o| o.name.as_deref().is_some_and(|m| norm(m) == norm(n))) {
                events.push(IdEvent::Rename { from: c.qualified_name.clone(), to: o.qualified_name.clone() });
            }
        }
        let (owner, index) = super::ids::place_of(&now, e);
        events.push(IdEvent::Delete { qn: cq.clone(), owner, index });
    }
    step(work, EditOp::Delete { target: cq.clone() })?;
    Ok(json!({
        "folded": cq,
        "into": iq,
        "retyped": retyped,
        "hidden_ports": hidden,
        "parts_left_out": parts,
        "next": if retyped.is_empty() { format!("no usage was typed by {}; {} is the one definition now", qshow(&cq), qshow(&iq)) } else { format!("{} usage(s) are typed by {} now", retyped.len(), qshow(&iq)) },
    }))
}
