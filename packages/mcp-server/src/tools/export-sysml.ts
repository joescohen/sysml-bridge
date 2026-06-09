import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";
import { serializeToSysml } from "../utils/sysml-serializer.js";

export function registerExportSysml(server: McpServer, smaps: ModelStore) {
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
        // Relationship-elements (satisfy/verify/allocate/etc.) are not yet
        // emitted as first-class textual statements by the serializer, so
        // exclude them here to keep the .sysml clean for Cameo import.
        const relationshipIds = new Set(relationships.map((r) => r.id));
        const structuralElements = elements.filter((e) => !relationshipIds.has(e.id));
        const sysmlText = serializeToSysml(structuralElements, relationships);
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
