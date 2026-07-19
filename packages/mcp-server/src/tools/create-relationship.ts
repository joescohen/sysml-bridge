import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";
import { SYSML_RELATIONSHIP_TYPES } from "@sysml-bridge/model";
import { structuralCheck, resolveGateCorpus } from "@sysml-bridge/gates";
import type { Finding } from "@sysml-bridge/gates";

export function registerCreateRelationship(server: McpServer, smaps: ModelStore) {
  server.tool(
    "create_relationship",
    "Create a relationship between SysML v2 elements (Dependency, Specialization, SatisfyRequirementUsage, etc.)",
    {
      type: z
        .string()
        .describe("Relationship type (e.g. Dependency, Specialization, SatisfyRequirementUsage)"),
      source_id: z.string().describe("Source element ID"),
      target_id: z.string().describe("Target element ID"),
      attributes: z.record(z.unknown()).optional().describe("Additional attributes"),
      allow_invalid: z
        .boolean()
        .optional()
        .describe(
          "Skip structural Gate 1 checks — only for multi-step construction where intermediate states are incomplete"
        ),
    },
    async ({ type, source_id, target_id, attributes, allow_invalid }) => {
      // ── Existing SYSML_RELATIONSHIP_TYPES guard (keep verbatim) ──
      const allowed = SYSML_RELATIONSHIP_TYPES as readonly string[];
      if (!allowed.includes(type)) {
        return {
          content: [
            {
              type: "text" as const,
              text: `Error: unknown relationship type '${type}'. Allowed: ${SYSML_RELATIONSHIP_TYPES.join(", ")}`,
            },
          ],
          isError: true,
        };
      }

      try {
        // ── GATE-05 pre-add structural check (check-BEFORE-add) ──
        // Build the existing element set: elements + relationships mapped to
        // id-bearing shapes (mirrors validate-model.ts allElementIds semantics
        // so dangling-endpoint checks include relationship ids as valid targets).
        const existingElements = await smaps.queryElements();
        const existingRelationships = await smaps.queryRelationships();
        const existingForGate = [
          ...existingElements,
          ...existingRelationships.map((r) => ({
            id: r.id,
            elementId: r.id,
            type: r.type,
            name: null as null,
            shortName: null as null,
            qualifiedName: null as null,
            ownerId: null as null,
            ownedElementIds: [] as string[],
            raw: r.raw,
          })),
        ];

        const candidate = {
          type,
          sourceIds: [source_id],
          targetIds: [target_id],
          provenanceSourceId: attributes?.provenanceSourceId,
        };

        const resolutionSet = await resolveGateCorpus();
        const findings: Finding[] = structuralCheck(candidate, existingForGate, resolutionSet);
        const errors = findings.filter((f) => f.severity === "error");

        if (errors.length > 0 && !allow_invalid) {
          // Reject: store untouched
          return {
            isError: true,
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ rejected: true, findings }, null, 2),
              },
            ],
          };
        }

        // ── Persist ──
        const element = await smaps.createElement(type, "", {
          source: [{ "@id": source_id }],
          target: [{ "@id": target_id }],
          ...(attributes ?? {}),
        });

        // RESPONSE SHAPE RULE (plan B2): when ANY findings exist, return
        // {element, findings} with the FULL set — including error-severity
        // findings when allow_invalid bypassed them (bypass must be auditable
        // in transcripts; never filter errors out). When zero findings, return
        // the element verbatim so existing tests parse the bare element.
        if (findings.length > 0) {
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ element, findings }, null, 2),
              },
            ],
          };
        }

        return {
          content: [{ type: "text" as const, text: JSON.stringify(element, null, 2) }],
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
