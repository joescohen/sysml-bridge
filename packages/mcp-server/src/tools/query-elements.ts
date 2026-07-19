import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";

export function registerQueryElements(server: McpServer, smaps: ModelStore) {
  server.tool(
    "query_elements",
    "Find SysML v2 elements by type and/or name pattern.",
    {
      type: z.string().optional().describe("Filter by element type (e.g. PartDefinition)"),
      name_pattern: z.string().optional().describe("Filter by name (substring match)"),
    },
    async ({ type, name_pattern }) => {
      try {
        const elements = await smaps.queryElements(type, name_pattern);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                { count: elements.length, elements },
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
