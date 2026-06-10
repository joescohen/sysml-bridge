/**
 * GATE-02 relational rule pack.
 *
 * Pure function over (SysmlElement[], SysmlRelationship[]) — no ModelStore,
 * no I/O. Emits Finding[] for structural/relational model defects.
 *
 * Rules implemented:
 *   R4-def-operand          error   — trace rel operand is a Definition (not a Usage)
 *   GATE02-dangling-endpoint error  — rel source/target not in element id set
 *   GATE02-id-duplicate      error  — two elements share the same id
 *   GATE02-orphan            warning — leaf design element (PartDefinition/ActionDefinition)
 *                                      with no satisfy/allocate/derive edge
 *   GATE02-unverified        warning — system req has no verify edge
 *   GATE02-unsatisfied       warning — system req has no satisfy edge
 *   GATE02-unbacktraced      warning — system req has no derive trace
 *   GATE02-uncovered-need    warning — stakeholder need not covered by derive
 */

import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";
import type { Finding } from "./findings.js";

// ---------------------------------------------------------------------------
// Exported type-set constants
// ---------------------------------------------------------------------------

/** All trace relationship types that carry satisfy/verify/derive/allocate semantics. */
export const TRACE_TYPES = new Set([
  "SatisfyRequirementUsage",
  "AllocationUsage",
  "VerifyRequirementUsage",
  "DeriveRequirementUsage",
  "TraceRequirementUsage",
]);

/** Forward trace relationship types (satisfy / allocate). Single-source export. */
export const FORWARD_TYPES = new Set(["SatisfyRequirementUsage", "AllocationUsage"]);
/** Verify relationship types. Single-source export. */
export const VERIFY_TYPES = new Set([
  "VerifyRequirementUsage",
  "RequirementVerificationMembership",
]);
/** Backward (derive) trace relationship types. Single-source export. */
export const BACKWARD_TYPES = new Set(["DeriveRequirementUsage"]);

const ORPHAN_TRACE_TYPES = new Set([
  "SatisfyRequirementUsage",
  "AllocationUsage",
  "DeriveRequirementUsage",
]);

// ---------------------------------------------------------------------------
// Main export
// ---------------------------------------------------------------------------

/**
 * Produce GATE-02 findings for a snapshot of model elements + relationships.
 *
 * Decision A3: R4-def-operand is always severity "error" — never downgraded.
 * The full audit reporting the legacy generator's Definition-operand edges as
 * errors is correct keystone behavior; the generator gets fixed when next
 * regenerated.
 */
export function relationalFindings(
  elements: SysmlElement[],
  relationships: SysmlRelationship[]
): Finding[] {
  const findings: Finding[] = [];

  // ── Build id lookup ──
  // allElementIds includes both element ids AND relationship ids (mirrors
  // validate-model.ts allElementIds semantics so dangling-endpoint checks
  // include relationships as valid resolution targets).
  const byId = new Map<string, SysmlElement>();
  for (const el of elements) {
    byId.set(el.id, el);
  }
  const allElementIds = new Set<string>([
    ...elements.map((e) => e.id),
    ...relationships.map((r) => r.id),
  ]);

  // ── Defect 5: GATE02-id-duplicate ──
  // Map-based duplicate scan over element ids; fires before orphan/coverage
  // so ordering matches defect numbering.
  {
    const seen = new Map<string, string>(); // id → name
    for (const el of elements) {
      if (seen.has(el.id)) {
        findings.push({
          elementId: el.id,
          ruleId: "GATE02-id-duplicate",
          severity: "error",
          message: `Duplicate element id '${el.id}' (names: '${seen.get(el.id)}', '${el.name ?? el.id}').`,
          suggestedFix: "Assign a unique id to each element.",
        });
      } else {
        seen.set(el.id, el.name ?? el.id);
      }
    }
  }

  // ── Defect 1: R4-def-operand ──
  // For each relationship in TRACE_TYPES, for each DISTINCT id in sourceIds ∪ targetIds,
  // resolve via byId; if resolved and NOT a Usage → error (decision A3: error, always).
  // WR-03: deduplicate before iterating to avoid duplicate findings for self-refs.
  for (const rel of relationships) {
    if (!TRACE_TYPES.has(rel.type)) continue;
    for (const id of [...new Set([...rel.sourceIds, ...rel.targetIds])]) {
      const el = byId.get(id);
      if (el && !el.type.endsWith("Usage")) {
        findings.push({
          elementId: id,
          ruleId: "R4-def-operand",
          severity: "error",
          message: `Trace ${rel.type} (${rel.id}) references Definition '${el.name ?? id}' (${el.type}); operands must be Usages.`,
          suggestedFix: `Re-target the ${rel.type} to a ${el.type.replace("Definition", "Usage")} instead of the Definition.`,
        });
      }
      // Unresolved ids are skipped here — the dangling rule owns them.
    }
  }

  // ── Defect 2: GATE02-dangling-endpoint ──
  for (const rel of relationships) {
    const danglingIds: string[] = [];
    for (const sid of rel.sourceIds) {
      if (!allElementIds.has(sid)) danglingIds.push(sid);
    }
    for (const tid of rel.targetIds) {
      if (!allElementIds.has(tid)) danglingIds.push(tid);
    }
    if (danglingIds.length > 0) {
      findings.push({
        elementId: rel.id,
        ruleId: "GATE02-dangling-endpoint",
        severity: "error",
        message: `Relationship ${rel.type} (${rel.id}) has dangling endpoint(s): ${danglingIds.join(", ")}`,
        suggestedFix: "Ensure all source/target element ids exist in the model.",
      });
    }
  }

  // ── Defect 3: GATE02-orphan ──
  // Leaf PartDefinition/ActionDefinition with no satisfy/allocate/derive edge
  // (either direction) and not a FeatureMembership container.
  // Mirrors validate-model.ts lines 94–113.
  const designTypes = new Set(["PartDefinition", "ActionDefinition"]);
  for (const el of elements) {
    if (!designTypes.has(el.type)) continue;

    // Build the set of rels touching this element
    const rels = relationships.filter(
      (r) => r.sourceIds.includes(el.id) || r.targetIds.includes(el.id)
    );

    const hasTraceEdge = rels.some((r) => ORPHAN_TRACE_TYPES.has(r.type));
    if (hasTraceEdge) continue;

    // Check container: owns >= 1 child via FeatureMembership (el is SOURCE)
    const isContainer = rels.some(
      (r) => r.type === "FeatureMembership" && r.sourceIds.includes(el.id)
    );
    if (isContainer) continue;

    findings.push({
      elementId: el.id,
      ruleId: "GATE02-orphan",
      severity: "warning",
      message: `Leaf element '${el.name ?? el.id}' (${el.type}) has no satisfy/allocate/derive edge.`,
      suggestedFix:
        "Add a SatisfyRequirementUsage, AllocationUsage, or DeriveRequirementUsage.",
    });
  }

  // ── Defects 4, 7: Completeness for system requirements (warn-only) ──
  // Needs: RequirementDefinition with raw.stakeholderNeed === true
  // System reqs: RequirementDefinition without raw.stakeholderNeed
  const needs = elements.filter(
    (e) => e.type === "RequirementDefinition" && e.raw.stakeholderNeed === true
  );
  const systemReqs = elements.filter(
    (e) => e.type === "RequirementDefinition" && e.raw.stakeholderNeed !== true
  );

  for (const req of systemReqs) {
    const rels = relationships.filter(
      (r) => r.sourceIds.includes(req.id) || r.targetIds.includes(req.id)
    );

    const hasForward = rels.some((r) => FORWARD_TYPES.has(r.type));
    if (!hasForward) {
      findings.push({
        elementId: req.id,
        ruleId: "GATE02-unsatisfied",
        severity: "warning",
        message: `System requirement '${req.name ?? req.id}' has no satisfy edge (SatisfyRequirementUsage or AllocationUsage).`,
        suggestedFix: "Add a SatisfyRequirementUsage or AllocationUsage from a design element.",
      });
    }

    const hasVerify = rels.some((r) => VERIFY_TYPES.has(r.type));
    if (!hasVerify) {
      findings.push({
        elementId: req.id,
        ruleId: "GATE02-unverified",
        severity: "warning",
        message: `System requirement '${req.name ?? req.id}' has no verify edge (VerifyRequirementUsage or RequirementVerificationMembership).`,
        suggestedFix:
          "Add a VerifyRequirementUsage or RequirementVerificationMembership from a verification case.",
      });
    }

    // CR-02 fix: req must be the SOURCE of the derive edge (req → need).
    // A req that is only the TARGET of a DeriveRequirementUsage (chained
    // derivation from a peer req) has NOT backtraced to a stakeholder Need.
    const hasBackward = rels.some(
      (r) => BACKWARD_TYPES.has(r.type) && r.sourceIds.includes(req.id)
    );
    if (!hasBackward) {
      findings.push({
        elementId: req.id,
        ruleId: "GATE02-unbacktraced",
        severity: "warning",
        message: `System requirement '${req.name ?? req.id}' has no backward derive trace (no DeriveRequirementUsage to a Need).`,
        suggestedFix:
          "Add a DeriveRequirementUsage from this requirement to a stakeholder Need.",
      });
    }
  }

  // ── Defect 6: GATE02-uncovered-need ──
  // A Need is covered iff it is the TARGET of >= 1 DeriveRequirementUsage.
  for (const need of needs) {
    const hasDeriveInbound = relationships.some(
      (r) => BACKWARD_TYPES.has(r.type) && r.targetIds.includes(need.id)
    );
    if (!hasDeriveInbound) {
      findings.push({
        elementId: need.id,
        ruleId: "GATE02-uncovered-need",
        severity: "warning",
        message: `Stakeholder need '${need.name ?? need.id}' has no inbound DeriveRequirementUsage (not covered by any system requirement).`,
        suggestedFix:
          "Add a DeriveRequirementUsage from a system requirement to this need.",
      });
    }
  }

  return findings;
}
