import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";
import { SYSML_RELATIONSHIP_TYPES } from "../types/sysml-elements.js";

export function registerCreateRelationship(server: McpServer, smaps: ModelStore) {
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
      const allowed = SYSML_RELATIONSHIP_TYPES as readonly string[];
      if (!allowed.includes(type)) {
        return {
          content: [
            {
              type: "text" as const,
              text: `Error: unknown relationship type '${type}'. Allowed: ${SYSML_RELATIONSHIP_TYPES.join(", ")}`,
            },
          ],
          isError: true,
        };
      }
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
