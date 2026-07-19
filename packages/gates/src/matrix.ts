/**
 * GATE-06 coverage matrix — one row per system requirement with
 * satisfied / verified / derived booleans.
 *
 * Legacy scope: RequirementDefinition elements only (not RequirementUsage).
 * This matches the existing coverage semantics in validate-model.ts.
 * RequirementUsage elements are intentionally excluded so the matrix rows
 * align 1-to-1 with the RequirementDefinition nodes that the validate_model
 * tool has always counted; this is noted here for future extension.
 *
 * Edge-type sets imported from relational.ts (single source of truth — WR-02 fix):
 *   FORWARD_TYPES  = SatisfyRequirementUsage | AllocationUsage
 *   VERIFY_TYPES   = VerifyRequirementUsage | RequirementVerificationMembership
 *   BACKWARD_TYPES = DeriveRequirementUsage
 *
 * An edge "touches" a requirement if req.id appears in EITHER sourceIds OR targetIds
 * (equivalent of validate-model's queryRelationships(req.id, "both")).
 */

import type { SysmlElement, SysmlRelationship } from "@sysml-bridge/model";
import type { MatrixRow } from "./findings.js";
import { FORWARD_TYPES, VERIFY_TYPES, BACKWARD_TYPES } from "./relational.js";

/**
 * Build the GATE-06 requirement coverage matrix.
 *
 * @param elements      All SysML elements in the model
 * @param relationships All relationships in the model
 * @returns             One MatrixRow per system requirement (RequirementDefinition
 *                      WITHOUT raw.stakeholderNeed === true)
 */
export function coverageMatrix(
  elements: SysmlElement[],
  relationships: SysmlRelationship[]
): MatrixRow[] {
  // Needs/systemReqs split — copy from validate-model.ts lines 36–37
  const requirements = elements.filter((el) => el.type === "RequirementDefinition");
  const systemReqs = requirements.filter((r) => r.raw.stakeholderNeed !== true);

  const rows: MatrixRow[] = [];

  for (const req of systemReqs) {
    // Scan all relationships for edges touching req.id in either sourceIds or targetIds
    // (array-based equivalent of smaps.queryRelationships(req.id, "both"))
    const touching = relationships.filter(
      (rel) => rel.sourceIds.includes(req.id) || rel.targetIds.includes(req.id)
    );

    const satisfied = touching.some((r) => FORWARD_TYPES.has(r.type));
    const verified = touching.some((r) => VERIFY_TYPES.has(r.type));
    // CR-02 fix: req must be the SOURCE of the derive edge (req → need).
    const derived = touching.some(
      (r) => BACKWARD_TYPES.has(r.type) && r.sourceIds.includes(req.id)
    );

    rows.push({
      reqId: req.id,
      reqName: req.name,
      satisfied,
      verified,
      derived,
    });
  }

  return rows;
}
