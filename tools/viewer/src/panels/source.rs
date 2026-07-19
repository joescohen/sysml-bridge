#![allow(dead_code)]
use crate::model::{ElementId, ElementKind, Model};

/// Show reconstructed textual notation for the selected element.
pub fn show(ui: &mut egui::Ui, model: &Model, selected: Option<ElementId>) {
    ui.heading("Source");
    ui.separator();

    let id = match selected {
        Some(id) => id,
        None => {
            ui.label("No element selected.");
            return;
        }
    };

    let text = reconstruct(model, id, 0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(&mut text.as_str())
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .interactive(false),
        );
    });
}

/// Reconstruct textual notation for an element.
fn reconstruct(model: &Model, id: ElementId, indent: usize) -> String {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return String::new(),
    };

    let pad = "    ".repeat(indent);
    let mut out = String::new();

    // Qualifiers
    for q in &elem.qualifiers {
        out.push_str(&format!("{pad}{q} "));
    }

    // Keyword
    if elem.is_abstract {
        out.push_str(&format!("{pad}abstract "));
    }
    if elem.is_variation {
        out.push_str("variation ");
    }
    out.push_str(elem.kind.keyword());

    // Name
    if let Some(ref short) = elem.short_name {
        out.push_str(&format!(" <{short}>"));
    }
    if let Some(ref name) = elem.name {
        out.push_str(&format!(" {name}"));
    }

    // Type
    if let Some(ref tref) = elem.type_ref {
        let conj = if elem.is_conjugated { "~" } else { "" };
        out.push_str(&format!(" : {conj}{tref}"));
    }

    // Specializations
    for spec in &elem.specializes {
        out.push_str(&format!(" :> {spec}"));
    }

    // Multiplicity
    if let Some(ref mult) = elem.multiplicity {
        out.push_str(&format!(" [{mult}]"));
    }

    // Children
    let structural_children: Vec<ElementId> = elem
        .children
        .iter()
        .copied()
        .filter(|&cid| {
            model
                .element(cid)
                .is_some_and(|c| !matches!(c.kind, ElementKind::Import | ElementKind::Unknown))
        })
        .collect();

    if structural_children.is_empty() {
        out.push(';');
    } else {
        out.push_str(" {\n");
        for &child_id in &structural_children {
            out.push_str(&reconstruct(model, child_id, indent + 1));
            out.push('\n');
        }
        out.push_str(&format!("{pad}}}"));
    }

    out
}
