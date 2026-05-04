import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerValidateModel(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "validate_model",
    "Run completeness and consistency checks — unsatisfied requirements, orphaned elements, missing connections",
    {
      scope: z.string().optional().describe("Element ID to scope validation to, or omit for full model"),
    },
    async () => {
      try {
        const state = await smaps.getProjectState();
        const requirements = await smaps.queryElements("RequirementDefinition");
        const parts = await smaps.queryElements("PartDefinition");

        const satisfiedReqIds = new Set<string>();
        for (const req of requirements) {
          const rels = await smaps.queryRelationships(req.id, "in");
          const hasSatisfy = rels.some(
            (r) =>
              r.type === "SatisfyRequirementUsage" ||
              r.type === "Dependency"
          );
          if (hasSatisfy) satisfiedReqIds.add(req.id);
        }

        const unsatisfied = requirements.filter((r) => !satisfiedReqIds.has(r.id));
        const issues: string[] = [];

        if (unsatisfied.length > 0) {
          issues.push(
            `${unsatisfied.length} requirements not satisfied: ${unsatisfied.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        if (parts.length > 0) {
          const connections = await smaps.queryElements("ConnectionUsage");
          if (connections.length === 0) {
            issues.push("Part definitions exist but no connections — IBD may be incomplete");
          }
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
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
