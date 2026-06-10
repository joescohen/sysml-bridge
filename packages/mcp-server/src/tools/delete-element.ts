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
          "Delete even if it would leave dangling relationship endpoints"
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
        // Collect any relationship whose sourceIds OR targetIds includes element_id.
        const allRelationships = await smaps.queryRelationships();
        const danglingRels = allRelationships.filter(
          (r) =>
            r.sourceIds.includes(element_id) || r.targetIds.includes(element_id)
        );

        const findings: Finding[] = danglingRels.map((r) => ({
          elementId: r.id,
          ruleId: "EDIT-delete-would-dangle",
          severity: "error" as const,
          message: `Deleting element '${element_id}' would leave relationship '${r.type}' (${r.id}) with a dangling endpoint.`,
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
        await smaps.deleteElement(element_id);

        // RESPONSE SHAPE: when findings existed (allow_invalid bypass),
        // include them for auditability. When zero findings, return bare result.
        if (findings.length > 0) {
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ deleted: true, elementId: element_id, findings }, null, 2),
              },
            ],
          };
        }

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ deleted: true, elementId: element_id }, null, 2),
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
