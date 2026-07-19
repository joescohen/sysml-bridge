//! Tests for the faceted query model.

use sysmlv2_gui::model::query::{Query, QueryTerm};
use sysmlv2_gui::model::{Element, ElementKind, Model};

/// Helper: build a small model for query tests.
fn test_model() -> Model {
    let mut model = Model::new();

    // Package: VehicleSystems
    let pkg_id = model.add_element(Element {
        id: 0,
        kind: ElementKind::Package,
        name: Some("VehicleSystems".into()),
        short_name: None,
        type_ref: None,
        specializes: vec![],
        multiplicity: None,
        doc: None,
        parent: None,
        children: vec![],
        is_conjugated: false,
        is_abstract: false,
        is_variation: false,
        qualifiers: vec![],
    });
    model.root_ids.push(pkg_id);

    // Part def: EngineAssembly (abstract, has doc)
    let engine_def_id = model.add_element(Element {
        id: 0,
        kind: ElementKind::PartDef,
        name: Some("EngineAssembly".into()),
        short_name: None,
        type_ref: None,
        specializes: vec!["MechanicalAssembly".into()],
        multiplicity: None,
        doc: Some("The main engine assembly for thermal management".into()),
        parent: Some(pkg_id),
        children: vec![],
        is_conjugated: false,
        is_abstract: true,
        is_variation: false,
        qualifiers: vec![],
    });

    // Part usage: engine : EngineAssembly (with port child)
    let engine_usage_id = model.add_element(Element {
        id: 0,
        kind: ElementKind::PartUsage,
        name: Some("engine".into()),
        short_name: None,
        type_ref: Some("EngineAssembly".into()),
        specializes: vec![],
        multiplicity: None,
        doc: None,
        parent: Some(pkg_id),
        children: vec![],
        is_conjugated: false,
        is_abstract: false,
        is_variation: false,
        qualifiers: vec!["redefines".into()],
    });

    // Port: fuelPort on engine
    let port_id = model.add_element(Element {
        id: 0,
        kind: ElementKind::PortUsage,
        name: Some("fuelPort".into()),
        short_name: None,
        type_ref: Some("FuelPort".into()),
        specializes: vec![],
        multiplicity: None,
        doc: None,
        parent: Some(engine_usage_id),
        children: vec![],
        is_conjugated: false,
        is_abstract: false,
        is_variation: false,
        qualifiers: vec![],
    });

    // Attribute: weight
    let attr_id = model.add_element(Element {
        id: 0,
        kind: ElementKind::AttributeUsage,
        name: Some("weight".into()),
        short_name: None,
        type_ref: Some("Real".into()),
        specializes: vec![],
        multiplicity: None,
        doc: None,
        parent: Some(engine_usage_id),
        children: vec![],
        is_conjugated: false,
        is_abstract: false,
        is_variation: false,
        qualifiers: vec![],
    });

    // Action: startEngine
    let action_id = model.add_element(Element {
        id: 0,
        kind: ElementKind::ActionUsage,
        name: Some("startEngine".into()),
        short_name: None,
        type_ref: None,
        specializes: vec![],
        multiplicity: None,
        doc: None,
        parent: Some(pkg_id),
        children: vec![],
        is_conjugated: false,
        is_abstract: false,
        is_variation: false,
        qualifiers: vec![],
    });

    // Wire up children
    model.element_mut(pkg_id).unwrap().children = vec![engine_def_id, engine_usage_id, action_id];
    model.element_mut(engine_usage_id).unwrap().children = vec![port_id, attr_id];

    model
}

// --- Query parser tests ---

#[test]
fn parse_bare_word() {
    let q = Query::parse("engine");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Name(s) if s == "engine"));
}

#[test]
fn parse_kind_prefix() {
    let q = Query::parse("kind:part");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Kind(s) if s == "part"));
}

#[test]
fn parse_in_prefix() {
    let q = Query::parse("in:VehicleSystems");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::InPackage(s) if s == "vehiclesystems"));
}

#[test]
fn parse_type_prefix() {
    let q = Query::parse("type:EngineAssembly");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::TypeRef(s) if s == "engineassembly"));
}

#[test]
fn parse_specializes() {
    let q = Query::parse(":>Vehicle");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Specializes(s) if s == "vehicle"));
}

#[test]
fn parse_has_prefix() {
    let q = Query::parse("has:port");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::HasFeature(s) if s == "port"));
}

#[test]
fn parse_doc_prefix() {
    let q = Query::parse("doc:thermal");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Doc(s) if s == "thermal"));
}

#[test]
fn parse_abstract_yes() {
    let q = Query::parse("abstract:yes");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Abstract(true)));
}

#[test]
fn parse_abstract_no() {
    let q = Query::parse("abstract:no");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Abstract(false)));
}

#[test]
fn parse_qualifier() {
    let q = Query::parse("qual:redefines");
    assert_eq!(q.terms.len(), 1);
    assert!(matches!(&q.terms[0], QueryTerm::Qualifier(s) if s == "redefines"));
}

#[test]
fn parse_multiple_terms() {
    let q = Query::parse("kind:part engine");
    assert_eq!(q.terms.len(), 2);
    assert!(matches!(&q.terms[0], QueryTerm::Kind(s) if s == "part"));
    assert!(matches!(&q.terms[1], QueryTerm::Name(s) if s == "engine"));
}

#[test]
fn parse_empty_query() {
    let q = Query::parse("");
    assert!(q.is_empty());
}

// --- Query matching tests ---

#[test]
fn query_name_match() {
    let model = test_model();
    let results = model.query(&Query::parse("engine"));
    // Should match: EngineAssembly, engine, startEngine
    assert_eq!(results.len(), 3);
}

#[test]
fn query_name_case_insensitive() {
    let model = test_model();
    let results = model.query(&Query::parse("ENGINE"));
    assert_eq!(results.len(), 3);
}

#[test]
fn query_kind_part() {
    let model = test_model();
    let results = model.query(&Query::parse("kind:part"));
    // PartDef (EngineAssembly) + PartUsage (engine) — keyword "part def" and "part" both contain "part"
    assert_eq!(results.len(), 2);
}

#[test]
fn query_kind_def_metatype() {
    let model = test_model();
    let results = model.query(&Query::parse("kind:def"));
    // Package, PartDef — all definitions
    assert_eq!(results.len(), 2);
}

#[test]
fn query_kind_usage_metatype() {
    let model = test_model();
    let results = model.query(&Query::parse("kind:usage"));
    // PartUsage, PortUsage, AttributeUsage, ActionUsage
    assert_eq!(results.len(), 4);
}

#[test]
fn query_in_package() {
    let model = test_model();
    let results = model.query(&Query::parse("in:vehiclesystems"));
    // All elements under VehicleSystems (including VehicleSystems itself)
    assert!(results.len() >= 4);
}

#[test]
fn query_type_ref() {
    let model = test_model();
    let results = model.query(&Query::parse("type:engineassembly"));
    // engine : EngineAssembly
    assert_eq!(results.len(), 1);
}

#[test]
fn query_specializes() {
    let model = test_model();
    let results = model.query(&Query::parse(":>Mechanical"));
    // EngineAssembly specializes MechanicalAssembly
    assert_eq!(results.len(), 1);
}

#[test]
fn query_has_port() {
    let model = test_model();
    let results = model.query(&Query::parse("has:port"));
    // engine has fuelPort
    assert_eq!(results.len(), 1);
}

#[test]
fn query_has_attribute() {
    let model = test_model();
    let results = model.query(&Query::parse("has:attribute"));
    // engine has weight
    assert_eq!(results.len(), 1);
}

#[test]
fn query_doc() {
    let model = test_model();
    let results = model.query(&Query::parse("doc:thermal"));
    // EngineAssembly has "thermal management" in doc
    assert_eq!(results.len(), 1);
}

#[test]
fn query_abstract() {
    let model = test_model();
    let results = model.query(&Query::parse("abstract:yes"));
    // EngineAssembly is abstract
    assert_eq!(results.len(), 1);
}

#[test]
fn query_qualifier() {
    let model = test_model();
    let results = model.query(&Query::parse("qual:redefines"));
    // engine has redefines qualifier
    assert_eq!(results.len(), 1);
}

#[test]
fn query_combined_kind_and_name() {
    let model = test_model();
    let results = model.query(&Query::parse("kind:part engine"));
    // PartDef "EngineAssembly" and PartUsage "engine" — both match "engine" in name
    // and "part" in keyword
    assert_eq!(results.len(), 2);
}

#[test]
fn query_empty_returns_nothing() {
    let model = test_model();
    let results = model.query(&Query::parse(""));
    assert!(results.is_empty());
}

#[test]
fn query_no_matches() {
    let model = test_model();
    let results = model.query(&Query::parse("nonexistent"));
    assert!(results.is_empty());
}
