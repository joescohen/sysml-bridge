import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";

export function registerCreateElement(server: McpServer, smaps: ModelStore) {
  server.tool(
    "create_element",
    "Create a SysML v2 element via a SMAPS commit. Supports all SysML v2 types: PartDefinition, RequirementDefinition, ActionDefinition, etc.",
    {
      type: z
        .string()
        .describe("SysML v2 element type (e.g. PartDefinition, RequirementDefinition, Package)"),
      name: z.string().describe("Element name"),
      attributes: z
        .record(z.unknown())
        .optional()
        .describe("Additional element attributes"),
    },
    async ({ type, name, attributes }) => {
      try {
        const element = await smaps.createElement(type, name, attributes ?? {});
        return {
          content: [{ type: "text" as const, text: JSON.stringify(element, null, 2) }],
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
