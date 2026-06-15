//! Element Library window: faceted search + results list + model tree.
//!
//! A floating window (draggable, resizable, collapsible) that provides:
//! 1. A faceted query bar (top) for searching elements by multiple criteria
//! 2. A flat results list (middle) for browsing query results
//! 3. A model tree (bottom) that auto-reveals the selected element
//!
//! When no query is active, the results section is hidden and the tree
//! fills the window. The window floats over the canvas without stealing
//! space from it.

use crate::model::query::Query;
use crate::model::{ElementId, Model};
use crate::panels::browser;
use crate::panels::canvas::SearchHighlight;

/// Sort column for the results table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum SortColumn {
    #[default]
    Name,
    Kind,
    Package,
    Type,
}


/// State for the Element Library panel.
pub struct ElementLibrary {
    /// Raw input string in the query field.
    pub query_text: String,
    /// Parsed query from the input.
    parsed: Query,
    /// Matching element IDs.
    pub results: Vec<ElementId>,
    /// Currently selected index in the results list.
    pub selected_index: usize,
    /// Current sort column.
    pub sort_column: SortColumn,
    /// Sort ascending vs descending.
    pub sort_ascending: bool,
    /// Whether the library panel is open.
    pub visible: bool,
    /// Navigation request from library → canvas.
    pub nav_request: Option<ElementId>,
    /// Whether to request focus on the query field.
    pub wants_focus: bool,
}

impl Default for ElementLibrary {
    fn default() -> Self {
        Self {
            query_text: String::new(),
            parsed: Query { terms: vec![] },
            results: Vec::new(),
            selected_index: 0,
            sort_column: SortColumn::default(),
            sort_ascending: true,
            visible: false,
            nav_request: None,
            wants_focus: false,
        }
    }
}

impl ElementLibrary {
    /// Update the query and recompute results.
    pub fn update_query(&mut self, model: &Model) {
        self.parsed = Query::parse(&self.query_text);
        self.results = model.query(&self.parsed);
        self.sort_results(model);
        if self.selected_index >= self.results.len() {
            self.selected_index = 0;
        }
    }

    /// Sort results by the current sort column.
    fn sort_results(&mut self, model: &Model) {
        let col = self.sort_column;
        let asc = self.sort_ascending;
        self.results.sort_by(|&a, &b| {
            let ea = model.element(a);
            let eb = model.element(b);
            let cmp = match col {
                SortColumn::Name => {
                    let na = ea.map(|e| e.display_name()).unwrap_or("");
                    let nb = eb.map(|e| e.display_name()).unwrap_or("");
                    na.to_lowercase().cmp(&nb.to_lowercase())
                }
                SortColumn::Kind => {
                    let ka = ea.map(|e| e.kind.keyword()).unwrap_or("");
                    let kb = eb.map(|e| e.kind.keyword()).unwrap_or("");
                    ka.cmp(kb)
                }
                SortColumn::Package => {
                    let pa = ea.map(|e| e.qualified_name(model)).unwrap_or_default();
                    let pb = eb.map(|e| e.qualified_name(model)).unwrap_or_default();
                    pa.to_lowercase().cmp(&pb.to_lowercase())
                }
                SortColumn::Type => {
                    let ta = ea.and_then(|e| e.type_ref.as_deref()).unwrap_or("");
                    let tb = eb.and_then(|e| e.type_ref.as_deref()).unwrap_or("");
                    ta.to_lowercase().cmp(&tb.to_lowercase())
                }
            };
            if asc { cmp } else { cmp.reverse() }
        });
    }

    /// Get the currently selected result element ID.
    pub fn current_result(&self) -> Option<ElementId> {
        self.results.get(self.selected_index).copied()
    }

    /// Advance to the next result.
    pub fn next_result(&mut self) {
        if !self.results.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.results.len();
        }
    }

    /// Go to the previous result.
    pub fn prev_result(&mut self) {
        if !self.results.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.results.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// Build a SearchHighlight for the canvas from the library's current state.
    pub fn search_highlight(&self) -> SearchHighlight {
        if self.query_text.is_empty() || !self.visible {
            return SearchHighlight::default();
        }
        SearchHighlight {
            active: true,
            matches: self.results.clone(),
            current: self.current_result(),
        }
    }

    /// Clear the query and results.
    pub fn clear_query(&mut self) {
        self.query_text.clear();
        self.parsed = Query { terms: vec![] };
        self.results.clear();
        self.selected_index = 0;
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.wants_focus = true;
        }
    }

    /// Open the panel and focus the query field.
    pub fn open_and_focus(&mut self) {
        self.visible = true;
        self.wants_focus = true;
    }
}

/// Render the Element Library as a native OS window (viewport).
///
/// On native platforms (OpenBSD, Windows, Linux, macOS), this creates a
/// real OS window managed by the window manager. On WASM, egui falls
/// back to an embedded `egui::Window` overlay automatically.
pub fn show_window(
    ctx: &egui::Context,
    model: &Model,
    library: &mut ElementLibrary,
    selected: &mut Option<ElementId>,
) {
    let width = crate::render::theme::LIBRARY_PANEL_WIDTH;

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("element_library"),
        egui::ViewportBuilder::default()
            .with_title("Element Library")
            .with_inner_size([width, 500.0])
            .with_min_inner_size([250.0, 200.0]),
        |ctx, class| {
            // Light theme to match the main window
            ctx.set_visuals(egui::Visuals::light());

            match class {
                egui::ViewportClass::Embedded => {
                    // WASM fallback: render as an egui::Window overlay
                    let mut open = true;
                    egui::Window::new("Element Library")
                        .id(egui::Id::new("element_library_embedded"))
                        .open(&mut open)
                        .default_width(width)
                        .default_height(500.0)
                        .resizable(true)
                        .collapsible(true)
                        .show(ctx, |ui| {
                            show_contents(ui, model, library, selected);
                        });
                    if !open {
                        library.visible = false;
                    }
                }
                _ => {
                    // Native: real OS window — use the full viewport
                    egui::CentralPanel::default().show(ctx, |ui| {
                        show_contents(ui, model, library, selected);
                    });

                    // Handle OS window close button
                    if ctx.input(|i| i.viewport().close_requested()) {
                        library.visible = false;
                    }
                }
            }
        },
    );
}

/// Inner contents drawn inside the window.
fn show_contents(
    ui: &mut egui::Ui,
    model: &Model,
    library: &mut ElementLibrary,
    selected: &mut Option<ElementId>,
) {
    // Query input field
    ui.horizontal(|ui| {
        ui.label("Query:");
        let response = ui.text_edit_singleline(&mut library.query_text);
        if response.changed() {
            library.update_query(model);
        }
        if library.wants_focus {
            response.request_focus();
            library.wants_focus = false;
        }
    });

    let has_query = !library.query_text.is_empty();

    // Results section (only shown when query is active)
    if has_query {
        ui.separator();

        // Result count and sort controls
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} results", library.results.len()))
                    .small()
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sort_options = [
                    (SortColumn::Name, "Name"),
                    (SortColumn::Kind, "Kind"),
                    (SortColumn::Type, "Type"),
                ];
                for (col, label) in sort_options {
                    let is_active = library.sort_column == col;
                    let arrow = if is_active {
                        if library.sort_ascending {
                            " \u{25B2}"
                        } else {
                            " \u{25BC}"
                        }
                    } else {
                        ""
                    };
                    let text = format!("{label}{arrow}");
                    let style = if is_active {
                        egui::RichText::new(text).small().strong()
                    } else {
                        egui::RichText::new(text).small().weak()
                    };
                    if ui.small_button(style).clicked() {
                        if library.sort_column == col {
                            library.sort_ascending = !library.sort_ascending;
                        } else {
                            library.sort_column = col;
                            library.sort_ascending = true;
                        }
                        library.sort_results(model);
                    }
                }
            });
        });

        // Scrollable results list
        let available = ui.available_height() * 0.6; // 60% for results, 40% for tree
        egui::ScrollArea::both()
            .id_salt("library_results")
            .max_height(available)
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for (i, &id) in library.results.iter().enumerate() {
                    let elem = match model.element(id) {
                        Some(e) => e,
                        None => continue,
                    };

                    let is_selected = i == library.selected_index;
                    let icon = elem.kind.browser_icon();
                    let name = elem.display_name();
                    let kind = elem.kind.keyword();

                    let text = format!("{icon} {name}  {kind}");
                    let style = if is_selected {
                        egui::RichText::new(&text).small().strong()
                    } else {
                        egui::RichText::new(&text).small()
                    };

                    let resp = ui.selectable_label(is_selected, style);

                    if resp.clicked() {
                        library.selected_index = i;
                        *selected = Some(id);
                        library.nav_request = Some(id);
                    }
                    if resp.double_clicked() {
                        library.nav_request = Some(id);
                    }

                    // Scroll the selected result into view
                    if is_selected {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                }
            });

        ui.separator();
    }

    // Model tree (always available, fills remaining space)
    ui.label(egui::RichText::new("Model Browser").strong());
    ui.separator();
    let mut view_context_request: Option<ElementId> = None;
    egui::ScrollArea::both()
        .id_salt("library_tree")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            browser::show_tree(ui, model, selected, &mut view_context_request);
        });

    // If the user double-clicked in the tree, treat it as a nav request
    if let Some(ctx_id) = view_context_request {
        library.nav_request = Some(ctx_id);
    }
}
