import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";
import { serializeToSysml } from "../utils/sysml-serializer.js";

export function registerExportSysml(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "export_sysml",
    "Export model elements as SysML v2 textual notation (.sysml format)",
    {
      scope: z.string().optional().describe("Element ID to export, or omit for full model"),
    },
    async () => {
      try {
        const elements = await smaps.queryElements();
        const relationships = await smaps.queryRelationships();
        const sysmlText = serializeToSysml(elements, relationships);
        return {
          content: [{ type: "text" as const, text: sysmlText }],
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
