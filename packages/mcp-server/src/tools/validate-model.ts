import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";

export function registerValidateModel(server: McpServer, smaps: ModelStore) {
  server.tool(
    "validate_model",
    "Run completeness and consistency checks — unsatisfied requirements, orphaned elements, missing connections",
    {
      scope: z.string().optional().describe("Element ID to scope validation to, or omit for full model"),
    },
    async () => {
      try {
        const state = await smaps.getProjectState();
        const requirements = await smaps.queryElements("RequirementDefinition");
        const parts = await smaps.queryElements("PartDefinition");
        const actions = await smaps.queryElements("ActionDefinition");

        // Collect all relationships once for dangling endpoint check
        const allRels = await smaps.queryRelationships();
        const allElementIds = new Set<string>(
          [
            ...requirements,
            ...parts,
            ...actions,
            // Also include any other element types that may exist as endpoints
            ...(await smaps.queryElements()),
          ].map((e) => e.id)
        );

        // ── 1. Forward trace (SatisfyRequirementUsage OR AllocationUsage, outgoing from req OR incoming) ──
        // A requirement is forward-traced if it has >= 1 edge of either type in either direction.
        // The spec says "outgoing" but in practice tools wire source=part, target=req (incoming to req).
        // We check both directions for robustness: either the req appears as source OR target of
        // SatisfyRequirementUsage / AllocationUsage.
        const FORWARD_TYPES = new Set(["SatisfyRequirementUsage", "AllocationUsage"]);
        const VERIFY_TYPES = new Set(["VerifyRequirementUsage", "RequirementVerificationMembership"]);
        const BACKWARD_TYPES = new Set(["DeriveRequirementUsage"]);

        const forwardTracedIds = new Set<string>();
        const verifiedIds = new Set<string>();
        const backwardTracedIds = new Set<string>();

        for (const req of requirements) {
          const rels = await smaps.queryRelationships(req.id, "both");

          // Forward: any edge of forward type touching this requirement
          const hasForward = rels.some((r) => FORWARD_TYPES.has(r.type));
          if (hasForward) forwardTracedIds.add(req.id);

          // Verify: any edge of verify type, either direction
          const hasVerify = rels.some((r) => VERIFY_TYPES.has(r.type));
          if (hasVerify) verifiedIds.add(req.id);

          // Backward: any DeriveRequirementUsage edge, either direction
          const hasBackward = rels.some((r) => BACKWARD_TYPES.has(r.type));
          if (hasBackward) backwardTracedIds.add(req.id);
        }

        const totalReqs = requirements.length;
        const forwardPercent =
          totalReqs > 0 ? Math.round((forwardTracedIds.size / totalReqs) * 100) : 0;
        const verifyPercent =
          totalReqs > 0 ? Math.round((verifiedIds.size / totalReqs) * 100) : 0;
        const backwardPercent =
          totalReqs > 0 ? Math.round((backwardTracedIds.size / totalReqs) * 100) : 0;

        // ── 4. Orphan design elements (PartDefinition, ActionDefinition with no inbound satisfy/derive) ──
        const ORPHAN_INBOUND_TYPES = new Set(["SatisfyRequirementUsage", "DeriveRequirementUsage"]);
        const designElements = [...parts, ...actions];
        const orphanElements: Array<{ id: string; name: string | null; type: string }> = [];

        for (const el of designElements) {
          const rels = await smaps.queryRelationships(el.id, "in");
          const hasInbound = rels.some((r) => ORPHAN_INBOUND_TYPES.has(r.type));
          if (!hasInbound) {
            orphanElements.push({ id: el.id, name: el.name, type: el.type });
          }
        }

        // ── 5. Provenance coverage ──
        // RequirementDefinition + ActionDefinition + PartDefinition missing raw.provenanceSourceId
        const provenanceElements = [...requirements, ...parts, ...actions];
        const elementsMissingBackpointer: Array<{ id: string; name: string | null; type: string }> = [];

        for (const el of provenanceElements) {
          const prov = el.raw.provenanceSourceId;
          if (!prov || (typeof prov === "string" && prov.trim() === "")) {
            elementsMissingBackpointer.push({ id: el.id, name: el.name, type: el.type });
          }
        }

        const provenanceCoverage =
          provenanceElements.length > 0
            ? Math.round(
                ((provenanceElements.length - elementsMissingBackpointer.length) /
                  provenanceElements.length) *
                  100
              )
            : 100;

        // ── 6. Dangling relationships (source or target resolves to no existing element) ──
        const danglingRelationships: Array<{ id: string; type: string; danglingIds: string[] }> = [];

        for (const rel of allRels) {
          const danglingIds: string[] = [];
          for (const sid of rel.sourceIds) {
            if (!allElementIds.has(sid)) danglingIds.push(sid);
          }
          for (const tid of rel.targetIds) {
            if (!allElementIds.has(tid)) danglingIds.push(tid);
          }
          if (danglingIds.length > 0) {
            danglingRelationships.push({ id: rel.id, type: rel.type, danglingIds });
          }
        }

        // ── Build issues list ──
        const issues: string[] = [];

        const unsatisfied = requirements.filter((r) => !forwardTracedIds.has(r.id));
        if (unsatisfied.length > 0) {
          issues.push(
            `${unsatisfied.length} requirements not forward-traced (no SatisfyRequirementUsage or AllocationUsage): ${unsatisfied.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        const unverified = requirements.filter((r) => !verifiedIds.has(r.id));
        if (unverified.length > 0) {
          issues.push(
            `${unverified.length} requirements not verified (no VerifyRequirementUsage or RequirementVerificationMembership): ${unverified.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        const unbacktraced = requirements.filter((r) => !backwardTracedIds.has(r.id));
        if (unbacktraced.length > 0) {
          issues.push(
            `${unbacktraced.length} requirements missing backward trace (no DeriveRequirementUsage): ${unbacktraced.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        if (orphanElements.length > 0) {
          issues.push(
            `${orphanElements.length} design elements have no inbound satisfy/derive edge (orphans): ${orphanElements.map((e) => e.name ?? e.id).join(", ")}`
          );
        }

        if (elementsMissingBackpointer.length > 0) {
          issues.push(
            `${elementsMissingBackpointer.length} elements missing provenanceSourceId: ${elementsMissingBackpointer.map((e) => e.name ?? e.id).join(", ")}`
          );
        }

        if (danglingRelationships.length > 0) {
          issues.push(
            `${danglingRelationships.length} relationships have dangling endpoint(s): ${danglingRelationships.map((r) => r.id).join(", ")}`
          );
        }

        if (parts.length > 0) {
          const connections = await smaps.queryElements("ConnectionUsage");
          if (connections.length === 0) {
            issues.push("Part definitions exist but no connections — IBD may be incomplete");
          }
        }

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  summary: state,
                  issues,
                  coverage: {
                    forwardPercent,
                    verifyPercent,
                    backwardPercent,
                    orphanElements,
                    provenanceCoverage,
                    elementsMissingBackpointer,
                    danglingRelationships,
                  },
                },
                null,
                2
              ),
            },
          ],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
