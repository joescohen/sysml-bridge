import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";

export function registerQueryRelationships(server: McpServer, smaps: ModelStore) {
  server.tool(
    "query_relationships",
    "Get relationships for an element, with direction filtering.",
    {
      element_id: z.string().optional().describe("Element to query relationships for"),
      direction: z
        .enum(["in", "out", "both"])
        .optional()
        .default("both")
        .describe("Relationship direction: in (targeting this element), out (sourced from), both"),
      type: z
        .string()
        .optional()
        .describe("Filter to a single relationship type, e.g. SatisfyRequirementUsage"),
    },
    async ({ element_id, direction, type }) => {
      try {
        const rels = await smaps.queryRelationships(element_id, direction);
        const filtered = type ? rels.filter((r) => r.type === type) : rels;
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ count: filtered.length, relationships: filtered }, null, 2),
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
