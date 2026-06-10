import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";
import type { Finding } from "../audit/findings.js";

export function registerDeleteElement(server: McpServer, smaps: ModelStore) {
  server.tool(
    "delete_element",
    "Delete a SysML v2 element from the store. Refuses to strand relationship endpoints unless allow_invalid is set.",
    {
      element_id: z.string().describe("ID of the element to delete"),
      allow_invalid: z
        .boolean()
        .optional()
        .describe(
          "Skip GATE-05 structural check — bypasses only the would-dangle guard; deletes even if it would leave dangling relationship endpoints"
        ),
    },
    async ({ element_id, allow_invalid }) => {
      try {
        // ── Existence check: stricter than the store's silent no-op ──
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

        // ── Pre-check: would-dangle relationships ──
        // Normalize to the canonical primary key (existing.id = "@id" / randomUUID).
        // The existence check above accepts both `id` and `elementId`; on backends
        // where they differ (e.g. SMAPS), sourceIds/targetIds hold `id` values, so
        // the dangle check must compare against existing.id — not the caller-supplied
        // alias — to avoid a phantom "clean" delete that bypasses the guard.
        const canonicalId = existing.id;
        const allRelationships = await smaps.queryRelationships();
        const danglingRels = allRelationships.filter(
          (r) =>
            r.sourceIds.includes(canonicalId) || r.targetIds.includes(canonicalId)
        );

        const findings: Finding[] = danglingRels.map((r) => ({
          elementId: r.id,
          ruleId: "EDIT-delete-would-dangle",
          severity: "error" as const,
          message: `Deleting element '${canonicalId}' would leave relationship '${r.type}' (${r.id}) with a dangling endpoint.`,
          suggestedFix:
            "Delete or retarget the relationship before deleting this element, or pass allow_invalid to force.",
        }));

        if (findings.length > 0 && !allow_invalid) {
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
        await smaps.deleteElement(canonicalId);

        // RESPONSE SHAPE: when findings existed (allow_invalid bypass),
        // include them for auditability. When zero findings, return bare result.
        if (findings.length > 0) {
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ deleted: true, elementId: canonicalId, findings }, null, 2),
              },
            ],
          };
        }

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ deleted: true, elementId: canonicalId }, null, 2),
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
