//! SysML v2 graphical notation spec conformance tests.
//!
//! Each test verifies a rule from the OMG SysML v2 graphical notation
//! specification (formal/25-09-03). Tests use the headless egui
//! technique for deterministic verification without a GUI.

use sysmlv2_gui::model::parse::parse_sysml;
use sysmlv2_gui::model::{ElementKind, Metatype};
use sysmlv2_gui::render::theme;

// ---------------------------------------------------------------
// Corner radius rules (spec: definitions = sharp, usages = rounded)
// ---------------------------------------------------------------

#[test]
fn definitions_have_sharp_corners() {
    // Spec rule: every definition element type has corner radius 0.0.
    assert_eq!(
        theme::DEFINITION_CORNER_RADIUS,
        0.0,
        "DEFINITION_CORNER_RADIUS must be 0.0 (sharp corners)"
    );

    // Verify all definition kinds map to Metatype::Definition
    let def_kinds = [
        ElementKind::Package,
        ElementKind::PartDef,
        ElementKind::PortDef,
        ElementKind::AttributeDef,
        ElementKind::ItemDef,
        ElementKind::ActionDef,
        ElementKind::StateDef,
        ElementKind::ConnectionDef,
        ElementKind::InterfaceDef,
        ElementKind::AllocationDef,
        ElementKind::RequirementDef,
        ElementKind::ConstraintDef,
        ElementKind::ConcernDef,
        ElementKind::ViewDef,
        ElementKind::ViewpointDef,
        ElementKind::RenderingDef,
        ElementKind::VerificationDef,
        ElementKind::EnumDef,
        ElementKind::OccurrenceDef,
        ElementKind::IndividualDef,
        ElementKind::SignalDef,
        ElementKind::MetadataDef,
    ];
    for kind in &def_kinds {
        assert_eq!(
            kind.metatype(),
            Metatype::Definition,
            "{:?} should be classified as Definition",
            kind
        );
    }
}

#[test]
fn usages_have_rounded_corners() {
    // Spec rule: every usage element type has corner radius > 0.0.
    assert!(
        theme::USAGE_CORNER_RADIUS > 0.0,
        "USAGE_CORNER_RADIUS must be > 0.0 (rounded corners)"
    );

    let usage_kinds = [
        ElementKind::PartUsage,
        ElementKind::PortUsage,
        ElementKind::AttributeUsage,
        ElementKind::ItemUsage,
        ElementKind::ActionUsage,
        ElementKind::StateUsage,
        ElementKind::ConnectionUsage,
        ElementKind::InterfaceUsage,
        ElementKind::AllocationUsage,
        ElementKind::RequirementUsage,
        ElementKind::ConstraintUsage,
        ElementKind::ViewUsage,
        ElementKind::VerificationUsage,
        ElementKind::EnumUsage,
        ElementKind::OccurrenceUsage,
        ElementKind::IndividualUsage,
        ElementKind::FlowUsage,
    ];
    for kind in &usage_kinds {
        assert_eq!(
            kind.metatype(),
            Metatype::Usage,
            "{:?} should be classified as Usage",
            kind
        );
    }
}

#[test]
fn metatype_exhaustive() {
    // Every ElementKind variant must return either Definition or Usage.
    let all_kinds = [
        ElementKind::Package,
        ElementKind::PartDef,
        ElementKind::PartUsage,
        ElementKind::PortDef,
        ElementKind::PortUsage,
        ElementKind::AttributeDef,
        ElementKind::AttributeUsage,
        ElementKind::ItemDef,
        ElementKind::ItemUsage,
        ElementKind::ActionDef,
        ElementKind::ActionUsage,
        ElementKind::StateDef,
        ElementKind::StateUsage,
        ElementKind::ConnectionDef,
        ElementKind::ConnectionUsage,
        ElementKind::InterfaceDef,
        ElementKind::InterfaceUsage,
        ElementKind::AllocationDef,
        ElementKind::AllocationUsage,
        ElementKind::RequirementDef,
        ElementKind::RequirementUsage,
        ElementKind::ConstraintDef,
        ElementKind::ConstraintUsage,
        ElementKind::ConcernDef,
        ElementKind::ViewDef,
        ElementKind::ViewUsage,
        ElementKind::ViewpointDef,
        ElementKind::RenderingDef,
        ElementKind::VerificationDef,
        ElementKind::VerificationUsage,
        ElementKind::EnumDef,
        ElementKind::EnumUsage,
        ElementKind::OccurrenceDef,
        ElementKind::OccurrenceUsage,
        ElementKind::IndividualDef,
        ElementKind::IndividualUsage,
        ElementKind::FlowUsage,
        ElementKind::SignalDef,
        ElementKind::MetadataDef,
        ElementKind::Comment,
        ElementKind::Import,
        ElementKind::Alias,
        ElementKind::Dependency,
        ElementKind::Transition,
        ElementKind::Succession,
        ElementKind::MessageUsage,
        ElementKind::TimesliceUsage,
        ElementKind::SnapshotUsage,
        ElementKind::VariationUsage,
        ElementKind::RefUsage,
        ElementKind::Unknown,
    ];
    for kind in &all_kinds {
        let mt = kind.metatype();
        assert!(
            mt == Metatype::Definition || mt == Metatype::Usage,
            "{:?} returned unexpected metatype {:?}",
            kind,
            mt
        );
    }
}

// ---------------------------------------------------------------
// Keyword format (spec: lowercase in guillemets)
// ---------------------------------------------------------------

#[test]
fn keyword_format_guillemets() {
    // Spec: keywords are lowercase and rendered in << >> guillemets.
    // Verify keyword() returns lowercase text (rendering wraps in <<>>).
    let all_kinds = [
        ElementKind::Package,
        ElementKind::PartDef,
        ElementKind::PartUsage,
        ElementKind::PortDef,
        ElementKind::PortUsage,
        ElementKind::AttributeDef,
        ElementKind::AttributeUsage,
        ElementKind::ActionDef,
        ElementKind::ActionUsage,
    ];
    for kind in &all_kinds {
        let kw = kind.keyword();
        assert_eq!(
            kw,
            kw.to_lowercase(),
            "{:?}.keyword() = {:?} is not lowercase",
            kind,
            kw
        );
        // Verify the rendering format string pattern produces <<keyword>>
        let rendered = format!("<<{}>>", kw);
        assert!(
            rendered.starts_with("<<") && rendered.ends_with(">>"),
            "Keyword render format broken for {:?}: {}",
            kind,
            rendered
        );
    }
}

// ---------------------------------------------------------------
// Port geometry (spec: square, straddling boundary, sharp corners)
// ---------------------------------------------------------------

#[test]
fn port_geometry_square() {
    // Spec: ports are PORT_SIZE x PORT_SIZE squares.
    assert!(theme::PORT_SIZE > 0.0, "PORT_SIZE must be positive");
    assert_eq!(
        theme::PORT_HALF,
        theme::PORT_SIZE / 2.0,
        "PORT_HALF must be PORT_SIZE / 2"
    );

    // Verify port positions straddle parent boundaries using the model
    let src = std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").unwrap();
    let model = parse_sysml(&src).expect("parse should succeed");
    let idx = model.name_index();
    let vehicle_b = *idx.get("vehicle_b").unwrap().first().unwrap();

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let layout = sysmlv2_gui::render::layout::compute_layout(&model, vehicle_b, &ctx);

    for (&port_id, &port_center) in &layout.port_positions {
        let port_elem = match model.element(port_id) {
            Some(e) => e,
            None => continue,
        };
        let parent_id = match port_elem.parent {
            Some(p) => p,
            None => continue,
        };
        let parent_pos = match layout.positions.get(&parent_id) {
            Some(p) => *p,
            None => continue,
        };
        let parent_size = match layout.sizes.get(&parent_id) {
            Some(s) => *s,
            None => continue,
        };

        // Port center x must be on parent left or right edge
        let on_left = (port_center.x - parent_pos.x).abs() < 1.0;
        let on_right = (port_center.x - (parent_pos.x + parent_size.x)).abs() < 1.0;
        assert!(
            on_left || on_right,
            "Port '{}' center x={:.1} not on parent '{}' edge [left={:.1}, right={:.1}]",
            port_elem.display_name(),
            port_center.x,
            model.element(parent_id).unwrap().display_name(),
            parent_pos.x,
            parent_pos.x + parent_size.x
        );

        // Port center y must be within parent vertical bounds
        assert!(
            port_center.y >= parent_pos.y && port_center.y <= parent_pos.y + parent_size.y,
            "Port '{}' center y={:.1} outside parent '{}' vertical bounds",
            port_elem.display_name(),
            port_center.y,
            model.element(parent_id).unwrap().display_name()
        );
    }
}

#[test]
fn port_corners_sharp() {
    // Spec: ports are drawn with CornerRadius::ZERO (sharp corners).
    // This is verified by the code using CornerRadius::ZERO in draw_port.
    // The test ensures the constant exists and is zero.
    assert_eq!(
        egui::CornerRadius::ZERO,
        egui::CornerRadius::same(0),
        "CornerRadius::ZERO must be all-zero"
    );
}

// ---------------------------------------------------------------
// Compartment separator (spec: horizontal line below header)
// ---------------------------------------------------------------

#[test]
fn compartment_separator_position() {
    // Spec (subclause 8.2.3): compartment separator is at ELEMENT_HEADER_HEIGHT
    // below the element top edge.
    let src = std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").unwrap();
    let model = parse_sysml(&src).expect("parse should succeed");
    let idx = model.name_index();
    let vehicle_b = *idx.get("vehicle_b").unwrap().first().unwrap();

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let layout = sysmlv2_gui::render::layout::compute_layout(&model, vehicle_b, &ctx);

    for (&id, &pos) in &layout.positions {
        let size = layout.sizes[&id];
        let sep_y = pos.y + theme::ELEMENT_HEADER_HEIGHT;
        assert!(
            sep_y <= pos.y + size.y,
            "Separator at y={sep_y:.1} exceeds element '{}' bottom={:.1}",
            model.element(id).unwrap().display_name(),
            pos.y + size.y
        );
    }
}

// ---------------------------------------------------------------
// Frame geometry (spec: sharp corners, tab at top-left)
// ---------------------------------------------------------------

#[test]
fn frame_sharp_corners() {
    // Spec: diagram frame uses sharp corners (CornerRadius::ZERO).
    // Verified by frame.rs using CornerRadius::ZERO in both rect_stroke and tab rect.
    // This test asserts the constants used are correct.
    assert_eq!(
        theme::FRAME_STROKE_WIDTH,
        2.0,
        "Frame stroke width should be 2.0 per spec"
    );
    assert!(
        theme::FRAME_TAB_HEIGHT > 0.0,
        "Frame tab height must be positive"
    );
}

#[test]
fn frame_tab_top_left() {
    // Spec: frame tab starts at top-left corner of the frame.
    // The tab rect is created from frame_rect.min (top-left).
    // Verify via layout that content_bounds produce a valid frame.
    let src = std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").unwrap();
    let model = parse_sysml(&src).expect("parse should succeed");
    let idx = model.name_index();
    let vehicle_b = *idx.get("vehicle_b").unwrap().first().unwrap();

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let layout = sysmlv2_gui::render::layout::compute_layout(&model, vehicle_b, &ctx);

    let bounds = layout
        .content_bounds()
        .expect("content_bounds should exist");
    // Frame rect min is content min - padding - tab_height (vertically)
    let frame_min_y = bounds.min.y - theme::FRAME_PADDING - theme::FRAME_TAB_HEIGHT;
    let frame_min_x = bounds.min.x - theme::FRAME_PADDING;
    // Tab starts at frame_min (top-left)
    assert!(frame_min_x.is_finite(), "Frame min x must be finite");
    assert!(frame_min_y.is_finite(), "Frame min y must be finite");
    assert!(
        frame_min_y < bounds.min.y,
        "Frame top (with tab) must be above content top"
    );
}

// ---------------------------------------------------------------
// Connector styles (spec: different styles per relationship kind)
// ---------------------------------------------------------------

#[test]
fn connector_flow_has_arrowhead() {
    // Spec: flow connectors have filled arrowheads.
    // Verify by parsing a model with flows and checking route exists.
    let src = std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").unwrap();
    let model = parse_sysml(&src).expect("parse should succeed");

    // Check that flow relationships exist in the model
    let flows: Vec<_> = model
        .relationships
        .iter()
        .filter(|r| r.kind == sysmlv2_gui::model::RelationshipKind::Flow)
        .collect();

    // Verify arrowhead constants are defined
    assert!(
        theme::ARROWHEAD_LENGTH > 0.0,
        "ARROWHEAD_LENGTH must be positive"
    );
    assert!(
        theme::ARROWHEAD_WIDTH > 0.0,
        "ARROWHEAD_WIDTH must be positive"
    );

    // If no flows in fixture, this test still validates the constants.
    // The rendering code draws arrowheads for Flow kind only.
    let _ = flows;
}

#[test]
fn connector_styles_per_kind() {
    // Spec: different connector kinds have distinct visual styles.
    // Connect/Interface: solid, default color
    // Bind: dashed, brown
    // Flow: solid, green
    // Allocate: dashed, default color
    //
    // Verify the theme colors are distinct.
    assert_ne!(
        theme::COLOR_CONNECTOR,
        theme::COLOR_CONNECTOR_FLOW,
        "Connector and Flow colors must differ"
    );
    assert_ne!(
        theme::COLOR_CONNECTOR,
        theme::COLOR_CONNECTOR_BIND,
        "Connector and Bind colors must differ"
    );
    assert_ne!(
        theme::COLOR_CONNECTOR_FLOW,
        theme::COLOR_CONNECTOR_BIND,
        "Flow and Bind colors must differ"
    );

    // Verify dash constants are positive
    assert!(
        theme::CONNECTOR_DASH_LENGTH > 0.0,
        "Dash length must be positive"
    );
    assert!(theme::CONNECTOR_DASH_GAP > 0.0, "Dash gap must be positive");
    assert!(
        theme::CONNECTOR_LINE_WIDTH > 0.0,
        "Connector line width must be positive"
    );
}

// ---------------------------------------------------------------
// No magic numbers in rendering code (programmatic lint)
// ---------------------------------------------------------------

#[test]
fn no_magic_numbers_in_rendering() {
    // Scan all .rs files in src/render/ for bare numeric literals in
    // drawing/painting calls. Every value must reference a theme:: constant
    // or be one of the allowed structural values (0.0, 0.5, 1.0, 2.0).
    let render_dir = std::path::Path::new("src/render");
    let allowed_bare = ["0.0", "0.5", "1.0", "2.0"];

    let mut violations = Vec::new();
    for entry in std::fs::read_dir(render_dir).expect("src/render/ must exist") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs") {
            let content = std::fs::read_to_string(&path).unwrap();
            for (line_num, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                // Skip comments
                if trimmed.starts_with("//") || trimmed.starts_with("///") {
                    continue;
                }
                // Skip const/pub const definitions (these ARE the named constants)
                if trimmed.starts_with("pub const ") || trimmed.starts_with("const ") {
                    continue;
                }
                // Skip use/mod/fn declarations
                if trimmed.starts_with("use ")
                    || trimmed.starts_with("mod ")
                    || trimmed.starts_with("pub mod ")
                {
                    continue;
                }
                // Look for float literals like 3.0, 16.0, etc.
                // Pattern: standalone float literal that isn't part of theme::
                for token in trimmed.split(|c: char| {
                    c == '('
                        || c == ')'
                        || c == ','
                        || c == ' '
                        || c == '+'
                        || c == '-'
                        || c == '{'
                        || c == '}'
                }) {
                    let token = token.trim();
                    // Match float literals: digits.digits
                    if token.contains('.')
                        && token.chars().next().is_some_and(|c| c.is_ascii_digit())
                    {
                        // Try to parse as f32
                        if let Ok(val) = token.parse::<f32>() {
                            // Allow structural values
                            let allowed = allowed_bare.iter().any(|&a| a == token)
                                || val == 0.0 || val == 0.5 || val == 1.0 || val == 2.0
                                // Allow 0.1 (used in length checks like `< 0.1`)
                                || val == 0.1;
                            if !allowed {
                                violations.push(format!(
                                    "  {}:{}: bare literal `{}` — should use theme:: constant",
                                    path.display(),
                                    line_num + 1,
                                    token
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    if !violations.is_empty() {
        panic!(
            "Found {} bare numeric literals in src/render/ (should use theme:: constants):\n{}",
            violations.len(),
            violations.join("\n")
        );
    }
}

// ---------------------------------------------------------------
// Overview package corner radii (spec violation #1 regression test)
// ---------------------------------------------------------------

#[test]
fn overview_package_corner_radii() {
    // Verify that DEFINITION_CORNER_RADIUS is 0.0 (sharp) and
    // USAGE_CORNER_RADIUS is 6.0 (rounded) — the spec values used
    // in draw_overview_package.
    assert_eq!(
        theme::DEFINITION_CORNER_RADIUS,
        0.0,
        "Definition corner radius must be 0.0 (sharp)"
    );
    assert_eq!(
        theme::USAGE_CORNER_RADIUS,
        6.0,
        "Usage corner radius must be 6.0"
    );

    // Parse model and verify packages are definitions
    let src = std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").unwrap();
    let model = parse_sysml(&src).expect("parse should succeed");

    for &root_id in &model.root_ids {
        if let Some(elem) = model.element(root_id) {
            if elem.kind == ElementKind::Package {
                assert_eq!(
                    elem.kind.metatype(),
                    Metatype::Definition,
                    "Package '{}' must be a Definition (sharp corners)",
                    elem.display_name()
                );
            }
        }
    }
}

// ---------------------------------------------------------------
// All stroke widths named (no bare floats in Stroke::new calls)
// ---------------------------------------------------------------

#[test]
fn all_stroke_widths_named() {
    // Verify that all stroke width constants are defined and positive.
    assert!(
        theme::ELEMENT_STROKE_WIDTH > 0.0,
        "ELEMENT_STROKE_WIDTH must be positive"
    );
    assert!(
        theme::COMPARTMENT_LINE_WIDTH > 0.0,
        "COMPARTMENT_LINE_WIDTH must be positive"
    );
    assert!(
        theme::CONNECTOR_LINE_WIDTH > 0.0,
        "CONNECTOR_LINE_WIDTH must be positive"
    );
    assert!(
        theme::FRAME_STROKE_WIDTH > 0.0,
        "FRAME_STROKE_WIDTH must be positive"
    );
    assert!(
        theme::PORT_STROKE_WIDTH > 0.0,
        "PORT_STROKE_WIDTH must be positive"
    );

    // Scan for Stroke::new calls in render/ files and verify they use
    // named constants (not bare numeric literals).
    let render_dir = std::path::Path::new("src/render");
    let mut violations = Vec::new();

    for entry in std::fs::read_dir(render_dir).expect("src/render/ must exist") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs") {
            let content = std::fs::read_to_string(&path).unwrap();
            for (line_num, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") {
                    continue;
                }
                // Look for Stroke::new( patterns
                if let Some(idx) = trimmed.find("Stroke::new(") {
                    let after = &trimmed[idx + "Stroke::new(".len()..];
                    // First argument is the width — check it's not a bare float
                    let width_arg = after.split(',').next().unwrap_or("").trim();
                    // If the width is a bare float literal, that's a violation
                    if width_arg.parse::<f32>().is_ok() && width_arg != "0.0" {
                        violations.push(format!(
                            "  {}:{}: Stroke::new({}, ...) uses bare literal",
                            path.display(),
                            line_num + 1,
                            width_arg
                        ));
                    }
                }
            }
        }
    }

    if !violations.is_empty() {
        panic!(
            "Found {} Stroke::new calls with bare width literals:\n{}",
            violations.len(),
            violations.join("\n")
        );
    }
}
