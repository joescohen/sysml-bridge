#![allow(dead_code)]
//! Kind filter chips for the model map.
//!
//! Toggle chips overlaid on the map edge to filter by element metatype
//! or relationship kind. Filtered-out elements fade to low opacity.

use std::collections::HashMap;

use crate::model::{ElementId, ElementKind, Model, RelationshipKind};

/// Filter state for the map view.
#[derive(Default)]
pub struct FilterState {
    /// Which element kinds are visible (all true by default).
    pub kind_visible: HashMap<ElementKind, bool>,
    /// Which relationship kinds are visible (all true by default).
    pub rel_visible: HashMap<RelationshipKind, bool>,
    /// Neighborhood filter: show only N-hop neighbors of selected.
    pub neighborhood_hops: Option<usize>,
    /// Cached neighborhood set (element IDs in the neighborhood).
    pub neighborhood_set: Vec<ElementId>,
}


impl FilterState {
    /// Whether any filter is active.
    pub fn is_active(&self) -> bool {
        self.kind_visible.values().any(|&v| !v)
            || self.rel_visible.values().any(|&v| !v)
            || self.neighborhood_hops.is_some()
    }

    /// Compute the opacity for an element based on current filters.
    /// Returns 255 for fully visible, lower values for dimmed.
    pub fn element_opacity(&self, id: ElementId, model: &Model) -> u8 {
        let elem = match model.element(id) {
            Some(e) => e,
            None => return 255,
        };

        // Kind filter
        if let Some(&visible) = self.kind_visible.get(&elem.kind)
            && !visible {
                return crate::render::theme::FILTER_HIDDEN_ALPHA;
            }

        // Neighborhood filter
        if self.neighborhood_hops.is_some() && !self.neighborhood_set.contains(&id) {
            return crate::render::theme::FILTER_HIDDEN_ALPHA;
        }

        255
    }

    /// Update neighborhood set from a selected element.
    pub fn update_neighborhood(&mut self, selected: Option<ElementId>, model: &Model) {
        match (self.neighborhood_hops, selected) {
            (Some(hops), Some(sel)) => {
                self.neighborhood_set = collect_neighborhood(model, sel, hops);
            }
            _ => {
                self.neighborhood_set.clear();
            }
        }
    }

    /// Clear all filters.
    pub fn clear(&mut self) {
        for v in self.kind_visible.values_mut() {
            *v = true;
        }
        for v in self.rel_visible.values_mut() {
            *v = true;
        }
        self.neighborhood_hops = None;
        self.neighborhood_set.clear();
    }

    /// Ensure all element kinds present in the model have entries.
    pub fn initialize_from_model(&mut self, model: &Model) {
        if !self.kind_visible.is_empty() {
            return; // Already initialized
        }
        for kind in significant_kinds() {
            self.kind_visible.entry(kind).or_insert(true);
        }
        for kind in all_rel_kinds() {
            self.rel_visible.entry(kind).or_insert(true);
        }
        let _ = model; // used for future expansion
    }
}

/// Collect elements within N hops of `start`.
fn collect_neighborhood(model: &Model, start: ElementId, hops: usize) -> Vec<ElementId> {
    let mut visited = vec![start];
    let mut frontier = vec![start];

    for _ in 0..hops {
        let mut next_frontier = Vec::new();
        for &id in &frontier {
            for neighbor in model.neighbors(id) {
                if !visited.contains(&neighbor) {
                    visited.push(neighbor);
                    next_frontier.push(neighbor);
                }
            }
        }
        frontier = next_frontier;
    }

    visited
}

/// The significant element kinds shown as filter chips.
fn significant_kinds() -> Vec<ElementKind> {
    vec![
        ElementKind::Package,
        ElementKind::PartDef,
        ElementKind::PartUsage,
        ElementKind::PortDef,
        ElementKind::PortUsage,
        ElementKind::AttributeDef,
        ElementKind::AttributeUsage,
        ElementKind::ActionDef,
        ElementKind::ActionUsage,
        ElementKind::StateDef,
        ElementKind::StateUsage,
        ElementKind::RequirementDef,
        ElementKind::RequirementUsage,
        ElementKind::ConnectionDef,
        ElementKind::InterfaceDef,
        ElementKind::EnumDef,
    ]
}

fn all_rel_kinds() -> Vec<RelationshipKind> {
    vec![
        RelationshipKind::Connect,
        RelationshipKind::Bind,
        RelationshipKind::Flow,
        RelationshipKind::Interface,
        RelationshipKind::Allocate,
    ]
}

/// Render filter chips as a horizontal row at the top of the canvas.
pub fn show_filter_chips(ui: &mut egui::Ui, state: &mut FilterState) {
    egui::Area::new(egui::Id::new("filter_chips"))
        .fixed_pos(egui::pos2(
            ui.min_rect().left() + 8.0,
            ui.min_rect().top() + 44.0,
        ))
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                // Kind filter chips (show only the most important ones)
                let chip_kinds = [
                    (ElementKind::Package, "Pkg"),
                    (ElementKind::PartDef, "Part Def"),
                    (ElementKind::PartUsage, "Parts"),
                    (ElementKind::PortDef, "Port Def"),
                    (ElementKind::ActionDef, "Actions"),
                    (ElementKind::RequirementDef, "Reqs"),
                    (ElementKind::EnumDef, "Enums"),
                ];

                for (kind, label) in chip_kinds {
                    let active = state.kind_visible.get(&kind).copied().unwrap_or(true);
                    let style = if active {
                        egui::RichText::new(label).strong()
                    } else {
                        egui::RichText::new(label).weak()
                    };
                    if ui.selectable_label(active, style).clicked() {
                        let entry = state.kind_visible.entry(kind).or_insert(true);
                        *entry = !*entry;
                    }
                }

                if state.is_active()
                    && ui.small_button("Clear filters").clicked() {
                        state.clear();
                    }
            });
        });
}
