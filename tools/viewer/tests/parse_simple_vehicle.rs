use sysmlv2_gui::model::ElementKind;
use sysmlv2_gui::model::parse::parse_sysml;

fn load_model() -> sysmlv2_gui::model::Model {
    let content =
        std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").expect("fixture file");
    parse_sysml(&content).expect("parse should succeed")
}

#[test]
fn root_package_is_simple_vehicle_model() {
    let model = load_model();
    assert_eq!(
        model.root_ids.len(),
        1,
        "should have exactly one root package"
    );
    let root = model.element(model.root_ids[0]).unwrap();
    assert_eq!(root.kind, ElementKind::Package);
    assert_eq!(root.name.as_deref(), Some("SimpleVehicleModel"));
}

#[test]
fn has_part_definitions() {
    let model = load_model();
    let part_defs = model.elements_of_kind(ElementKind::PartDef);
    assert!(
        part_defs.len() >= 30,
        "expected >= 30 part defs, got {}",
        part_defs.len()
    );
}

#[test]
fn has_port_definitions() {
    let model = load_model();
    let port_defs = model.elements_of_kind(ElementKind::PortDef);
    assert!(
        port_defs.len() >= 20,
        "expected >= 20 port defs, got {}",
        port_defs.len()
    );
}

#[test]
fn vehicle_part_def_has_ports() {
    let model = load_model();
    let idx = model.name_index();
    let vehicle_ids = idx.get("Vehicle").expect("Vehicle should exist");
    let vehicle = vehicle_ids
        .iter()
        .find_map(|&id| {
            let e = model.element(id)?;
            (e.kind == ElementKind::PartDef).then_some(e)
        })
        .expect("Vehicle part def should exist");
    let ports = vehicle.ports(&model);
    assert!(
        ports.len() >= 4,
        "Vehicle should have >= 4 ports, got {}",
        ports.len()
    );
}

#[test]
fn vehicle_b_exists_as_usage() {
    let model = load_model();
    let idx = model.name_index();
    let vb_ids = idx.get("vehicle_b").expect("vehicle_b should exist");
    let vb = vb_ids
        .iter()
        .find_map(|&id| {
            let e = model.element(id)?;
            (e.kind == ElementKind::PartUsage).then_some(e)
        })
        .expect("vehicle_b should be a part usage");
    assert!(!vb.children.is_empty(), "vehicle_b should have children");
}

#[test]
fn vehicle_b_has_relationships() {
    let model = load_model();
    let idx = model.name_index();
    let vb_id = idx.get("vehicle_b").expect("vehicle_b should exist")[0];
    let vb_rels: Vec<_> = model
        .relationships
        .iter()
        .filter(|r| r.owner == vb_id)
        .collect();
    assert!(!vb_rels.is_empty(), "vehicle_b should have relationships");
}

#[test]
fn parse_does_not_panic_or_error() {
    // The parser must handle the full 1579-line model without errors
    let content =
        std::fs::read_to_string("tests/fixtures/simple-vehicle.sysml").expect("fixture file");
    let result = parse_sysml(&content);
    assert!(result.is_ok(), "parser should not return error");
}

#[test]
fn model_has_enum_definitions() {
    let model = load_model();
    let enums = model.elements_of_kind(ElementKind::EnumDef);
    assert!(
        enums.len() >= 3,
        "expected >= 3 enum defs, got {}",
        enums.len()
    );
}

#[test]
fn model_has_action_definitions() {
    let model = load_model();
    let actions = model.elements_of_kind(ElementKind::ActionDef);
    assert!(!actions.is_empty(), "should have action definitions");
}

#[test]
fn model_has_state_definitions() {
    let model = load_model();
    let states = model.elements_of_kind(ElementKind::StateDef);
    let state_usages = model.elements_of_kind(ElementKind::StateUsage);
    assert!(
        !states.is_empty() || !state_usages.is_empty(),
        "should have state definitions or usages"
    );
}
