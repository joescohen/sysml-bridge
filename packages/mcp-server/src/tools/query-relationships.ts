import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerQueryRelationships(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "query_relationships",
    "Get relationships for an element or by type (satisfy, verify, refine, allocate)",
    {
      element_id: z.string().optional().describe("Filter relationships involving this element"),
      type: z.string().optional().describe("Filter by relationship type"),
    },
    async ({ element_id, type }) => {
      const rels = await smaps.queryRelationships(element_id, type);
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(rels, null, 2),
          },
        ],
      };
    }
  );
}
