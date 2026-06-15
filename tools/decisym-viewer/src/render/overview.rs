//! Overview layout: full-model compound graph with packages as nodes.
//!
//! Reuses the topological layering approach from `layout.rs`:
//! 1. Collect top-level packages (and direct root elements) as nodes
//! 2. Build connectivity graph from cross-package relationships
//! 3. Assign layers using topological sort
//! 4. Position packages with subtree weight determining node height
//! 5. Route inter-package edges

use std::collections::{HashMap, HashSet};

use egui::Pos2;

use crate::model::{ElementId, Model};
use crate::render::layout::{ConnectorRoute, Layout};
use crate::render::theme;

/// Compute layout for the model overview (all top-level elements as nodes).
pub fn compute_overview_layout(model: &Model, egui_ctx: &egui::Context) -> Layout {
    // Find the effective root: if single root package, use its children
    let top_nodes: Vec<ElementId> = model
        .root_ids
        .iter()
        .copied()
        .filter(|&id| {
            model
                .element(id)
                .is_some_and(|e| e.kind.is_structural() && e.name.is_some())
        })
        .collect();

    let context = if top_nodes.len() == 1 {
        Some(top_nodes[0])
    } else {
        None
    };

    compute_children_layout(model, context, egui_ctx)
}

/// Compute a layout showing the children of a given element as overview nodes.
///
/// If `context` is None, shows root-level elements.
/// If `context` is Some(id), shows the structural children of that element.
/// This is the general-purpose "show what's inside this thing" layout.
pub fn compute_children_layout(
    model: &Model,
    context: Option<ElementId>,
    egui_ctx: &egui::Context,
) -> Layout {
    let mut layout = Layout::default();

    let nodes: Vec<ElementId> = match context {
        Some(ctx_id) => {
            let ctx = match model.element(ctx_id) {
                Some(e) => e,
                None => return layout,
            };
            ctx.children
                .iter()
                .copied()
                .filter(|&id| {
                    model
                        .element(id)
                        .is_some_and(|e| e.kind.is_structural() && e.name.is_some())
                })
                .collect()
        }
        None => model
            .root_ids
            .iter()
            .copied()
            .filter(|&id| {
                model
                    .element(id)
                    .is_some_and(|e| e.kind.is_structural() && e.name.is_some())
            })
            .collect(),
    };

    if nodes.is_empty() {
        return layout;
    }

    // Build connectivity graph from cross-element relationships
    let (successors, has_incoming) = build_connectivity(&nodes, model);

    // Assign layers using topological sort
    let layers = assign_layers(&nodes, &successors, &has_incoming);

    // Compute node sizes based on subtree weight and text
    for &id in &nodes {
        let size = compute_overview_node_size(model, id, egui_ctx);
        layout.sizes.insert(id, size);
    }

    // Position nodes in layers
    let start_x = theme::FRAME_PADDING + theme::ROUTE_FRAME_MARGIN;
    let start_y = theme::FRAME_PADDING + theme::FRAME_TAB_HEIGHT + theme::ROUTE_FRAME_MARGIN;

    let mut x = start_x;
    for layer in &layers {
        let mut y = start_y;
        let mut max_width: f32 = 0.0;

        for &id in layer {
            let size = layout.sizes.get(&id).copied().unwrap_or(egui::vec2(
                theme::OVERVIEW_PACKAGE_MIN_WIDTH,
                theme::OVERVIEW_PACKAGE_MIN_HEIGHT,
            ));
            layout.positions.insert(id, Pos2::new(x, y));
            y += size.y + theme::OVERVIEW_PACKAGE_V_SPACING;
            max_width = max_width.max(size.x);
        }
        x += max_width + theme::OVERVIEW_PACKAGE_H_SPACING;
    }

    // Route inter-node edges (simple straight lines for overview)
    route_overview_edges(model, &nodes, &mut layout);

    layout
}

/// Build a connectivity graph between overview nodes.
fn build_connectivity(
    nodes: &[ElementId],
    model: &Model,
) -> (HashMap<ElementId, HashSet<ElementId>>, HashSet<ElementId>) {
    let node_set: HashSet<ElementId> = nodes.iter().copied().collect();
    let mut successors: HashMap<ElementId, HashSet<ElementId>> = HashMap::new();
    let mut has_incoming: HashSet<ElementId> = HashSet::new();

    // From explicit relationships: connect owner's top ancestor to target's top ancestor
    let name_index = model.name_index();
    for rel in &model.relationships {
        let src_ancestor = find_node_ancestor(model, rel.owner, &node_set);
        let tgt_name = rel.target_path.split('.').next().unwrap_or("");
        if let Some(tgt_ids) = name_index.get(tgt_name) {
            for &tgt_id in tgt_ids {
                let tgt_ancestor = find_node_ancestor(model, tgt_id, &node_set);
                if let (Some(s), Some(t)) = (src_ancestor, tgt_ancestor)
                    && s != t {
                        successors.entry(s).or_default().insert(t);
                        has_incoming.insert(t);
                        break;
                    }
            }
        }
    }

    // From specializations: element specializes something in another node
    for &nid in nodes {
        visit_specializations(
            model,
            nid,
            nid,
            &node_set,
            &name_index,
            &mut successors,
            &mut has_incoming,
        );
    }

    (successors, has_incoming)
}

/// Recursively find specialization edges from descendants of `id` back to the node level.
fn visit_specializations(
    model: &Model,
    root_node: ElementId,
    id: ElementId,
    node_set: &HashSet<ElementId>,
    name_index: &HashMap<&str, Vec<ElementId>>,
    successors: &mut HashMap<ElementId, HashSet<ElementId>>,
    has_incoming: &mut HashSet<ElementId>,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    for spec in &elem.specializes {
        let spec_name = spec.split("::").last().unwrap_or(spec);
        if let Some(spec_ids) = name_index.get(spec_name) {
            for &spec_id in spec_ids {
                let tgt = find_node_ancestor(model, spec_id, node_set);
                if let Some(t) = tgt
                    && t != root_node {
                        successors.entry(root_node).or_default().insert(t);
                        has_incoming.insert(t);
                        break;
                    }
            }
        }
    }
    for &child in &elem.children {
        visit_specializations(
            model,
            root_node,
            child,
            node_set,
            name_index,
            successors,
            has_incoming,
        );
    }
}

/// Walk up the parent chain from `id` until we find an element in `node_set`.
fn find_node_ancestor(
    model: &Model,
    id: ElementId,
    node_set: &HashSet<ElementId>,
) -> Option<ElementId> {
    let mut current = id;
    loop {
        if node_set.contains(&current) {
            return Some(current);
        }
        let elem = model.element(current)?;
        current = elem.parent?;
    }
}

/// Topological layer assignment (same approach as layout.rs).
fn assign_layers(
    nodes: &[ElementId],
    successors: &HashMap<ElementId, HashSet<ElementId>>,
    has_incoming: &HashSet<ElementId>,
) -> Vec<Vec<ElementId>> {
    let mut layers: Vec<Vec<ElementId>> = Vec::new();
    let mut assigned: HashSet<ElementId> = HashSet::new();

    // Layer 0: nodes with no incoming edges
    let mut current_layer: Vec<ElementId> = nodes
        .iter()
        .copied()
        .filter(|id| !has_incoming.contains(id))
        .collect();

    if current_layer.is_empty() {
        // No clear topology — single layer with all nodes
        return vec![nodes.to_vec()];
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

    // Add unassigned nodes to last layer
    let unassigned: Vec<ElementId> = nodes
        .iter()
        .copied()
        .filter(|id| !assigned.contains(id))
        .collect();
    if !unassigned.is_empty() {
        layers.push(unassigned);
    }

    layers
}

/// Compute the display size of an overview node.
fn compute_overview_node_size(
    model: &Model,
    id: ElementId,
    egui_ctx: &egui::Context,
) -> egui::Vec2 {
    let elem = match model.element(id) {
        Some(e) => e,
        None => {
            return egui::vec2(
                theme::OVERVIEW_PACKAGE_MIN_WIDTH,
                theme::OVERVIEW_PACKAGE_MIN_HEIGHT,
            );
        }
    };

    let base_font = egui::FontId::proportional(theme::BASE_FONT_SIZE);
    let small_font = egui::FontId::proportional(theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE);

    // Width: based on name text
    let name = elem.display_name();
    let keyword = elem.kind.keyword();
    let name_width = egui_ctx.fonts_mut(|f| {
        f.layout_no_wrap(name.to_string(), base_font.clone(), egui::Color32::BLACK)
            .rect
            .width()
    });
    let keyword_width = egui_ctx.fonts_mut(|f| {
        f.layout_no_wrap(format!("<<{keyword}>>"), small_font, egui::Color32::BLACK)
            .rect
            .width()
    });
    let text_width = name_width.max(keyword_width);
    let width = (text_width + theme::ELEMENT_PADDING * 2.0 + theme::OVERVIEW_TEXT_PADDING)
        .max(theme::OVERVIEW_PACKAGE_MIN_WIDTH);

    // Height: based on subtree weight
    let weight = model.subtree_weight(id);
    let height = (theme::OVERVIEW_PACKAGE_MIN_HEIGHT
        + weight as f32 * theme::OVERVIEW_WEIGHT_SCALE)
        .min(theme::OVERVIEW_PACKAGE_MAX_HEIGHT);

    egui::vec2(width, height)
}

/// Route edges between overview nodes as simple polylines.
fn route_overview_edges(model: &Model, nodes: &[ElementId], layout: &mut Layout) {
    let node_set: HashSet<ElementId> = nodes.iter().copied().collect();
    let name_index = model.name_index();
    let mut seen_edges: HashSet<(ElementId, ElementId)> = HashSet::new();

    // Collect edges from relationships
    for (ri, rel) in model.relationships.iter().enumerate() {
        let src_ancestor = find_node_ancestor(model, rel.owner, &node_set);
        let tgt_name = rel.target_path.split('.').next().unwrap_or("");
        if let Some(tgt_ids) = name_index.get(tgt_name) {
            for &tgt_id in tgt_ids {
                let tgt_ancestor = find_node_ancestor(model, tgt_id, &node_set);
                if let (Some(s), Some(t)) = (src_ancestor, tgt_ancestor)
                    && s != t && !seen_edges.contains(&(s, t)) {
                        seen_edges.insert((s, t));
                        if let Some(points) = simple_edge_route(s, t, layout) {
                            layout.connector_routes.push(ConnectorRoute {
                                points,
                                rel_index: ri,
                            });
                        }
                    }
            }
        }
    }
}

/// Simple edge route: right edge of source → left edge of target.
fn simple_edge_route(src: ElementId, tgt: ElementId, layout: &Layout) -> Option<Vec<Pos2>> {
    let sp = layout.positions.get(&src)?;
    let ss = layout.sizes.get(&src)?;
    let tp = layout.positions.get(&tgt)?;

    let src_center_y = sp.y + ss.y / 2.0;
    let tgt_size = layout.sizes.get(&tgt)?;
    let tgt_center_y = tp.y + tgt_size.y / 2.0;

    let start = Pos2::new(sp.x + ss.x, src_center_y);
    let end = Pos2::new(tp.x, tgt_center_y);

    // Simple L-shaped route
    let mid_x = (start.x + end.x) / 2.0;
    Some(vec![
        start,
        Pos2::new(mid_x, start.y),
        Pos2::new(mid_x, end.y),
        end,
    ])
}
