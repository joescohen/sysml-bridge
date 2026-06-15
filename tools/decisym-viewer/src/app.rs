use crate::model::{ElementId, Model};
use crate::panels::{canvas, filter, library};
use crate::render::theme;

/// Top-level application state.
pub struct SysmlApp {
    model: Model,
    selected: Option<ElementId>,
    canvas_state: canvas::CanvasState,
    library: library::ElementLibrary,
    filter_state: filter::FilterState,
    /// Whether to show the annotation card overlay.
    annotation_visible: bool,
    /// Whether zoom-to-fit is needed after a navigation.
    needs_zoom_to_fit: bool,
}

impl SysmlApp {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            selected: None,
            canvas_state: canvas::CanvasState::default(),
            library: library::ElementLibrary::default(),
            filter_state: filter::FilterState::default(),
            annotation_visible: false,
            needs_zoom_to_fit: true, // fit on first frame
        }
    }

    /// Handle keyboard shortcuts.
    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Check if any text edit has focus (library query field).
        // When a text edit is focused, don't intercept single-key shortcuts
        // like `/`, `N`, or `Enter` — only handle Escape and arrow navigation.
        let text_edit_focused = ctx.memory(|m| m.focused()).is_some() && self.library.visible;

        ctx.input(|i| {
            // `/` or Ctrl-F: open library panel and focus query
            if i.key_pressed(egui::Key::Slash) && !text_edit_focused
                || (i.modifiers.ctrl && i.key_pressed(egui::Key::F))
            {
                self.library.open_and_focus();
            }

            // Ctrl-B: toggle library panel
            if i.modifiers.ctrl && i.key_pressed(egui::Key::B) {
                self.library.toggle();
            }

            // Escape: layered dismissal
            if i.key_pressed(egui::Key::Escape) {
                if self.library.visible && !self.library.query_text.is_empty() {
                    // First: clear query
                    self.library.clear_query();
                } else if self.library.visible {
                    // Second: close panel
                    self.library.visible = false;
                } else if self.filter_state.is_active() {
                    self.filter_state.clear();
                } else if self.annotation_visible {
                    self.annotation_visible = false;
                } else if !self.canvas_state.nav_stack.is_empty() {
                    self.canvas_state.navigate_back();
                    self.needs_zoom_to_fit = true;
                } else {
                    // Already at overview — reset view
                    self.canvas_state.set_overview();
                    self.needs_zoom_to_fit = true;
                }
            }

            // Arrow keys in library: cycle results
            if self.library.visible && !self.library.query_text.is_empty() {
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.library.next_result();
                    if let Some(id) = self.library.current_result() {
                        self.selected = Some(id);
                    }
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    self.library.prev_result();
                    if let Some(id) = self.library.current_result() {
                        self.selected = Some(id);
                    }
                }
                if i.key_pressed(egui::Key::Enter) {
                    // Navigate to current result
                    if let Some(id) = self.library.current_result() {
                        self.selected = Some(id);
                        self.canvas_state.navigate_to_element(id, &self.model);
                        self.needs_zoom_to_fit = false; // center_on_element handles zoom
                    }
                }
            }

            // N: neighborhood filter (1-hop)
            if i.key_pressed(egui::Key::N) && !text_edit_focused {
                if i.modifiers.shift {
                    self.filter_state.neighborhood_hops = Some(2);
                } else {
                    self.filter_state.neighborhood_hops = Some(1);
                }
                self.filter_state
                    .update_neighborhood(self.selected, &self.model);
            }

            // Enter: navigate into selected element (when not in library query)
            if i.key_pressed(egui::Key::Enter) && !text_edit_focused
                && let Some(sel) = self.selected {
                    self.canvas_state.navigate_into(sel, &self.model);
                    self.needs_zoom_to_fit = true;
                }
        });
    }
}

impl eframe::App for SysmlApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Light theme
        ctx.set_visuals(egui::Visuals::light());

        // Initialize filter state
        self.filter_state.initialize_from_model(&self.model);

        // Handle keyboard shortcuts
        self.handle_keys(ctx);

        // Process double-click navigation from canvas
        if let Some(target) = self.canvas_state.double_click_target.take() {
            self.canvas_state.navigate_into(target, &self.model);
            self.needs_zoom_to_fit = true;
        }

        // Element Library floating window
        if self.library.visible {
            library::show_window(ctx, &self.model, &mut self.library, &mut self.selected);
        }

        // Build search highlight from library state
        let search_highlight = self.library.search_highlight();

        // Status bar at bottom (must be shown BEFORE CentralPanel so egui
        // reserves its space and the canvas doesn't overlap the scrollbar area)
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .fill(egui::Color32::from_rgb(240, 240, 245)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // View mode indicator
                    let mode_text = match &self.canvas_state.view_mode {
                        canvas::ViewMode::Overview => "Overview".to_string(),
                        canvas::ViewMode::ChildrenOf(id) => {
                            let name = self
                                .model
                                .element(*id)
                                .map(|e| e.display_name().to_string())
                                .unwrap_or_default();
                            format!("Children: {name}")
                        }
                        canvas::ViewMode::Interconnection(id) => {
                            let name = self
                                .model
                                .element(*id)
                                .map(|e| e.display_name().to_string())
                                .unwrap_or_default();
                            format!("Interconnection: {name}")
                        }
                    };
                    ui.label(egui::RichText::new(mode_text).small());

                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "Zoom: {:.0}%",
                            self.canvas_state.zoom * 100.0
                        ))
                        .small(),
                    );

                    if let Some(sel) = self.selected {
                        ui.separator();
                        let name = self
                            .model
                            .element(sel)
                            .map(|e| e.display_name().to_string())
                            .unwrap_or_default();
                        ui.label(egui::RichText::new(format!("Selected: {name}")).small());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(
                                "/ search  Ctrl-B library  Enter dive  Esc back  N neighbors",
                            )
                            .small()
                            .weak(),
                        );
                    });
                });
            });

        // Full-window central panel — the map IS the interface
        let panel_response =
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    // Canvas takes the entire window
                    canvas::show(
                        ui,
                        &self.model,
                        &mut self.canvas_state,
                        &mut self.selected,
                        &search_highlight,
                        &self.filter_state,
                    );
                    ui.min_rect()
                });

        let canvas_rect = panel_response.inner;

        // Process navigation request from library
        if let Some(target) = self.library.nav_request.take() {
            self.canvas_state.navigate_to_element(target, &self.model);
            // Ensure layout is computed before centering
            self.canvas_state.ensure_layout(&self.model, ctx);
        }

        // Process pending_center (deferred centering after layout)
        if let Some(target) = self.canvas_state.pending_center.take() {
            self.canvas_state.center_on_element(target, canvas_rect);
            // pending_center takes priority — cancel zoom_to_fit
            self.needs_zoom_to_fit = false;
        }

        // Zoom-to-fit after layout is computed and canvas rect is known
        if self.needs_zoom_to_fit {
            self.canvas_state.zoom_to_fit(canvas_rect);
            self.needs_zoom_to_fit = false;
        }

        // Process breadcrumb navigation
        if let Some(depth) = self.canvas_state.breadcrumb_nav.take() {
            self.canvas_state.navigate_to_depth(depth);
            self.needs_zoom_to_fit = true;
        }

        // Breadcrumb + context overlay (top-left)
        let breadcrumbs = self.canvas_state.breadcrumb_path(&self.model);
        if breadcrumbs.len() > 1 {
            let ctx_id = match &self.canvas_state.view_mode {
                canvas::ViewMode::ChildrenOf(id) | canvas::ViewMode::Interconnection(id) => {
                    Some(*id)
                }
                canvas::ViewMode::Overview => None,
            };

            egui::Area::new(egui::Id::new("breadcrumb_bar"))
                .fixed_pos(egui::pos2(8.0, 8.0))
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgba_unmultiplied(245, 245, 250, 230))
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .corner_radius(egui::CornerRadius::same(4))
                        .stroke(egui::Stroke::new(0.5, theme::COLOR_ELEMENT_STROKE))
                        .show(ui, |ui| {
                            // Line 1: breadcrumb navigation path
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let last_idx = breadcrumbs.len() - 1;
                                for (i, (label, depth)) in breadcrumbs.iter().enumerate() {
                                    if i == last_idx {
                                        ui.label(
                                            egui::RichText::new(label)
                                                .small()
                                                .strong()
                                                .color(theme::COLOR_BREADCRUMB_TEXT),
                                        );
                                    } else {
                                        let resp = ui.link(egui::RichText::new(label).small());
                                        if resp.clicked() {
                                            self.canvas_state.breadcrumb_nav = Some(*depth);
                                        }
                                        ui.label(egui::RichText::new(" > ").small().weak());
                                    }
                                }
                            });

                            // Line 2: relational context
                            if let Some(id) = ctx_id
                                && let Some(elem) = self.model.element(id) {
                                    let context_line = build_context_line(elem, id, &self.model);
                                    if !context_line.is_empty() {
                                        ui.label(egui::RichText::new(context_line).small().weak());
                                    }
                                }
                        });
                });
        }

        // Filter chips overlay (top-left, show for overview/children modes or when active)
        if self.filter_state.is_active()
            || matches!(
                self.canvas_state.view_mode,
                canvas::ViewMode::Overview | canvas::ViewMode::ChildrenOf(_)
            )
        {
            egui::Area::new(egui::Id::new("filter_chips_area"))
                .fixed_pos(egui::pos2(8.0, 44.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let chip_kinds = [
                            (crate::model::ElementKind::Package, "Pkg"),
                            (crate::model::ElementKind::PartDef, "Part Def"),
                            (crate::model::ElementKind::PartUsage, "Parts"),
                            (crate::model::ElementKind::PortDef, "Port Def"),
                            (crate::model::ElementKind::ActionDef, "Actions"),
                            (crate::model::ElementKind::RequirementDef, "Reqs"),
                            (crate::model::ElementKind::EnumDef, "Enums"),
                        ];
                        for (kind, label) in chip_kinds {
                            let active = self
                                .filter_state
                                .kind_visible
                                .get(&kind)
                                .copied()
                                .unwrap_or(true);
                            let style = if active {
                                egui::RichText::new(label).small().strong()
                            } else {
                                egui::RichText::new(label).small().weak()
                            };
                            if ui.selectable_label(active, style).clicked() {
                                let entry =
                                    self.filter_state.kind_visible.entry(kind).or_insert(true);
                                *entry = !*entry;
                            }
                        }
                        if self.filter_state.is_active()
                            && ui.small_button("Clear").clicked() {
                                self.filter_state.clear();
                            }
                    });
                });
        }

        // Annotation card overlay (near selected element)
        if self.annotation_visible
            && let Some(sel_id) = self.selected {
                self.show_annotation_card(ctx, sel_id);
            }
    }
}

impl SysmlApp {
    /// Show annotation card overlay near the selected element.
    fn show_annotation_card(&self, ctx: &egui::Context, id: ElementId) {
        let elem = match self.model.element(id) {
            Some(e) => e,
            None => return,
        };

        egui::Window::new("Element Details")
            .id(egui::Id::new("annotation_card"))
            .collapsible(false)
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                egui::Grid::new("annotation_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Kind:");
                        ui.label(elem.kind.keyword());
                        ui.end_row();

                        ui.label("Name:");
                        ui.label(elem.display_name());
                        ui.end_row();

                        if let Some(ref t) = elem.type_ref {
                            ui.label("Type:");
                            ui.label(t.as_str());
                            ui.end_row();
                        }

                        if !elem.specializes.is_empty() {
                            ui.label("Specializes:");
                            ui.label(elem.specializes.join(", "));
                            ui.end_row();
                        }

                        ui.label("Qualified:");
                        ui.label(elem.qualified_name(&self.model));
                        ui.end_row();

                        ui.label("Children:");
                        ui.label(format!("{}", elem.children.len()));
                        ui.end_row();

                        if let Some(ref doc) = elem.doc {
                            ui.label("Doc:");
                            ui.label(doc.as_str());
                            ui.end_row();
                        }
                    });
            });
    }
}

/// Build a compact context line showing how an element relates to the model.
fn build_context_line(
    elem: &crate::model::Element,
    id: crate::model::ElementId,
    model: &crate::model::Model,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Specialization
    if !elem.specializes.is_empty() {
        parts.push(format!(":> {}", elem.specializes.join(", ")));
    }

    // Type reference
    if let Some(ref t) = elem.type_ref {
        parts.push(format!(": {t}"));
    }

    // Siblings (other structural children of the same parent)
    if let Some(parent_id) = elem.parent
        && let Some(parent) = model.element(parent_id) {
            let siblings: Vec<&str> = parent
                .children
                .iter()
                .filter(|&&cid| cid != id)
                .filter_map(|&cid| model.element(cid))
                .filter(|e| e.kind.is_structural() && e.name.is_some())
                .map(|e| e.display_name())
                .collect();
            if !siblings.is_empty() {
                let max_show = 4;
                let mut names: Vec<&str> = siblings.iter().take(max_show).copied().collect();
                if siblings.len() > max_show {
                    names.push("...");
                }
                parts.push(format!("siblings: {}", names.join(", ")));
            }
        }

    parts.join("  |  ")
}
