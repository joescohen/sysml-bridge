import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerCreateRelationship(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "create_relationship",
    "Create a relationship between two SysML v2 elements (satisfy, verify, refine, allocate, compose)",
    {
      type: z.string().describe("Relationship type (e.g. SatisfyRequirementUsage, Dependency, Specialization)"),
      source_id: z.string().describe("Source element ID"),
      target_id: z.string().describe("Target element ID"),
      attributes: z.record(z.unknown()).optional().describe("Additional relationship attributes"),
    },
    async ({ type, source_id, target_id, attributes }) => {
      const rel = await smaps.createRelationship(type, source_id, target_id, attributes ?? {});
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(rel, null, 2),
          },
        ],
      };
    }
  );
}
