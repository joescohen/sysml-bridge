import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerCreateRelationship(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "create_relationship",
    "Create a relationship between SysML v2 elements (Dependency, Specialization, SatisfyRequirementUsage, etc.)",
    {
      type: z
        .string()
        .describe("Relationship type (e.g. Dependency, Specialization, SatisfyRequirementUsage)"),
      source_id: z.string().describe("Source element ID"),
      target_id: z.string().describe("Target element ID"),
      attributes: z.record(z.unknown()).optional().describe("Additional attributes"),
    },
    async ({ type, source_id, target_id, attributes }) => {
      try {
        const element = await smaps.createElement(type, "", {
          source: [{ "@id": source_id }],
          target: [{ "@id": target_id }],
          ...(attributes ?? {}),
        });
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
