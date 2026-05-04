import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerCreateElement(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "create_element",
    "Create a SysML v2 element (part definition, requirement, action, etc.)",
    {
      type: z.string().describe("SysML v2 element type (e.g. PartDefinition, RequirementDefinition)"),
      name: z.string().describe("Element name"),
      attributes: z.record(z.unknown()).optional().describe("Additional element attributes"),
    },
    async ({ type, name, attributes }) => {
      const element = await smaps.createElement(type, name, attributes ?? {});
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(element, null, 2),
          },
        ],
      };
    }
  );
}
