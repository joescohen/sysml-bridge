#![allow(dead_code)]
//! Search bar overlay for the model map.
//!
//! Activated by `/` or `Ctrl-F`. Provides type-ahead search by element
//! name, highlights matches on the map, and allows cycling through results.

use crate::model::{ElementId, Model};

/// Search overlay state.
#[derive(Default)]
pub struct SearchState {
    /// Current search query.
    pub query: String,
    /// Element IDs matching the query.
    pub matches: Vec<ElementId>,
    /// Index into `matches` for the current highlighted match.
    pub current_match: usize,
    /// Whether the search bar is visible/active.
    pub active: bool,
}

impl SearchState {
    /// Update search results based on the current query.
    pub fn update_matches(&mut self, model: &Model) {
        self.matches = model.search_by_name(&self.query);
        if self.current_match >= self.matches.len() {
            self.current_match = 0;
        }
    }

    /// Advance to the next match.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    /// Go to the previous match.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }

    /// Get the currently highlighted match.
    pub fn current(&self) -> Option<ElementId> {
        self.matches.get(self.current_match).copied()
    }

    /// Whether a given element matches the search.
    pub fn is_match(&self, id: ElementId) -> bool {
        self.matches.contains(&id)
    }

    /// Whether a given element is the current highlighted match.
    pub fn is_current(&self, id: ElementId) -> bool {
        self.current() == Some(id)
    }

    /// Clear the search and deactivate.
    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
        self.active = false;
    }
}

/// Render the search bar overlay at the top of the canvas.
pub fn show_search_bar(ui: &mut egui::Ui, state: &mut SearchState, model: &Model) {
    if !state.active {
        return;
    }

    let available_width = ui.available_width();
    let bar_width = (available_width * 0.4).min(400.0).max(200.0);

    egui::Area::new(egui::Id::new("search_bar"))
        .fixed_pos(egui::pos2(
            (available_width - bar_width) / 2.0 + ui.min_rect().left(),
            ui.min_rect().top() + 8.0,
        ))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(bar_width);
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let response = ui.text_edit_singleline(&mut state.query);
                    if response.changed() {
                        state.update_matches(model);
                    }
                    // Auto-focus
                    response.request_focus();

                    if !state.matches.is_empty() {
                        ui.label(format!(
                            "{}/{}",
                            state.current_match + 1,
                            state.matches.len()
                        ));
                    } else if !state.query.is_empty() {
                        ui.label("No matches");
                    }
                });
            });
        });
}
