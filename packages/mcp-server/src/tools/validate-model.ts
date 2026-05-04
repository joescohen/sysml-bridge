import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerValidateModel(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "validate_model",
    "Run consistency and completeness checks against the model — orphaned elements, unsatisfied requirements, missing connections",
    {
      scope: z.string().optional().describe("Element ID to scope validation to, or omit for full model"),
    },
    async ({ scope }) => {
      const state = await smaps.getProjectState();
      const requirements = await smaps.queryElements("RequirementDefinition");
      const satisfyRels = await smaps.queryRelationships(undefined, "SatisfyRequirementUsage");

      const satisfiedReqIds = new Set(satisfyRels.map((r) => r.targetId));
      const unsatisfied = requirements.filter((r) => !satisfiedReqIds.has(r.id));

      const parts = await smaps.queryElements("PartDefinition");
      const connections = await smaps.queryElements("ConnectionUsage");

      const issues: string[] = [];

      if (unsatisfied.length > 0) {
        issues.push(
          `${unsatisfied.length} requirements not satisfied by any element: ${unsatisfied.map((r) => r.name).join(", ")}`
        );
      }

      if (parts.length > 0 && connections.length === 0) {
        issues.push("Part definitions exist but no connections defined — IBD may be incomplete");
      }

      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(
              {
                summary: state,
                issues,
                requirementCoverage: {
                  total: requirements.length,
                  satisfied: satisfiedReqIds.size,
                  unsatisfied: unsatisfied.length,
                  coveragePercent:
                    requirements.length > 0
                      ? Math.round((satisfiedReqIds.size / requirements.length) * 100)
                      : 0,
                },
              },
              null,
              2
            ),
          },
        ],
      };
    }
  );
}
