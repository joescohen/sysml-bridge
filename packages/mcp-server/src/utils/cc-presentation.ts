import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

// ---------------------------------------------------------------------------
// Presentation projection (store model -> Cameo-valid, ELEMENT-LEAN model)
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
// ELEMENT-LEAN DESIGN — this projection emits each trace participant EXACTLY
// ONCE so the model fits Cameo Community Edition's 500 "major element" cap.
// Cameo CE blocks model creation past that threshold; the previous def+usage
// projection (plus nested structural duplicates) roughly doubled the count and
// overflowed CE. The lean projection:
//
//   - Requirements/needs -> ONE `requirement` USAGE each (already lean).
//   - Components (6)      -> ONE UNTYPED `part` USAGE each, named by the
//                            component name: `part 'C&C Power Module';`.
//                            NO `part def`, NO nested `sub_*` duplicate.
//   - Leaf functions (15) -> ONE UNTYPED `action` USAGE each, named by the
//                            function name: `action 'Receive & Authenticate
//                            Request';`. NO `action def`, NO `f1_*`/`f8_*`.
//   - Verification (4)    -> kept as `verification def` (needed for verify
//                            coverage: `objective { verify <reqName>; }`).
//   - The subsystem `part def` container and the F1/F8 `action def` containers
//     are DROPPED entirely — they existed only for BDD/decomposition nesting,
//     which is what inflated the count. Traceability does not need them.
//
// UNTYPED usages are Cameo-semantic-safe: a usage with no `: Type` has no
// unresolved-type reference (an earlier Cameo error) and, because it is the
// only element with that name, no duplicate-name clash with a def (the other
// earlier error). satisfy/allocate/dependency reference each usage BY NAME
// (the serializer's refName resolves the usage's name and quotes it), so naming
// the component/function usage with its human-readable name makes every trace
// operand resolve to exactly one declared usage.
//
// It does NOT mutate the store. The generator calls it only at the serialize
// step. The validate_model tool and the fidelity comparator read the store
// directly and are unaffected.
// ---------------------------------------------------------------------------

const SUBSYSTEM_PROVENANCE = "C&C";
const PARENT_FUNCTION_PROVENANCE = new Set(["F1", "F8"]);

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
 * Project the def-based store model into a Cameo-valid presentation model.
 * Pure function — does not mutate its inputs.
 */
export function projectForPresentation(
  elements: SysmlElement[],
  relationships: SysmlRelationship[]
): ProjectionResult {
  const outElements: SysmlElement[] = [];

  // Map: original definition element id -> synthesized package-level usage id.
  // Trace relationships are re-pointed through this map.
  const defToUsage = new Map<string, string>();

  // ---- classify the structural definitions (LEAN: one usage per participant)
  for (const el of elements) {
    if (el.type === "RequirementDefinition") {
      // requirement/need (incl. stakeholderNeed) -> emit the USAGE ONLY.
      // Nothing is typed by a requirement, so a `requirement def` is redundant
      // and — because the usage shares the same name — would collide with it in
      // Cameo (two members named e.g. `N1` in one namespace). satisfy/verify/
      // derive reference these by name, which resolves to the single usage.
      const usageId = `${el.id}__usage`;
      const prov = provenanceOf(el);
      const usage: SysmlElement = {
        id: usageId,
        elementId: usageId,
        type: "RequirementUsage",
        // Reference name = the requirement/need name (unique in corpus).
        name: el.name,
        // short name = provenance id, e.g. <'ANGARS-4'> / <'N1'>.
        shortName: prov ?? null,
        qualifiedName: null,
        ownerId: null,
        ownedElementIds: [],
        raw: { provenanceSourceId: prov },
      };
      outElements.push(usage);
      defToUsage.set(el.id, usageId);
      continue;
    }

    if (el.type === "PartDefinition") {
      const prov = provenanceOf(el);
      // DROP the subsystem container entirely — it existed only to nest the
      // `sub_*` part usages for BDD, which inflated the element count. The
      // traceability does not reference it.
      if (prov === SUBSYSTEM_PROVENANCE) continue;

      // Component -> ONE untyped `part` USAGE named by the component name.
      // No `part def`, no nested `sub_*` duplicate. allocate references it by
      // this name. Omitting raw.typeName means the serializer emits no
      // `: 'Type'` clause (no unresolved-type reference in Cameo).
      const usageId = `${el.id}__usage`;
      const usage: SysmlElement = {
        id: usageId,
        elementId: usageId,
        type: "PartUsage",
        name: el.name,
        shortName: null,
        qualifiedName: null,
        ownerId: null,
        ownedElementIds: [],
        raw: { provenanceSourceId: prov },
      };
      outElements.push(usage);
      defToUsage.set(el.id, usageId);
      continue;
    }

    if (el.type === "ActionDefinition") {
      const prov = provenanceOf(el);
      // DROP the F1/F8 parent containers entirely — they existed only to nest
      // the `f1_*`/`f8_*` action usages for functional decomposition, which
      // inflated the count. Only leaf functions participate in traceability.
      if (PARENT_FUNCTION_PROVENANCE.has(prov ?? "")) continue;

      if (isLeafFunctionProvenance(prov)) {
        // Leaf function -> ONE untyped `action` USAGE named by the function
        // name. No `action def`, no nested `f*_` duplicate. satisfy/allocate
        // reference it by this name.
        const usageId = `${el.id}__usage`;
        const usage: SysmlElement = {
          id: usageId,
          elementId: usageId,
          type: "ActionUsage",
          name: el.name,
          shortName: null,
          qualifiedName: null,
          ownerId: null,
          ownedElementIds: [],
          raw: { provenanceSourceId: prov },
        };
        outElements.push(usage);
        defToUsage.set(el.id, usageId);
      }
      continue;
    }

    if (el.type === "VerificationCaseDefinition") {
      // Kept as a def; its body (objective { verify ... }) is built by the
      // serializer from VerifyRequirementUsage relationships. Pass through.
      outElements.push(cloneAsRoot(el));
      continue;
    }

    // Any other structural element (shouldn't occur in this model) — pass
    // through as a root element unchanged.
    outElements.push(cloneAsRoot(el));
  }

  // ---- re-point trace relationships to the usage ids ---------------------
  const outRelationships: SysmlRelationship[] = [];
  for (const rel of relationships) {
    // FeatureMembership is consumed structurally above; drop it from output.
    if (rel.type === "FeatureMembership") continue;

    const newSources = rel.sourceIds.map((id) => defToUsage.get(id) ?? id);
    const newTargets = rel.targetIds.map((id) => defToUsage.get(id) ?? id);

    // VerifyRequirementUsage: source stays the VerificationCaseDefinition
    // (def), target re-points to the requirement usage.
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
