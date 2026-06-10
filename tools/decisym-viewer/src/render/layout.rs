use std::collections::{HashMap, HashSet};

use egui::Pos2;

use crate::model::{ElementId, ElementKind, Metatype, Model};
use crate::render::theme;

/// Computed layout positions for elements and ports.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// Top-left position for each element.
    pub positions: HashMap<ElementId, Pos2>,
    /// Computed size for each element.
    pub sizes: HashMap<ElementId, egui::Vec2>,
    /// Port positions (center of the port square).
    pub port_positions: HashMap<ElementId, Pos2>,
    /// Connector routes: (source_pos, target_pos).
    pub connector_routes: Vec<ConnectorRoute>,
    /// Activity control pseudonodes (initial / final). Export-only; empty for
    /// other views.
    pub control_nodes: Vec<ControlNode>,
    /// Synthetic decoration edges (initial->source, sink->final) as polylines.
    /// Export-only; empty for other views.
    pub decoration_edges: Vec<Vec<Pos2>>,
    /// Typed edges with end markers + labels (composition, specialization,
    /// satisfy/derive/verify) for BDD / requirements / traceability.
    pub decorated_edges: Vec<DecoratedEdge>,
}

impl Layout {
    /// Compute a bounding rect encompassing all elements, ports, and connector waypoints.
    ///
    /// This is the single source of truth for frame sizing — the frame must
    /// encompass this rect to guarantee all content (including connector detours)
    /// is visible inside the frame border.
    pub fn content_bounds(&self) -> Option<egui::Rect> {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        // Element rects
        for (&id, &pos) in &self.positions {
            let size = self.sizes.get(&id).copied().unwrap_or(egui::vec2(
                theme::ELEMENT_MIN_WIDTH,
                theme::ELEMENT_MIN_HEIGHT,
            ));
            min_x = min_x.min(pos.x);
            min_y = min_y.min(pos.y);
            max_x = max_x.max(pos.x + size.x);
            max_y = max_y.max(pos.y + size.y);
        }

        // Port positions (with PORT_HALF extent)
        for &pos in self.port_positions.values() {
            min_x = min_x.min(pos.x - theme::PORT_HALF);
            min_y = min_y.min(pos.y - theme::PORT_HALF);
            max_x = max_x.max(pos.x + theme::PORT_HALF);
            max_y = max_y.max(pos.y + theme::PORT_HALF);
        }

        // Connector waypoints
        for route in &self.connector_routes {
            for pt in &route.points {
                min_x = min_x.min(pt.x);
                min_y = min_y.min(pt.y);
                max_x = max_x.max(pt.x);
                max_y = max_y.max(pt.y);
            }
        }

        // Activity control nodes (with radius) + decoration edge waypoints
        for cn in &self.control_nodes {
            min_x = min_x.min(cn.center.x - theme::CONTROL_NODE_RADIUS);
            min_y = min_y.min(cn.center.y - theme::CONTROL_NODE_RADIUS);
            max_x = max_x.max(cn.center.x + theme::CONTROL_NODE_RADIUS);
            max_y = max_y.max(cn.center.y + theme::CONTROL_NODE_RADIUS);
        }
        for edge in &self.decoration_edges {
            for pt in edge {
                min_x = min_x.min(pt.x);
                min_y = min_y.min(pt.y);
                max_x = max_x.max(pt.x);
                max_y = max_y.max(pt.y);
            }
        }
        for edge in &self.decorated_edges {
            for pt in &edge.points {
                min_x = min_x.min(pt.x);
                min_y = min_y.min(pt.y);
                max_x = max_x.max(pt.x);
                max_y = max_y.max(pt.y);
            }
        }

        if min_x == f32::INFINITY {
            None
        } else {
            Some(egui::Rect::from_min_max(
                egui::Pos2::new(min_x, min_y),
                egui::Pos2::new(max_x, max_y),
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectorRoute {
    pub points: Vec<Pos2>,
    pub rel_index: usize,
}

/// An activity control pseudonode (initial or final), used by the Action Flow
/// export to draw the standard SysML start/end markers.
#[derive(Debug, Clone, Copy)]
pub enum ControlNodeKind {
    Initial,
    Final,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlNode {
    pub center: Pos2,
    pub kind: ControlNodeKind,
}

/// End-decoration for a typed edge (BDD / requirements / traceability).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeMarker {
    None,
    OpenArrow,
    FilledDiamond,
    HollowTriangle,
}

/// A typed edge with end markers + optional mid label (composition,
/// specialization, satisfy/derive/verify). Drawn by the export's
/// render_decorated_edges.
#[derive(Debug, Clone)]
pub struct DecoratedEdge {
    pub points: Vec<Pos2>,
    pub start_marker: EdgeMarker,
    pub end_marker: EdgeMarker,
    pub dashed: bool,
    pub label: Option<String>,
}

/// Compute layout for an interconnection view of the given context element.
pub fn compute_layout(model: &Model, context: ElementId, egui_ctx: &egui::Context) -> Layout {
    let mut layout = Layout::default();

    let ctx = match model.element(context) {
        Some(e) => e,
        None => return layout,
    };

    // Collect direct part/component children (not ports, attributes, etc.)
    let part_children: Vec<ElementId> = ctx
        .children
        .iter()
        .copied()
        .filter(|&cid| {
            model.element(cid).is_some_and(|e| {
                matches!(
                    e.kind,
                    ElementKind::PartUsage
                        | ElementKind::PartDef
                        | ElementKind::InterfaceUsage
                        | ElementKind::OccurrenceUsage
                )
            })
        })
        .collect();

    if part_children.is_empty() {
        return layout;
    }

    // Collect relationships owned by or within the context
    let context_rels: Vec<usize> = model
        .relationships
        .iter()
        .enumerate()
        .filter(|(_, r)| r.owner == context)
        .map(|(i, _)| i)
        .collect();

    // Build adjacency for layering: which parts connect to which
    let mut name_to_id: HashMap<&str, ElementId> = HashMap::new();
    for &cid in &part_children {
        if let Some(e) = model.element(cid)
            && let Some(ref name) = e.name {
                name_to_id.insert(name.as_str(), cid);
            }
    }

    // Build a graph of connections between parts
    let mut successors: HashMap<ElementId, HashSet<ElementId>> = HashMap::new();
    let mut has_incoming: HashSet<ElementId> = HashSet::new();
    let child_set: HashSet<ElementId> = part_children.iter().copied().collect();

    for &ri in &context_rels {
        let rel = &model.relationships[ri];
        let src_part = first_segment(&rel.source_path);
        let tgt_part = first_segment(&rel.target_path);
        if let (Some(&src_id), Some(&tgt_id)) = (name_to_id.get(src_part), name_to_id.get(tgt_part))
            && src_id != tgt_id && child_set.contains(&src_id) && child_set.contains(&tgt_id) {
                successors.entry(src_id).or_default().insert(tgt_id);
                has_incoming.insert(tgt_id);
            }
    }

    // Assign layers using a simple topological approach
    let mut layers: Vec<Vec<ElementId>> = Vec::new();
    let mut assigned: HashSet<ElementId> = HashSet::new();

    // Layer 0: elements with no incoming edges
    let mut current_layer: Vec<ElementId> = part_children
        .iter()
        .copied()
        .filter(|id| !has_incoming.contains(id))
        .collect();

    if current_layer.is_empty() {
        // No clear topology — use grid layout
        return grid_layout(model, context, &part_children, egui_ctx);
    }

    while !current_layer.is_empty() {
        for &id in &current_layer {
            assigned.insert(id);
        }
        layers.push(current_layer.clone());

        let mut next_layer = Vec::new();
        for &id in &current_layer {
            if let Some(succs) = successors.get(&id) {
                for &s in succs {
                    if !assigned.contains(&s) && !next_layer.contains(&s) {
                        next_layer.push(s);
                    }
                }
            }
        }
        current_layer = next_layer;
    }

    // Add any unassigned elements to the last layer
    let unassigned: Vec<ElementId> = part_children
        .iter()
        .copied()
        .filter(|id| !assigned.contains(id))
        .collect();
    if !unassigned.is_empty() {
        layers.push(unassigned);
    }

    // Compute element sizes (accounting for inherited ports)
    for &cid in &part_children {
        let size = compute_element_size(model, cid, egui_ctx);
        layout.sizes.insert(cid, size);
    }

    // Position elements in layers — dedicated routing corridor between frame edge
    // and element field per Principle 2 (ROUTE_FRAME_MARGIN + ROUTE_COLUMN_GAP_MARGIN).
    let start_x = theme::FRAME_PADDING + theme::ROUTE_FRAME_MARGIN + theme::ROUTE_COLUMN_GAP_MARGIN;
    let start_y = theme::FRAME_PADDING + theme::FRAME_TAB_HEIGHT + theme::ROUTE_FRAME_MARGIN;

    let mut x = start_x;
    for layer in &layers {
        let mut y = start_y;
        let mut max_width: f32 = 0.0;

        for &cid in layer {
            let size = layout.sizes.get(&cid).copied().unwrap_or(egui::vec2(
                theme::ELEMENT_MIN_WIDTH,
                theme::ELEMENT_MIN_HEIGHT,
            ));
            layout.positions.insert(cid, Pos2::new(x, y));
            y += size.y + theme::ELEMENT_V_SPACING;
            max_width = max_width.max(size.x);
        }
        x += max_width + theme::ELEMENT_H_SPACING;
    }

    // Place ports along element boundaries (including inherited ports)
    place_ports(model, &part_children, &mut layout);

    // Also place context-level ports
    let context_ports = ctx.ports(model);
    let min_x = layout
        .positions
        .values()
        .map(|p| p.x)
        .fold(f32::INFINITY, f32::min);
    for (i, &pid) in context_ports.iter().enumerate() {
        let x = (min_x - theme::ROUTE_FRAME_MARGIN).max(theme::FRAME_PADDING);
        let y = theme::FRAME_PADDING
            + theme::FRAME_TAB_HEIGHT
            + theme::ROUTE_FRAME_MARGIN
            + theme::ROUTE_STUB_LENGTH
            + i as f32 * theme::ROUTE_FRAME_MARGIN;
        layout.port_positions.insert(pid, Pos2::new(x, y));
    }

    // Route connectors
    route_connectors(model, context, &context_rels, &mut layout);

    layout
}

/// Fallback grid layout when there's no connection topology.
fn grid_layout(
    model: &Model,
    context: ElementId,
    parts: &[ElementId],
    egui_ctx: &egui::Context,
) -> Layout {
    let mut layout = Layout::default();
    let cols = (parts.len() as f32).sqrt().ceil() as usize;

    let start_x = theme::FRAME_PADDING + theme::ROUTE_FRAME_MARGIN + theme::ROUTE_COLUMN_GAP_MARGIN;
    let start_y = theme::FRAME_PADDING + theme::FRAME_TAB_HEIGHT + theme::ROUTE_FRAME_MARGIN;

    for (i, &cid) in parts.iter().enumerate() {
        let size = compute_element_size(model, cid, egui_ctx);
        let col = i % cols;
        let row = i / cols;
        let x = start_x + col as f32 * (theme::ELEMENT_MIN_WIDTH + theme::ELEMENT_H_SPACING);
        let y = start_y + row as f32 * (size.y + theme::ELEMENT_V_SPACING);
        layout.positions.insert(cid, Pos2::new(x, y));
        layout.sizes.insert(cid, size);
    }

    place_ports(model, parts, &mut layout);

    // Route connectors for context relationships
    let context_rels: Vec<usize> = model
        .relationships
        .iter()
        .enumerate()
        .filter(|(_, r)| r.owner == context)
        .map(|(i, _)| i)
        .collect();
    route_connectors(model, context, &context_rels, &mut layout);

    layout
}

/// Measure exact pixel width of text using egui's font system.
fn measure_text_width(egui_ctx: &egui::Context, text: &str, font: &egui::FontId) -> f32 {
    egui_ctx.fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::BLACK)
            .rect
            .width()
    })
}

/// Compute the display size of an element based on its text content.
///
/// Uses egui's font system for deterministic pixel-accurate measurement.
/// Accounts for inherited ports from type definitions.
fn compute_element_size(model: &Model, id: ElementId, egui_ctx: &egui::Context) -> egui::Vec2 {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return egui::vec2(theme::ELEMENT_MIN_WIDTH, theme::ELEMENT_MIN_HEIGHT),
    };

    // Use effective ports (includes inherited from type definition)
    let ports = model.effective_ports(id);
    let features = elem.features(model);
    let visible_features: Vec<ElementId> = features
        .iter()
        .copied()
        .filter(|&fid| {
            model.element(fid).is_some_and(|e| {
                matches!(
                    e.kind,
                    ElementKind::AttributeUsage
                        | ElementKind::PartUsage
                        | ElementKind::ActionUsage
                        | ElementKind::StateUsage
                )
            })
        })
        .take(8)
        .collect();

    // Font IDs must match those used in draw_element (base zoom)
    let base_font = egui::FontId::proportional(theme::BASE_FONT_SIZE);
    let small_font = egui::FontId::proportional(theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE);
    let feature_font =
        egui::FontId::proportional(theme::BASE_FONT_SIZE * theme::FEATURE_FONT_SCALE);

    let mut max_width: f32 = 0.0;

    // Header keyword line: <<keyword>>
    let keyword_text = format!("<<{}>>", elem.kind.keyword());
    max_width = max_width.max(measure_text_width(egui_ctx, &keyword_text, &small_font));

    // Header name line: name: Type
    let name = elem.display_name();
    let type_str = elem
        .type_ref
        .as_deref()
        .map(|t| format!(": {t}"))
        .unwrap_or_default();
    let name_line = format!("{name}{type_str}");
    max_width = max_width.max(measure_text_width(egui_ctx, &name_line, &base_font));

    // Port lines: port name: ~Type
    for &pid in &ports {
        if let Some(p) = model.element(pid) {
            let conj = if p.is_conjugated { "~" } else { "" };
            let tref = p
                .type_ref
                .as_deref()
                .map(|t| format!(": {conj}{t}"))
                .unwrap_or_default();
            let label = format!("port {}{}", p.display_name(), tref);
            max_width = max_width.max(measure_text_width(egui_ctx, &label, &feature_font));
        }
    }

    // Feature lines: keyword name: Type
    for &fid in &visible_features {
        if let Some(f) = model.element(fid) {
            let tref = f
                .type_ref
                .as_deref()
                .map(|t| format!(": {t}"))
                .unwrap_or_default();
            let label = format!("{} {}{}", f.kind.keyword(), f.display_name(), tref);
            max_width = max_width.max(measure_text_width(egui_ctx, &label, &feature_font));
        }
    }

    // Add padding on both sides, clamp to [min, max]
    let width = (max_width + theme::ELEMENT_PADDING * 2.0)
        .clamp(theme::ELEMENT_MIN_WIDTH, theme::ELEMENT_MAX_WIDTH);

    let feature_count = visible_features.len() + ports.len();
    let height = theme::ELEMENT_HEADER_HEIGHT
        + if feature_count > 0 {
            theme::COMPARTMENT_LINE_WIDTH
                + feature_count as f32 * theme::ELEMENT_FEATURE_ROW_HEIGHT
                + theme::ELEMENT_PADDING
        } else {
            theme::ELEMENT_PADDING
        };

    egui::vec2(width, height.max(theme::ELEMENT_MIN_HEIGHT))
}

/// Place ports along element boundaries (left and right sides).
///
/// Uses `effective_ports` to include ports inherited from type definitions,
/// which is essential for interconnection views where usages reference
/// ports defined on their type.
fn place_ports(model: &Model, parts: &[ElementId], layout: &mut Layout) {
    for &cid in parts {
        let pos = match layout.positions.get(&cid) {
            Some(p) => *p,
            None => continue,
        };
        let size = layout.sizes.get(&cid).copied().unwrap_or(egui::vec2(
            theme::ELEMENT_MIN_WIDTH,
            theme::ELEMENT_MIN_HEIGHT,
        ));

        // Use effective ports (includes inherited from type definition)
        let ports = model.effective_ports(cid);
        if ports.is_empty() {
            continue;
        }

        let left_count = ports.len() / 2 + ports.len() % 2;
        let left_ports: Vec<ElementId> = ports.iter().copied().take(left_count).collect();
        let right_ports: Vec<ElementId> = ports.iter().copied().skip(left_count).collect();

        for (i, &pid) in left_ports.iter().enumerate() {
            let y = pos.y
                + theme::ELEMENT_HEADER_HEIGHT
                + (i as f32 + 0.5) * theme::ELEMENT_FEATURE_ROW_HEIGHT;
            layout.port_positions.insert(pid, Pos2::new(pos.x, y));
        }
        for (i, &pid) in right_ports.iter().enumerate() {
            let y = pos.y
                + theme::ELEMENT_HEADER_HEIGHT
                + (i as f32 + 0.5) * theme::ELEMENT_FEATURE_ROW_HEIGHT;
            layout
                .port_positions
                .insert(pid, Pos2::new(pos.x + size.x, y));
        }
    }
}

/// Route connectors between elements with obstacle-aware orthogonal routing.
///
/// Connectors exit ports going OUTWARD (left for left-side ports, right for
/// right-side ports) to avoid overlapping port labels. Routes use vertical
/// channels placed in column gaps between element groups. Horizontal segments
/// that would cross element interiors are detoured above or below.
fn route_connectors(model: &Model, context: ElementId, rel_indices: &[usize], layout: &mut Layout) {
    let name_index = model.name_index();
    let stub_dist = theme::ROUTE_STUB_LENGTH;

    let gaps = compute_column_gaps(layout);
    let elem_rects: Vec<egui::Rect> = layout
        .positions
        .iter()
        .filter_map(|(&id, &pos)| {
            layout
                .sizes
                .get(&id)
                .map(|&s| egui::Rect::from_min_size(pos, s))
        })
        .collect();

    // Minimum y for any route waypoint: must stay below the frame tab area.
    // Tab bottom (from elements) = element_min_y - FRAME_PADDING.
    // Routes must clear this by at least ROUTE_EDGE_TOLERANCE.
    let element_min_y = layout
        .positions
        .values()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);
    let tab_bottom_y = element_min_y - theme::FRAME_PADDING;
    let min_route_y = (element_min_y - theme::ROUTE_ELEMENT_CLEARANCE)
        .max(tab_bottom_y + theme::ROUTE_EDGE_TOLERANCE);

    let mut channel_count: HashMap<i32, usize> = HashMap::new();
    // Track placed horizontal segments for parallel deconfliction (Principle 5).
    // Each entry: (min_x, max_x, y) of a horizontal segment.
    let mut used_h_levels: Vec<(f32, f32, f32)> = Vec::new();

    for &ri in rel_indices {
        let rel = &model.relationships[ri];
        let src = resolve_endpoint(model, context, &rel.source_path, layout, &name_index);
        let tgt = resolve_endpoint(model, context, &rel.target_path, layout, &name_index);

        if let (Some(sp), Some(tp)) = (src, tgt) {
            if (sp.x - tp.x).abs() < 1.0 && (sp.y - tp.y).abs() < 1.0 {
                continue;
            }

            let src_dir = outward_direction(sp, layout);
            let tgt_dir = outward_direction(tp, layout);

            let sp_stub = Pos2::new(sp.x + src_dir * stub_dist, sp.y);
            let tp_stub = Pos2::new(tp.x + tgt_dir * stub_dist, tp.y);

            // Find the best column gap for the vertical channel
            let preferred_x = (sp_stub.x + tp_stub.x) / 2.0;
            let gap_x = nearest_gap(&gaps, preferred_x);

            let key = (gap_x / theme::ROUTE_STUB_LENGTH) as i32;
            let idx = *channel_count.entry(key).or_insert(0);
            channel_count.insert(key, idx + 1);
            // Alternate channels left/right from gap center (0, -1, +1, -2, +2, ...)
            // so they stay balanced within the gap instead of drifting into elements.
            let offset = if idx.is_multiple_of(2) {
                (idx / 2) as f32
            } else {
                -(idx.div_ceil(2) as f32)
            };
            let channel_x = gap_x + offset * theme::ROUTE_CHANNEL_SPREAD;

            let points = build_obstacle_aware_route(
                sp,
                sp_stub,
                tp,
                tp_stub,
                channel_x,
                &elem_rects,
                &used_h_levels,
                min_route_y,
            );

            // Record horizontal segments from this route for future deconfliction
            for w in points.windows(2) {
                if (w[0].y - w[1].y).abs() < 0.5 {
                    let min_x = w[0].x.min(w[1].x);
                    let max_x = w[0].x.max(w[1].x);
                    if (max_x - min_x) > 1.0 {
                        used_h_levels.push((min_x, max_x, w[0].y));
                    }
                }
            }

            layout.connector_routes.push(ConnectorRoute {
                points,
                rel_index: ri,
            });
        }
    }
}

/// Build an orthogonal route that avoids crossing element interiors.
///
/// Route structure: sp → sp_stub → [channel approach] → [channel] → [target approach] → tp_stub → tp
/// If a horizontal segment would cross an element, detour above or below it.
/// Detour y-values are deconflicted against `used_h_levels` (Principle 5)
/// and clamped to `min_route_y` to stay below the frame tab (Principle 1).
fn build_obstacle_aware_route(
    sp: Pos2,
    sp_stub: Pos2,
    tp: Pos2,
    tp_stub: Pos2,
    channel_x: f32,
    rects: &[egui::Rect],
    used_h_levels: &[(f32, f32, f32)],
    min_route_y: f32,
) -> Vec<Pos2> {
    let mut points = vec![sp, sp_stub];

    // Phase 1: horizontal from sp_stub to channel_x at sp.y
    let h1_end = Pos2::new(channel_x, sp.y);
    if !h_segment_crosses_element(sp_stub, h1_end, rects) {
        points.push(h1_end);
    } else {
        // Detour above or below blocking elements
        let clear_y = find_clear_y(sp_stub.x, channel_x, sp.y, rects);
        let deconf_y = deconflict_y(clear_y, sp_stub.x, channel_x, used_h_levels).max(min_route_y);
        points.push(Pos2::new(sp_stub.x, deconf_y));
        points.push(Pos2::new(channel_x, deconf_y));
    }

    // Phase 2+3: from channel to tp_stub
    // Try direct: vertical in channel to tp.y, then horizontal to tp_stub
    let h2_start = Pos2::new(channel_x, tp.y);
    if !h_segment_crosses_element(h2_start, tp_stub, rects) {
        // Safe: go to tp.y in channel, then horizontal to tp_stub
        if let Some(last) = points.last()
            && (last.y - tp.y).abs() > 1.0 {
                points.push(Pos2::new(channel_x, tp.y));
            }
        points.push(tp_stub);
    } else {
        // Blocked: find clear y, horizontal to above tp_stub.x, vertical to tp.y
        let clear_y = find_clear_y(channel_x, tp_stub.x, tp.y, rects);
        let deconf_y = deconflict_y(clear_y, channel_x, tp_stub.x, used_h_levels).max(min_route_y);
        if let Some(last) = points.last()
            && (last.y - deconf_y).abs() > 1.0 {
                points.push(Pos2::new(channel_x, deconf_y));
            }
        points.push(Pos2::new(tp_stub.x, deconf_y));
        points.push(tp_stub);
    }

    points.push(tp);

    // Remove near-duplicate consecutive points
    points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5);
    points
}

/// Check if a horizontal segment crosses any element's interior.
fn h_segment_crosses_element(from: Pos2, to: Pos2, rects: &[egui::Rect]) -> bool {
    let y = from.y;
    let min_x = from.x.min(to.x);
    let max_x = from.x.max(to.x);
    rects.iter().any(|rect| {
        y > rect.top() + theme::ROUTE_BOUNDARY_MARGIN
            && y < rect.bottom() - theme::ROUTE_BOUNDARY_MARGIN
            && min_x < rect.right() - theme::ROUTE_BOUNDARY_MARGIN
            && max_x > rect.left() + theme::ROUTE_BOUNDARY_MARGIN
    })
}

/// Find a y-coordinate clear of all elements in an x-range.
///
/// Picks the nearest clear y above or below the current y.
fn find_clear_y(x1: f32, x2: f32, current_y: f32, rects: &[egui::Rect]) -> f32 {
    let min_x = x1.min(x2);
    let max_x = x1.max(x2);

    // Collect elements that overlap the x-range
    let relevant: Vec<&egui::Rect> = rects
        .iter()
        .filter(|r| {
            r.left() < max_x + theme::ROUTE_BOUNDARY_MARGIN
                && r.right() > min_x - theme::ROUTE_BOUNDARY_MARGIN
        })
        .collect();

    if relevant.is_empty() {
        return current_y;
    }

    let above = relevant
        .iter()
        .map(|r| r.top())
        .fold(f32::INFINITY, f32::min)
        - theme::ROUTE_ELEMENT_CLEARANCE;
    let below = relevant
        .iter()
        .map(|r| r.bottom())
        .fold(f32::NEG_INFINITY, f32::max)
        + theme::ROUTE_ELEMENT_CLEARANCE;

    if (current_y - above).abs() <= (current_y - below).abs() {
        above
    } else {
        below
    }
}

/// Offset a y-coordinate until it's clear of all existing horizontal segments
/// that overlap in x, enforcing `ROUTE_PARALLEL_SPACING` (Principle 5).
fn deconflict_y(mut y: f32, x1: f32, x2: f32, used: &[(f32, f32, f32)]) -> f32 {
    let seg_min = x1.min(x2);
    let seg_max = x1.max(x2);

    // Try up to 20 offsets (enough for any realistic diagram)
    for _ in 0..20 {
        let conflict = used.iter().any(|&(umin, umax, uy)| {
            // Segments overlap in x and are too close in y
            umin < seg_max && umax > seg_min && (y - uy).abs() < theme::ROUTE_PARALLEL_SPACING
        });
        if !conflict {
            break;
        }
        // Move upward (away from elements below, toward frame top corridor)
        y -= theme::ROUTE_PARALLEL_SPACING;
    }
    y
}

/// Compute column gaps: vertical strips between groups of elements.
fn compute_column_gaps(layout: &Layout) -> Vec<f32> {
    let mut ranges: Vec<(f32, f32)> = layout
        .positions
        .iter()
        .filter_map(|(&id, &pos)| layout.sizes.get(&id).map(|&s| (pos.x, pos.x + s.x)))
        .collect();

    if ranges.is_empty() {
        return vec![100.0];
    }

    ranges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Merge overlapping/adjacent ranges into columns
    let mut columns: Vec<(f32, f32)> = Vec::new();
    for (left, right) in ranges {
        if let Some(last) = columns.last_mut()
            && left <= last.1 + theme::ROUTE_COLUMN_MERGE {
                last.1 = last.1.max(right);
                continue;
            }
        columns.push((left, right));
    }

    let mut gaps = Vec::new();
    if let Some(first) = columns.first() {
        gaps.push(first.0 - theme::ROUTE_COLUMN_GAP_MARGIN);
    }
    for w in columns.windows(2) {
        gaps.push((w[0].1 + w[1].0) / 2.0);
    }
    if let Some(last) = columns.last() {
        gaps.push(last.1 + theme::ROUTE_COLUMN_GAP_MARGIN);
    }
    gaps
}

/// Find the column gap nearest to a preferred x position.
fn nearest_gap(gaps: &[f32], preferred: f32) -> f32 {
    gaps.iter()
        .copied()
        .min_by(|&a, &b| {
            (a - preferred)
                .abs()
                .partial_cmp(&(b - preferred).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(preferred)
}

/// Determine whether a point is on the left or right edge of an element.
///
/// Returns -1.0 for left-side (connector should go left) or
/// +1.0 for right-side (connector should go right).
fn outward_direction(pos: Pos2, layout: &Layout) -> f32 {
    for (&_id, &elem_pos) in &layout.positions {
        if let Some(&size) = layout.sizes.get(&_id)
            && pos.y >= elem_pos.y - theme::ROUTE_BOUNDARY_MARGIN
                && pos.y <= elem_pos.y + size.y + theme::ROUTE_BOUNDARY_MARGIN
            {
                if (pos.x - elem_pos.x).abs() < theme::ROUTE_EDGE_TOLERANCE {
                    return -1.0; // on left edge
                }
                if (pos.x - (elem_pos.x + size.x)).abs() < theme::ROUTE_EDGE_TOLERANCE {
                    return 1.0; // on right edge
                }
            }
    }
    1.0 // default: right
}

/// Resolve a dot-path endpoint to a position.
///
/// Resolution order:
/// 1. Direct path resolution to a port in port_positions
/// 2. Type-based resolution: find port on the type definition
/// 3. Fallback: nearest edge of the element (not center)
fn resolve_endpoint(
    model: &Model,
    context: ElementId,
    path: &str,
    layout: &Layout,
    name_index: &HashMap<&str, Vec<ElementId>>,
) -> Option<Pos2> {
    // 1. Try resolving the full path directly
    if let Some(id) = model.resolve_path(context, path)
        && let Some(&pos) = layout.port_positions.get(&id) {
            return Some(pos);
        }

    // Parse the path into segments
    let segments: Vec<&str> = path.split('.').collect();
    let first = segments[0];

    // Find the child element (the part usage)
    let ctx = model.element(context)?;
    let child_id = ctx.children.iter().copied().find(|&cid| {
        model
            .element(cid)
            .and_then(|e| e.name.as_deref())
            .is_some_and(|n| n == first)
    })?;

    // 2. If there's a port name segment, try type-based resolution
    if segments.len() > 1 {
        let port_name = segments[1];
        if let Some(child_elem) = model.element(child_id)
            && let Some(ref type_name) = child_elem.type_ref
                && let Some(type_ids) = name_index.get(type_name.as_str()) {
                    for &type_id in type_ids {
                        if let Some(type_elem) = model.element(type_id)
                            && type_elem.kind.metatype() == Metatype::Definition {
                                // Find the port in the definition's children
                                let port_id = type_elem.children.iter().copied().find(|&cid| {
                                    model
                                        .element(cid)
                                        .and_then(|e| e.name.as_deref())
                                        .is_some_and(|n| n == port_name)
                                });
                                if let Some(pid) = port_id
                                    && let Some(&pos) = layout.port_positions.get(&pid) {
                                        return Some(pos);
                                    }
                            }
                    }
                }
    }

    // 3. Fallback: use the element's right edge center (most connectors go left→right)
    let pos = layout.positions.get(&child_id)?;
    let size = layout.sizes.get(&child_id)?;
    Some(Pos2::new(pos.x + size.x, pos.y + size.y / 2.0))
}

/// Extract the first segment of a dot-separated path.
fn first_segment(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}
