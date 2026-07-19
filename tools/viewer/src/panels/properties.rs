#![allow(dead_code)]
use crate::model::{ElementId, Model};

/// Show the properties panel for the currently selected element.
pub fn show(ui: &mut egui::Ui, model: &Model, selected: Option<ElementId>) {
    ui.heading("Properties");
    ui.separator();

    let id = match selected {
        Some(id) => id,
        None => {
            ui.label("No element selected.");
            return;
        }
    };

    let elem = match model.element(id) {
        Some(e) => e,
        None => {
            ui.label("Element not found.");
            return;
        }
    };

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("properties_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Kind:");
                ui.label(elem.kind.keyword());
                ui.end_row();

                ui.label("Metatype:");
                ui.label(format!("{:?}", elem.kind.metatype()));
                ui.end_row();

                ui.label("Name:");
                ui.label(elem.display_name());
                ui.end_row();

                if let Some(ref short) = elem.short_name {
                    ui.label("Short name:");
                    ui.label(short);
                    ui.end_row();
                }

                if let Some(ref tref) = elem.type_ref {
                    ui.label("Type:");
                    ui.label(tref);
                    ui.end_row();
                }

                if !elem.specializes.is_empty() {
                    ui.label("Specializes:");
                    ui.label(elem.specializes.join(", "));
                    ui.end_row();
                }

                if let Some(ref mult) = elem.multiplicity {
                    ui.label("Multiplicity:");
                    ui.label(mult);
                    ui.end_row();
                }

                if elem.is_conjugated {
                    ui.label("Conjugated:");
                    ui.label("yes");
                    ui.end_row();
                }

                if elem.is_abstract {
                    ui.label("Abstract:");
                    ui.label("yes");
                    ui.end_row();
                }

                if !elem.qualifiers.is_empty() {
                    ui.label("Qualifiers:");
                    ui.label(elem.qualifiers.join(", "));
                    ui.end_row();
                }

                ui.label("Qualified name:");
                ui.label(elem.qualified_name(model));
                ui.end_row();

                if let Some(ref doc) = elem.doc {
                    ui.label("Doc:");
                    ui.label(doc);
                    ui.end_row();
                }

                ui.label("Children:");
                ui.label(format!("{}", elem.children.len()));
                ui.end_row();
            });
    });
}
