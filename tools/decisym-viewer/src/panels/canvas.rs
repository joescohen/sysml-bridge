use egui::{Pos2, Rect, Sense, Vec2};

use crate::model::{ElementId, ElementKind, Model};
use crate::panels::filter::FilterState;
use crate::render::connectors;
use crate::render::elements;
use crate::render::frame;
use crate::render::layout::{self, Layout};
use crate::render::overview;
use crate::render::ports;
use crate::render::theme;

/// Search highlight state passed from the library/search panel to the canvas.
/// Decouples canvas rendering from the specific search implementation.
#[derive(Default)]
pub struct SearchHighlight {
    /// Whether search is active (should dim non-matching elements).
    pub active: bool,
    /// Element IDs that match the current query.
    pub matches: Vec<ElementId>,
    /// The currently focused match (highlighted differently).
    pub current: Option<ElementId>,
}


impl SearchHighlight {
    /// Whether a given element matches the search.
    pub fn is_match(&self, id: ElementId) -> bool {
        self.matches.contains(&id)
    }

    /// Whether a given element is the current highlighted match.
    pub fn is_current(&self, id: ElementId) -> bool {
        self.current == Some(id)
    }
}

/// The current view mode of the canvas.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum ViewMode {
    /// Full-model overview (compound graph of top-level elements).
    #[default]
    Overview,
    /// Children overview of a specific element (shows its children as nodes).
    ChildrenOf(ElementId),
    /// Interconnection view of a specific context element (SysML connectors).
    Interconnection(ElementId),
}


/// Which part of the canvas is being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragTarget {
    Canvas,
    HScrollbar,
    VScrollbar,
}

/// State for the canvas panel (pan, zoom, layout cache).
pub struct CanvasState {
    pub offset: Vec2,
    pub zoom: f32,
    pub view_mode: ViewMode,
    /// Navigation history stack for Escape to go back.
    pub nav_stack: Vec<ViewMode>,
    /// Cached layout for the current view.
    current_layout: Layout,
    layout_dirty: bool,
    /// Pending double-click navigation target.
    pub double_click_target: Option<ElementId>,
    /// Pending breadcrumb navigation target (depth index to pop to).
    pub breadcrumb_nav: Option<usize>,
    /// Element to center on after layout is computed (deferred centering).
    pub pending_center: Option<ElementId>,
    /// Tracks what the current drag is operating on.
    drag_target: Option<DragTarget>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            zoom: 1.0,
            view_mode: ViewMode::Overview,
            nav_stack: Vec::new(),
            current_layout: Layout::default(),
            layout_dirty: true,
            double_click_target: None,
            breadcrumb_nav: None,
            pending_center: None,
            drag_target: None,
        }
    }
}

impl CanvasState {
    /// Navigate into an element: push current view onto the stack,
    /// compute a new view, and zoom-to-fit.
    pub fn navigate_into(&mut self, id: ElementId, model: &Model) {
        let elem = match model.element(id) {
            Some(e) => e,
            None => return,
        };

        // Push current view onto navigation stack
        self.nav_stack.push(self.view_mode.clone());

        // Decide which view mode to use for this element
        let has_parts_with_connections = has_interconnection_content(model, id);

        if has_parts_with_connections {
            self.view_mode = ViewMode::Interconnection(id);
        } else if !elem.children.is_empty() {
            self.view_mode = ViewMode::ChildrenOf(id);
        } else {
            // Leaf element — pop back, nothing to navigate into
            self.nav_stack.pop();
            return;
        }

        self.layout_dirty = true;
        self.offset = Vec2::ZERO;
        self.zoom = 1.0;
    }

    /// Go back in navigation history.
    pub fn navigate_back(&mut self) {
        if let Some(prev) = self.nav_stack.pop() {
            self.view_mode = prev;
            self.layout_dirty = true;
            self.offset = Vec2::ZERO;
            self.zoom = 1.0;
        }
    }

    /// Switch to overview mode (root level), clearing the stack.
    pub fn set_overview(&mut self) {
        self.nav_stack.clear();
        self.view_mode = ViewMode::Overview;
        self.layout_dirty = true;
        self.offset = Vec2::ZERO;
        self.zoom = 1.0;
    }

    /// Set the view to interconnection mode for a specific context.
    #[allow(dead_code)]
    pub fn set_context(&mut self, context: ElementId) {
        self.nav_stack.clear();
        self.view_mode = ViewMode::Interconnection(context);
        self.layout_dirty = true;
    }

    /// Force layout recomputation.
    #[allow(dead_code)]
    pub fn invalidate_layout(&mut self) {
        self.layout_dirty = true;
    }

    /// Recompute layout if needed.
    pub fn ensure_layout(&mut self, model: &Model, egui_ctx: &egui::Context) {
        if !self.layout_dirty {
            return;
        }
        self.current_layout = match &self.view_mode {
            ViewMode::Overview => overview::compute_overview_layout(model, egui_ctx),
            ViewMode::ChildrenOf(id) => {
                overview::compute_children_layout(model, Some(*id), egui_ctx)
            }
            ViewMode::Interconnection(id) => layout::compute_layout(model, *id, egui_ctx),
        };
        self.layout_dirty = false;
    }

    /// Build the breadcrumb path: list of (label, depth_index) for navigation.
    /// depth_index 0 = Overview (root of stack).
    pub fn breadcrumb_path(&self, model: &Model) -> Vec<(String, usize)> {
        let mut path = Vec::new();
        path.push(("Overview".to_string(), 0));

        for (i, mode) in self.nav_stack.iter().enumerate() {
            match mode {
                ViewMode::Overview => {} // already added
                ViewMode::ChildrenOf(id) | ViewMode::Interconnection(id) => {
                    let label = model
                        .element(*id)
                        .map(|e| e.display_name().to_string())
                        .unwrap_or_else(|| "?".to_string());
                    path.push((label, i + 1));
                }
            }
        }

        // Current view mode (not on stack)
        match &self.view_mode {
            ViewMode::Overview => {} // already at root
            ViewMode::ChildrenOf(id) | ViewMode::Interconnection(id) => {
                let label = model
                    .element(*id)
                    .map(|e| e.display_name().to_string())
                    .unwrap_or_else(|| "?".to_string());
                path.push((label, self.nav_stack.len() + 1));
            }
        }

        path
    }

    /// Navigate to a specific depth in the breadcrumb path.
    /// depth 0 = Overview, depth 1 = first item in nav_stack, etc.
    pub fn navigate_to_depth(&mut self, depth: usize) {
        if depth == 0 {
            self.set_overview();
            return;
        }

        // The breadcrumb at depth N corresponds to nav_stack[N-1].
        // To navigate there, we restore that view and truncate the stack.
        let stack_index = depth - 1;
        if stack_index < self.nav_stack.len() {
            let target = self.nav_stack[stack_index].clone();
            self.nav_stack.truncate(stack_index);
            self.view_mode = target;
            self.layout_dirty = true;
            self.offset = Vec2::ZERO;
            self.zoom = 1.0;
        }
        // If depth == nav_stack.len() + 1, that's the current view — no-op
    }

    /// Auto zoom-to-fit: set zoom and offset so the content fills the canvas.
    pub fn zoom_to_fit(&mut self, canvas_rect: Rect) {
        let bounds = match self.current_layout.content_bounds() {
            Some(b) => b,
            None => return,
        };

        let content_w = bounds.width() + theme::FRAME_PADDING * 2.0;
        let content_h = bounds.height() + theme::FRAME_PADDING * 2.0 + theme::FRAME_TAB_HEIGHT;

        if content_w < 1.0 || content_h < 1.0 {
            return;
        }

        let margin = 40.0; // px margin around content
        let avail_w = canvas_rect.width() - margin * 2.0;
        let avail_h = canvas_rect.height() - margin * 2.0;

        let zoom = (avail_w / content_w)
            .min(avail_h / content_h)
            .clamp(0.2, 3.0);
        self.zoom = zoom;

        // Center the content
        let content_center_x = (bounds.min.x + bounds.max.x) / 2.0;
        let content_center_y = (bounds.min.y + bounds.max.y) / 2.0;
        let canvas_center_x = canvas_rect.width() / 2.0;
        let canvas_center_y = canvas_rect.height() / 2.0;

        self.offset = Vec2::new(
            canvas_center_x - content_center_x * zoom,
            canvas_center_y - content_center_y * zoom,
        );
    }

    /// Center the canvas on a specific element, zooming so it fills ~1/3 of the view.
    pub fn center_on_element(&mut self, id: ElementId, canvas_rect: Rect) {
        let pos = match self.current_layout.positions.get(&id) {
            Some(p) => *p,
            None => return,
        };
        let size = self
            .current_layout
            .sizes
            .get(&id)
            .copied()
            .unwrap_or(Vec2::new(160.0, 60.0));

        // Zoom so the element fills ~1/3 of the canvas
        let target_fraction = 3.0;
        let zoom_x = canvas_rect.width() / (size.x * target_fraction);
        let zoom_y = canvas_rect.height() / (size.y * target_fraction);
        let zoom = zoom_x.min(zoom_y).clamp(0.3, 3.0);
        self.zoom = zoom;

        // Center the element in the canvas
        let elem_center_x = pos.x + size.x / 2.0;
        let elem_center_y = pos.y + size.y / 2.0;
        let canvas_center_x = canvas_rect.width() / 2.0;
        let canvas_center_y = canvas_rect.height() / 2.0;

        self.offset = Vec2::new(
            canvas_center_x - elem_center_x * zoom,
            canvas_center_y - elem_center_y * zoom,
        );
    }

    /// Navigate the canvas to show a specific element.
    ///
    /// If the element is in the current layout, sets `pending_center`.
    /// Otherwise, finds its parent, pushes to nav stack, switches view,
    /// and sets `pending_center` for deferred centering.
    pub fn navigate_to_element(&mut self, id: ElementId, model: &Model) {
        // Check if element is visible in current layout
        if self.current_layout.positions.contains_key(&id) {
            self.pending_center = Some(id);
            return;
        }

        // Find the element's parent and navigate there
        let elem = match model.element(id) {
            Some(e) => e,
            None => return,
        };

        if let Some(parent_id) = elem.parent {
            // Push current view and navigate to parent
            self.nav_stack.push(self.view_mode.clone());

            let has_parts = has_interconnection_content(model, parent_id);
            if has_parts {
                self.view_mode = ViewMode::Interconnection(parent_id);
            } else {
                self.view_mode = ViewMode::ChildrenOf(parent_id);
            }
            self.layout_dirty = true;
            self.offset = Vec2::ZERO;
            self.zoom = 1.0;
            self.pending_center = Some(id);
        } else {
            // Root element — go to overview and center on it
            if !matches!(self.view_mode, ViewMode::Overview) {
                self.nav_stack.push(self.view_mode.clone());
                self.view_mode = ViewMode::Overview;
                self.layout_dirty = true;
                self.offset = Vec2::ZERO;
                self.zoom = 1.0;
            }
            self.pending_center = Some(id);
        }
    }

    /// Whether the given element is in the current layout.
    pub fn has_element_in_layout(&self, id: ElementId) -> bool {
        self.current_layout.positions.contains_key(&id)
    }
}

/// Check if an element has part children with connections (suitable for interconnection view).
fn has_interconnection_content(model: &Model, id: ElementId) -> bool {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return false,
    };

    let has_parts = elem.children.iter().any(|&cid| {
        model.element(cid).is_some_and(|e| {
            matches!(
                e.kind,
                ElementKind::PartUsage
                    | ElementKind::PartDef
                    | ElementKind::InterfaceUsage
                    | ElementKind::OccurrenceUsage
            )
        })
    });

    let has_rels = model.relationships.iter().any(|r| r.owner == id);

    has_parts && has_rels
}

/// Show the diagram canvas as a full-window panel.
pub fn show(
    ui: &mut egui::Ui,
    model: &Model,
    state: &mut CanvasState,
    selected: &mut Option<ElementId>,
    search: &SearchHighlight,
    filter: &FilterState,
) {
    state.ensure_layout(model, ui.ctx());

    let available = ui.available_size();
    let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
    let canvas_rect = response.rect;

    // Fill background
    painter.rect_filled(canvas_rect, 0.0, theme::COLOR_CANVAS_BG);

    // Scrollbar geometry (computed before drag handling)
    let sb = ScrollbarGeometry::compute(state, canvas_rect);

    // Determine drag target on drag start
    if response.dragged_by(egui::PointerButton::Primary) && state.drag_target.is_none()
        && let Some(pos) = response.interact_pointer_pos() {
            state.drag_target = Some(if sb.v_track.contains(pos) {
                DragTarget::VScrollbar
            } else if sb.h_track.contains(pos) {
                DragTarget::HScrollbar
            } else {
                DragTarget::Canvas
            });
        }

    // Handle drag based on target
    if response.dragged_by(egui::PointerButton::Primary) && !response.clicked() {
        let delta = response.drag_delta();
        match state.drag_target {
            Some(DragTarget::HScrollbar) => {
                if sb.h_total_range > 0.0 {
                    let track_w = sb.h_track.width();
                    state.offset.x -= delta.x / track_w * sb.h_total_range * state.zoom;
                }
            }
            Some(DragTarget::VScrollbar) => {
                if sb.v_total_range > 0.0 {
                    let track_h = sb.v_track.height();
                    state.offset.y -= delta.y / track_h * sb.v_total_range * state.zoom;
                }
            }
            _ => {
                state.offset += delta;
            }
        }
    }

    // Clear drag target when released
    if !response.dragged_by(egui::PointerButton::Primary) {
        state.drag_target = None;
    }

    // Handle zoom (scroll)
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 {
        let factor = 1.0 + scroll * 0.002;
        state.zoom = (state.zoom * factor).clamp(0.2, 5.0);
    }

    // Draw content based on view mode
    match &state.view_mode {
        ViewMode::Overview | ViewMode::ChildrenOf(_) => {
            draw_overview_view(
                ui,
                &painter,
                model,
                state,
                selected,
                search,
                filter,
                canvas_rect,
            );
        }
        ViewMode::Interconnection(ctx_id) => {
            let ctx_id = *ctx_id;
            draw_interconnection(
                ui,
                &painter,
                model,
                state,
                selected,
                search,
                filter,
                canvas_rect,
                ctx_id,
            );
        }
    }

    // Draw minimap (current view with viewport rectangle)
    draw_minimap(&painter, state, canvas_rect);

    // Draw scrollbars on top of everything
    draw_scrollbars(&painter, &sb, state.drag_target, response.hover_pos());

    // Handle click and double-click (ignore if in scrollbar area)
    let in_scrollbar = response
        .interact_pointer_pos()
        .is_some_and(|p| sb.h_track.contains(p) || sb.v_track.contains(p));
    if !in_scrollbar {
        if response.double_clicked() {
            if let Some(click_pos) = response.interact_pointer_pos()
                && let Some(hit_id) = find_hit(model, state, click_pos, canvas_rect) {
                    state.double_click_target = Some(hit_id);
                }
        } else if response.clicked()
            && let Some(click_pos) = response.interact_pointer_pos() {
                *selected = find_hit(model, state, click_pos, canvas_rect);
            }
    }

    // Show hover tooltip (not in scrollbar area)
    if let Some(hover_pos) = response.hover_pos()
        && !sb.h_track.contains(hover_pos) && !sb.v_track.contains(hover_pos) {
            show_hover_tooltip(ui, model, state, hover_pos, canvas_rect);
        }
}

/// Draw overview or children-of view (packages/elements as nodes).
fn draw_overview_view(
    _ui: &mut egui::Ui,
    painter: &egui::Painter,
    model: &Model,
    state: &CanvasState,
    selected: &mut Option<ElementId>,
    search: &SearchHighlight,
    filter: &FilterState,
    canvas_rect: Rect,
) {
    let scaled = scale_layout(
        &state.current_layout,
        state.zoom,
        state.offset,
        canvas_rect.min,
    );
    let font_id = egui::FontId::proportional(13.0 * state.zoom);

    // Frame label
    let label = match &state.view_mode {
        ViewMode::ChildrenOf(id) => model
            .element(*id)
            .map(|e| format!("{} {}", e.kind.keyword(), e.display_name()))
            .unwrap_or_else(|| "Children".to_string()),
        _ => "Model Overview".to_string(),
    };
    frame::draw_frame(painter, &scaled, &label, &font_id, state.zoom);

    // Edges before nodes (Principle 6)
    connectors::draw_all_connectors(painter, model, &scaled, state.zoom);

    // Draw nodes
    for &id in scaled.positions.keys() {
        let opacity = compute_element_opacity(id, model, search, filter);
        elements::draw_overview_package(
            painter, model, id, &scaled, *selected, &font_id, state.zoom, opacity,
        );

        if search.active && search.is_match(id) {
            draw_search_highlight(painter, id, &scaled, search.is_current(id), state.zoom);
        }
    }
}

/// Draw the interconnection view (SysML connectors between parts).
fn draw_interconnection(
    _ui: &mut egui::Ui,
    painter: &egui::Painter,
    model: &Model,
    state: &CanvasState,
    selected: &mut Option<ElementId>,
    search: &SearchHighlight,
    filter: &FilterState,
    canvas_rect: Rect,
    ctx_id: ElementId,
) {
    let scaled = scale_layout(
        &state.current_layout,
        state.zoom,
        state.offset,
        canvas_rect.min,
    );
    let font_id = egui::FontId::proportional(13.0 * state.zoom);

    let label = model
        .element(ctx_id)
        .map(|e| format!("ibv [Interconnection] {}", e.display_name()))
        .unwrap_or_default();
    frame::draw_frame(painter, &scaled, &label, &font_id, state.zoom);

    // Connectors BEFORE elements (Principle 6)
    connectors::draw_all_connectors(painter, model, &scaled, state.zoom);

    // Elements
    if let Some(ctx_elem) = model.element(ctx_id) {
        for &child_id in &ctx_elem.children {
            if let Some(child) = model.element(child_id)
                && matches!(
                    child.kind,
                    ElementKind::PartUsage
                        | ElementKind::PartDef
                        | ElementKind::InterfaceUsage
                        | ElementKind::OccurrenceUsage
                ) {
                    let opacity = compute_element_opacity(child_id, model, search, filter);
                    if opacity == 255 {
                        elements::draw_element(
                            painter, model, child_id, &scaled, *selected, &font_id, state.zoom,
                        );
                    } else {
                        elements::draw_element_lod(
                            painter, model, child_id, &scaled, *selected, &font_id, state.zoom,
                            opacity,
                        );
                    }

                    if search.active && search.is_match(child_id) {
                        draw_search_highlight(
                            painter,
                            child_id,
                            &scaled,
                            search.is_current(child_id),
                            state.zoom,
                        );
                    }
                }
        }
    }

    // Ports on top
    ports::draw_all_ports(painter, model, &scaled, state.zoom);
}

/// Compute opacity for an element based on search and filter state.
fn compute_element_opacity(
    id: ElementId,
    model: &Model,
    search: &SearchHighlight,
    filter: &FilterState,
) -> u8 {
    let filter_opacity = filter.element_opacity(id, model);
    if filter_opacity < 255 {
        return filter_opacity;
    }
    if search.active && !search.matches.is_empty() && !search.is_match(id) {
        return theme::FILTER_DIM_ALPHA;
    }
    255
}

/// Draw a highlight ring around a search-matched element.
fn draw_search_highlight(
    painter: &egui::Painter,
    id: ElementId,
    layout: &Layout,
    is_current: bool,
    zoom: f32,
) {
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return,
    };
    let size = match layout.sizes.get(&id) {
        Some(s) => *s,
        None => return,
    };
    let rect = Rect::from_min_size(pos, size).expand(3.0 * zoom);
    let color = if is_current {
        theme::COLOR_SEARCH_CURRENT
    } else {
        theme::COLOR_SEARCH_HIGHLIGHT
    };
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same((4.0 * zoom) as u8),
        egui::Stroke::new(2.0 * zoom, color),
        egui::StrokeKind::Outside,
    );
}

/// Draw the minimap in the bottom-right corner.
///
/// Shows the current view's layout as small rectangles with a viewport
/// rectangle indicating the visible region.  Breadcrumbs handle hierarchy;
/// the minimap handles spatial orientation within the current view.
fn draw_minimap(painter: &egui::Painter, state: &CanvasState, canvas_rect: Rect) {
    let content_bounds = match state.current_layout.content_bounds() {
        Some(b) => b,
        None => return,
    };

    let minimap_size = theme::MINIMAP_SIZE;
    let margin = theme::MINIMAP_MARGIN;

    let minimap_rect = Rect::from_min_size(
        Pos2::new(
            canvas_rect.max.x - minimap_size - margin,
            canvas_rect.max.y - minimap_size - margin,
        ),
        Vec2::new(minimap_size, minimap_size),
    );

    let bg = egui::Color32::from_rgba_unmultiplied(
        theme::COLOR_MINIMAP_BG.r(),
        theme::COLOR_MINIMAP_BG.g(),
        theme::COLOR_MINIMAP_BG.b(),
        theme::MINIMAP_BG_ALPHA,
    );
    painter.rect_filled(minimap_rect, egui::CornerRadius::same(4), bg);
    painter.rect_stroke(
        minimap_rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, theme::COLOR_ELEMENT_STROKE),
        egui::StrokeKind::Outside,
    );

    let content_w = content_bounds.width().max(1.0);
    let content_h = content_bounds.height().max(1.0);
    let padding = 4.0;
    let scale_x = (minimap_size - padding * 2.0) / content_w;
    let scale_y = (minimap_size - padding * 2.0) / content_h;
    let scale = scale_x.min(scale_y);

    let minimap_origin = Pos2::new(minimap_rect.min.x + padding, minimap_rect.min.y + padding);

    let clip = painter.clip_rect().intersect(minimap_rect);
    let mini_painter = egui::Painter::new(painter.ctx().clone(), painter.layer_id(), clip);

    // Draw edges
    for route in &state.current_layout.connector_routes {
        if route.points.len() >= 2 {
            for pair in route.points.windows(2) {
                let p0 = Pos2::new(
                    minimap_origin.x + (pair[0].x - content_bounds.min.x) * scale,
                    minimap_origin.y + (pair[0].y - content_bounds.min.y) * scale,
                );
                let p1 = Pos2::new(
                    minimap_origin.x + (pair[1].x - content_bounds.min.x) * scale,
                    minimap_origin.y + (pair[1].y - content_bounds.min.y) * scale,
                );
                mini_painter
                    .line_segment([p0, p1], egui::Stroke::new(0.5, theme::COLOR_TYPE_REF_EDGE));
            }
        }
    }

    // Draw elements as small filled rectangles
    for (&id, &pos) in &state.current_layout.positions {
        let size = state
            .current_layout
            .sizes
            .get(&id)
            .copied()
            .unwrap_or(Vec2::new(20.0, 10.0));
        let r = Rect::from_min_size(
            Pos2::new(
                minimap_origin.x + (pos.x - content_bounds.min.x) * scale,
                minimap_origin.y + (pos.y - content_bounds.min.y) * scale,
            ),
            Vec2::new((size.x * scale).max(3.0), (size.y * scale).max(2.0)),
        );
        mini_painter.rect_filled(r, egui::CornerRadius::ZERO, theme::COLOR_DEF_DOT);
        mini_painter.rect_stroke(
            r,
            egui::CornerRadius::ZERO,
            egui::Stroke::new(0.5, theme::COLOR_ELEMENT_STROKE),
            egui::StrokeKind::Outside,
        );
    }

    // Viewport rectangle
    let viewport_min_x = -state.offset.x / state.zoom;
    let viewport_min_y = -state.offset.y / state.zoom;
    let viewport_w = canvas_rect.width() / state.zoom;
    let viewport_h = canvas_rect.height() / state.zoom;

    let vr = Rect::from_min_size(
        Pos2::new(
            minimap_origin.x + (viewport_min_x - content_bounds.min.x) * scale,
            minimap_origin.y + (viewport_min_y - content_bounds.min.y) * scale,
        ),
        Vec2::new(viewport_w * scale, viewport_h * scale),
    )
    .intersect(minimap_rect);

    if vr.is_positive() {
        mini_painter.rect_stroke(
            vr,
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.5, theme::COLOR_MINIMAP_VIEWPORT),
            egui::StrokeKind::Outside,
        );
    }
}

/// Find which element is under a click position.
fn find_hit(
    model: &Model,
    state: &CanvasState,
    click_pos: Pos2,
    canvas_rect: Rect,
) -> Option<ElementId> {
    let scaled = scale_layout(
        &state.current_layout,
        state.zoom,
        state.offset,
        canvas_rect.min,
    );

    match &state.view_mode {
        ViewMode::Overview | ViewMode::ChildrenOf(_) => {
            for &id in scaled.positions.keys() {
                if elements::hit_test(&scaled, id, click_pos) {
                    return Some(id);
                }
            }
        }
        ViewMode::Interconnection(ctx_id) => {
            if let Some(ctx_elem) = model.element(*ctx_id) {
                for &child_id in &ctx_elem.children {
                    if elements::hit_test(&scaled, child_id, click_pos) {
                        return Some(child_id);
                    }
                }
            }
        }
    }
    None
}

/// Show hover tooltip near the hovered element.
fn show_hover_tooltip(
    ui: &mut egui::Ui,
    model: &Model,
    state: &CanvasState,
    hover_pos: Pos2,
    canvas_rect: Rect,
) {
    let scaled = scale_layout(
        &state.current_layout,
        state.zoom,
        state.offset,
        canvas_rect.min,
    );

    let hovered = scaled
        .positions
        .keys()
        .find(|&&id| elements::hit_test(&scaled, id, hover_pos));

    if let Some(&id) = hovered
        && let Some(elem) = model.element(id) {
            let has_children = !elem.children.is_empty();
            #[allow(deprecated)]
            egui::show_tooltip_at(
                ui.ctx(),
                ui.layer_id(),
                egui::Id::new("element_tooltip"),
                hover_pos + Vec2::new(15.0, 15.0),
                |ui| {
                    ui.strong(elem.display_name());
                    ui.label(format!("Kind: {}", elem.kind.keyword()));
                    if let Some(ref t) = elem.type_ref {
                        ui.label(format!("Type: {t}"));
                    }
                    let child_count = elem.children.len();
                    if child_count > 0 {
                        ui.label(format!("Children: {child_count}"));
                    }
                    if !elem.specializes.is_empty() {
                        ui.label(format!("Specializes: {}", elem.specializes.join(", ")));
                    }
                    let qname = elem.qualified_name(model);
                    if qname != elem.display_name() {
                        ui.label(egui::RichText::new(qname).weak().small());
                    }
                    if has_children {
                        ui.label(
                            egui::RichText::new("Double-click to explore")
                                .weak()
                                .italics(),
                        );
                    }
                },
            );
        }
}

/// Pre-computed scrollbar geometry for one frame.
struct ScrollbarGeometry {
    /// Horizontal scrollbar track (bottom edge of canvas).
    h_track: Rect,
    /// Horizontal scrollbar thumb.
    h_thumb: Rect,
    /// Vertical scrollbar track (right edge of canvas).
    v_track: Rect,
    /// Vertical scrollbar thumb.
    v_thumb: Rect,
    /// Total layout range in layout coords (horizontal).
    h_total_range: f32,
    /// Total layout range in layout coords (vertical).
    v_total_range: f32,
}

impl ScrollbarGeometry {
    fn compute(state: &CanvasState, canvas_rect: Rect) -> Self {
        let sw = theme::SCROLLBAR_WIDTH;
        let margin = theme::SCROLLBAR_CONTENT_MARGIN;
        let min_thumb = theme::SCROLLBAR_MIN_THUMB;

        // Track rectangles at canvas edges
        let h_track = Rect::from_min_max(
            Pos2::new(canvas_rect.min.x, canvas_rect.max.y - sw),
            Pos2::new(canvas_rect.max.x - sw, canvas_rect.max.y),
        );
        let v_track = Rect::from_min_max(
            Pos2::new(canvas_rect.max.x - sw, canvas_rect.min.y),
            Pos2::new(canvas_rect.max.x, canvas_rect.max.y - sw),
        );

        // Viewport in layout coords
        let vp_x_min = -state.offset.x / state.zoom;
        let vp_y_min = -state.offset.y / state.zoom;
        let vp_w = canvas_rect.width() / state.zoom;
        let vp_h = canvas_rect.height() / state.zoom;
        let vp_x_max = vp_x_min + vp_w;
        let vp_y_max = vp_y_min + vp_h;

        // Content bounds in layout coords (with margin)
        let (cx_min, cx_max, cy_min, cy_max) =
            if let Some(cb) = state.current_layout.content_bounds() {
                (
                    cb.min.x - margin,
                    cb.max.x + margin,
                    cb.min.y - margin,
                    cb.max.y + margin,
                )
            } else {
                (vp_x_min, vp_x_max, vp_y_min, vp_y_max)
            };

        // Total scrollable range = union of content and viewport
        let total_x_min = cx_min.min(vp_x_min);
        let total_x_max = cx_max.max(vp_x_max);
        let total_y_min = cy_min.min(vp_y_min);
        let total_y_max = cy_max.max(vp_y_max);
        let h_total_range = total_x_max - total_x_min;
        let v_total_range = total_y_max - total_y_min;

        // Horizontal thumb
        let h_thumb = if h_total_range > 0.01 {
            let frac_start = (vp_x_min - total_x_min) / h_total_range;
            let frac_end = (vp_x_max - total_x_min) / h_total_range;
            let track_w = h_track.width();
            let thumb_start = h_track.min.x + frac_start * track_w;
            let thumb_end = h_track.min.x + frac_end * track_w;
            let thumb_w = (thumb_end - thumb_start).max(min_thumb);
            Rect::from_min_size(
                Pos2::new(
                    thumb_start.clamp(h_track.min.x, h_track.max.x - thumb_w),
                    h_track.min.y,
                ),
                Vec2::new(thumb_w.min(track_w), sw),
            )
        } else {
            h_track
        };

        // Vertical thumb
        let v_thumb = if v_total_range > 0.01 {
            let frac_start = (vp_y_min - total_y_min) / v_total_range;
            let frac_end = (vp_y_max - total_y_min) / v_total_range;
            let track_h = v_track.height();
            let thumb_start = v_track.min.y + frac_start * track_h;
            let thumb_end = v_track.min.y + frac_end * track_h;
            let thumb_h = (thumb_end - thumb_start).max(min_thumb);
            Rect::from_min_size(
                Pos2::new(
                    v_track.min.x,
                    thumb_start.clamp(v_track.min.y, v_track.max.y - thumb_h),
                ),
                Vec2::new(sw, thumb_h.min(track_h)),
            )
        } else {
            v_track
        };

        Self {
            h_track,
            h_thumb,
            v_track,
            v_thumb,
            h_total_range,
            v_total_range,
        }
    }
}

/// Draw scrollbar tracks and thumbs over the canvas.
fn draw_scrollbars(
    painter: &egui::Painter,
    sb: &ScrollbarGeometry,
    drag_target: Option<DragTarget>,
    hover_pos: Option<Pos2>,
) {
    // Horizontal track + thumb
    painter.rect_filled(sb.h_track, 0.0, theme::COLOR_SCROLLBAR_TRACK);
    let h_active = drag_target == Some(DragTarget::HScrollbar)
        || hover_pos.is_some_and(|p| sb.h_thumb.contains(p));
    let h_color = if h_active {
        theme::COLOR_SCROLLBAR_THUMB_ACTIVE
    } else {
        theme::COLOR_SCROLLBAR_THUMB
    };
    painter.rect_filled(sb.h_thumb, 3.0, h_color);

    // Vertical track + thumb
    painter.rect_filled(sb.v_track, 0.0, theme::COLOR_SCROLLBAR_TRACK);
    let v_active = drag_target == Some(DragTarget::VScrollbar)
        || hover_pos.is_some_and(|p| sb.v_thumb.contains(p));
    let v_color = if v_active {
        theme::COLOR_SCROLLBAR_THUMB_ACTIVE
    } else {
        theme::COLOR_SCROLLBAR_THUMB
    };
    painter.rect_filled(sb.v_thumb, 3.0, v_color);

    // Corner square where scrollbars meet
    let corner = Rect::from_min_max(
        Pos2::new(sb.v_track.min.x, sb.h_track.min.y),
        Pos2::new(sb.v_track.max.x, sb.h_track.max.y),
    );
    painter.rect_filled(corner, 0.0, theme::COLOR_SCROLLBAR_TRACK);
}

/// Create a scaled copy of the layout for rendering.
pub fn scale_layout(layout: &Layout, zoom: f32, offset: Vec2, origin: Pos2) -> Layout {
    let transform = |p: Pos2| -> Pos2 {
        Pos2::new(
            origin.x + p.x * zoom + offset.x,
            origin.y + p.y * zoom + offset.y,
        )
    };

    let positions = layout
        .positions
        .iter()
        .map(|(&id, &pos)| (id, transform(pos)))
        .collect();

    let sizes = layout
        .sizes
        .iter()
        .map(|(&id, &size)| (id, size * zoom))
        .collect();

    let port_positions = layout
        .port_positions
        .iter()
        .map(|(&id, &pos)| (id, transform(pos)))
        .collect();

    let connector_routes = layout
        .connector_routes
        .iter()
        .map(|route| layout::ConnectorRoute {
            points: route.points.iter().map(|&p| transform(p)).collect(),
            rel_index: route.rel_index,
        })
        .collect();

    Layout {
        positions,
        sizes,
        port_positions,
        connector_routes,
        ..Default::default()
    }
}
