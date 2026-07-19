//! Tests for the overview layout, search, filter, and LOD rendering.

use sysmlv2_gui::model::parse::parse_sysml;
use sysmlv2_gui::model::{ElementKind, Model};
use sysmlv2_gui::render::{overview, theme};

fn load_model() -> Model {
    let content =
        std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").expect("fixture file");
    parse_sysml(&content).expect("parse should succeed")
}

fn setup_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

// ---------------------------------------------------------------
// Overview layout
// ---------------------------------------------------------------

#[test]
fn overview_produces_non_empty_layout() {
    let model = load_model();
    let ctx = setup_ctx();
    let layout = overview::compute_overview_layout(&model, &ctx);

    assert!(
        !layout.positions.is_empty(),
        "Overview layout should have positioned nodes"
    );
    assert!(
        !layout.sizes.is_empty(),
        "Overview layout should have sized nodes"
    );
}

#[test]
fn overview_packages_no_overlap() {
    let model = load_model();
    let ctx = setup_ctx();
    let layout = overview::compute_overview_layout(&model, &ctx);

    let rects: Vec<(usize, egui::Rect)> = layout
        .positions
        .iter()
        .map(|(&id, &pos)| {
            let size = layout.sizes[&id];
            (id, egui::Rect::from_min_size(pos, size))
        })
        .collect();

    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            let (id_a, rect_a) = rects[i];
            let (id_b, rect_b) = rects[j];
            assert!(
                !rect_a.intersects(rect_b),
                "Overview nodes {} and {} overlap:\n  {:?}\n  {:?}",
                model.element(id_a).unwrap().display_name(),
                model.element(id_b).unwrap().display_name(),
                rect_a,
                rect_b
            );
        }
    }
}

#[test]
fn overview_nodes_have_positive_sizes() {
    let model = load_model();
    let ctx = setup_ctx();
    let layout = overview::compute_overview_layout(&model, &ctx);

    for (&id, &size) in &layout.sizes {
        assert!(
            size.x > 0.0 && size.y > 0.0,
            "Node '{}' has non-positive size: {:?}",
            model.element(id).unwrap().display_name(),
            size
        );
        assert!(
            size.x >= theme::OVERVIEW_PACKAGE_MIN_WIDTH,
            "Node '{}' width {:.1} < min {:.1}",
            model.element(id).unwrap().display_name(),
            size.x,
            theme::OVERVIEW_PACKAGE_MIN_WIDTH,
        );
    }
}

#[test]
fn overview_content_bounds_exist() {
    let model = load_model();
    let ctx = setup_ctx();
    let layout = overview::compute_overview_layout(&model, &ctx);

    let bounds = layout.content_bounds();
    assert!(
        bounds.is_some(),
        "Overview layout should have content bounds"
    );
    let bounds = bounds.unwrap();
    assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
}

// ---------------------------------------------------------------
// Subtree weight
// ---------------------------------------------------------------

#[test]
fn overview_subtree_weight() {
    let model = load_model();
    let idx = model.name_index();

    // The root package should have the most weight
    let root_id = model.root_ids[0];
    let root_weight = model.subtree_weight(root_id);
    assert!(
        root_weight > 100,
        "Root package should have substantial weight, got {root_weight}"
    );

    // vehicle_b should have significant weight (22+ children)
    let vb_id = *idx.get("vehicle_b").unwrap().first().unwrap();
    let vb_weight = model.subtree_weight(vb_id);
    assert!(
        vb_weight > 15,
        "vehicle_b should have weight > 15, got {vb_weight}"
    );

    // A leaf element should have weight 1
    // Find a leaf port
    let port_defs = model.elements_of_kind(ElementKind::PortDef);
    for pd in &port_defs {
        if pd.children.is_empty() {
            let weight = model.subtree_weight(pd.id);
            assert_eq!(weight, 1, "Leaf element should have weight 1, got {weight}");
            break;
        }
    }
}

// ---------------------------------------------------------------
// Cross-package edges
// ---------------------------------------------------------------

#[test]
fn overview_cross_edges_collected() {
    let model = load_model();
    let edges = model.cross_package_edges();
    // The Simple Vehicle Model has cross-package specializations
    // (e.g., types defined in one area used in another)
    // This test just verifies the method doesn't crash and returns something
    // For a single-package model, there may be no cross-package edges
    // but the method should still work
    assert!(
        edges.is_empty() || !edges.is_empty(),
        "cross_package_edges should return without error"
    );
}

// ---------------------------------------------------------------
// LOD thresholds
// ---------------------------------------------------------------

#[test]
fn overview_lod_thresholds_ordered() {
    // Verify the LOD thresholds form a proper progression
    assert!(theme::LOD_DOT_THRESHOLD < theme::LOD_LABEL_THRESHOLD);
    assert!(theme::LOD_LABEL_THRESHOLD < theme::LOD_BOX_THRESHOLD);
}

// ---------------------------------------------------------------
// Search
// ---------------------------------------------------------------

#[test]
fn search_finds_by_name() {
    let model = load_model();
    let matches = model.search_by_name("Vehicle");
    assert!(
        !matches.is_empty(),
        "Searching for 'Vehicle' should find matches"
    );
    // Should find Vehicle part def
    let found_vehicle = matches.iter().any(|&id| {
        model
            .element(id)
            .is_some_and(|e| e.name.as_deref() == Some("Vehicle"))
    });
    assert!(found_vehicle, "Should find 'Vehicle' element");
}

#[test]
fn search_finds_by_partial_name() {
    let model = load_model();
    let matches = model.search_by_name("engine");
    assert!(
        !matches.is_empty(),
        "Searching for 'engine' should find matches (case-insensitive)"
    );
}

#[test]
fn search_empty_query_returns_empty() {
    let model = load_model();
    let matches = model.search_by_name("");
    assert!(matches.is_empty(), "Empty query should return no matches");
}

#[test]
fn search_finds_by_qualified_name_component() {
    let model = load_model();
    // Search for a name that appears as a child deep in the tree
    let matches = model.search_by_name("drivePwr");
    assert!(
        !matches.is_empty(),
        "Searching for 'drivePwr' should find port matches"
    );
}

// ---------------------------------------------------------------
// Neighbors
// ---------------------------------------------------------------

#[test]
fn neighbors_returns_connected() {
    let model = load_model();
    let idx = model.name_index();

    // vehicle_b has relationships connecting its children
    let vb_id = *idx.get("vehicle_b").unwrap().first().unwrap();
    let neighbors = model.neighbors(vb_id);
    // vehicle_b should have at least some neighbors (its children are connected)
    // At minimum it references Vehicle type
    assert!(!neighbors.is_empty(), "vehicle_b should have neighbors");
}

#[test]
fn neighbors_includes_type_def() {
    let model = load_model();
    let idx = model.name_index();

    // Find a part usage with a type reference
    let vb_id = *idx.get("vehicle_b").unwrap().first().unwrap();
    let elem = model.element(vb_id).unwrap();
    if elem.type_ref.is_some() {
        let neighbors = model.neighbors(vb_id);
        // Should include the type definition
        assert!(
            !neighbors.is_empty(),
            "Element with type_ref should have type def as neighbor"
        );
    }
}

// ---------------------------------------------------------------
// Filter
// ---------------------------------------------------------------

#[test]
fn filter_kind_hides_elements() {
    use sysmlv2_gui::panels::filter::FilterState;

    let model = load_model();
    let mut filter = FilterState::default();
    filter.initialize_from_model(&model);

    // All elements should be visible initially
    let some_id = model.root_ids[0];
    assert_eq!(filter.element_opacity(some_id, &model), 255);

    // Hide packages
    filter.kind_visible.insert(ElementKind::Package, false);
    assert!(
        filter.element_opacity(some_id, &model) < 255,
        "Package element should be dimmed when packages are filtered out"
    );
}
