//! Faceted query model for searching SysML elements.
//!
//! A query is a sequence of terms. Bare words match the name facet.
//! Prefixed terms match specific facets:
//!
//! ```text
//! engine                — name contains "engine"
//! kind:part             — ElementKind keyword contains "part"
//! kind:def              — all definitions (metatype filter)
//! kind:usage            — all usages
//! in:VehicleSystems     — qualified path contains "VehicleSystems"
//! type:EngineAssembly   — type_ref contains "EngineAssembly"
//! :>Vehicle             — specializes "Vehicle"
//! has:port              — has port children
//! doc:thermal           — doc contains "thermal"
//! abstract:yes          — abstract elements
//! qual:redefines        — qualifier contains "redefines"
//! ```

use crate::model::{ElementId, ElementKind, Metatype, Model};

/// A parsed search query: a conjunction of faceted terms.
#[derive(Debug, Clone)]
pub struct Query {
    pub terms: Vec<QueryTerm>,
}

/// A single faceted search term.
#[derive(Debug, Clone)]
pub enum QueryTerm {
    /// Bare word: name or short_name substring.
    Name(String),
    /// `kind:part` — keyword substring, or `kind:def`/`kind:usage` for metatype.
    Kind(String),
    /// `in:VehicleSystems` — qualified path substring.
    InPackage(String),
    /// `type:EngineAssembly` — type_ref substring.
    TypeRef(String),
    /// `:>Vehicle` — specializes substring.
    Specializes(String),
    /// `has:port` — has children of a specific kind.
    HasFeature(String),
    /// `doc:thermal` — doc substring.
    Doc(String),
    /// `abstract:yes` or `abstract:no`.
    Abstract(bool),
    /// `qual:redefines` — qualifier substring.
    Qualifier(String),
}

impl Query {
    /// Parse a query string into a sequence of terms.
    pub fn parse(input: &str) -> Self {
        let terms = input
            .split_whitespace()
            .map(|token| {
                if let Some(rest) = token.strip_prefix("kind:") {
                    QueryTerm::Kind(rest.to_lowercase())
                } else if let Some(rest) = token.strip_prefix("in:") {
                    QueryTerm::InPackage(rest.to_lowercase())
                } else if let Some(rest) = token.strip_prefix("type:") {
                    QueryTerm::TypeRef(rest.to_lowercase())
                } else if let Some(rest) = token.strip_prefix(":>") {
                    QueryTerm::Specializes(rest.to_lowercase())
                } else if let Some(rest) = token.strip_prefix("has:") {
                    QueryTerm::HasFeature(rest.to_lowercase())
                } else if let Some(rest) = token.strip_prefix("doc:") {
                    QueryTerm::Doc(rest.to_lowercase())
                } else if let Some(rest) = token.strip_prefix("abstract:") {
                    QueryTerm::Abstract(rest.eq_ignore_ascii_case("yes"))
                } else if let Some(rest) = token.strip_prefix("qual:") {
                    QueryTerm::Qualifier(rest.to_lowercase())
                } else {
                    QueryTerm::Name(token.to_lowercase())
                }
            })
            .collect();
        Self { terms }
    }

    /// Whether this query is empty (no terms).
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

impl Model {
    /// Execute a faceted query, returning all element IDs matching ALL terms.
    pub fn query(&self, q: &Query) -> Vec<ElementId> {
        if q.is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        for id in 0..self.element_count() {
            if let Some(elem) = self.element(id)
                && q.terms.iter().all(|term| match_term(term, elem, self)) {
                    results.push(id);
                }
        }
        results
    }
}

/// Test whether a single query term matches an element.
fn match_term(term: &QueryTerm, elem: &crate::model::Element, model: &Model) -> bool {
    match term {
        QueryTerm::Name(s) => {
            elem.name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(s))
                || elem
                    .short_name
                    .as_deref()
                    .is_some_and(|n| n.to_lowercase().contains(s))
        }
        QueryTerm::Kind(s) => {
            // Special metatype filters
            if s == "def" || s == "definition" {
                return elem.kind.metatype() == Metatype::Definition;
            }
            if s == "usage" {
                return elem.kind.metatype() == Metatype::Usage;
            }
            // Otherwise match keyword substring
            elem.kind.keyword().to_lowercase().contains(s)
        }
        QueryTerm::InPackage(s) => {
            let qname = elem.qualified_name(model).to_lowercase();
            qname.contains(s)
        }
        QueryTerm::TypeRef(s) => elem
            .type_ref
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains(s)),
        QueryTerm::Specializes(s) => elem
            .specializes
            .iter()
            .any(|spec| spec.to_lowercase().contains(s)),
        QueryTerm::HasFeature(s) => match_has_feature(s, elem, model),
        QueryTerm::Doc(s) => elem
            .doc
            .as_deref()
            .is_some_and(|d| d.to_lowercase().contains(s)),
        QueryTerm::Abstract(b) => elem.is_abstract == *b,
        QueryTerm::Qualifier(s) => elem.qualifiers.iter().any(|q| q.to_lowercase().contains(s)),
    }
}

/// Check if an element has children of a particular kind category.
fn match_has_feature(feature: &str, elem: &crate::model::Element, model: &Model) -> bool {
    elem.children.iter().any(|&cid| {
        model.element(cid).is_some_and(|child| match feature {
            "port" => matches!(child.kind, ElementKind::PortUsage | ElementKind::PortDef),
            "attribute" => matches!(
                child.kind,
                ElementKind::AttributeUsage | ElementKind::AttributeDef
            ),
            "part" => matches!(child.kind, ElementKind::PartUsage | ElementKind::PartDef),
            "action" => matches!(
                child.kind,
                ElementKind::ActionUsage | ElementKind::ActionDef
            ),
            "state" => matches!(child.kind, ElementKind::StateUsage | ElementKind::StateDef),
            "requirement" => matches!(
                child.kind,
                ElementKind::RequirementUsage | ElementKind::RequirementDef
            ),
            _ => child.kind.keyword().to_lowercase().contains(feature),
        })
    })
}
