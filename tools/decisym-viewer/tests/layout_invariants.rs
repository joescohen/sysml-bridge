//! Headless layout invariant tests.
//!
//! These tests verify rendering correctness without a GUI by using
//! `egui::Context::default()` for deterministic font measurement.
//! This catches layout and zoom-scaling bugs that would otherwise
//! require visual inspection.
//!
//! Technique: egui::Context works headlessly — it initialises fonts
//! and can measure text without a window or graphics context. We
//! parse a model, compute layout, and assert geometric properties
//! that must hold for the diagram to render correctly.

use sysmlv2_gui::model::parse::parse_sysml;
use sysmlv2_gui::render::{layout, theme};

/// Set up: parse the Simple Vehicle Model and return the model,
/// the vehicle_b element id, and a headless egui context.
///
/// egui requires `Context::run()` to be called once before fonts are
/// available. We bootstrap with a dummy frame so that `fonts_mut()`
/// works for text measurement in the layout engine and tests.
fn setup() -> (sysmlv2_gui::model::Model, usize, egui::Context) {
    let src = std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").unwrap();
    let model = parse_sysml(&src).expect("parse should succeed");
    let idx = model.name_index();
    let vehicle_b = *idx
        .get("vehicle_b")
        .expect("vehicle_b not found")
        .first()
        .unwrap();

    let ctx = egui::Context::default();
    // Bootstrap fonts by running a dummy frame
    let _ = ctx.run(egui::RawInput::default(), |_ctx| {});
    (model, vehicle_b, ctx)
}

/// Measure text width using the same method as the layout engine.
fn measure_text(ctx: &egui::Context, text: &str, font: &egui::FontId) -> f32 {
    ctx.fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::BLACK)
            .rect
            .width()
    })
}

// ---------------------------------------------------------------
// Layout correctness at base zoom
// ---------------------------------------------------------------

#[test]
fn element_header_text_fits_within_boxes() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);
    let base_font = egui::FontId::proportional(13.0);

    for (&id, &size) in &base_layout.sizes {
        let elem = model.element(id).unwrap();
        let available = size.x - theme::ELEMENT_PADDING * 2.0;

        // Header name line (widest header text)
        let name = elem.display_name();
        let type_str = elem
            .type_ref
            .as_deref()
            .map(|t| format!(": {t}"))
            .unwrap_or_default();
        let name_line = format!("{name}{type_str}");
        let text_width = measure_text(&ctx, &name_line, &base_font);

        // If element hit ELEMENT_MAX_WIDTH, text may be clipped (by design)
        if size.x < theme::ELEMENT_MAX_WIDTH {
            assert!(
                text_width <= available + 0.5,
                "Element '{}': header '{name_line}' ({text_width:.1}px) exceeds available width ({available:.1}px)",
                elem.display_name()
            );
        }
    }
}

#[test]
fn element_feature_text_fits_within_boxes() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);
    let feature_font = egui::FontId::proportional(13.0 * theme::FEATURE_FONT_SCALE);

    for (&id, &size) in &base_layout.sizes {
        let elem = model.element(id).unwrap();
        let available = size.x - theme::ELEMENT_PADDING * 2.0;

        // Port labels
        for &pid in &elem.ports(&model) {
            if let Some(p) = model.element(pid) {
                let conj = if p.is_conjugated { "~" } else { "" };
                let tref = p
                    .type_ref
                    .as_deref()
                    .map(|t| format!(": {conj}{t}"))
                    .unwrap_or_default();
                let label = format!("port {}{}", p.display_name(), tref);
                let w = measure_text(&ctx, &label, &feature_font);

                if size.x < theme::ELEMENT_MAX_WIDTH {
                    assert!(
                        w <= available + 0.5,
                        "Element '{}' port label '{label}' ({w:.1}px) exceeds available ({available:.1}px)",
                        elem.display_name()
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------
// Element geometry
// ---------------------------------------------------------------

#[test]
fn separator_within_element_bounds() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    for (&id, &pos) in &base_layout.positions {
        let size = base_layout.sizes[&id];
        let sep_y = pos.y + theme::ELEMENT_HEADER_HEIGHT;

        assert!(
            sep_y >= pos.y && sep_y <= pos.y + size.y,
            "Element '{}': separator y={sep_y:.1} outside bounds [{:.1}, {:.1}]",
            model.element(id).unwrap().display_name(),
            pos.y,
            pos.y + size.y
        );
    }
}

#[test]
fn no_element_overlap() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    let rects: Vec<(usize, egui::Rect)> = base_layout
        .positions
        .iter()
        .map(|(&id, &pos)| {
            let size = base_layout.sizes[&id];
            (id, egui::Rect::from_min_size(pos, size))
        })
        .collect();

    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            let (id_a, rect_a) = rects[i];
            let (id_b, rect_b) = rects[j];
            assert!(
                !rect_a.intersects(rect_b),
                "Elements '{}' and '{}' overlap:\n  {:?}\n  {:?}",
                model.element(id_a).unwrap().display_name(),
                model.element(id_b).unwrap().display_name(),
                rect_a,
                rect_b
            );
        }
    }
}

#[test]
fn ports_on_element_boundaries() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    for (&port_id, &port_pos) in &base_layout.port_positions {
        let port_elem = match model.element(port_id) {
            Some(e) => e,
            None => continue,
        };
        let parent_id = match port_elem.parent {
            Some(p) => p,
            None => continue,
        };
        let parent_pos = match base_layout.positions.get(&parent_id) {
            Some(p) => *p,
            None => continue,
        };
        let parent_size = match base_layout.sizes.get(&parent_id) {
            Some(s) => *s,
            None => continue,
        };

        // Port x should be on left or right edge of parent
        let on_left = (port_pos.x - parent_pos.x).abs() < 1.0;
        let on_right = (port_pos.x - (parent_pos.x + parent_size.x)).abs() < 1.0;
        assert!(
            on_left || on_right,
            "Port '{}' at x={:.1} not on parent '{}' boundary [left={:.1}, right={:.1}]",
            port_elem.display_name(),
            port_pos.x,
            model.element(parent_id).unwrap().display_name(),
            parent_pos.x,
            parent_pos.x + parent_size.x
        );

        // Port y should be within parent bounds
        assert!(
            port_pos.y >= parent_pos.y && port_pos.y <= parent_pos.y + parent_size.y,
            "Port '{}' at y={:.1} outside parent '{}' bounds [{:.1}, {:.1}]",
            port_elem.display_name(),
            port_pos.y,
            model.element(parent_id).unwrap().display_name(),
            parent_pos.y,
            parent_pos.y + parent_size.y
        );
    }
}

// ---------------------------------------------------------------
// Zoom scaling invariants
//
// These verify that at any zoom level, the relationship between
// layout sizes and text sizes is preserved. The layout is computed
// once at base zoom, then sizes are scaled by zoom. Text is measured
// at the zoomed font size. The invariant is:
//   text_width(zoom*font) <= zoom * element_width - 2 * zoom * padding
// ---------------------------------------------------------------

#[test]
fn text_fits_at_multiple_zoom_levels() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    for zoom in [0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0] {
        let font = egui::FontId::proportional(13.0 * zoom);

        for (&id, &size) in &base_layout.sizes {
            let elem = model.element(id).unwrap();
            let scaled_width = size.x * zoom;
            let available = scaled_width - theme::ELEMENT_PADDING * 2.0 * zoom;

            // Skip elements clamped to max width (clipping handles overflow)
            if size.x >= theme::ELEMENT_MAX_WIDTH {
                continue;
            }

            // Header name line
            let name = elem.display_name();
            let type_str = elem
                .type_ref
                .as_deref()
                .map(|t| format!(": {t}"))
                .unwrap_or_default();
            let name_line = format!("{name}{type_str}");
            let text_width = measure_text(&ctx, &name_line, &font);

            // Font rasterization is not perfectly linear across sizes:
            // text_width(3*font) may differ from 3*text_width(font) by a
            // few pixels due to hinting/rounding. The clip rect in
            // draw_element handles this visually. Allow proportional
            // tolerance to distinguish this from systematic layout bugs.
            let tolerance = 1.0 + zoom;
            assert!(
                text_width <= available + tolerance,
                "zoom={zoom}: '{}' header '{name_line}' ({text_width:.1}px) exceeds available ({available:.1}px)",
                elem.display_name()
            );
        }
    }
}

#[test]
fn separator_within_bounds_at_all_zoom_levels() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    for zoom in [0.3, 0.5, 1.0, 2.0, 5.0] {
        for (&id, _) in &base_layout.positions {
            let size = base_layout.sizes[&id];

            // This replicates what draw_element does: sep_y = pos.y + HEADER_HEIGHT * zoom
            // The scaled box height is size.y * zoom.
            let sep_y_offset = theme::ELEMENT_HEADER_HEIGHT * zoom;
            let box_height = size.y * zoom;

            assert!(
                sep_y_offset <= box_height,
                "zoom={zoom}: Element '{}': separator offset {sep_y_offset:.1} exceeds box height {box_height:.1}",
                model.element(id).unwrap().display_name()
            );
        }
    }
}

// ---------------------------------------------------------------
// Connector routing
// ---------------------------------------------------------------

#[test]
fn connector_routes_have_valid_endpoints() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    // Every routed connector should have at least 2 points
    for route in &base_layout.connector_routes {
        assert!(
            route.points.len() >= 2,
            "Connector route for relationship {} has fewer than 2 points",
            route.rel_index
        );

        // Endpoints should be finite
        for pt in &route.points {
            assert!(
                pt.x.is_finite() && pt.y.is_finite(),
                "Connector has non-finite point: {:?}",
                pt
            );
        }
    }

    // At least some connectors should be routed for vehicle_b
    assert!(
        !base_layout.connector_routes.is_empty(),
        "No connectors routed for vehicle_b"
    );
}

#[test]
fn connector_segments_do_not_cross_element_interiors() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    let rects: Vec<(usize, egui::Rect)> = base_layout
        .positions
        .iter()
        .map(|(&id, &pos)| {
            let size = base_layout.sizes[&id];
            (id, egui::Rect::from_min_size(pos, size))
        })
        .collect();

    for route in &base_layout.connector_routes {
        for window in route.points.windows(2) {
            let (p0, p1) = (window[0], window[1]);
            // For each horizontal segment, check it doesn't pass through
            // the interior of any element (touching the boundary is fine).
            if (p0.y - p1.y).abs() < 1.0 {
                let min_x = p0.x.min(p1.x);
                let max_x = p0.x.max(p1.x);
                let y = p0.y;
                for &(id, rect) in &rects {
                    // Segment is inside the element's vertical range
                    if y > rect.top() + 1.0 && y < rect.bottom() - 1.0 {
                        // Segment passes through the element's horizontal span
                        let enters = min_x > rect.left() + 1.0 && min_x < rect.right() - 1.0;
                        let exits = max_x > rect.left() + 1.0 && max_x < rect.right() - 1.0;
                        let spans = min_x <= rect.left() + 1.0 && max_x >= rect.right() - 1.0;
                        assert!(
                            !enters && !exits && !spans,
                            "Connector for rel {} has horizontal segment ({:.0},{:.0})→({:.0},{:.0}) \
                             crossing interior of '{}' {:?}",
                            route.rel_index,
                            p0.x,
                            p0.y,
                            p1.x,
                            p1.y,
                            model.element(id).unwrap().display_name(),
                            rect
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn connector_routes_inside_content_bounds() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    let bounds = base_layout
        .content_bounds()
        .expect("content_bounds should exist for vehicle_b");

    for route in &base_layout.connector_routes {
        for (i, pt) in route.points.iter().enumerate() {
            assert!(
                pt.x >= bounds.min.x - 0.5
                    && pt.x <= bounds.max.x + 0.5
                    && pt.y >= bounds.min.y - 0.5
                    && pt.y <= bounds.max.y + 0.5,
                "Connector rel {} waypoint {i} ({:.1},{:.1}) outside content_bounds {:?}",
                route.rel_index,
                pt.x,
                pt.y,
                bounds
            );
        }
    }
}

#[test]
fn parallel_horizontal_segments_separated() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    // Collect DETOURED horizontal segments: (min_x, max_x, y, connector_index).
    // Port-level segments (at the y of the route's start or end point) are
    // inherently at fixed port heights and cannot be deconflicted, so skip them.
    let mut h_segments: Vec<(f32, f32, f32, usize)> = Vec::new();
    for (ci, route) in base_layout.connector_routes.iter().enumerate() {
        let start_y = route.points.first().map(|p| p.y).unwrap_or(0.0);
        let end_y = route.points.last().map(|p| p.y).unwrap_or(0.0);
        for w in route.points.windows(2) {
            if (w[0].y - w[1].y).abs() < 0.5 {
                let y = w[0].y;
                // Skip port-level segments (at endpoint heights)
                if (y - start_y).abs() < 1.0 || (y - end_y).abs() < 1.0 {
                    continue;
                }
                let min_x = w[0].x.min(w[1].x);
                let max_x = w[0].x.max(w[1].x);
                if (max_x - min_x) > 1.0 {
                    h_segments.push((min_x, max_x, y, ci));
                }
            }
        }
    }

    // For segments from DIFFERENT connectors that overlap in x,
    // verify they are separated by at least ROUTE_PARALLEL_SPACING.
    for i in 0..h_segments.len() {
        for j in (i + 1)..h_segments.len() {
            let (a_min, a_max, a_y, a_ci) = h_segments[i];
            let (b_min, b_max, b_y, b_ci) = h_segments[j];
            if a_ci == b_ci {
                continue; // same connector, skip
            }
            // Check x-overlap
            if a_min < b_max && b_min < a_max {
                let gap = (a_y - b_y).abs();
                // Allow tiny tolerance for floating point
                assert!(
                    gap >= theme::ROUTE_PARALLEL_SPACING - 0.5 || gap < 0.5,
                    "Overlapping h-segments from connectors {a_ci} and {b_ci} \
                     at y={a_y:.1} and y={b_y:.1} (gap={gap:.1}px) are closer than \
                     ROUTE_PARALLEL_SPACING ({:.1}px)",
                    theme::ROUTE_PARALLEL_SPACING
                );
            }
        }
    }
}

#[test]
fn connector_routes_clear_of_frame_tab() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    // Compute the frame tab position from ELEMENT positions only (not routes).
    // The frame rect min_y = element_min_y - FRAME_PADDING - FRAME_TAB_HEIGHT.
    // Tab bottom = element_min_y - FRAME_PADDING.
    // Routes must stay below this to avoid visual overlap with the tab.
    let element_min_y = base_layout
        .positions
        .values()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);
    if element_min_y == f32::INFINITY {
        return;
    }
    let tab_bottom = element_min_y - theme::FRAME_PADDING;

    for route in &base_layout.connector_routes {
        for (i, pt) in route.points.iter().enumerate() {
            assert!(
                pt.y > tab_bottom + 0.5,
                "Connector rel {} waypoint {i} at y={:.1} is within the element-based frame tab area \
                 (tab_bottom={tab_bottom:.1})",
                route.rel_index,
                pt.y
            );
        }
    }
}

#[test]
fn debug_layout_positions() {
    let (model, vehicle_b, ctx) = setup();
    let base_layout = layout::compute_layout(&model, vehicle_b, &ctx);

    // --- Element positions and sizes ---
    println!("\n=== ELEMENT POSITIONS AND SIZES ===");
    let mut elems: Vec<_> = base_layout.positions.iter().collect();
    elems.sort_by(|a, b| {
        a.1.x
            .partial_cmp(&b.1.x)
            .unwrap()
            .then(a.1.y.partial_cmp(&b.1.y).unwrap())
    });
    for &(&id, &pos) in &elems {
        let size = base_layout
            .sizes
            .get(&id)
            .copied()
            .unwrap_or(egui::vec2(0.0, 0.0));
        let name = model.element(id).map(|e| e.display_name()).unwrap_or("?");
        let kind = model
            .element(id)
            .map(|e| format!("{:?}", e.kind))
            .unwrap_or_default();
        println!(
            "  [{:>3}] {:<30} {:>20}  pos=({:>7.1}, {:>7.1})  size=({:>6.1} x {:>6.1})  right_edge={:>7.1}  bottom={:>7.1}",
            id,
            name,
            kind,
            pos.x,
            pos.y,
            size.x,
            size.y,
            pos.x + size.x,
            pos.y + size.y
        );
    }

    // --- Column analysis (reconstruct column gaps) ---
    println!("\n=== COLUMN ANALYSIS ===");
    let mut ranges: Vec<(f32, f32, String)> = base_layout
        .positions
        .iter()
        .filter_map(|(&id, &pos)| {
            base_layout.sizes.get(&id).map(|&s| {
                let name = model.element(id).map(|e| e.display_name()).unwrap_or("?");
                (pos.x, pos.x + s.x, name.to_string())
            })
        })
        .collect();
    ranges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    // Merge overlapping ranges into columns
    let mut columns: Vec<(f32, f32, Vec<String>)> = Vec::new();
    for (left, right, name) in &ranges {
        if let Some(last) = columns.last_mut() {
            if *left <= last.1 + theme::ROUTE_COLUMN_MERGE {
                last.1 = last.1.max(*right);
                last.2.push(name.clone());
                continue;
            }
        }
        columns.push((*left, *right, vec![name.clone()]));
    }

    for (i, (left, right, names)) in columns.iter().enumerate() {
        println!(
            "  Column {i}: x=[{left:.1}, {right:.1}] width={:.1}  elements: {}",
            right - left,
            names.join(", ")
        );
    }

    // Column gaps
    println!("\n=== COLUMN GAPS ===");
    let mut gaps = Vec::new();
    if !columns.is_empty() {
        gaps.push(columns[0].0 - theme::ROUTE_COLUMN_GAP_MARGIN);
        for w in columns.windows(2) {
            let gap_x = (w[0].1 + w[1].0) / 2.0;
            let gap_width = w[1].0 - w[0].1;
            println!(
                "  Gap between col {} and col {}: center_x={gap_x:.1}  width={gap_width:.1}",
                columns.iter().position(|c| c.0 == w[0].0).unwrap(),
                columns.iter().position(|c| c.0 == w[1].0).unwrap()
            );
            gaps.push(gap_x);
        }
        gaps.push(columns.last().unwrap().1 + theme::ROUTE_COLUMN_GAP_MARGIN);
        println!("  Left margin gap: x={:.1}", gaps[0]);
        println!("  Right margin gap: x={:.1}", *gaps.last().unwrap());
    }

    // --- Port positions ---
    println!("\n=== PORT POSITIONS ===");
    let mut ports: Vec<_> = base_layout.port_positions.iter().collect();
    ports.sort_by(|a, b| {
        a.1.x
            .partial_cmp(&b.1.x)
            .unwrap()
            .then(a.1.y.partial_cmp(&b.1.y).unwrap())
    });
    for &(&id, &pos) in &ports {
        let name = model.element(id).map(|e| e.display_name()).unwrap_or("?");
        let kind = model
            .element(id)
            .map(|e| format!("{:?}", e.kind))
            .unwrap_or_default();
        let type_ref = model
            .element(id)
            .and_then(|e| e.type_ref.as_deref())
            .unwrap_or("-");
        let conj = model
            .element(id)
            .map(|e| if e.is_conjugated { " (conjugated)" } else { "" })
            .unwrap_or("");
        println!(
            "  [{:>3}] {:<25} {:<18} type={:<20} pos=({:>7.1}, {:>7.1}){}",
            id, name, kind, type_ref, pos.x, pos.y, conj
        );
    }

    // --- Connector routes ---
    println!(
        "\n=== CONNECTOR ROUTES ({} total) ===",
        base_layout.connector_routes.len()
    );
    for (i, route) in base_layout.connector_routes.iter().enumerate() {
        let rel = &model.relationships[route.rel_index];
        println!(
            "  Route {i}: rel[{}] {} -> {}",
            route.rel_index, rel.source_path, rel.target_path
        );
        for (j, pt) in route.points.iter().enumerate() {
            let dir = if j > 0 {
                let prev = route.points[j - 1];
                if (pt.x - prev.x).abs() < 0.5 {
                    format!("vertical   dy={:>+7.1}", pt.y - prev.y)
                } else if (pt.y - prev.y).abs() < 0.5 {
                    format!("horizontal dx={:>+7.1}", pt.x - prev.x)
                } else {
                    format!(
                        "diagonal   dx={:>+7.1} dy={:>+7.1}",
                        pt.x - prev.x,
                        pt.y - prev.y
                    )
                }
            } else {
                "start".to_string()
            };
            println!("    [{j}] ({:>7.1}, {:>7.1})  {dir}", pt.x, pt.y);
        }
        // Total route length
        let total_len: f32 = route
            .points
            .windows(2)
            .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
            .sum();
        println!(
            "    total length: {total_len:.1}px  segments: {}",
            route.points.len() - 1
        );
    }

    // --- Content bounds ---
    if let Some(bounds) = base_layout.content_bounds() {
        println!("\n=== CONTENT BOUNDS ===");
        println!(
            "  min=({:.1}, {:.1})  max=({:.1}, {:.1})  size=({:.1} x {:.1})",
            bounds.min.x,
            bounds.min.y,
            bounds.max.x,
            bounds.max.y,
            bounds.width(),
            bounds.height()
        );
    }

    println!("\n=== THEME CONSTANTS ===");
    println!("  ELEMENT_H_SPACING={}", theme::ELEMENT_H_SPACING);
    println!("  ELEMENT_V_SPACING={}", theme::ELEMENT_V_SPACING);
    println!("  FRAME_PADDING={}", theme::FRAME_PADDING);
    println!("  FRAME_TAB_HEIGHT={}", theme::FRAME_TAB_HEIGHT);
    println!("  ROUTE_FRAME_MARGIN={}", theme::ROUTE_FRAME_MARGIN);
    println!(
        "  ROUTE_COLUMN_GAP_MARGIN={}",
        theme::ROUTE_COLUMN_GAP_MARGIN
    );
    println!("  ROUTE_STUB_LENGTH={}", theme::ROUTE_STUB_LENGTH);
    println!(
        "  ROUTE_ELEMENT_CLEARANCE={}",
        theme::ROUTE_ELEMENT_CLEARANCE
    );
    println!("  ROUTE_CHANNEL_SPREAD={}", theme::ROUTE_CHANNEL_SPREAD);
    println!("  ROUTE_PARALLEL_SPACING={}", theme::ROUTE_PARALLEL_SPACING);
}
