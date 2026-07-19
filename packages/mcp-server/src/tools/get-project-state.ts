import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";

export function registerGetProjectState(server: McpServer, smaps: ModelStore) {
  server.tool(
    "get_project_state",
    "Get a summary of the current model — element counts by type, project/branch/commit IDs",
    {},
    async () => {
      try {
        const state = await smaps.getProjectState();
        return {
          content: [{ type: "text" as const, text: JSON.stringify(state, null, 2) }],
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
