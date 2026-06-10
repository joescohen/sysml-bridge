import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";
import { structuralCheck, resolveGateCorpus } from "../audit/structural.js";
import type { Finding } from "../audit/findings.js";
import { TRACE_TYPES } from "../audit/relational.js";
import { SYSML_RELATIONSHIP_TYPES } from "../types/sysml-elements.js";

export function registerUpdateElement(server: McpServer, smaps: ModelStore) {
  server.tool(
    "update_element",
    "Update an existing SysML v2 element: rename, retype, or retarget its source/target endpoints. Runs GATE-05 structural check before persisting — rejects on error-severity findings unless allow_invalid is set.",
    {
      element_id: z.string().describe("ID of the element to update"),
      updates: z
        .record(z.unknown())
        .describe(
          "Fields to update (e.g. { name: 'NewName' }, { source: [{'@id': 'id1'}] }, { type: 'PartUsage' })"
        ),
      allow_invalid: z
        .boolean()
        .optional()
        .describe(
          "Skip GATE-05 structural checks — only for multi-step construction where intermediate states are incomplete"
        ),
    },
    async ({ element_id, updates, allow_invalid }) => {
      try {
        // ── Resolve existing element ──
        const allElements = await smaps.queryElements();
        const existing = allElements.find(
          (e) => e.id === element_id || e.elementId === element_id
        );
        if (!existing) {
          return {
            isError: true,
            content: [
              {
                type: "text" as const,
                text: `Element not found: ${element_id}`,
              },
            ],
          };
        }

        // ── GATE-05 pre-update structural check (check-BEFORE-update) ──
        // Build the post-update Candidate by merging updates over existing values.
        const extractIds = (key: string, overrideVal: unknown): string[] => {
          // Use overrideVal if present, else fall back to existing raw
          const val = overrideVal !== undefined ? overrideVal : existing.raw[key];
          if (!Array.isArray(val)) return [];
          return val
            .filter(
              (v): v is { "@id": string } =>
                v !== null && typeof v === "object" && "@id" in v
            )
            .map((v) => v["@id"]);
        };

        const candidateType =
          typeof updates.type === "string" ? updates.type : existing.type;
        const candidateName =
          typeof updates.name === "string" ? updates.name : (existing.name ?? undefined);
        const sourceIds = extractIds("source", updates.source);
        const targetIds = extractIds("target", updates.target);
        const provenanceSourceId =
          "provenanceSourceId" in updates
            ? updates.provenanceSourceId
            : existing.raw.provenanceSourceId;

        // ── CR-02 guard: reject any update that would clear ALL endpoints on
        // a relationship-kind element.  The existing structural gate (GATE02)
        // short-circuits when both arrays are empty, so it cannot catch this
        // class of update.  Reject early, before the gate and before the store
        // is touched, so the store is never mutated by this path.
        const isRelType =
          TRACE_TYPES.has(candidateType) ||
          (SYSML_RELATIONSHIP_TYPES as readonly string[]).includes(candidateType);
        if (
          isRelType &&
          sourceIds.length === 0 &&
          targetIds.length === 0 &&
          (updates.source !== undefined || updates.target !== undefined)
        ) {
          return {
            isError: true,
            content: [
              {
                type: "text" as const,
                text: JSON.stringify(
                  {
                    rejected: true,
                    findings: [
                      {
                        elementId: element_id,
                        ruleId: "EDIT-empty-endpoints",
                        severity: "error",
                        message:
                          "Cannot clear all endpoints on a relationship element.",
                        suggestedFix:
                          "Provide at least one source and one target id, or retarget to valid endpoints.",
                      },
                    ],
                  },
                  null,
                  2
                ),
              },
            ],
          };
        }

        const candidate = {
          id: existing.id,
          type: candidateType,
          name: candidateName,
          sourceIds,
          targetIds,
          provenanceSourceId,
        };

        // Build existing-set: elements + relationships mapped to id-bearing shapes
        // (copy create-relationship.ts existingForGate verbatim so retarget dangling
        // checks resolve against both elements and relationship ids)
        const existingRelationships = await smaps.queryRelationships();
        const existingForGate = [
          ...allElements,
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

        const resolutionSet = await resolveGateCorpus();
        const findings: Finding[] = structuralCheck(
          candidate,
          existingForGate,
          resolutionSet
        );
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
        const element = await smaps.updateElement(element_id, updates);

        // RESPONSE SHAPE RULE (plan B2): when ANY findings exist, return
        // {element, findings} with the FULL set — including error-severity
        // findings when allow_invalid bypassed them (bypass must be auditable
        // in transcripts; never filter errors out). When zero findings, return
        // the element verbatim.
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
