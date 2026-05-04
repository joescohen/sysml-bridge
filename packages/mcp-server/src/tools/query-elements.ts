import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerQueryElements(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "query_elements",
    "Find SysML v2 elements by type and/or name pattern",
    {
      type: z.string().optional().describe("Filter by element type"),
      name_pattern: z.string().optional().describe("Filter by name (substring match)"),
    },
    async ({ type, name_pattern }) => {
      const elements = await smaps.queryElements(type, name_pattern);
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(elements, null, 2),
          },
        ],
      };
    }
  );
}
