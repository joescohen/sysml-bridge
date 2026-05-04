import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerGetProjectState(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "get_project_state",
    "Get a summary of the current model — element counts by type, coverage statistics",
    {},
    async () => {
      const state = await smaps.getProjectState();
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(state, null, 2),
          },
        ],
      };
    }
  );
}
