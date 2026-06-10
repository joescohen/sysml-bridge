#![allow(dead_code)]
use std::collections::HashSet;

use crate::model::{ElementId, ElementKind, Model};

/// Collect all ancestor IDs from `id` up to the root.
fn ancestor_ids(model: &Model, id: ElementId) -> HashSet<ElementId> {
    let mut ancestors = HashSet::new();
    let mut current = model.element(id).and_then(|e| e.parent);
    while let Some(pid) = current {
        ancestors.insert(pid);
        current = model.element(pid).and_then(|e| e.parent);
    }
    ancestors
}

/// Show the model browser tree in a side panel (with heading, separator, ScrollArea).
pub fn show(
    ui: &mut egui::Ui,
    model: &Model,
    selected: &mut Option<ElementId>,
    view_context_request: &mut Option<ElementId>,
) {
    ui.heading("Model Browser");
    ui.separator();

    // Compute ancestors of the selected element so we can auto-expand
    // the tree path when something is clicked on the canvas.
    let reveal = selected
        .map(|id| ancestor_ids(model, id))
        .unwrap_or_default();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for &root_id in &model.root_ids {
            show_tree_node(
                ui,
                model,
                root_id,
                selected,
                view_context_request,
                0,
                &reveal,
            );
        }
    });
}

/// Render just the tree nodes (no heading, no separator, no ScrollArea).
/// For use inside a caller-provided ScrollArea (e.g. the Element Library).
pub fn show_tree(
    ui: &mut egui::Ui,
    model: &Model,
    selected: &mut Option<ElementId>,
    view_context_request: &mut Option<ElementId>,
) {
    let reveal = selected
        .map(|id| ancestor_ids(model, id))
        .unwrap_or_default();

    for &root_id in &model.root_ids {
        show_tree_node(
            ui,
            model,
            root_id,
            selected,
            view_context_request,
            0,
            &reveal,
        );
    }
}

fn show_tree_node(
    ui: &mut egui::Ui,
    model: &Model,
    id: ElementId,
    selected: &mut Option<ElementId>,
    view_context_request: &mut Option<ElementId>,
    depth: usize,
    reveal: &HashSet<ElementId>,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };

    // Skip imports, aliases, and other non-structural elements at depth > 1
    if depth > 1
        && matches!(
            elem.kind,
            ElementKind::Import | ElementKind::Alias | ElementKind::Comment | ElementKind::Unknown
        )
    {
        return;
    }

    let icon = elem.kind.browser_icon();
    let name = elem.display_name();
    let label = format!("{icon} {name}");
    let is_selected = *selected == Some(id);

    // Use theme-aware colors instead of hardcoded values.
    let text_color = if is_selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };

    let has_structural_children = elem.children.iter().any(|&cid| {
        model.element(cid).is_some_and(|c| {
            !matches!(
                c.kind,
                ElementKind::Import
                    | ElementKind::Alias
                    | ElementKind::Comment
                    | ElementKind::Unknown
            )
        })
    });

    if has_structural_children {
        // Auto-expand if this node is an ancestor of the selected element,
        // so clicking a part on the canvas reveals it in the tree.
        let force_open = reveal.contains(&id);
        let mut header = egui::CollapsingHeader::new(egui::RichText::new(&label).color(text_color))
            .id_salt(id)
            .default_open(depth < 2);

        if force_open {
            header = header.open(Some(true));
        }

        let resp = header.show(ui, |ui| {
            for &child_id in &elem.children {
                show_tree_node(
                    ui,
                    model,
                    child_id,
                    selected,
                    view_context_request,
                    depth + 1,
                    reveal,
                );
            }
        });

        // Click on header selects element
        if resp.header_response.clicked() {
            *selected = Some(id);
        }
        // Double-click sets as view context
        if resp.header_response.double_clicked() {
            *view_context_request = Some(id);
        }

        // Scroll the selected node into view
        if is_selected {
            resp.header_response.scroll_to_me(Some(egui::Align::Center));
        }
    } else {
        let resp = ui.selectable_label(is_selected, egui::RichText::new(&label).color(text_color));
        if resp.clicked() {
            *selected = Some(id);
        }
        if resp.double_clicked() {
            *view_context_request = Some(id);
        }
        if is_selected {
            resp.scroll_to_me(Some(egui::Align::Center));
        }
    }
}
