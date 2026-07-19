import type { SysmlElement, SysmlRelationship } from "../../../packages/model/src/index.js";

// ---------------------------------------------------------------------------
// Presentation projection (store model -> Cameo-valid, RENDERABLE model)
// ---------------------------------------------------------------------------
//
// The store holds a DEFINITION-based model (RequirementDefinition,
// PartDefinition, ActionDefinition, VerificationCaseDefinition) plus trace
// relationships whose operands point at those DEFINITIONS.
//
// SysML v2 / Cameo require the operands of `satisfy ... by ...`,
// `allocate ... to ...`, `verify ...`, and `dependency from ... to ...` to be
// USAGES (Features), never definitions.
//
// RENDERABLE DESIGN — this projection emits each trace participant EXACTLY ONCE
// (so the model fits Cameo Community Edition's 500 "major element" cap) AND
// nests those participants inside two corpus-grounded container usages so the
// decisym viewer can render structural views:
//
//   - Requirements/needs -> ONE `requirement` USAGE each (package level).
//   - Components (6)      -> ONE UNTYPED `part` USAGE each, NESTED inside a
//                            'C&C Subsystem' container part usage (owner edge).
//   - Component flows     -> ONE ConnectionUsage each, from the component-scope
//                            n2Interfaces slice, emitted as `connect A to B;`
//                            inside the 'C&C Subsystem' body (endpoints are the
//                            nested part names). provenanceSourceId = n2 row id.
//   - Leaf functions (15) -> ONE UNTYPED `action` USAGE each, NESTED inside a
//                            'C&C Operations' container action usage.
//   - Verification (4)    -> kept as `verification def` (needed for verify
//                            coverage: `objective { verify <reqName>; }`).
//
// The two containers ('C&C Subsystem', 'C&C Operations') are corpus-grounded:
// the subsystem container carries the same provenance id ("C&C") the 6 parts
// derive from; the operations container carries the same "C&C" subsystem
// provenance (the F1/F8 activity decomposition it groups). The viewer's default
// and --spec views resolve these names for General / Interconnection / ActionFlow
// contexts.
//
// UNTYPED usages are Cameo-semantic-safe: a usage with no `: Type` has no
// unresolved-type reference and, being the only element with that name, no
// duplicate-name clash. satisfy/allocate/dependency reference each usage BY NAME
// (refName resolves the usage's own name regardless of owner), so nesting a part
// or action inside a container does NOT break package-level trace resolution.
//
// It does NOT mutate the store. The generator calls it only at the serialize
// step. The audit and the fidelity comparator read the store directly and are
// unaffected — so nesting/containers/connections cannot change Gate-1 findings
// or the 28/28 fidelity.
// ---------------------------------------------------------------------------

const SUBSYSTEM_PROVENANCE = "C&C";
const PARENT_FUNCTION_PROVENANCE = new Set(["F1", "F8"]);

// Synthetic container element ids (stable, deterministic, prefixed to avoid any
// clash with store-generated ids).
const SUBSYSTEM_CONTAINER_ID = "cc-presentation::subsystem-container";
const OPERATIONS_CONTAINER_ID = "cc-presentation::operations-container";
const REQUIREMENTS_CONTAINER_ID = "cc-presentation::requirements-container";

// Viewer context names (must match views.json / default VIEW_SPECS).
const SUBSYSTEM_CONTAINER_NAME = "C&C Subsystem";
const OPERATIONS_CONTAINER_NAME = "C&C Operations";
const REQUIREMENTS_CONTAINER_NAME = "C&C Requirements";

/** One corpus-grounded component-scope interface flow (from cc-extracted's n2Interfaces). */
export interface ComponentFlow {
  id: string;
  sourceLabel: string;
  targetLabel: string;
  flow: string;
}

interface ProjectionResult {
  elements: SysmlElement[];
  relationships: SysmlRelationship[];
}

function provenanceOf(el: SysmlElement): string | undefined {
  const p = el.raw.provenanceSourceId;
  return typeof p === "string" && p.length > 0 ? p : undefined;
}

function isLeafFunctionProvenance(prov: string | undefined): boolean {
  // Leaf functions have dotted ids (F1.1, F8.3, ...). Parents are F1, F8.
  return typeof prov === "string" && prov.includes(".");
}

/**
 * Project the def-based store model into a Cameo-valid, renderable presentation
 * model. Pure function — does not mutate its inputs.
 *
 * @param componentFlows corpus component-scope n2Interfaces (both endpoints are
 *        C&C components). Each becomes one `connect` ConnectionUsage nested in
 *        the 'C&C Subsystem' container, provenance = the n2 row id.
 */
export function projectForPresentation(
  elements: SysmlElement[],
  relationships: SysmlRelationship[],
  componentFlows: ComponentFlow[] = []
): ProjectionResult {
  const outElements: SysmlElement[] = [];

  // Map: original definition element id -> synthesized package-level usage id.
  // Trace relationships are re-pointed through this map.
  const defToUsage = new Map<string, string>();

  // Component usage id keyed by component NAME (for connection endpoints) and by
  // usage id (to reparent under the subsystem container).
  const componentUsageIdByName = new Map<string, string>();
  const componentUsageIds: string[] = [];
  // Leaf action usage ids (to reparent under the operations container).
  const leafActionUsageIds: string[] = [];
  // Requirement/need usage ids (to reparent under the requirements container).
  const requirementUsageIds: string[] = [];

  // ---- classify the structural definitions (one usage per participant) -----
  for (const el of elements) {
    if (el.type === "RequirementDefinition") {
      // requirement/need -> emit the USAGE ONLY, NESTED inside the
      // 'C&C Requirements' container (owner edge). Nesting does NOT change trace
      // resolution: satisfy/dependency/verify reference each usage BY NAME, and
      // refName resolves the usage's own name regardless of owner. The container
      // is corpus-grounded (provenance "C&C") and gives the Requirements view a
      // named context that scopes the derive tree (system reqs -> stakeholder
      // needs), matching the requirements-trace.sysml probe shape.
      const usageId = `${el.id}__usage`;
      const prov = provenanceOf(el);
      const usage: SysmlElement = {
        id: usageId,
        elementId: usageId,
        type: "RequirementUsage",
        name: el.name,
        shortName: prov ?? null,
        qualifiedName: null,
        ownerId: REQUIREMENTS_CONTAINER_ID,
        ownedElementIds: [],
        raw: { provenanceSourceId: prov },
      };
      outElements.push(usage);
      requirementUsageIds.push(usageId);
      defToUsage.set(el.id, usageId);
      continue;
    }

    if (el.type === "PartDefinition") {
      const prov = provenanceOf(el);
      // The subsystem container is REBUILT below as a usage that OWNS the 6
      // component usages — drop the store def here (its usage is synthesized).
      if (prov === SUBSYSTEM_PROVENANCE) continue;

      // Component -> ONE untyped `part` USAGE named by the component name,
      // NESTED under the 'C&C Subsystem' container (ownerId set below once the
      // container id is known — it is a constant, so we set it directly).
      const usageId = `${el.id}__usage`;
      const usage: SysmlElement = {
        id: usageId,
        elementId: usageId,
        type: "PartUsage",
        name: el.name,
        shortName: null,
        qualifiedName: null,
        ownerId: SUBSYSTEM_CONTAINER_ID,
        ownedElementIds: [],
        raw: { provenanceSourceId: prov },
      };
      outElements.push(usage);
      defToUsage.set(el.id, usageId);
      if (el.name) componentUsageIdByName.set(el.name, usageId);
      componentUsageIds.push(usageId);
      continue;
    }

    if (el.type === "ActionDefinition") {
      const prov = provenanceOf(el);
      // DROP the F1/F8 parent containers — the single 'C&C Operations' container
      // synthesized below groups all leaf functions (mirrors the reference model).
      if (PARENT_FUNCTION_PROVENANCE.has(prov ?? "")) continue;

      if (isLeafFunctionProvenance(prov)) {
        // Leaf function -> ONE untyped `action` USAGE, NESTED under the
        // 'C&C Operations' container.
        const usageId = `${el.id}__usage`;
        const usage: SysmlElement = {
          id: usageId,
          elementId: usageId,
          type: "ActionUsage",
          name: el.name,
          shortName: null,
          qualifiedName: null,
          ownerId: OPERATIONS_CONTAINER_ID,
          ownedElementIds: [],
          raw: { provenanceSourceId: prov },
        };
        outElements.push(usage);
        defToUsage.set(el.id, usageId);
        leafActionUsageIds.push(usageId);
      }
      continue;
    }

    if (el.type === "VerificationCaseDefinition") {
      // Kept as a def; its body (objective { verify ... }) is built by the
      // serializer from VerifyRequirementUsage relationships. Pass through.
      outElements.push(cloneAsRoot(el));
      continue;
    }

    // Any other structural element (shouldn't occur) — pass through as a root.
    outElements.push(cloneAsRoot(el));
  }

  // ---- synthesize the three corpus-grounded container usages ---------------
  // 'C&C Requirements' package — owns the 34 requirement/need usages so the
  // Requirements view has a named context (the requirements-trace.sysml probe
  // shape). A Package (not a usage) mirrors the probe's `package 'C&C
  // Requirements' { requirement ...; }` form; the derive (dependency) trace
  // statements resolve by name and stay in the 'C&C Trace' package. Provenance
  // = the "C&C" subsystem corpus id these requirements decompose.
  const requirementsContainer: SysmlElement = {
    id: REQUIREMENTS_CONTAINER_ID,
    elementId: REQUIREMENTS_CONTAINER_ID,
    type: "Package",
    name: REQUIREMENTS_CONTAINER_NAME,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [...requirementUsageIds],
    raw: { provenanceSourceId: SUBSYSTEM_PROVENANCE },
  };
  outElements.push(requirementsContainer);

  // 'C&C Subsystem' part usage — owns the 6 component usages. Provenance = the
  // subsystem corpus id the 6 parts derive from.
  const subsystemContainer: SysmlElement = {
    id: SUBSYSTEM_CONTAINER_ID,
    elementId: SUBSYSTEM_CONTAINER_ID,
    type: "PartUsage",
    name: SUBSYSTEM_CONTAINER_NAME,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [...componentUsageIds],
    raw: { provenanceSourceId: SUBSYSTEM_PROVENANCE },
  };
  outElements.push(subsystemContainer);

  // 'C&C Operations' action usage — owns the 15 leaf action usages. Provenance =
  // the same "C&C" subsystem corpus id (the F1/F8 activity decomposition it groups).
  const operationsContainer: SysmlElement = {
    id: OPERATIONS_CONTAINER_ID,
    elementId: OPERATIONS_CONTAINER_ID,
    type: "ActionUsage",
    name: OPERATIONS_CONTAINER_NAME,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [...leafActionUsageIds],
    raw: { provenanceSourceId: SUBSYSTEM_PROVENANCE },
  };
  outElements.push(operationsContainer);

  // ---- corpus-grounded connections between component usages -----------------
  // Each component-scope n2 flow becomes ONE ConnectionUsage nested in the
  // subsystem container. Emitted by the serializer as `connect A to B;` (name
  // null -> plain connect form, which the viewer parses into a Connect edge that
  // populates the Interconnection view). Endpoints are the nested part usage ids
  // (the serializer resolves them to the child part names). Direction preserved
  // (source -> target); the flow label is carried on the element for provenance.
  for (const cf of componentFlows) {
    const srcUsageId = componentUsageIdByName.get(cf.sourceLabel);
    const tgtUsageId = componentUsageIdByName.get(cf.targetLabel);
    if (!srcUsageId || !tgtUsageId) continue; // endpoint not a C&C component usage
    outElements.push({
      id: `conn::${cf.id}`,
      elementId: `conn::${cf.id}`,
      type: "ConnectionUsage",
      name: null, // null -> `connect src to tgt;` (viewer-parseable Connect edge)
      shortName: null,
      qualifiedName: null,
      ownerId: SUBSYSTEM_CONTAINER_ID,
      ownedElementIds: [],
      raw: {
        // sourceEnd/targetEnd drive the serializer's nested-connect emission.
        sourceEnd: srcUsageId,
        targetEnd: tgtUsageId,
        provenanceSourceId: cf.id,
        // Corpus flow label — carried for provenance/inspection (not shown in
        // the plain-connect IBD label, which the grammar keeps type-free).
        flowLabel: cf.flow,
      },
    });
  }

  // ---- re-point trace relationships to the usage ids ------------------------
  const outRelationships: SysmlRelationship[] = [];
  for (const rel of relationships) {
    // FeatureMembership is consumed structurally above (containers now own their
    // children via ownerId); drop it from output.
    if (rel.type === "FeatureMembership") continue;

    const newSources = rel.sourceIds.map((id) => defToUsage.get(id) ?? id);
    const newTargets = rel.targetIds.map((id) => defToUsage.get(id) ?? id);

    // VerifyRequirementUsage: source stays the VerificationCaseDefinition (def),
    // target re-points to the requirement usage.
    if (rel.type === "VerifyRequirementUsage" || rel.type === "RequirementVerificationMembership") {
      outRelationships.push({
        ...rel,
        sourceIds: rel.sourceIds, // keep the verification def
        targetIds: newTargets,
      });
      continue;
    }

    outRelationships.push({
      ...rel,
      sourceIds: newSources,
      targetIds: newTargets,
    });
  }

  return { elements: outElements, relationships: outRelationships };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Clone an element as a root (ownerId reset to null), preserving everything
 *  else. Used for definitions that should sit at package level (the kept
 *  verification defs). */
function cloneAsRoot(el: SysmlElement): SysmlElement {
  return { ...el, ownerId: null, ownedElementIds: [] };
}
