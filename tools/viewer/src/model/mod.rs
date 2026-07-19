pub mod parse;
pub mod query;

use std::collections::HashMap;

/// Unique identifier for model elements.
pub type ElementId = usize;

/// Whether an element is a definition or a usage (drives rendering shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Metatype {
    Definition,
    Usage,
}

/// The SysML v2 element kind (keyword that introduced it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum ElementKind {
    Package,
    PartDef,
    PartUsage,
    PortDef,
    PortUsage,
    AttributeDef,
    AttributeUsage,
    ItemDef,
    ItemUsage,
    ActionDef,
    ActionUsage,
    StateDef,
    StateUsage,
    ConnectionDef,
    ConnectionUsage,
    InterfaceDef,
    InterfaceUsage,
    AllocationDef,
    AllocationUsage,
    RequirementDef,
    RequirementUsage,
    ConstraintDef,
    ConstraintUsage,
    ConcernDef,
    ViewDef,
    ViewUsage,
    ViewpointDef,
    RenderingDef,
    VerificationDef,
    VerificationUsage,
    EnumDef,
    EnumUsage,
    OccurrenceDef,
    OccurrenceUsage,
    IndividualDef,
    IndividualUsage,
    FlowUsage,
    SignalDef,
    MetadataDef,
    Comment,
    Import,
    Alias,
    Dependency,
    Transition,
    Succession,
    DecisionNode,
    ForkNode,
    JoinNode,
    MergeNode,
    MessageUsage,
    TimesliceUsage,
    SnapshotUsage,
    VariationUsage,
    RefUsage,
    Unknown,
}

impl ElementKind {
    /// Returns the metatype (definition vs usage) for rendering decisions.
    pub fn metatype(self) -> Metatype {
        match self {
            Self::Package
            | Self::PartDef
            | Self::PortDef
            | Self::AttributeDef
            | Self::ItemDef
            | Self::ActionDef
            | Self::StateDef
            | Self::ConnectionDef
            | Self::InterfaceDef
            | Self::AllocationDef
            | Self::RequirementDef
            | Self::ConstraintDef
            | Self::ConcernDef
            | Self::ViewDef
            | Self::ViewpointDef
            | Self::RenderingDef
            | Self::VerificationDef
            | Self::EnumDef
            | Self::OccurrenceDef
            | Self::IndividualDef
            | Self::SignalDef
            | Self::MetadataDef => Metatype::Definition,
            _ => Metatype::Usage,
        }
    }

    /// Short keyword label for display in the browser tree.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::PartDef => "part def",
            Self::PartUsage => "part",
            Self::PortDef => "port def",
            Self::PortUsage => "port",
            Self::AttributeDef => "attribute def",
            Self::AttributeUsage => "attribute",
            Self::ItemDef => "item def",
            Self::ItemUsage => "item",
            Self::ActionDef => "action def",
            Self::ActionUsage => "action",
            Self::StateDef => "state def",
            Self::StateUsage => "state",
            Self::ConnectionDef => "connection def",
            Self::ConnectionUsage => "connect",
            Self::InterfaceDef => "interface def",
            Self::InterfaceUsage => "interface",
            Self::AllocationDef => "allocation def",
            Self::AllocationUsage => "allocation",
            Self::RequirementDef => "requirement def",
            Self::RequirementUsage => "requirement",
            Self::ConstraintDef => "constraint def",
            Self::ConstraintUsage => "constraint",
            Self::ConcernDef => "concern def",
            Self::ViewDef => "view def",
            Self::ViewUsage => "view",
            Self::ViewpointDef => "viewpoint def",
            Self::RenderingDef => "rendering def",
            Self::VerificationDef => "verification def",
            Self::VerificationUsage => "verification",
            Self::EnumDef => "enum def",
            Self::EnumUsage => "enum",
            Self::OccurrenceDef => "occurrence def",
            Self::OccurrenceUsage => "occurrence",
            Self::IndividualDef => "individual def",
            Self::IndividualUsage => "individual",
            Self::FlowUsage => "flow",
            Self::SignalDef => "signal def",
            Self::MetadataDef => "metadata def",
            Self::Comment => "comment",
            Self::Import => "import",
            Self::Alias => "alias",
            Self::Dependency => "dependency",
            Self::Transition => "transition",
            Self::Succession => "succession",
            Self::DecisionNode => "decide",
            Self::ForkNode => "fork",
            Self::JoinNode => "join",
            Self::MergeNode => "merge",
            Self::MessageUsage => "message",
            Self::TimesliceUsage => "timeslice",
            Self::SnapshotUsage => "snapshot",
            Self::VariationUsage => "variation",
            Self::RefUsage => "ref",
            Self::Unknown => "unknown",
        }
    }

    /// Whether this kind is a structural/behavioral element (as opposed to metadata).
    /// Used for filtering in the overview.
    pub fn is_structural(self) -> bool {
        !matches!(
            self,
            Self::Comment | Self::Import | Self::Alias | Self::Unknown
        )
    }

    /// Short text icon for the model browser: `[D]` for definitions, `(u)` for usages.
    pub fn browser_icon(self) -> &'static str {
        match self {
            Self::Package => "[Pkg]",
            Self::PartDef => "[P]",
            Self::PartUsage => "(p)",
            Self::PortDef => "[Po]",
            Self::PortUsage => "(po)",
            Self::AttributeDef => "[A]",
            Self::AttributeUsage => "(a)",
            Self::ItemDef => "[I]",
            Self::ItemUsage => "(i)",
            Self::ActionDef => "[Act]",
            Self::ActionUsage => "(act)",
            Self::StateDef => "[S]",
            Self::StateUsage => "(s)",
            Self::ConnectionDef => "[C]",
            Self::ConnectionUsage => "(c)",
            Self::InterfaceDef => "[If]",
            Self::InterfaceUsage => "(if)",
            Self::AllocationDef => "[Al]",
            Self::AllocationUsage => "(al)",
            Self::RequirementDef => "[R]",
            Self::RequirementUsage => "(r)",
            Self::ConstraintDef => "[Cn]",
            Self::ConstraintUsage => "(cn)",
            Self::EnumDef => "[E]",
            Self::FlowUsage => "(fl)",
            Self::ViewDef => "[V]",
            Self::ViewUsage => "(v)",
            _ => "(.)",
        }
    }
}

/// A model element (definition or usage).
#[derive(Debug, Clone)]
pub struct Element {
    pub id: ElementId,
    pub kind: ElementKind,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub type_ref: Option<String>,
    pub specializes: Vec<String>,
    pub multiplicity: Option<String>,
    /// Feature value from a `= <value>` clause (attribute/requirement text, etc.).
    pub value: Option<String>,
    pub doc: Option<String>,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub is_conjugated: bool,
    pub is_abstract: bool,
    pub is_variation: bool,
    /// Keyword qualifiers like `redefines`, `perform`, `exhibit`.
    pub qualifiers: Vec<String>,
}

impl Element {
    /// Display name: name or short_name or "<anonymous>".
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.short_name.as_deref())
            .unwrap_or("<anonymous>")
    }

    /// Returns the qualified name by walking up the parent chain.
    pub fn qualified_name(&self, model: &Model) -> String {
        let mut parts = vec![self.display_name().to_string()];
        let mut current = self.parent;
        while let Some(pid) = current {
            if let Some(parent) = model.element(pid) {
                if parent.name.is_some() {
                    parts.push(parent.display_name().to_string());
                }
                current = parent.parent;
            } else {
                break;
            }
        }
        parts.reverse();
        parts.join("::")
    }

    /// Whether this element has port children.
    pub fn ports(&self, model: &Model) -> Vec<ElementId> {
        self.children
            .iter()
            .copied()
            .filter(|&cid| {
                model.element(cid).is_some_and(|e| {
                    e.kind == ElementKind::PortUsage || e.kind == ElementKind::PortDef
                })
            })
            .collect()
    }

    /// Non-port children (attributes, parts, etc.).
    pub fn features(&self, model: &Model) -> Vec<ElementId> {
        self.children
            .iter()
            .copied()
            .filter(|&cid| {
                model.element(cid).is_some_and(|e| {
                    !matches!(
                        e.kind,
                        ElementKind::PortUsage
                            | ElementKind::PortDef
                            | ElementKind::Import
                            | ElementKind::Comment
                            | ElementKind::Alias
                    )
                })
            })
            .collect()
    }
}

/// A relationship between elements (connect, bind, flow, etc.).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Relationship {
    pub kind: RelationshipKind,
    pub name: Option<String>,
    pub source_path: String,
    pub target_path: String,
    pub type_ref: Option<String>,
    /// The owning element (context in which the relationship was declared).
    pub owner: ElementId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum RelationshipKind {
    Connect,
    Bind,
    Flow,
    Interface,
    Allocate,
    Satisfy,
    Dependency,
    Message,
    Succession,
    Transition,
    Verify,
}

/// The top-level model.
#[derive(Debug, Clone)]
pub struct Model {
    elements: Vec<Element>,
    pub relationships: Vec<Relationship>,
    pub root_ids: Vec<ElementId>,
}

impl Model {
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
            relationships: Vec::new(),
            root_ids: Vec::new(),
        }
    }

    /// Add an element and return its id.
    pub fn add_element(&mut self, mut elem: Element) -> ElementId {
        let id = self.elements.len();
        elem.id = id;
        self.elements.push(elem);
        id
    }

    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(id)
    }

    pub fn element_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        self.elements.get_mut(id)
    }

    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Find an element by name within a parent scope (or root if parent is None).
    pub fn find_child(&self, parent: Option<ElementId>, name: &str) -> Option<ElementId> {
        let children = match parent {
            Some(pid) => self.element(pid).map(|e| &e.children[..]).unwrap_or(&[]),
            None => &self.root_ids[..],
        };
        children.iter().copied().find(|&cid| {
            self.element(cid)
                .and_then(|e| e.name.as_deref())
                .is_some_and(|n| n == name)
        })
    }

    /// Resolve a dot-separated path relative to a context element.
    pub fn resolve_path(&self, context: ElementId, path: &str) -> Option<ElementId> {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return None;
        }
        // Try resolving from context first, then from context's children
        let mut current = self.find_child(Some(context), parts[0]);
        if current.is_none() {
            // Try siblings (same parent)
            if let Some(parent) = self.element(context).and_then(|e| e.parent) {
                current = self.find_child(Some(parent), parts[0]);
            }
        }
        for &part in &parts[1..] {
            current = current.and_then(|cid| self.find_child(Some(cid), part));
        }
        current
    }

    /// All elements of a given kind.
    #[allow(dead_code)]
    pub fn elements_of_kind(&self, kind: ElementKind) -> Vec<&Element> {
        self.elements.iter().filter(|e| e.kind == kind).collect()
    }

    /// Count elements matching a predicate.
    #[allow(dead_code)]
    pub fn count_where(&self, f: impl Fn(&Element) -> bool) -> usize {
        self.elements.iter().filter(|e| f(e)).count()
    }

    /// Build an index of elements by name for fast lookup.
    pub fn name_index(&self) -> HashMap<&str, Vec<ElementId>> {
        let mut idx: HashMap<&str, Vec<ElementId>> = HashMap::new();
        for elem in &self.elements {
            if let Some(ref name) = elem.name {
                idx.entry(name.as_str()).or_default().push(elem.id);
            }
        }
        idx
    }

    /// Compute the number of descendants (recursive child count).
    /// Used to size package nodes proportionally in the overview.
    pub fn subtree_weight(&self, id: ElementId) -> usize {
        let elem = match self.element(id) {
            Some(e) => e,
            None => return 0,
        };
        let mut count = 1; // count self
        for &child in &elem.children {
            count += self.subtree_weight(child);
        }
        count
    }

    /// Collect cross-package edges: relationships where source and target
    /// resolve to elements in different top-level packages.
    /// Returns (source_package_id, target_package_id, rel_index) triples.
    #[allow(dead_code)]
    pub fn cross_package_edges(&self) -> Vec<(ElementId, ElementId, usize)> {
        let mut edges = Vec::new();
        for (i, rel) in self.relationships.iter().enumerate() {
            let src_top = self.top_level_ancestor(rel.owner);
            // Try to resolve target by name — look for first segment of target_path
            let tgt_name = rel.target_path.split('.').next().unwrap_or("");
            if let Some(tgt_ids) = self.name_index().get(tgt_name) {
                for &tgt_id in tgt_ids {
                    let tgt_top = self.top_level_ancestor(tgt_id);
                    if let (Some(s), Some(t)) = (src_top, tgt_top)
                        && s != t {
                            edges.push((s, t, i));
                            break;
                        }
                }
            }
        }
        // Also check specializations: elements that specialize elements in other packages
        for elem in &self.elements {
            for spec in &elem.specializes {
                let spec_name = spec.split("::").last().unwrap_or(spec);
                if let Some(spec_ids) = self.name_index().get(spec_name) {
                    for &spec_id in spec_ids {
                        let src_top = self.top_level_ancestor(elem.id);
                        let tgt_top = self.top_level_ancestor(spec_id);
                        if let (Some(s), Some(t)) = (src_top, tgt_top)
                            && s != t {
                                edges.push((s, t, usize::MAX)); // MAX = specialization, not rel index
                                break;
                            }
                    }
                }
            }
        }
        edges
    }

    /// Find the top-level ancestor (root_id) of an element.
    #[allow(dead_code)]
    pub fn top_level_ancestor(&self, id: ElementId) -> Option<ElementId> {
        let mut current = id;
        loop {
            let elem = self.element(current)?;
            match elem.parent {
                Some(pid) => current = pid,
                None => return Some(current),
            }
        }
    }

    /// Search elements by name (case-insensitive substring match).
    pub fn search_by_name(&self, query: &str) -> Vec<ElementId> {
        if query.is_empty() {
            return Vec::new();
        }
        let lower = query.to_lowercase();
        self.elements
            .iter()
            .filter(|e| {
                e.name
                    .as_deref()
                    .is_some_and(|n| n.to_lowercase().contains(&lower))
                    || e.short_name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase().contains(&lower))
            })
            .map(|e| e.id)
            .collect()
    }

    /// Return directly connected element IDs (neighbors in the graph).
    /// Includes: relationship endpoints, type definitions, specialization targets.
    pub fn neighbors(&self, id: ElementId) -> Vec<ElementId> {
        let mut result = Vec::new();
        let elem = match self.element(id) {
            Some(e) => e,
            None => return result,
        };
        let idx = self.name_index();

        // Relationships where this element owns or is referenced
        for rel in &self.relationships {
            if rel.owner == id {
                // Resolve source and target first segments
                for path in [&rel.source_path, &rel.target_path] {
                    let first = path.split('.').next().unwrap_or("");
                    if let Some(ids) = idx.get(first) {
                        for &cid in ids {
                            if cid != id && !result.contains(&cid) {
                                result.push(cid);
                            }
                        }
                    }
                }
            }
        }

        // Type definition
        if let Some(ref type_name) = elem.type_ref
            && let Some(ids) = idx.get(type_name.as_str()) {
                for &tid in ids {
                    if tid != id && !result.contains(&tid) {
                        result.push(tid);
                    }
                }
            }

        // Specialization targets
        for spec in &elem.specializes {
            let spec_name = spec.split("::").last().unwrap_or(spec);
            if let Some(ids) = idx.get(spec_name) {
                for &sid in ids {
                    if sid != id && !result.contains(&sid) {
                        result.push(sid);
                    }
                }
            }
        }

        result
    }

    /// Get effective ports for an element: direct port children, or
    /// inherited from the type definition if no direct ports exist.
    ///
    /// In SysML v2, usages like `part engine : Engine` inherit ports
    /// from the Engine definition. This method resolves that inheritance
    /// so that layout and rendering can show ports on usage elements.
    pub fn effective_ports(&self, id: ElementId) -> Vec<ElementId> {
        let elem = match self.element(id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        let direct = elem.ports(self);
        if !direct.is_empty() {
            return direct;
        }
        // Inherit from type definition
        if let Some(ref type_name) = elem.type_ref {
            let idx = self.name_index();
            if let Some(candidates) = idx.get(type_name.as_str()) {
                for &cand_id in candidates {
                    if let Some(cand) = self.element(cand_id)
                        && cand.kind.metatype() == Metatype::Definition {
                            let ports = cand.ports(self);
                            if !ports.is_empty() {
                                return ports;
                            }
                        }
                }
            }
        }
        Vec::new()
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}
