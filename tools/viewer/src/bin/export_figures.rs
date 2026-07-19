use std::fmt::Write as _;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

use egui::{Color32, Context, Pos2, Rect, Vec2};
use serde::Deserialize;
use sysmlv2_gui::model::parse::parse_sysml;
use sysmlv2_gui::model::{ElementId, ElementKind, Metatype, Model, RelationshipKind};
use sysmlv2_gui::render::layout::{self, ConnectorRoute};
use sysmlv2_gui::render::theme;

const EXPORT_PAGE_MARGIN: f32 = 2.0;
const PDF_BASELINE_FROM_TOP_FACTOR: f32 = 0.78;
const PDF_MIN_FONT_SIZE: f32 = 6.5;
const EXPORT_FRAME_PADDING: f32 = 10.0;
const EXPORT_ELEMENT_MIN_WIDTH: f32 = 96.0;
// Header must fully contain the keyword line + the name line ABOVE the
// compartment separator: padding + keyword/name gap + name font + bottom margin.
// (Previously a flat 34.0, which let the name's lower edge clip the separator.)
const EXPORT_ELEMENT_HEADER_HEIGHT: f32 =
    theme::ELEMENT_PADDING + theme::KEYWORD_NAME_GAP + theme::BASE_FONT_SIZE + 7.0;
const EXPORT_ELEMENT_FEATURE_ROW_HEIGHT: f32 = 16.0;
const EXPORT_FEATURE_BOTTOM_PADDING: f32 = 8.0;
const EXPORT_COLUMN_GAP: f32 = 34.0;
const EXPORT_ROW_GAP: f32 = 22.0;
/// Max width (in layout points, before frame padding) a single rank/row may
/// occupy in the graph-layout views (requirements / traceability) before it
/// wraps into multiple stacked sub-rows. At ANGARS scale a rank of 28+
/// requirement boxes laid out as one horizontal row produced a ~16500px-wide
/// ribbon (16562×581) that is unreadable at gallery/README size; wrapping caps
/// the canvas width so the diagram reads as a legible block, not a hairline
/// strip. The demo/gallery rasterizes PDF points to PNG at 150 DPI (×150/72 ≈
/// ×2.08), so a 1700-pt cap lands the ANGARS renders around ~3600px wide — the
/// README-readable target — while keeping ~3-4 typical requirement boxes per
/// sub-row. Tuned so the widest ANGARS view stays under ~3600px in the
/// committed gallery PNGs.
const EXPORT_GRAPH_MAX_ROW_WIDTH: f32 = 1700.0;

#[derive(Clone, Copy, Debug)]
enum ViewKind {
    General,
    Interconnection,
    ActionFlow,
    StateTransition,
    Bdd,
    Requirements,
    Traceability,
}

/// Owned view spec — either from the default list or parsed from a --spec JSON file.
#[derive(Clone, Debug)]
struct ViewSpec {
    file_stem: String,
    context_name: String,
    frame_label: String,
    kind: ViewKind,
}

/// JSON entry in a --spec file.  All fields are strings; `kind` is
/// resolved by `parse_kind`.
#[derive(Deserialize)]
struct SpecEntry {
    file_stem: String,
    context_name: String,
    frame_label: String,
    kind: String,
}

/// Map a kind string to `ViewKind`.  Returns `Err` for unknown strings.
fn parse_kind(s: &str) -> Result<ViewKind, String> {
    match s {
        "general" => Ok(ViewKind::General),
        "interconnection" => Ok(ViewKind::Interconnection),
        "action" => Ok(ViewKind::ActionFlow),
        "state" => Ok(ViewKind::StateTransition),
        "bdd" => Ok(ViewKind::Bdd),
        "requirements" => Ok(ViewKind::Requirements),
        "traceability" => Ok(ViewKind::Traceability),
        other => Err(format!("unknown view kind: {other:?} (expected one of: general, interconnection, action, state, bdd, requirements, traceability)")),
    }
}

/// Load a `--spec` JSON file and convert to `Vec<ViewSpec>`.
fn load_spec_file(path: &str) -> Result<Vec<ViewSpec>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("cannot read spec file {path:?}: {e}"))?;
    let entries: Vec<SpecEntry> = serde_json::from_str(&content)
        .map_err(|e| format!("malformed JSON in spec file {path:?}: {e}"))?;
    entries
        .into_iter()
        .map(|e| {
            let kind = parse_kind(&e.kind)?;
            Ok(ViewSpec {
                file_stem: e.file_stem,
                context_name: e.context_name,
                frame_label: e.frame_label,
                kind,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map_err(Into::into)
}

/// Default view specs (preserves the original 11 entries byte-for-byte).
fn default_view_specs() -> Vec<ViewSpec> {
    vec![
        ViewSpec {
            file_stem: "angars-ibd-subsystem".into(),
            context_name: "C&C Subsystem".into(),
            frame_label: "interconnection".into(),
            kind: ViewKind::Interconnection,
        },
        ViewSpec {
            file_stem: "angars-general-subsystem".into(),
            context_name: "C&C Subsystem".into(),
            frame_label: "general".into(),
            kind: ViewKind::General,
        },
        ViewSpec {
            file_stem: "angars-activity-operations".into(),
            context_name: "C&C Operations".into(),
            frame_label: "action".into(),
            kind: ViewKind::ActionFlow,
        },
        ViewSpec {
            file_stem: "activity-control-flow".into(),
            context_name: "Refueling Request Handling".into(),
            frame_label: "action".into(),
            kind: ViewKind::ActionFlow,
        },
        ViewSpec {
            file_stem: "state-machine".into(),
            context_name: "C&C Mode".into(),
            frame_label: "state".into(),
            kind: ViewKind::StateTransition,
        },
        ViewSpec {
            file_stem: "bdd-structure".into(),
            context_name: "C&C Architecture".into(),
            frame_label: "bdd".into(),
            kind: ViewKind::Bdd,
        },
        ViewSpec {
            file_stem: "requirements".into(),
            context_name: "C&C Requirements".into(),
            frame_label: "requirements".into(),
            kind: ViewKind::Requirements,
        },
        ViewSpec {
            file_stem: "traceability".into(),
            context_name: "C&C Trace".into(),
            frame_label: "traceability".into(),
            kind: ViewKind::Traceability,
        },
        // Full ANGARS model (context = the C&C Architecture package).
        ViewSpec {
            file_stem: "angars-bdd".into(),
            context_name: "C&C Architecture".into(),
            frame_label: "bdd".into(),
            kind: ViewKind::Bdd,
        },
        ViewSpec {
            file_stem: "angars-requirements".into(),
            context_name: "C&C Architecture".into(),
            frame_label: "requirements".into(),
            kind: ViewKind::Requirements,
        },
        ViewSpec {
            file_stem: "angars-traceability".into(),
            context_name: "C&C Architecture".into(),
            frame_label: "traceability".into(),
            kind: ViewKind::Traceability,
        },
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: export_figures <input.sysml> <output-dir> [--spec views.json]")?;
    let output_dir = args
        .next()
        .ok_or("usage: export_figures <input.sysml> <output-dir> [--spec views.json]")?;

    // Parse optional --spec / --stats flags.
    let mut spec_path: Option<String> = None;
    let mut print_stats = false;
    let mut rest = args.peekable();
    while let Some(arg) = rest.next() {
        if arg == "--spec" {
            spec_path = Some(
                rest.next()
                    .ok_or("--spec requires a path argument")?,
            );
        } else if arg == "--stats" {
            // Emit one `STATS ...` line per exported view (node / edge counts)
            // to stderr — a machine-checkable signal for the parity regression
            // guards (e.g. state-transition endpoint counts).
            print_stats = true;
        } else {
            return Err(format!("unexpected argument: {arg:?}").into());
        }
    }

    let specs = match spec_path {
        Some(ref path) => load_spec_file(path)?,
        None => default_view_specs(),
    };

    let source = fs::read_to_string(&input)?;
    let model = parse_sysml(&source)?;
    let egui_ctx = bootstrap_egui();
    let output_dir = PathBuf::from(output_dir);
    fs::create_dir_all(&output_dir)?;

    let mut exported = 0usize;
    let mut skip_reasons: Vec<String> = Vec::new();

    for spec in &specs {
        match export_view(&model, &egui_ctx, spec, &output_dir, print_stats) {
            Ok(output_path) => {
                println!("{}", output_path.display());
                exported += 1;
            }
            Err(e) => {
                let reason = format!("skip {}: {}", spec.file_stem, e);
                eprintln!("{reason}");
                skip_reasons.push(reason);
            }
        }
    }

    if exported == 0 {
        eprintln!("error: no views were exported (all {} spec(s) skipped)", specs.len());
        for reason in &skip_reasons {
            eprintln!("  {reason}");
        }
        std::process::exit(1);
    }

    Ok(())
}

fn export_view(
    model: &Model,
    egui_ctx: &Context,
    spec: &ViewSpec,
    output_dir: &Path,
    print_stats: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let context_id = find_named_element(model, &spec.context_name)
        .ok_or_else(|| format!("missing element '{}'", spec.context_name))?;

    let layout = match spec.kind {
        ViewKind::General => build_general_layout(model, context_id),
        ViewKind::Interconnection => {
            build_interconnection_layout(model, context_id, &spec.context_name)
                .or_else(|| build_generic_ibd_layout(model, context_id))
                .unwrap_or_else(|| layout::compute_layout(model, context_id, egui_ctx))
        }
        ViewKind::ActionFlow => build_flow_layout(
            model,
            context_id,
            |k| {
                matches!(
                    k,
                    ElementKind::ActionUsage
                        | ElementKind::ActionDef
                        | ElementKind::DecisionNode
                        | ElementKind::ForkNode
                        | ElementKind::JoinNode
                        | ElementKind::MergeNode
                )
            },
            |r| matches!(r, RelationshipKind::Succession | RelationshipKind::Flow),
        )
        .unwrap_or_else(|| build_general_layout(model, context_id)),
        ViewKind::StateTransition => build_flow_layout(
            model,
            context_id,
            |k| matches!(k, ElementKind::StateUsage | ElementKind::StateDef),
            |r| matches!(r, RelationshipKind::Transition),
        )
        .unwrap_or_else(|| build_general_layout(model, context_id)),
        ViewKind::Bdd => build_bdd_layout(model, context_id)
            .unwrap_or_else(|| build_general_layout(model, context_id)),
        ViewKind::Requirements => build_requirements_layout(model, context_id)
            .unwrap_or_else(|| build_general_layout(model, context_id)),
        ViewKind::Traceability => build_traceability_layout(model, context_id)
            .unwrap_or_else(|| build_general_layout(model, context_id)),
    };

    if print_stats {
        // `connector_routes` are the flow/succession/transition edges (each a
        // drawn polyline linking two boxes → both endpoints present). For the
        // state view every `transition first X then Y;` yields one route, so a
        // guard can assert routes == transition-count. `decorated_edges` are the
        // typed trace/BDD edges. Only routes with ≥2 waypoints (i.e. actually
        // drawn) are counted.
        let drawn_routes = layout
            .connector_routes
            .iter()
            .filter(|r| r.points.len() >= 2)
            .count();
        eprintln!(
            "STATS file_stem={} kind={:?} nodes={} connector_routes={} decorated_edges={} control_nodes={}",
            spec.file_stem,
            spec.kind,
            layout.positions.len(),
            drawn_routes,
            layout.decorated_edges.len(),
            layout.control_nodes.len(),
        );
    }

    let frame_rect = frame_rect_for_layout(&layout)
        .ok_or_else(|| format!("view '{}' has no visible content", spec.context_name))?;
    let translation = Vec2::new(
        EXPORT_PAGE_MARGIN - frame_rect.min.x,
        EXPORT_PAGE_MARGIN - frame_rect.min.y,
    );

    let frame_label = match spec.kind {
        ViewKind::ActionFlow
        | ViewKind::StateTransition
        | ViewKind::Bdd
        | ViewKind::Requirements
        | ViewKind::Traceability => format!("{} {}", spec.frame_label, spec.context_name),
        _ => spec.frame_label.to_string(),
    };
    let document = build_document(model, &layout, &frame_label, frame_rect, translation);
    let output_path = output_dir.join(format!("{}.pdf", spec.file_stem));
    write_pdf(&document, &output_path)?;
    Ok(output_path)
}

fn bootstrap_egui() -> Context {
    let ctx = Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

fn find_named_element(model: &Model, name: &str) -> Option<ElementId> {
    model
        .name_index()
        .get(name)
        .and_then(|ids| ids.first().copied())
}

fn frame_rect_for_layout(layout: &layout::Layout) -> Option<Rect> {
    let content = layout.content_bounds()?;
    Some(Rect::from_min_max(
        Pos2::new(
            content.min.x - EXPORT_FRAME_PADDING,
            content.min.y - EXPORT_FRAME_PADDING - theme::FRAME_TAB_HEIGHT,
        ),
        Pos2::new(
            content.max.x + EXPORT_FRAME_PADDING,
            content.max.y + EXPORT_FRAME_PADDING,
        ),
    ))
}

fn build_general_layout(model: &Model, context_id: ElementId) -> layout::Layout {
    let mut layout = layout::Layout::default();
    let size = compute_export_element_size(model, context_id);
    layout.positions.insert(
        context_id,
        Pos2::new(
            EXPORT_FRAME_PADDING,
            EXPORT_FRAME_PADDING + theme::FRAME_TAB_HEIGHT,
        ),
    );
    layout.sizes.insert(context_id, size);
    layout
}

/// Maximum characters for an aggregated IBD flow label before ellipsis.
const IBD_LABEL_MAX_CHARS: usize = 40;
/// Vertical spacing between distinct rail channels (px).
const IBD_CHANNEL_STEP: f32 = 18.0;
/// Gap between the part column and the first rail channel (px).
const IBD_RAIL_GAP: f32 = 46.0;
/// Vertical gap between stacked part boxes (px).
const IBD_STACK_GAP: f32 = 64.0;
/// Label y-stagger between adjacent channels to avoid horizontal collisions (px).
const IBD_LABEL_STAGGER: f32 = 14.0;

/// Generic Interconnection (IBD) layout for any context: part-usage children
/// stacked in one column (hub — highest flow degree — centered), one
/// aggregated edge per directed (source, target) pair labeled with the union
/// of flow items, routed on side rails with a dedicated channel per edge, and
/// port glyphs at every box attachment.
fn build_generic_ibd_layout(model: &Model, context_id: ElementId) -> Option<layout::Layout> {
    use std::collections::{BTreeMap, HashMap};

    let context = model.element(context_id)?;
    // Nodes: named part-typed children.
    let mut name_to_id: HashMap<&str, ElementId> = HashMap::new();
    let mut nodes: Vec<ElementId> = Vec::new();
    for &cid in &context.children {
        if let Some(c) = model.element(cid)
            && matches!(c.kind, ElementKind::PartUsage | ElementKind::PartDef)
            && let Some(n) = c.name.as_deref()
        {
            name_to_id.insert(n, cid);
            nodes.push(cid);
        }
    }
    if nodes.len() < 2 {
        return None;
    }

    // Aggregate Flow/Connect relationships by directed (source, target) pair.
    let mut agg: BTreeMap<(ElementId, ElementId), Vec<String>> = BTreeMap::new();
    for rel in &model.relationships {
        if !matches!(rel.kind, RelationshipKind::Flow | RelationshipKind::Connect) {
            continue;
        }
        let (Some(&s), Some(&t)) = (
            name_to_id.get(first_segment(&rel.source_path)),
            name_to_id.get(first_segment(&rel.target_path)),
        ) else {
            continue;
        };
        let items = agg.entry((s, t)).or_default();
        if let Some(item) = rel.type_ref.as_deref()
            && !items.iter().any(|i| i == item)
        {
            items.push(item.to_string());
        }
    }
    if agg.is_empty() {
        return None;
    }

    // Order: hub (highest degree) in the middle of the stack, rest by degree
    // alternating above/below.
    let mut degree: HashMap<ElementId, usize> = HashMap::new();
    for &(s, t) in agg.keys() {
        *degree.entry(s).or_default() += 1;
        *degree.entry(t).or_default() += 1;
    }
    let mut by_degree = nodes.clone();
    by_degree.sort_by_key(|id| std::cmp::Reverse(degree.get(id).copied().unwrap_or(0)));
    let mut order: Vec<ElementId> = Vec::with_capacity(by_degree.len());
    for (i, id) in by_degree.into_iter().enumerate() {
        if i % 2 == 0 {
            order.insert(order.len() / 2, id);
        } else {
            order.insert(order.len().div_ceil(2), id);
        }
    }

    // Stack the boxes in one centered column.
    let sizes: HashMap<ElementId, Vec2> = order
        .iter()
        .map(|&id| (id, compute_export_element_size(model, id)))
        .collect();
    let max_w = sizes.values().map(|s| s.x).fold(0.0_f32, f32::max);
    // Left rail channels are allocated for upward (later→earlier) edges, right
    // rail for downward; count them first so the column x offset reserves space.
    let row_of: HashMap<ElementId, usize> =
        order.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let n_left = agg.keys().filter(|(s, t)| row_of[t] < row_of[s]).count();
    let col_x = EXPORT_FRAME_PADDING + IBD_RAIL_GAP + n_left as f32 * IBD_CHANNEL_STEP;

    let mut lay = layout::Layout::default();
    let mut y = EXPORT_FRAME_PADDING + theme::FRAME_TAB_HEIGHT;
    for &id in &order {
        let s = sizes[&id];
        lay.positions
            .insert(id, Pos2::new(col_x + (max_w - s.x) * 0.5, y));
        lay.sizes.insert(id, s);
        y += s.y + IBD_STACK_GAP;
    }

    let rect_of = |id: ElementId| -> Rect {
        Rect::from_min_size(lay.positions[&id], lay.sizes[&id])
    };

    // Per-box attachment staggering: distribute each side's attachment points
    // along the box height.
    let mut side_counts: HashMap<(ElementId, bool), usize> = HashMap::new();
    for &(s, t) in agg.keys() {
        let down = row_of[&s] < row_of[&t];
        side_counts
            .entry((s, !down))
            .and_modify(|c| *c += 1)
            .or_insert(1); // source attaches: right if down
        side_counts
            .entry((t, !down))
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }
    let mut side_used: HashMap<(ElementId, bool), usize> = HashMap::new();
    let mut attach = |id: ElementId, left: bool| -> Pos2 {
        let total = side_counts.get(&(id, left)).copied().unwrap_or(1);
        let used = side_used.entry((id, left)).or_insert(0);
        let r = rect_of(id);
        let frac = (*used + 1) as f32 / (total + 1) as f32;
        *used += 1;
        let x = if left { r.left() } else { r.right() };
        Pos2::new(x, r.top() + r.height() * frac)
    };

    let mut right_ch = 0usize;
    let mut left_ch = 0usize;
    for (&(s, t), items) in &agg {
        let down = row_of[&s] < row_of[&t];
        // Downward edges route on the right rail, upward on the left.
        let left_side = !down;
        let sp = attach(s, left_side);
        let tp = attach(t, left_side);
        let rail_x = if left_side {
            left_ch += 1;
            col_x - IBD_RAIL_GAP - (left_ch - 1) as f32 * IBD_CHANNEL_STEP
        } else {
            right_ch += 1;
            col_x + max_w + IBD_RAIL_GAP + (right_ch - 1) as f32 * IBD_CHANNEL_STEP
        };
        let mut label = items.join(", ");
        if label.chars().count() > IBD_LABEL_MAX_CHARS {
            label = format!(
                "{}...",
                label.chars().take(IBD_LABEL_MAX_CHARS).collect::<String>()
            );
        }
        let ch_index = if left_side { left_ch } else { right_ch };
        let stagger = ((ch_index % 3) as f32 - 1.0) * IBD_LABEL_STAGGER;
        lay.port_glyphs.push(sp);
        lay.port_glyphs.push(tp);
        lay.decorated_edges.push(layout::DecoratedEdge {
            points: vec![
                sp,
                Pos2::new(rail_x, sp.y),
                Pos2::new(rail_x, tp.y),
                tp,
            ],
            start_marker: layout::EdgeMarker::None,
            end_marker: layout::EdgeMarker::OpenArrow,
            dashed: false,
            label: Some(label),
            label_pos: Some(Pos2::new(rail_x, (sp.y + tp.y) * 0.5 + stagger)),
        });
    }

    Some(lay)
}

fn build_interconnection_layout(
    model: &Model,
    context_id: ElementId,
    context_name: &str,
) -> Option<layout::Layout> {
    let placements = match context_name {
        "coreArchitecture" => &[
            ("authoritativeSourceData", 0_usize, 0_usize),
            ("portableSemanticArtifact", 1, 0),
            ("groundedAIService", 2, 0),
            ("operator", 2, 1),
        ][..],
        "assuranceArchitecture" => &[
            ("authoritativeSourceData", 0_usize, 0_usize),
            ("portableSemanticArtifact", 1, 0),
            ("groundedAIService", 2, 0),
        ][..],
        "edgeFederationArchitecture" => &[
            ("edgeNode", 0_usize, 0_usize),
            ("operator", 0, 1),
            ("localArtifact", 1, 0),
            ("groundedAIService", 1, 1),
            ("reachbackNode", 2, 0),
            ("remoteArtifact", 3, 0),
        ][..],
        _ => return None,
    };

    let context = model.element(context_id)?;
    let mut children_by_name = std::collections::HashMap::new();
    for child_id in &context.children {
        if let Some(child) = model.element(*child_id)
            && let Some(name) = child.name.as_deref() {
                children_by_name.insert(name, *child_id);
            }
    }

    let mut entries = Vec::new();
    for (name, col, row) in placements {
        let child_id = *children_by_name.get(name)?;
        let size = compute_export_element_size(model, child_id);
        entries.push((*name, child_id, *col, *row, size));
    }

    let col_count = placements.iter().map(|(_, col, _)| *col).max()? + 1;
    let row_count = placements.iter().map(|(_, _, row)| *row).max()? + 1;
    let mut col_widths = vec![0.0_f32; col_count];
    let mut row_heights = vec![0.0_f32; row_count];
    for (_, _, col, row, size) in &entries {
        col_widths[*col] = col_widths[*col].max(size.x);
        row_heights[*row] = row_heights[*row].max(size.y);
    }

    let mut x_positions = vec![EXPORT_FRAME_PADDING; col_count];
    let mut y_positions = vec![EXPORT_FRAME_PADDING + theme::FRAME_TAB_HEIGHT; row_count];
    for column in 1..col_count {
        x_positions[column] = x_positions[column - 1] + col_widths[column - 1] + EXPORT_COLUMN_GAP;
    }
    for row in 1..row_count {
        y_positions[row] = y_positions[row - 1] + row_heights[row - 1] + EXPORT_ROW_GAP;
    }

    let mut layout = layout::Layout::default();
    for (_, child_id, col, row, size) in &entries {
        layout
            .positions
            .insert(*child_id, Pos2::new(x_positions[*col], y_positions[*row]));
        layout.sizes.insert(*child_id, *size);
    }

    layout.connector_routes = build_manual_routes(model, context_id, context_name, &layout);
    Some(layout)
}

fn build_manual_routes(
    model: &Model,
    context_id: ElementId,
    context_name: &str,
    layout: &layout::Layout,
) -> Vec<ConnectorRoute> {
    let rel_indices: Vec<usize> = model
        .relationships
        .iter()
        .enumerate()
        .filter(|(_, rel)| rel.owner == context_id)
        .map(|(index, _)| index)
        .collect();

    let named_rect = |name: &str| -> Option<Rect> {
        let context = model.element(context_id)?;
        let child_id = context.children.iter().copied().find(|child_id| {
            model
                .element(*child_id)
                .and_then(|child| child.name.as_deref())
                == Some(name)
        })?;
        let position = layout.positions.get(&child_id).copied()?;
        let size = layout.sizes.get(&child_id).copied()?;
        Some(Rect::from_min_size(position, size))
    };

    let mut routes = Vec::new();
    for rel_index in rel_indices {
        let rel = &model.relationships[rel_index];
        let source = first_segment(&rel.source_path);
        let target = first_segment(&rel.target_path);
        let Some(src_rect) = named_rect(source) else {
            continue;
        };
        let Some(tgt_rect) = named_rect(target) else {
            continue;
        };

        let points = match context_name {
            "coreArchitecture" | "assuranceArchitecture" => {
                route_core_like(source, target, src_rect, tgt_rect)
            }
            "edgeFederationArchitecture" => route_edge(source, target, src_rect, tgt_rect),
            _ => route_simple(src_rect, tgt_rect),
        };

        routes.push(ConnectorRoute { points, rel_index });
    }

    routes
}

fn route_core_like(source: &str, target: &str, src_rect: Rect, tgt_rect: Rect) -> Vec<Pos2> {
    if source == "operator" && target == "groundedAIService" {
        return vec![
            Pos2::new(src_rect.center().x, src_rect.top()),
            Pos2::new(tgt_rect.center().x, tgt_rect.bottom()),
        ];
    }

    vec![
        Pos2::new(src_rect.right(), src_rect.center().y),
        Pos2::new(tgt_rect.left(), tgt_rect.center().y),
    ]
}

fn route_edge(source: &str, target: &str, src_rect: Rect, tgt_rect: Rect) -> Vec<Pos2> {
    match (source, target) {
        ("edgeNode", "localArtifact")
        | ("operator", "groundedAIService")
        | ("reachbackNode", "remoteArtifact") => vec![
            Pos2::new(src_rect.right(), src_rect.center().y),
            Pos2::new(tgt_rect.left(), tgt_rect.center().y),
        ],
        ("localArtifact", "groundedAIService") => vec![
            Pos2::new(src_rect.center().x, src_rect.bottom()),
            Pos2::new(tgt_rect.center().x, tgt_rect.top()),
        ],
        ("remoteArtifact", "groundedAIService") => {
            let start = Pos2::new(src_rect.left(), src_rect.center().y);
            let end = Pos2::new(tgt_rect.right(), tgt_rect.center().y);
            let mid_y = tgt_rect.top() - EXPORT_ROW_GAP * 0.35;
            vec![
                start,
                Pos2::new(start.x - EXPORT_COLUMN_GAP * 0.3, start.y),
                Pos2::new(start.x - EXPORT_COLUMN_GAP * 0.3, mid_y),
                Pos2::new(end.x + EXPORT_COLUMN_GAP * 0.2, mid_y),
                Pos2::new(end.x + EXPORT_COLUMN_GAP * 0.2, end.y),
                end,
            ]
        }
        ("edgeNode", "reachbackNode") => {
            let start = Pos2::new(src_rect.right(), src_rect.center().y);
            let end = Pos2::new(tgt_rect.left(), tgt_rect.center().y);
            let route_y = src_rect.top() - EXPORT_ROW_GAP * 0.45;
            vec![
                start,
                Pos2::new(start.x + EXPORT_COLUMN_GAP * 0.25, start.y),
                Pos2::new(start.x + EXPORT_COLUMN_GAP * 0.25, route_y),
                Pos2::new(end.x - EXPORT_COLUMN_GAP * 0.25, route_y),
                Pos2::new(end.x - EXPORT_COLUMN_GAP * 0.25, end.y),
                end,
            ]
        }
        _ => route_simple(src_rect, tgt_rect),
    }
}

fn route_simple(src_rect: Rect, tgt_rect: Rect) -> Vec<Pos2> {
    if src_rect.center().x < tgt_rect.center().x {
        vec![
            Pos2::new(src_rect.right(), src_rect.center().y),
            Pos2::new(tgt_rect.left(), tgt_rect.center().y),
        ]
    } else if src_rect.center().x > tgt_rect.center().x {
        vec![
            Pos2::new(src_rect.left(), src_rect.center().y),
            Pos2::new(tgt_rect.right(), tgt_rect.center().y),
        ]
    } else if src_rect.center().y < tgt_rect.center().y {
        vec![
            Pos2::new(src_rect.center().x, src_rect.bottom()),
            Pos2::new(tgt_rect.center().x, tgt_rect.top()),
        ]
    } else {
        vec![
            Pos2::new(src_rect.center().x, src_rect.top()),
            Pos2::new(tgt_rect.center().x, tgt_rect.bottom()),
        ]
    }
}

fn first_segment(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

/// Build a layered (top-to-bottom) Action Flow layout: the action usages of the
/// context action def are ranked by their succession/flow edges (longest-path
/// layering) and laid out as a directed flow. Returns None if the context has
/// no action children.
/// Generic trace/graph layout: nodes placed top-down (target ranked ABOVE
/// source), each edge drawn source->target with a dashed line, an open arrow at
/// the target, and a kind label. Used by Requirements + Traceability.
fn build_graph_layout(
    model: &Model,
    nodes: &[ElementId],
    edges: &[(ElementId, ElementId, String)],
) -> Option<layout::Layout> {
    use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

    let mut node_set: Vec<ElementId> = Vec::new();
    let mut seen: HashSet<ElementId> = HashSet::new();
    for &n in nodes {
        if seen.insert(n) {
            node_set.push(n);
        }
    }
    if node_set.is_empty() {
        return None;
    }

    // Rank edges: target ABOVE source -> rank-graph edge (target -> source).
    let rank_edges: Vec<(ElementId, ElementId)> = edges.iter().map(|(s, t, _)| (*t, *s)).collect();

    // Cycle removal (DFS).
    let mut adj: HashMap<ElementId, Vec<(ElementId, usize)>> = HashMap::new();
    for (i, (a, b)) in rank_edges.iter().enumerate() {
        adj.entry(*a).or_default().push((*b, i));
    }
    let mut vs: HashMap<ElementId, u8> = HashMap::new();
    let mut is_back = vec![false; rank_edges.len()];
    for &start in &node_set {
        if vs.get(&start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut st = vec![(start, 0usize)];
        vs.insert(start, 1);
        while let Some(&mut (nd, ref mut ci)) = st.last_mut() {
            let kids = adj.get(&nd).cloned().unwrap_or_default();
            if *ci < kids.len() {
                let (c, ei) = kids[*ci];
                *ci += 1;
                match vs.get(&c).copied().unwrap_or(0) {
                    0 => {
                        vs.insert(c, 1);
                        st.push((c, 0));
                    }
                    1 => is_back[ei] = true,
                    _ => {}
                }
            } else {
                vs.insert(nd, 2);
                st.pop();
            }
        }
    }

    let mut succ: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    let mut indeg: HashMap<ElementId, usize> = node_set.iter().map(|&n| (n, 0usize)).collect();
    for (i, (a, b)) in rank_edges.iter().enumerate() {
        if is_back[i] {
            continue;
        }
        succ.entry(*a).or_default().push(*b);
        *indeg.entry(*b).or_default() += 1;
    }
    let mut rank: HashMap<ElementId, usize> = node_set.iter().map(|&n| (n, 0usize)).collect();
    let mut iw = indeg.clone();
    let mut q: VecDeque<ElementId> = node_set.iter().copied().filter(|n| iw[n] == 0).collect();
    while let Some(n) = q.pop_front() {
        let rn = rank[&n];
        if let Some(ch) = succ.get(&n).cloned() {
            for m in ch {
                if rn + 1 > rank[&m] {
                    rank.insert(m, rn + 1);
                }
                if let Some(d) = iw.get_mut(&m) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        q.push_back(m);
                    }
                }
            }
        }
    }

    let mut by_rank: BTreeMap<usize, Vec<ElementId>> = BTreeMap::new();
    for &n in &node_set {
        by_rank.entry(rank[&n]).or_default().push(n);
    }
    for v in by_rank.values_mut() {
        v.sort_by_key(|&id| model.element(id).and_then(|e| e.name.clone()).unwrap_or_default());
    }

    let sizes: HashMap<ElementId, Vec2> =
        node_set.iter().map(|&n| (n, compute_export_element_size(model, n))).collect();
    let row_gap = EXPORT_ROW_GAP * 1.7;
    let col_gap = EXPORT_COLUMN_GAP;

    // Wrap each rank into sub-rows: a rank whose single-row width would exceed
    // EXPORT_GRAPH_MAX_ROW_WIDTH is greedily split into consecutive chunks that
    // each fit the cap, and the chunks stack vertically within the rank's band.
    // At probe scale (a handful of nodes per rank) no rank exceeds the cap, so
    // this is a no-op there and the existing single-row layout is preserved.
    let split_into_subrows = |ids: &[ElementId]| -> Vec<Vec<ElementId>> {
        let mut rows: Vec<Vec<ElementId>> = Vec::new();
        let mut cur: Vec<ElementId> = Vec::new();
        let mut cur_w = 0.0_f32;
        for &id in ids {
            let w = sizes[&id].x;
            let added = if cur.is_empty() { w } else { cur_w + col_gap + w };
            if !cur.is_empty() && added > EXPORT_GRAPH_MAX_ROW_WIDTH {
                rows.push(std::mem::take(&mut cur));
                cur_w = w;
                cur.push(id);
            } else {
                cur_w = added;
                cur.push(id);
            }
        }
        if !cur.is_empty() {
            rows.push(cur);
        }
        rows
    };
    let row_width = |ids: &[ElementId]| -> f32 {
        ids.iter().map(|id| sizes[id].x).sum::<f32>()
            + col_gap * ids.len().saturating_sub(1) as f32
    };

    // Overall canvas width = widest sub-row across every rank (post-wrap).
    let max_row_w = by_rank
        .values()
        .flat_map(|ids| split_into_subrows(ids).into_iter().map(|r| row_width(&r)))
        .fold(0.0_f32, f32::max);

    let mut layout = layout::Layout::default();
    let mut y = EXPORT_FRAME_PADDING + theme::FRAME_TAB_HEIGHT;
    for ids in by_rank.values() {
        for sub in split_into_subrows(ids) {
            let row_h = sub.iter().map(|id| sizes[id].y).fold(0.0_f32, f32::max);
            let row_w = row_width(&sub);
            let mut x = EXPORT_FRAME_PADDING + (max_row_w - row_w) * 0.5;
            for &id in &sub {
                let s = sizes[&id];
                layout.positions.insert(id, Pos2::new(x, y + (row_h - s.y) * 0.5));
                layout.sizes.insert(id, s);
                x += s.x + col_gap;
            }
            y += row_h + row_gap;
        }
    }

    let rect_of = |id: ElementId, lay: &layout::Layout| -> Option<Rect> {
        Some(Rect::from_min_size(*lay.positions.get(&id)?, *lay.sizes.get(&id)?))
    };
    let orth = |from: Pos2, to: Pos2| -> Vec<Pos2> {
        if (from.x - to.x).abs() < 0.5 {
            vec![from, to]
        } else {
            let m = (from.y + to.y) * 0.5;
            vec![from, Pos2::new(from.x, m), Pos2::new(to.x, m), to]
        }
    };
    // All element rects, so a label anchor can avoid landing on a box.
    let box_rects: Vec<Rect> = layout
        .positions
        .iter()
        .filter_map(|(id, &p)| layout.sizes.get(id).map(|&sz| Rect::from_min_size(p, sz)))
        .collect();
    let clear_of_boxes = |p: Pos2| -> bool {
        !box_rects.iter().any(|r| r.expand(1.0).contains(p))
    };
    // Pick a label anchor on the route that does NOT sit over a box: prefer the
    // midpoint of the longest segment whose midpoint is clear (channel runs in
    // the row gaps qualify), else fall back to the polyline midpoint. Without
    // this the raw midpoint frequently lands on a box edge and the box fill (drawn
    // after the edge) truncates the label — the "allocate" → "allo" clip (T5).
    let label_anchor = |pts: &[Pos2]| -> Option<Pos2> {
        let mut best: Option<(f32, Pos2)> = None;
        for w in pts.windows(2) {
            let mid = Pos2::new((w[0].x + w[1].x) * 0.5, (w[0].y + w[1].y) * 0.5);
            if !clear_of_boxes(mid) {
                continue;
            }
            let len = (w[1] - w[0]).length();
            if best.map(|(bl, _)| len > bl).unwrap_or(true) {
                best = Some((len, mid));
            }
        }
        best.map(|(_, p)| p).or_else(|| polyline_midpoint(pts))
    };
    for (s, t, label) in edges {
        let (Some(sr), Some(tr)) = (rect_of(*s, &layout), rect_of(*t, &layout)) else {
            continue;
        };
        let pts = if tr.center().y < sr.center().y - 1.0 {
            orth(
                Pos2::new(sr.center().x, sr.top()),
                Pos2::new(tr.center().x, tr.bottom()),
            )
        } else {
            let xx = sr.right().max(tr.right()) + col_gap * 0.5;
            vec![
                Pos2::new(sr.right(), sr.center().y),
                Pos2::new(xx, sr.center().y),
                Pos2::new(xx, tr.center().y),
                Pos2::new(tr.right(), tr.center().y),
            ]
        };
        let label_pos = if label.is_empty() { None } else { label_anchor(&pts) };
        layout.decorated_edges.push(layout::DecoratedEdge {
            points: pts,
            start_marker: layout::EdgeMarker::None,
            end_marker: layout::EdgeMarker::OpenArrow,
            dashed: true,
            label: if label.is_empty() {
                None
            } else {
                Some(label.clone())
            },
            label_pos,
        });
    }
    Some(layout)
}

/// Build a Requirement diagram: requirement boxes (id + text) + derive
/// hierarchy + satisfy/verify edges to satisfiers/verifiers.
fn build_requirements_layout(model: &Model, _context_id: ElementId) -> Option<layout::Layout> {
    use std::collections::HashMap;
    let mut name_to_id: HashMap<String, ElementId> = HashMap::new();
    for i in 0..model.element_count() {
        if let Some(e) = model.element(i)
            && let Some(n) = &e.name
        {
            name_to_id.entry(n.clone()).or_insert(i);
        }
    }
    let is_req = |id: ElementId| {
        model.element(id).is_some_and(|e| {
            matches!(e.kind, ElementKind::RequirementDef | ElementKind::RequirementUsage)
        })
    };
    let mut nodes: Vec<ElementId> = (0..model.element_count()).filter(|&i| is_req(i)).collect();
    if nodes.is_empty() {
        return None;
    }
    let mut edges: Vec<(ElementId, ElementId, String)> = Vec::new();
    for rel in &model.relationships {
        let s = name_to_id.get(first_segment(&rel.source_path)).copied();
        let t = name_to_id.get(first_segment(&rel.target_path)).copied();
        let (Some(s), Some(t)) = (s, t) else { continue };
        match rel.kind {
            RelationshipKind::Dependency if is_req(s) && is_req(t) => {
                edges.push((s, t, "derive".into()));
            }
            RelationshipKind::Satisfy if is_req(t) => {
                edges.push((s, t, "satisfy".into()));
                nodes.push(s);
            }
            RelationshipKind::Verify if is_req(t) => {
                edges.push((s, t, "verify".into()));
                nodes.push(s);
            }
            _ => {}
        }
    }
    build_graph_layout(model, &nodes, &edges)
}

/// Build a cross-pillar Traceability view: every satisfy/allocate/derive/verify
/// edge + its endpoint nodes (requirements, functions, components, verifications).
fn build_traceability_layout(model: &Model, _context_id: ElementId) -> Option<layout::Layout> {
    use std::collections::HashMap;
    let mut name_to_id: HashMap<String, ElementId> = HashMap::new();
    for i in 0..model.element_count() {
        if let Some(e) = model.element(i)
            && let Some(n) = &e.name
        {
            name_to_id.entry(n.clone()).or_insert(i);
        }
    }
    let mut nodes: Vec<ElementId> = Vec::new();
    let mut edges: Vec<(ElementId, ElementId, String)> = Vec::new();
    for rel in &model.relationships {
        let label = match rel.kind {
            RelationshipKind::Satisfy => "satisfy",
            RelationshipKind::Allocate => "allocate",
            RelationshipKind::Dependency => "derive",
            RelationshipKind::Verify => "verify",
            _ => continue,
        };
        if let (Some(s), Some(t)) = (
            name_to_id.get(first_segment(&rel.source_path)).copied(),
            name_to_id.get(first_segment(&rel.target_path)).copied(),
        ) {
            nodes.push(s);
            nodes.push(t);
            edges.push((s, t, label.into()));
        }
    }
    if nodes.is_empty() {
        return None;
    }
    build_graph_layout(model, &nodes, &edges)
}

/// Build a Block Definition Diagram: part-def boxes (attribute compartments)
/// laid out top-down by composition, with composition (◆) and specialization (▷)
/// decorated edges + multiplicity labels.
fn build_bdd_layout(model: &Model, context_id: ElementId) -> Option<layout::Layout> {
    use std::collections::{BTreeMap, HashMap, VecDeque};

    let context = model.element(context_id)?;
    let defs: Vec<ElementId> = context
        .children
        .iter()
        .copied()
        .filter(|&c| model.element(c).is_some_and(|e| e.kind == ElementKind::PartDef))
        .collect();
    if defs.is_empty() {
        return None;
    }
    let name_to_def: HashMap<String, ElementId> = defs
        .iter()
        .filter_map(|&d| model.element(d).and_then(|e| e.name.clone()).map(|n| (n, d)))
        .collect();

    enum Kind {
        Comp,
        Spec,
    }
    // (from, to, kind, label)
    let mut bedges: Vec<(ElementId, ElementId, Kind, Option<String>)> = Vec::new();
    let mut rank_edges: Vec<(ElementId, ElementId)> = Vec::new(); // parent (above) -> child (below)
    for &d in &defs {
        let Some(de) = model.element(d) else { continue };
        for &ch in &de.children {
            if let Some(c) = model.element(ch)
                && c.kind == ElementKind::PartUsage
                && let Some(t) = c.type_ref.as_deref()
                && let Some(&tid) = name_to_def.get(t)
            {
                let label = c.multiplicity.as_deref().map(|m| format!("[{m}]"));
                bedges.push((d, tid, Kind::Comp, label));
                rank_edges.push((d, tid)); // owner above contained
            }
        }
        for s in &de.specializes {
            if let Some(&sid) = name_to_def.get(s.as_str()) {
                bedges.push((d, sid, Kind::Spec, None));
                rank_edges.push((sid, d)); // supertype above subtype
            }
        }
    }

    // Longest-path layering (Kahn) over rank edges.
    let mut succ: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    let mut indeg: HashMap<ElementId, usize> = defs.iter().map(|&d| (d, 0usize)).collect();
    for (a, b) in &rank_edges {
        succ.entry(*a).or_default().push(*b);
        *indeg.entry(*b).or_default() += 1;
    }
    let mut rank: HashMap<ElementId, usize> = defs.iter().map(|&d| (d, 0usize)).collect();
    let mut iw = indeg.clone();
    let mut q: VecDeque<ElementId> = defs.iter().copied().filter(|d| iw[d] == 0).collect();
    while let Some(n) = q.pop_front() {
        let rn = rank[&n];
        if let Some(ch) = succ.get(&n).cloned() {
            for m in ch {
                if rn + 1 > rank[&m] {
                    rank.insert(m, rn + 1);
                }
                if let Some(dd) = iw.get_mut(&m) {
                    *dd = dd.saturating_sub(1);
                    if *dd == 0 {
                        q.push_back(m);
                    }
                }
            }
        }
    }

    let mut by_rank: BTreeMap<usize, Vec<ElementId>> = BTreeMap::new();
    for &d in &defs {
        by_rank.entry(rank[&d]).or_default().push(d);
    }
    for v in by_rank.values_mut() {
        v.sort_by_key(|&id| model.element(id).and_then(|e| e.name.clone()).unwrap_or_default());
    }

    let sizes: HashMap<ElementId, Vec2> =
        defs.iter().map(|&d| (d, compute_export_element_size(model, d))).collect();
    let row_gap = EXPORT_ROW_GAP * 1.9;
    let col_gap = EXPORT_COLUMN_GAP;
    let max_row_w = by_rank
        .values()
        .map(|ids| {
            ids.iter().map(|id| sizes[id].x).sum::<f32>()
                + col_gap * ids.len().saturating_sub(1) as f32
        })
        .fold(0.0_f32, f32::max);

    let mut layout = layout::Layout::default();
    let mut y = EXPORT_FRAME_PADDING + theme::FRAME_TAB_HEIGHT;
    for ids in by_rank.values() {
        let row_h = ids.iter().map(|id| sizes[id].y).fold(0.0_f32, f32::max);
        let row_w = ids.iter().map(|id| sizes[id].x).sum::<f32>()
            + col_gap * ids.len().saturating_sub(1) as f32;
        let mut x = EXPORT_FRAME_PADDING + (max_row_w - row_w) * 0.5;
        for &id in ids {
            let s = sizes[&id];
            layout.positions.insert(id, Pos2::new(x, y + (row_h - s.y) * 0.5));
            layout.sizes.insert(id, s);
            x += s.x + col_gap;
        }
        y += row_h + row_gap;
    }

    let rect_of = |id: ElementId, lay: &layout::Layout| -> Option<Rect> {
        Some(Rect::from_min_size(*lay.positions.get(&id)?, *lay.sizes.get(&id)?))
    };
    let orth = |from: Pos2, to: Pos2| -> Vec<Pos2> {
        if (from.x - to.x).abs() < 0.5 {
            vec![from, to]
        } else {
            let m = (from.y + to.y) * 0.5;
            vec![from, Pos2::new(from.x, m), Pos2::new(to.x, m), to]
        }
    };

    for (from, to, kind, label) in bedges {
        let (Some(fr), Some(tr)) = (rect_of(from, &layout), rect_of(to, &layout)) else {
            continue;
        };
        let edge = match kind {
            Kind::Comp => layout::DecoratedEdge {
                points: orth(
                    Pos2::new(fr.center().x, fr.bottom()),
                    Pos2::new(tr.center().x, tr.top()),
                ),
                start_marker: layout::EdgeMarker::FilledDiamond,
                end_marker: layout::EdgeMarker::None,
                dashed: false,
                label,
                label_pos: None,
            },
            Kind::Spec => layout::DecoratedEdge {
                points: orth(
                    Pos2::new(fr.center().x, fr.top()),
                    Pos2::new(tr.center().x, tr.bottom()),
                ),
                start_marker: layout::EdgeMarker::None,
                end_marker: layout::EdgeMarker::HollowTriangle,
                dashed: false,
                label: None,
                label_pos: None,
            },
        };
        layout.decorated_edges.push(edge);
    }
    Some(layout)
}

fn build_flow_layout(
    model: &Model,
    context_id: ElementId,
    node_match: impl Fn(ElementKind) -> bool,
    edge_match: impl Fn(RelationshipKind) -> bool,
) -> Option<layout::Layout> {
    use std::collections::{BTreeMap, HashMap, VecDeque};

    let context = model.element(context_id)?;
    let actions: Vec<ElementId> = context
        .children
        .iter()
        .copied()
        .filter(|&c| model.element(c).is_some_and(|e| node_match(e.kind)))
        .collect();
    if actions.is_empty() {
        return None;
    }

    let mut name_to_id: HashMap<String, ElementId> = HashMap::new();
    for &a in &actions {
        if let Some(e) = model.element(a)
            && let Some(name) = e.name.clone()
        {
            name_to_id.insert(name, a);
        }
    }

    // Flow edges owned by this context, resolved to node ids.
    let mut edges: Vec<(usize, ElementId, ElementId)> = Vec::new();
    for (idx, rel) in model.relationships.iter().enumerate() {
        if rel.owner != context_id {
            continue;
        }
        if !edge_match(rel.kind) {
            continue;
        }
        let s = first_segment(&rel.source_path);
        let t = first_segment(&rel.target_path);
        if let (Some(&sid), Some(&tid)) = (name_to_id.get(s), name_to_id.get(t)) {
            edges.push((idx, sid, tid));
        }
    }

    // Cycle removal (DFS): mark back-edges so the layered ranking sees a DAG.
    // State machines are cyclic; back-edges are still DRAWN (they route as
    // elbows), just excluded from rank computation so Kahn terminates.
    let mut adj: HashMap<ElementId, Vec<(ElementId, usize)>> = HashMap::new();
    for (i, (_, sid, tid)) in edges.iter().enumerate() {
        adj.entry(*sid).or_default().push((*tid, i));
    }
    let mut visit_state: HashMap<ElementId, u8> = HashMap::new(); // 0 unseen, 1 on-stack, 2 done
    let mut is_back = vec![false; edges.len()];
    for &start in &actions {
        if visit_state.get(&start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<(ElementId, usize)> = vec![(start, 0)];
        visit_state.insert(start, 1);
        while let Some(&mut (node, ref mut ci)) = stack.last_mut() {
            let kids = adj.get(&node).cloned().unwrap_or_default();
            if *ci < kids.len() {
                let (child, eidx) = kids[*ci];
                *ci += 1;
                match visit_state.get(&child).copied().unwrap_or(0) {
                    0 => {
                        visit_state.insert(child, 1);
                        stack.push((child, 0));
                    }
                    1 => is_back[eidx] = true,
                    _ => {}
                }
            } else {
                visit_state.insert(node, 2);
                stack.pop();
            }
        }
    }

    // Ranking graph from forward (non-back) edges only.
    let mut succ: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    let mut indeg: HashMap<ElementId, usize> = actions.iter().map(|&a| (a, 0usize)).collect();
    for (i, (_, sid, tid)) in edges.iter().enumerate() {
        if is_back[i] {
            continue;
        }
        succ.entry(*sid).or_default().push(*tid);
        *indeg.entry(*tid).or_default() += 1;
    }

    // Longest-path layering via Kahn topological order.
    let mut rank: HashMap<ElementId, usize> = actions.iter().map(|&a| (a, 0usize)).collect();
    let mut indeg_work = indeg.clone();
    let mut queue: VecDeque<ElementId> =
        actions.iter().copied().filter(|a| indeg_work[a] == 0).collect();
    while let Some(n) = queue.pop_front() {
        let rn = rank[&n];
        if let Some(children) = succ.get(&n).cloned() {
            for m in children {
                if rn + 1 > rank[&m] {
                    rank.insert(m, rn + 1);
                }
                if let Some(d) = indeg_work.get_mut(&m) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push_back(m);
                    }
                }
            }
        }
    }

    // Group by rank; stable order within a rank by name.
    let mut by_rank: BTreeMap<usize, Vec<ElementId>> = BTreeMap::new();
    for &a in &actions {
        by_rank.entry(rank[&a]).or_default().push(a);
    }
    for ids in by_rank.values_mut() {
        ids.sort_by_key(|&id| model.element(id).and_then(|e| e.name.clone()).unwrap_or_default());
    }

    // Control nodes render as fixed-size shapes (diamond / synchronization bar),
    // not name-sized boxes.
    let sizes: HashMap<ElementId, Vec2> = actions
        .iter()
        .map(|&a| {
            let kind = model.element(a).map(|e| e.kind);
            let s = match kind {
                Some(ElementKind::DecisionNode) | Some(ElementKind::MergeNode) => {
                    Vec2::new(36.0, 36.0)
                }
                Some(ElementKind::ForkNode) | Some(ElementKind::JoinNode) => {
                    Vec2::new(130.0, 12.0)
                }
                _ => compute_export_element_size(model, a),
            };
            (a, s)
        })
        .collect();

    let radius = theme::CONTROL_NODE_RADIUS;
    let row_gap = EXPORT_ROW_GAP * 1.6;
    let col_gap = EXPORT_COLUMN_GAP;
    let max_row_w = by_rank
        .values()
        .map(|ids| {
            ids.iter().map(|id| sizes[id].x).sum::<f32>()
                + col_gap * ids.len().saturating_sub(1) as f32
        })
        .fold(0.0_f32, f32::max);

    let mut layout = layout::Layout::default();
    let top0 = EXPORT_FRAME_PADDING + theme::FRAME_TAB_HEIGHT;
    let center_x = EXPORT_FRAME_PADDING + max_row_w * 0.5;

    // Initial node at the top; action rows flow below it.
    let initial_center = Pos2::new(center_x, top0 + radius);
    layout.control_nodes.push(layout::ControlNode {
        center: initial_center,
        kind: layout::ControlNodeKind::Initial,
    });

    let mut y = top0 + radius * 2.0 + row_gap;
    for ids in by_rank.values() {
        let row_h = ids.iter().map(|id| sizes[id].y).fold(0.0_f32, f32::max);
        let row_w = ids.iter().map(|id| sizes[id].x).sum::<f32>()
            + col_gap * ids.len().saturating_sub(1) as f32;
        let mut x = EXPORT_FRAME_PADDING + (max_row_w - row_w) * 0.5;
        for &id in ids {
            let s = sizes[&id];
            layout.positions.insert(id, Pos2::new(x, y + (row_h - s.y) * 0.5));
            layout.sizes.insert(id, s);
            x += s.x + col_gap;
        }
        y += row_h + row_gap;
    }

    // Final node below the last row.
    let final_center = Pos2::new(center_x, y + radius);
    layout.control_nodes.push(layout::ControlNode {
        center: final_center,
        kind: layout::ControlNodeKind::Final,
    });

    let rect_of = |id: ElementId, lay: &layout::Layout| -> Option<Rect> {
        Some(Rect::from_min_size(*lay.positions.get(&id)?, *lay.sizes.get(&id)?))
    };

    // Orthogonal elbow between two points: vertical / horizontal / vertical.
    let orth = |from: Pos2, to: Pos2| -> Vec<Pos2> {
        if (from.x - to.x).abs() < 0.5 {
            vec![from, to]
        } else {
            let mid_y = (from.y + to.y) * 0.5;
            vec![from, Pos2::new(from.x, mid_y), Pos2::new(to.x, mid_y), to]
        }
    };

    // Decoration edges: initial -> each source; each sink -> final.
    for &a in &actions {
        let Some(r) = rect_of(a, &layout) else {
            continue;
        };
        if indeg[&a] == 0 {
            layout
                .decoration_edges
                .push(orth(initial_center, Pos2::new(r.center().x, r.top())));
        }
        let has_out = succ.get(&a).is_some_and(|v| !v.is_empty());
        if !has_out {
            layout
                .decoration_edges
                .push(orth(Pos2::new(r.center().x, r.bottom()), final_center));
        }
    }

    // Real edges: orthogonal forward; back/same-rank edges elbow on the right.
    let mut routes = Vec::new();
    for (rel_index, sid, tid) in edges {
        let (Some(src), Some(tgt)) = (rect_of(sid, &layout), rect_of(tid, &layout)) else {
            continue;
        };
        let points = if tgt.center().y > src.center().y + 1.0 {
            orth(
                Pos2::new(src.center().x, src.bottom()),
                Pos2::new(tgt.center().x, tgt.top()),
            )
        } else {
            let x = src.right().max(tgt.right()) + col_gap * 0.5;
            vec![
                Pos2::new(src.right(), src.center().y),
                Pos2::new(x, src.center().y),
                Pos2::new(x, tgt.center().y),
                Pos2::new(tgt.right(), tgt.center().y),
            ]
        };
        routes.push(ConnectorRoute { points, rel_index });
    }
    layout.connector_routes = routes;
    Some(layout)
}

fn build_document(
    model: &Model,
    layout: &layout::Layout,
    frame_label: &str,
    frame_rect: Rect,
    translation: Vec2,
) -> PdfDocument {
    let mut document = PdfDocument::new(
        frame_rect.width() + EXPORT_PAGE_MARGIN * 2.0,
        frame_rect.height() + EXPORT_PAGE_MARGIN * 2.0,
    );

    render_frame(
        &mut document,
        frame_rect.translate(translation),
        frame_label,
    );
    render_connectors(&mut document, model, layout, translation);
    render_control_nodes(&mut document, layout, translation);
    render_decorated_edges(&mut document, layout, translation);
    render_elements(&mut document, model, layout, translation);
    render_ports(&mut document, model, layout, translation);
    // Edge labels last: keeps satisfy/derive/allocate/verify labels on top of
    // any box they overlap (T5 — the "allocate" label was being clipped to
    // "allo" by an element box painted after it).
    render_decorated_edge_labels(&mut document, layout, translation);

    document
}

fn render_frame(document: &mut PdfDocument, frame_rect: Rect, frame_label: &str) {
    document.push_rect(RectShape {
        rect: frame_rect,
        radius: 0.0,
        fill: None,
        stroke: Some(StrokeStyle {
            color: theme::COLOR_FRAME_STROKE,
            width: theme::FRAME_STROKE_WIDTH,
            dash: None,
        }),
    });

    let tab_padding = theme::ELEMENT_PADDING;
    let label_size = fitted_font_size(
        frame_label,
        theme::BASE_FONT_SIZE,
        frame_rect.width() * theme::FRAME_TAB_MAX_FRACTION - tab_padding * 2.0,
    );
    let tab_text_width = estimated_text_width(frame_label, label_size);
    let tab_width = (tab_text_width + tab_padding * 2.0)
        .min(frame_rect.width() * theme::FRAME_TAB_MAX_FRACTION);
    let tab_rect = Rect::from_min_size(
        frame_rect.min,
        Vec2::new(tab_width, theme::FRAME_TAB_HEIGHT),
    );

    document.push_rect(RectShape {
        rect: tab_rect,
        radius: 0.0,
        fill: Some(theme::COLOR_FRAME_TAB),
        stroke: Some(StrokeStyle {
            color: theme::COLOR_FRAME_STROKE,
            width: theme::FRAME_STROKE_WIDTH,
            dash: None,
        }),
    });
    document.push_text(TextShape {
        position: Pos2::new(
            tab_rect.center().x - tab_text_width * 0.5,
            tab_rect.min.y + (tab_rect.height() - label_size) * 0.5,
        ),
        text: frame_label.to_string(),
        font_size: label_size,
        color: theme::COLOR_FRAME_STROKE,
    });
}

fn render_connectors(
    document: &mut PdfDocument,
    model: &Model,
    layout: &layout::Layout,
    translation: Vec2,
) {
    let mut routes: Vec<&layout::ConnectorRoute> = layout.connector_routes.iter().collect();
    routes.sort_by_key(|route| route.rel_index);

    for route in routes {
        let Some(rel) = model.relationships.get(route.rel_index) else {
            continue;
        };
        if route.points.len() < 2 {
            continue;
        }

        let (color, dash) = match rel.kind {
            RelationshipKind::Connect | RelationshipKind::Interface => {
                (theme::COLOR_CONNECTOR, None)
            }
            RelationshipKind::Bind => (
                theme::COLOR_CONNECTOR_BIND,
                Some((theme::CONNECTOR_DASH_LENGTH, theme::CONNECTOR_DASH_GAP)),
            ),
            RelationshipKind::Flow
            | RelationshipKind::Succession
            | RelationshipKind::Transition => (theme::COLOR_CONNECTOR, None),
            RelationshipKind::Allocate => (
                theme::COLOR_CONNECTOR,
                Some((theme::CONNECTOR_DASH_LENGTH, theme::CONNECTOR_DASH_GAP)),
            ),
            _ => (theme::COLOR_CONNECTOR, None),
        };

        let points: Vec<Pos2> = route
            .points
            .iter()
            .map(|point| *point + translation)
            .collect();
        document.push_polyline(PolylineShape {
            points: points.clone(),
            stroke: StrokeStyle {
                color,
                width: theme::CONNECTOR_LINE_WIDTH,
                dash,
            },
        });

        if matches!(
            rel.kind,
            RelationshipKind::Flow | RelationshipKind::Succession | RelationshipKind::Transition
        ) && let [.., previous, tip] = points.as_slice()
        {
            // Open (stick) arrowhead, drawn as a polyline — the Cameo/UML
            // activity edge style, not a filled triangle.
            document.push_polyline(PolylineShape {
                points: open_arrowhead(*previous, *tip),
                stroke: StrokeStyle {
                    color,
                    width: theme::CONNECTOR_LINE_WIDTH,
                    dash: None,
                },
            });
        }

        // Object-flow item-type label (e.g. "Schedule", "Report") at the midpoint.
        if rel.kind == RelationshipKind::Flow
            && let Some(item) = rel.type_ref.as_deref()
            && let Some(mid) = polyline_midpoint(&points)
        {
            let label_size = theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE;
            let width = estimated_text_width(item, label_size);
            document.push_rect(RectShape {
                rect: Rect::from_min_size(
                    Pos2::new(mid.x - width * 0.5 - 2.0, mid.y - label_size * 0.5 - 1.0),
                    Vec2::new(width + 4.0, label_size + 2.0),
                ),
                radius: 0.0,
                fill: Some(theme::COLOR_ELEMENT_FILL),
                stroke: None,
            });
            document.push_text(TextShape {
                position: Pos2::new(mid.x - width * 0.5, mid.y - label_size * 0.5),
                text: item.to_string(),
                font_size: label_size,
                color: theme::COLOR_KEYWORD_TEXT,
            });
        }

        // Guard label `[expr]` on a guarded control flow (a decision branch).
        if rel.kind == RelationshipKind::Succession
            && let Some(guard) = rel.name.as_deref()
            && let Some(mid) = polyline_midpoint(&points)
        {
            let text = format!("[{guard}]");
            let label_size = theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE;
            let width = estimated_text_width(&text, label_size);
            document.push_rect(RectShape {
                rect: Rect::from_min_size(
                    Pos2::new(mid.x - width * 0.5 - 2.0, mid.y - label_size * 0.5 - 1.0),
                    Vec2::new(width + 4.0, label_size + 2.0),
                ),
                radius: 0.0,
                fill: Some(theme::COLOR_ELEMENT_FILL),
                stroke: None,
            });
            document.push_text(TextShape {
                position: Pos2::new(mid.x - width * 0.5, mid.y - label_size * 0.5),
                text,
                font_size: label_size,
                color: theme::COLOR_KEYWORD_TEXT,
            });
        }

        // Transition label `trigger [guard] / effect` on a state transition.
        if rel.kind == RelationshipKind::Transition
            && let Some(lbl) = rel.name.as_deref()
            && let Some(mid) = polyline_midpoint(&points)
        {
            let label_size = theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE;
            let width = estimated_text_width(lbl, label_size);
            document.push_rect(RectShape {
                rect: Rect::from_min_size(
                    Pos2::new(mid.x - width * 0.5 - 2.0, mid.y - label_size * 0.5 - 1.0),
                    Vec2::new(width + 4.0, label_size + 2.0),
                ),
                radius: 0.0,
                fill: Some(theme::COLOR_ELEMENT_FILL),
                stroke: None,
            });
            document.push_text(TextShape {
                position: Pos2::new(mid.x - width * 0.5, mid.y - label_size * 0.5),
                text: lbl.to_string(),
                font_size: label_size,
                color: theme::COLOR_KEYWORD_TEXT,
            });
        }
    }
}

/// Open "stick" arrowhead as a polyline (left → tip → right).
fn open_arrowhead(previous: Pos2, tip: Pos2) -> Vec<Pos2> {
    let p = arrowhead_points(previous, tip); // [tip, left, right]
    vec![p[1], p[0], p[2]]
}

fn polyline_midpoint(points: &[Pos2]) -> Option<Pos2> {
    if points.len() < 2 {
        return None;
    }
    let i = points.len() / 2;
    Some(Pos2::new(
        (points[i - 1].x + points[i].x) * 0.5,
        (points[i - 1].y + points[i].y) * 0.5,
    ))
}

/// Approximate a circle as a 24-gon polygon (the PDF writer has no native circle).
fn circle_points(center: Pos2, radius: f32) -> Vec<Pos2> {
    let n = 24;
    (0..n)
        .map(|i| {
            let a = std::f32::consts::TAU * (i as f32) / (n as f32);
            Pos2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

/// Draw typed edges (composition / specialization / trace) with end markers
/// and an optional mid label.
fn render_decorated_edges(document: &mut PdfDocument, layout: &layout::Layout, translation: Vec2) {
    for edge in &layout.decorated_edges {
        if edge.points.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = edge.points.iter().map(|p| *p + translation).collect();
        let dash = if edge.dashed {
            Some((theme::CONNECTOR_DASH_LENGTH, theme::CONNECTOR_DASH_GAP))
        } else {
            None
        };
        document.push_polyline(PolylineShape {
            points: pts.clone(),
            stroke: StrokeStyle {
                color: theme::COLOR_CONNECTOR,
                width: theme::CONNECTOR_LINE_WIDTH,
                dash,
            },
        });
        let n = pts.len();
        draw_edge_marker(document, edge.start_marker, pts[0], (pts[1] - pts[0]).normalized());
        draw_edge_marker(
            document,
            edge.end_marker,
            pts[n - 1],
            (pts[n - 2] - pts[n - 1]).normalized(),
        );
        // NOTE: edge labels are NOT drawn here. They are emitted in a separate
        // pass (render_decorated_edge_labels) AFTER element boxes, so a label
        // that sits near a box edge is never overpainted by the box fill —
        // that overpaint was the "allocate" → "allo" clipping (T5).
    }

    // Port glyphs: PORT_SIZE squares straddling the box border at each
    // connector attachment (IBD notation; presentation-only, not model elements).
    for &center in &layout.port_glyphs {
        let c = center + translation;
        document.push_rect(RectShape {
            rect: Rect::from_center_size(c, Vec2::splat(theme::PORT_SIZE)),
            radius: 0.0,
            fill: Some(theme::COLOR_PORT_FILL),
            stroke: Some(StrokeStyle {
                color: theme::COLOR_ELEMENT_STROKE,
                width: theme::ELEMENT_STROKE_WIDTH,
                dash: None,
            }),
        });
    }
}

/// Draw the labels of decorated edges (satisfy / derive / allocate / verify /
/// multiplicity). Run AFTER `render_elements` so each label's opaque background
/// and text sit on top of any element box it overlaps, rather than being
/// overpainted by a box fill drawn later. That overpaint truncated the
/// `allocate` label to `allo` when the edge midpoint landed on a box edge (T5).
fn render_decorated_edge_labels(
    document: &mut PdfDocument,
    layout: &layout::Layout,
    translation: Vec2,
) {
    for edge in &layout.decorated_edges {
        if edge.points.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = edge.points.iter().map(|p| *p + translation).collect();
        let Some(lbl) = &edge.label else { continue };
        let Some(mid) = edge
            .label_pos
            .map(|p| p + translation)
            .or_else(|| polyline_midpoint(&pts))
        else {
            continue;
        };
        let label_size = theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE;
        let width = estimated_text_width(lbl, label_size);
        document.push_rect(RectShape {
            rect: Rect::from_min_size(
                Pos2::new(mid.x - width * 0.5 - 2.0, mid.y - label_size * 0.5 - 1.0),
                Vec2::new(width + 4.0, label_size + 2.0),
            ),
            radius: 0.0,
            fill: Some(theme::COLOR_ELEMENT_FILL),
            stroke: None,
        });
        document.push_text(TextShape {
            position: Pos2::new(mid.x - width * 0.5, mid.y - label_size * 0.5),
            text: lbl.clone(),
            font_size: label_size,
            color: theme::COLOR_KEYWORD_TEXT,
        });
    }
}

/// Draw an edge end marker at `at`, oriented along `dir` (unit vector pointing
/// from the attach point into the edge).
fn draw_edge_marker(document: &mut PdfDocument, marker: layout::EdgeMarker, at: Pos2, dir: Vec2) {
    use layout::EdgeMarker;
    if dir.length() < f32::EPSILON {
        return;
    }
    let u = dir.normalized();
    let perp = Vec2::new(-u.y, u.x);
    match marker {
        EdgeMarker::None => {}
        EdgeMarker::OpenArrow => {
            let prev = at + u * theme::ARROWHEAD_LENGTH;
            document.push_polyline(PolylineShape {
                points: open_arrowhead(prev, at),
                stroke: StrokeStyle {
                    color: theme::COLOR_CONNECTOR,
                    width: theme::CONNECTOR_LINE_WIDTH,
                    dash: None,
                },
            });
        }
        EdgeMarker::FilledDiamond => {
            let d = theme::ARROWHEAD_LENGTH * 1.5;
            let w = theme::ARROWHEAD_WIDTH * 0.8;
            let mid = at + u * (d * 0.5);
            let far = at + u * d;
            document.push_polygon(PolygonShape {
                points: vec![at, mid + perp * (w * 0.5), far, mid - perp * (w * 0.5)],
                fill: theme::COLOR_CONTROL_NODE,
                stroke: None,
            });
        }
        EdgeMarker::HollowTriangle => {
            let d = theme::ARROWHEAD_LENGTH * 1.4;
            let w = theme::ARROWHEAD_WIDTH * 1.3;
            let base = at + u * d;
            document.push_polygon(PolygonShape {
                points: vec![at, base + perp * (w * 0.5), base - perp * (w * 0.5)],
                fill: theme::COLOR_ELEMENT_FILL,
                stroke: Some(StrokeStyle {
                    color: theme::COLOR_ELEMENT_STROKE,
                    width: theme::ELEMENT_STROKE_WIDTH,
                    dash: None,
                }),
            });
        }
    }
}

/// Draw an activity control-node shape: a diamond (decision / merge) or a solid
/// synchronization bar (fork / join).
fn draw_control_shape(document: &mut PdfDocument, kind: ElementKind, rect: Rect) {
    match kind {
        ElementKind::DecisionNode | ElementKind::MergeNode => {
            let c = rect.center();
            document.push_polygon(PolygonShape {
                points: vec![
                    Pos2::new(c.x, rect.top()),
                    Pos2::new(rect.right(), c.y),
                    Pos2::new(c.x, rect.bottom()),
                    Pos2::new(rect.left(), c.y),
                ],
                fill: theme::COLOR_ELEMENT_FILL,
                stroke: Some(StrokeStyle {
                    color: theme::COLOR_ELEMENT_STROKE,
                    width: theme::ELEMENT_STROKE_WIDTH,
                    dash: None,
                }),
            });
        }
        ElementKind::ForkNode | ElementKind::JoinNode => {
            document.push_rect(RectShape {
                rect,
                radius: 1.5,
                fill: Some(theme::COLOR_CONTROL_NODE),
                stroke: None,
            });
        }
        _ => {}
    }
}

/// Draw the activity initial (filled disc) and final (ring + inner disc) nodes,
/// plus their decoration edges (initial->source, sink->final) with open arrowheads.
fn render_control_nodes(document: &mut PdfDocument, layout: &layout::Layout, translation: Vec2) {
    for edge in &layout.decoration_edges {
        if edge.len() < 2 {
            continue;
        }
        let points: Vec<Pos2> = edge.iter().map(|p| *p + translation).collect();
        document.push_polyline(PolylineShape {
            points: points.clone(),
            stroke: StrokeStyle {
                color: theme::COLOR_CONNECTOR,
                width: theme::CONNECTOR_LINE_WIDTH,
                dash: None,
            },
        });
        if let [.., previous, tip] = points.as_slice() {
            document.push_polyline(PolylineShape {
                points: open_arrowhead(*previous, *tip),
                stroke: StrokeStyle {
                    color: theme::COLOR_CONNECTOR,
                    width: theme::CONNECTOR_LINE_WIDTH,
                    dash: None,
                },
            });
        }
    }
    for node in &layout.control_nodes {
        let c = node.center + translation;
        let r = theme::CONTROL_NODE_RADIUS;
        match node.kind {
            layout::ControlNodeKind::Initial => {
                document.push_polygon(PolygonShape {
                    points: circle_points(c, r),
                    fill: theme::COLOR_CONTROL_NODE,
                    stroke: None,
                });
            }
            layout::ControlNodeKind::Final => {
                document.push_polygon(PolygonShape {
                    points: circle_points(c, r),
                    fill: theme::COLOR_ELEMENT_FILL,
                    stroke: Some(StrokeStyle {
                        color: theme::COLOR_CONTROL_NODE,
                        width: theme::ELEMENT_STROKE_WIDTH,
                        dash: None,
                    }),
                });
                document.push_polygon(PolygonShape {
                    points: circle_points(c, r * 0.5),
                    fill: theme::COLOR_CONTROL_NODE,
                    stroke: None,
                });
            }
        }
    }
}

fn render_elements(
    document: &mut PdfDocument,
    model: &Model,
    layout: &layout::Layout,
    translation: Vec2,
) {
    let mut ids: Vec<ElementId> = layout.positions.keys().copied().collect();
    ids.sort_by(|left, right| {
        let left_pos = layout.positions.get(left).copied().unwrap_or(Pos2::ZERO);
        let right_pos = layout.positions.get(right).copied().unwrap_or(Pos2::ZERO);
        left_pos
            .y
            .partial_cmp(&right_pos.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left_pos
                    .x
                    .partial_cmp(&right_pos.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.cmp(right))
    });

    for id in ids {
        let Some(element) = model.element(id) else {
            continue;
        };
        let Some(position) = layout.positions.get(&id).copied() else {
            continue;
        };
        let size = layout.sizes.get(&id).copied().unwrap_or(Vec2::new(
            theme::ELEMENT_MIN_WIDTH,
            theme::ELEMENT_MIN_HEIGHT,
        ));
        let rect = Rect::from_min_size(position + translation, size);
        if matches!(
            element.kind,
            ElementKind::DecisionNode
                | ElementKind::MergeNode
                | ElementKind::ForkNode
                | ElementKind::JoinNode
        ) {
            draw_control_shape(document, element.kind, rect);
            continue;
        }
        let radius = if element.kind.metatype() == Metatype::Definition {
            theme::DEFINITION_CORNER_RADIUS
        } else {
            theme::USAGE_CORNER_RADIUS
        };

        document.push_rect(RectShape {
            rect,
            radius,
            fill: Some(theme::COLOR_ELEMENT_FILL),
            stroke: Some(StrokeStyle {
                color: theme::COLOR_ELEMENT_STROKE,
                width: theme::ELEMENT_STROKE_WIDTH,
                dash: None,
            }),
        });

        let padding = theme::ELEMENT_PADDING;
        let header_height = EXPORT_ELEMENT_HEADER_HEIGHT;
        let feature_row_height = EXPORT_ELEMENT_FEATURE_ROW_HEIGHT;
        let keyword_size = fitted_font_size(
            &format!("<<{}>>", element.kind.keyword()),
            theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE,
            rect.width() - padding * 2.0,
        );
        document.push_text(TextShape {
            position: Pos2::new(rect.min.x + padding, rect.min.y + padding),
            text: format!("<<{}>>", element.kind.keyword()),
            font_size: keyword_size,
            color: theme::COLOR_KEYWORD_TEXT,
        });

        let name_line = element_name_line(element);
        let name_size = fitted_font_size(
            &name_line,
            theme::BASE_FONT_SIZE,
            rect.width() - padding * 2.0,
        );
        document.push_text(TextShape {
            position: Pos2::new(
                rect.min.x + padding,
                rect.min.y + padding + theme::KEYWORD_NAME_GAP,
            ),
            text: name_line,
            font_size: name_size,
            color: theme::COLOR_HEADER_TEXT,
        });

        let ports = model.effective_ports(id);
        let features = visible_features(model, id);
        if !ports.is_empty() || !features.is_empty() {
            let separator_y = rect.min.y + header_height;
            document.push_polyline(PolylineShape {
                points: vec![
                    Pos2::new(rect.min.x, separator_y),
                    Pos2::new(rect.max.x, separator_y),
                ],
                stroke: StrokeStyle {
                    color: theme::COLOR_ELEMENT_STROKE,
                    width: theme::COMPARTMENT_LINE_WIDTH,
                    dash: None,
                },
            });

            let mut current_y = separator_y + theme::COMPARTMENT_TEXT_INSET;
            for port_id in ports {
                let Some(port) = model.element(port_id) else {
                    continue;
                };
                let line = feature_line(port, true);
                let font_size = fitted_font_size(
                    &line,
                    theme::BASE_FONT_SIZE * theme::FEATURE_FONT_SCALE,
                    rect.width() - padding * 2.0,
                );
                document.push_text(TextShape {
                    position: Pos2::new(rect.min.x + padding, current_y),
                    text: line,
                    font_size,
                    color: theme::COLOR_FEATURE_TEXT,
                });
                current_y += feature_row_height;
            }

            for feature_id in features {
                let Some(feature) = model.element(feature_id) else {
                    continue;
                };
                let line = feature_line(feature, false);
                let font_size = fitted_font_size(
                    &line,
                    theme::BASE_FONT_SIZE * theme::FEATURE_FONT_SCALE,
                    rect.width() - padding * 2.0,
                );
                document.push_text(TextShape {
                    position: Pos2::new(rect.min.x + padding, current_y),
                    text: line,
                    font_size,
                    color: theme::COLOR_FEATURE_TEXT,
                });
                current_y += feature_row_height;
            }
        }
    }
}

fn render_ports(
    document: &mut PdfDocument,
    model: &Model,
    layout: &layout::Layout,
    translation: Vec2,
) {
    let mut port_ids: Vec<ElementId> = layout.port_positions.keys().copied().collect();
    port_ids.sort_unstable();

    for port_id in port_ids {
        let Some(position) = layout.port_positions.get(&port_id).copied() else {
            continue;
        };
        let Some(port) = model.element(port_id) else {
            continue;
        };
        let fill = if port.is_conjugated {
            theme::COLOR_PORT_CONJUGATED
        } else {
            theme::COLOR_PORT_FILL
        };
        let rect = Rect::from_center_size(position + translation, Vec2::splat(theme::PORT_SIZE));
        document.push_rect(RectShape {
            rect,
            radius: 0.0,
            fill: Some(fill),
            stroke: Some(StrokeStyle {
                color: theme::COLOR_PORT_STROKE,
                width: theme::PORT_STROKE_WIDTH,
                dash: None,
            }),
        });
    }
}

fn arrowhead_points(previous: Pos2, tip: Pos2) -> Vec<Pos2> {
    let direction = tip - previous;
    let length = direction.length();
    if length < f32::EPSILON {
        return vec![tip, tip, tip];
    }
    let unit = direction / length;
    let perpendicular = Vec2::new(-unit.y, unit.x);
    let base = tip - unit * theme::ARROWHEAD_LENGTH;
    let left = base + perpendicular * (theme::ARROWHEAD_WIDTH * 0.5);
    let right = base - perpendicular * (theme::ARROWHEAD_WIDTH * 0.5);
    vec![tip, left, right]
}

fn element_name_line(element: &sysmlv2_gui::model::Element) -> String {
    let mut line = String::new();
    // Show the short-name id (e.g. requirement "R1") ahead of the name.
    if let Some(sn) = &element.short_name
        && element.name.is_some()
    {
        let _ = write!(line, "{sn} ");
    }
    line.push_str(element.display_name());
    if let Some(type_ref) = &element.type_ref {
        let _ = write!(line, ": {type_ref}");
    }
    line
}

fn feature_line(element: &sysmlv2_gui::model::Element, is_port: bool) -> String {
    let mut line = if is_port {
        format!("port {}", element.display_name())
    } else {
        format!("{} {}", element.kind.keyword(), element.display_name())
    };

    if let Some(type_ref) = &element.type_ref {
        if is_port && element.is_conjugated {
            let _ = write!(line, ": ~{type_ref}");
        } else {
            let _ = write!(line, ": {type_ref}");
        }
    }

    if let Some(v) = &element.value {
        let shown = if v.chars().count() > 44 {
            format!("{}…", v.chars().take(44).collect::<String>())
        } else {
            v.clone()
        };
        let _ = write!(line, " = {shown}");
    }

    line
}

fn visible_features(model: &Model, element_id: ElementId) -> Vec<ElementId> {
    let Some(element) = model.element(element_id) else {
        return Vec::new();
    };

    element
        .features(model)
        .into_iter()
        .filter(|feature_id| {
            model.element(*feature_id).is_some_and(|feature| {
                matches!(
                    feature.kind,
                    ElementKind::AttributeUsage
                        | ElementKind::PartUsage
                        | ElementKind::ActionUsage
                        | ElementKind::StateUsage
                )
            })
        })
        .take(8)
        .collect()
}

fn fitted_font_size(text: &str, preferred: f32, available_width: f32) -> f32 {
    if available_width <= 0.0 {
        return PDF_MIN_FONT_SIZE;
    }
    let estimated = estimated_text_width(text, preferred);
    if estimated <= available_width {
        return preferred;
    }

    (preferred * (available_width / estimated)).max(PDF_MIN_FONT_SIZE)
}

fn compute_export_element_size(model: &Model, element_id: ElementId) -> Vec2 {
    let Some(element) = model.element(element_id) else {
        return Vec2::new(EXPORT_ELEMENT_MIN_WIDTH, EXPORT_ELEMENT_HEADER_HEIGHT);
    };

    let padding = theme::ELEMENT_PADDING;
    let keyword = format!("<<{}>>", element.kind.keyword());
    let name_line = element_name_line(element);

    let mut max_width =
        estimated_text_width(&keyword, theme::BASE_FONT_SIZE * theme::KEYWORD_FONT_SCALE)
            .max(estimated_text_width(&name_line, theme::BASE_FONT_SIZE));

    let ports = model.effective_ports(element_id);
    for port_id in &ports {
        if let Some(port) = model.element(*port_id) {
            let line = feature_line(port, true);
            max_width = max_width.max(estimated_text_width(
                &line,
                theme::BASE_FONT_SIZE * theme::FEATURE_FONT_SCALE,
            ));
        }
    }

    let features = visible_features(model, element_id);
    for feature_id in &features {
        if let Some(feature) = model.element(*feature_id) {
            let line = feature_line(feature, false);
            max_width = max_width.max(estimated_text_width(
                &line,
                theme::BASE_FONT_SIZE * theme::FEATURE_FONT_SCALE,
            ));
        }
    }

    let width = (max_width + padding * 2.0).max(EXPORT_ELEMENT_MIN_WIDTH);
    let row_count = ports.len() + features.len();
    let height = EXPORT_ELEMENT_HEADER_HEIGHT
        + if row_count > 0 {
            theme::COMPARTMENT_LINE_WIDTH
                + row_count as f32 * EXPORT_ELEMENT_FEATURE_ROW_HEIGHT
                + theme::COMPARTMENT_TEXT_INSET
                + EXPORT_FEATURE_BOTTOM_PADDING
        } else {
            padding
        };

    Vec2::new(width, height)
}

fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars().map(pdf_char_width_factor).sum::<f32>() * font_size
}

fn pdf_char_width_factor(ch: char) -> f32 {
    match ch {
        ' ' => 0.28,
        'i' | 'j' | 'l' | '!' | '.' | ',' | ':' | ';' | '\'' | '|' | '`' => 0.26,
        'f' | 'r' | 't' | '(' | ')' | '[' | ']' => 0.35,
        'm' | 'w' | 'M' | 'W' | '@' | '&' | '%' | '#' => 0.9,
        'A'..='Z' => 0.68,
        '0'..='9' => 0.56,
        '<' | '>' | '=' | '-' | '~' | '_' => 0.5,
        _ => 0.56,
    }
}

fn write_pdf(document: &PdfDocument, output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut content = String::new();
    content.push_str("1 J\n1 j\n");
    for operation in &document.operations {
        match operation {
            Operation::Rect(shape) => write_rect(&mut content, document.height, shape),
            Operation::Polyline(shape) => write_polyline(&mut content, document.height, shape),
            Operation::Polygon(shape) => write_polygon(&mut content, document.height, shape),
            Operation::Text(shape) => write_text(&mut content, document.height, shape),
        }
    }

    let mut pdf = PdfFile::new();
    let font_object = pdf.next_object_number();
    pdf.add_object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
    let contents_object = pdf.next_object_number();
    pdf.add_stream(content.as_bytes().to_vec());
    let page_object_number = pdf.next_object_number();
    let pages_object_number = page_object_number + 1;
    let page_object = pdf.add_object(format!(
        "<< /Type /Page /Parent {} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 {} 0 R >> >> /Contents {} 0 R >>",
        pages_object_number,
        format_number(document.width),
        format_number(document.height),
        font_object,
        contents_object
    ));
    let pages_object = pdf.add_object(format!(
        "<< /Type /Pages /Count 1 /Kids [{} 0 R] >>",
        page_object
    ));
    debug_assert_eq!(page_object, page_object_number);
    debug_assert_eq!(pages_object, pages_object_number);
    let catalog_object =
        pdf.add_object(format!("<< /Type /Catalog /Pages {} 0 R >>", pages_object));

    fs::write(output_path, pdf.finish(catalog_object))?;
    Ok(())
}

fn write_rect(content: &mut String, document_height: f32, shape: &RectShape) {
    if let Some(fill) = shape.fill {
        write_fill_color(content, fill);
    }
    if let Some(stroke) = &shape.stroke {
        write_stroke_style(content, stroke);
    }
    if shape.radius <= 0.0 {
        let x = shape.rect.min.x;
        let y = document_height - shape.rect.max.y;
        let width = shape.rect.width();
        let height = shape.rect.height();
        let _ = writeln!(
            content,
            "{} {} {} {} re",
            format_number(x),
            format_number(y),
            format_number(width),
            format_number(height)
        );
    } else {
        write_rounded_rect_path(content, document_height, shape.rect, shape.radius);
    }

    match (shape.fill, shape.stroke.as_ref()) {
        (Some(_), Some(_)) => content.push_str("B\n"),
        (Some(_), None) => content.push_str("f\n"),
        (None, Some(_)) => content.push_str("S\n"),
        (None, None) => {}
    }
}

fn write_polyline(content: &mut String, document_height: f32, shape: &PolylineShape) {
    if shape.points.len() < 2 {
        return;
    }
    write_stroke_style(content, &shape.stroke);
    let first = shape.points[0];
    let _ = writeln!(
        content,
        "{} {} m",
        format_number(first.x),
        format_number(document_height - first.y)
    );
    for point in &shape.points[1..] {
        let _ = writeln!(
            content,
            "{} {} l",
            format_number(point.x),
            format_number(document_height - point.y)
        );
    }
    content.push_str("S\n");
    if shape.stroke.dash.is_some() {
        content.push_str("[] 0 d\n");
    }
}

fn write_polygon(content: &mut String, document_height: f32, shape: &PolygonShape) {
    if shape.points.len() < 3 {
        return;
    }
    write_fill_color(content, shape.fill);
    if let Some(stroke) = &shape.stroke {
        write_stroke_style(content, stroke);
    }
    let first = shape.points[0];
    let _ = writeln!(
        content,
        "{} {} m",
        format_number(first.x),
        format_number(document_height - first.y)
    );
    for point in &shape.points[1..] {
        let _ = writeln!(
            content,
            "{} {} l",
            format_number(point.x),
            format_number(document_height - point.y)
        );
    }
    content.push_str("h\n");
    match shape.stroke.as_ref() {
        Some(_) => content.push_str("B\n"),
        None => content.push_str("f\n"),
    }
}

fn write_text(content: &mut String, document_height: f32, shape: &TextShape) {
    write_fill_color(content, shape.color);
    let baseline_y =
        document_height - (shape.position.y + shape.font_size * PDF_BASELINE_FROM_TOP_FACTOR);
    let escaped = escape_pdf_text(&shape.text);
    let _ = writeln!(
        content,
        "BT /F1 {} Tf 1 0 0 1 {} {} Tm ({}) Tj ET",
        format_number(shape.font_size),
        format_number(shape.position.x),
        format_number(baseline_y),
        escaped
    );
}

fn write_rounded_rect_path(content: &mut String, document_height: f32, rect: Rect, radius: f32) {
    let radius = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let kappa = 0.552_284_8_f32;

    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let bottom = rect.bottom();

    let _ = writeln!(
        content,
        "{} {} m",
        format_number(left + radius),
        format_number(document_height - top)
    );
    let _ = writeln!(
        content,
        "{} {} l",
        format_number(right - radius),
        format_number(document_height - top)
    );
    write_curve(
        content,
        document_height,
        Pos2::new(right - radius + radius * kappa, top),
        Pos2::new(right, top + radius - radius * kappa),
        Pos2::new(right, top + radius),
    );
    let _ = writeln!(
        content,
        "{} {} l",
        format_number(right),
        format_number(document_height - (bottom - radius))
    );
    write_curve(
        content,
        document_height,
        Pos2::new(right, bottom - radius + radius * kappa),
        Pos2::new(right - radius + radius * kappa, bottom),
        Pos2::new(right - radius, bottom),
    );
    let _ = writeln!(
        content,
        "{} {} l",
        format_number(left + radius),
        format_number(document_height - bottom)
    );
    write_curve(
        content,
        document_height,
        Pos2::new(left + radius - radius * kappa, bottom),
        Pos2::new(left, bottom - radius + radius * kappa),
        Pos2::new(left, bottom - radius),
    );
    let _ = writeln!(
        content,
        "{} {} l",
        format_number(left),
        format_number(document_height - (top + radius))
    );
    write_curve(
        content,
        document_height,
        Pos2::new(left, top + radius - radius * kappa),
        Pos2::new(left + radius - radius * kappa, top),
        Pos2::new(left + radius, top),
    );
    content.push_str("h\n");
}

fn write_curve(content: &mut String, document_height: f32, c1: Pos2, c2: Pos2, end: Pos2) {
    let _ = writeln!(
        content,
        "{} {} {} {} {} {} c",
        format_number(c1.x),
        format_number(document_height - c1.y),
        format_number(c2.x),
        format_number(document_height - c2.y),
        format_number(end.x),
        format_number(document_height - end.y)
    );
}

fn write_fill_color(content: &mut String, color: Color32) {
    let _ = writeln!(
        content,
        "{} {} {} rg",
        format_number(color.r() as f32 / 255.0),
        format_number(color.g() as f32 / 255.0),
        format_number(color.b() as f32 / 255.0)
    );
}

fn write_stroke_style(content: &mut String, stroke: &StrokeStyle) {
    let _ = writeln!(content, "{} w", format_number(stroke.width));
    let _ = writeln!(
        content,
        "{} {} {} RG",
        format_number(stroke.color.r() as f32 / 255.0),
        format_number(stroke.color.g() as f32 / 255.0),
        format_number(stroke.color.b() as f32 / 255.0)
    );
    if let Some((dash, gap)) = stroke.dash {
        let _ = writeln!(
            content,
            "[{} {}] 0 d",
            format_number(dash),
            format_number(gap)
        );
    } else {
        content.push_str("[] 0 d\n");
    }
}

fn escape_pdf_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn format_number(value: f32) -> String {
    let mut text = format!("{value:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text.is_empty() {
        "0".to_string()
    } else {
        text
    }
}

struct PdfDocument {
    width: f32,
    height: f32,
    operations: Vec<Operation>,
}

impl PdfDocument {
    fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            operations: Vec::new(),
        }
    }

    fn push_rect(&mut self, shape: RectShape) {
        self.operations.push(Operation::Rect(shape));
    }

    fn push_polyline(&mut self, shape: PolylineShape) {
        self.operations.push(Operation::Polyline(shape));
    }

    fn push_polygon(&mut self, shape: PolygonShape) {
        self.operations.push(Operation::Polygon(shape));
    }

    fn push_text(&mut self, shape: TextShape) {
        self.operations.push(Operation::Text(shape));
    }
}

enum Operation {
    Rect(RectShape),
    Polyline(PolylineShape),
    Polygon(PolygonShape),
    Text(TextShape),
}

struct RectShape {
    rect: Rect,
    radius: f32,
    fill: Option<Color32>,
    stroke: Option<StrokeStyle>,
}

struct PolylineShape {
    points: Vec<Pos2>,
    stroke: StrokeStyle,
}

struct PolygonShape {
    points: Vec<Pos2>,
    fill: Color32,
    stroke: Option<StrokeStyle>,
}

struct TextShape {
    position: Pos2,
    text: String,
    font_size: f32,
    color: Color32,
}

#[derive(Clone)]
struct StrokeStyle {
    color: Color32,
    width: f32,
    dash: Option<(f32, f32)>,
}

struct PdfFile {
    objects: Vec<Vec<u8>>,
}

impl PdfFile {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn next_object_number(&self) -> usize {
        self.objects.len() + 1
    }

    fn add_object(&mut self, object: String) -> usize {
        self.objects.push(object.into_bytes());
        self.objects.len()
    }

    fn add_stream(&mut self, bytes: Vec<u8>) -> usize {
        let mut object = format!("<< /Length {} >>\nstream\n", bytes.len()).into_bytes();
        object.extend(bytes);
        object.extend(b"\nendstream");
        self.objects.push(object);
        self.objects.len()
    }

    fn finish(self, root_object: usize) -> Vec<u8> {
        let mut output = b"%PDF-1.4\n%\xFF\xFF\xFF\xFF\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len() + 1);
        offsets.push(0_usize);

        for (index, object) in self.objects.iter().enumerate() {
            let object_number = index + 1;
            offsets.push(output.len());
            let _ = writeln!(output, "{object_number} 0 obj");
            output.extend(object);
            output.extend(b"\nendobj\n");
        }

        let xref_offset = output.len();
        let _ = writeln!(output, "xref");
        let _ = writeln!(output, "0 {}", self.objects.len() + 1);
        output.extend(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            let _ = writeln!(output, "{offset:010} 00000 n ");
        }
        let _ = writeln!(
            output,
            "trailer << /Size {} /Root {} 0 R >>",
            self.objects.len() + 1,
            root_object
        );
        let _ = writeln!(output, "startxref");
        let _ = writeln!(output, "{xref_offset}");
        output.extend(b"%%EOF\n");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 1: parsing a valid JSON spec array yields the correct ViewSpec.
    #[test]
    fn spec_parse_single_entry() {
        let json = r#"[{"file_stem":"x","context_name":"P","frame_label":"bdd","kind":"bdd"}]"#;
        let entries: Vec<SpecEntry> = serde_json::from_str(json).expect("should parse");
        assert_eq!(entries.len(), 1);
        let kind = parse_kind(&entries[0].kind).expect("bdd should map");
        assert!(matches!(kind, ViewKind::Bdd));
        assert_eq!(entries[0].file_stem, "x");
        assert_eq!(entries[0].context_name, "P");
    }

    /// Test 2: every kind string round-trips to the corresponding ViewKind;
    /// an unknown kind string returns Err, not a panic.
    #[test]
    fn kind_round_trip() {
        let cases = [
            ("general", matches!(parse_kind("general"), Ok(ViewKind::General))),
            ("interconnection", matches!(parse_kind("interconnection"), Ok(ViewKind::Interconnection))),
            ("action", matches!(parse_kind("action"), Ok(ViewKind::ActionFlow))),
            ("state", matches!(parse_kind("state"), Ok(ViewKind::StateTransition))),
            ("bdd", matches!(parse_kind("bdd"), Ok(ViewKind::Bdd))),
            ("requirements", matches!(parse_kind("requirements"), Ok(ViewKind::Requirements))),
            ("traceability", matches!(parse_kind("traceability"), Ok(ViewKind::Traceability))),
        ];
        for (name, ok) in cases {
            assert!(ok, "kind {name:?} should parse successfully");
        }
        let err = parse_kind("unknown_kind");
        assert!(err.is_err(), "unknown kind should return Err, got {err:?}");
    }

    /// Test 3: malformed JSON returns Err whose message names the spec path.
    #[test]
    fn malformed_spec_file_names_path() {
        // Write a temp file with invalid JSON.
        let dir = std::env::temp_dir();
        let path = dir.join("bad-spec-07-01-test.json");
        std::fs::write(&path, b"not json at all").expect("write temp file");
        let result = load_spec_file(path.to_str().unwrap());
        assert!(result.is_err(), "malformed JSON should return Err");
        let err_msg = result.err().unwrap().to_string();
        // Error message must mention the spec path.
        assert!(
            err_msg.contains("bad-spec-07-01-test.json"),
            "error should name the spec path, got: {err_msg:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
