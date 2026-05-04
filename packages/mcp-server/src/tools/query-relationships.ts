import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerQueryRelationships(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "query_relationships",
    "Get relationships for an element. Uses the SMAPS relationships endpoint with direction filtering.",
    {
      element_id: z.string().optional().describe("Element to query relationships for"),
      direction: z
        .enum(["in", "out", "both"])
        .optional()
        .default("both")
        .describe("Relationship direction: in (targeting this element), out (sourced from), both"),
    },
    async ({ element_id, direction }) => {
      try {
        const rels = await smaps.queryRelationships(element_id, direction);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ count: rels.length, relationships: rels }, null, 2),
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
