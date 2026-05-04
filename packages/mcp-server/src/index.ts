import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { SmapsClient } from "./smaps-client.js";
import { registerCreateElement } from "./tools/create-element.js";
import { registerQueryElements } from "./tools/query-elements.js";
import { registerCreateRelationship } from "./tools/create-relationship.js";
import { registerQueryRelationships } from "./tools/query-relationships.js";
import { registerValidateModel } from "./tools/validate-model.js";
import { registerExportSysml } from "./tools/export-sysml.js";
import { registerImportSysml } from "./tools/import-sysml.js";
import { registerGetProjectState } from "./tools/get-project-state.js";
import { z } from "zod";

const SMAPS_ENDPOINT = process.env.SMAPS_ENDPOINT ?? "http://localhost:9000";

const server = new McpServer({
  name: "sysml-bridge",
  version: "0.1.0",
});

const smaps = new SmapsClient(SMAPS_ENDPOINT);

server.tool(
  "init_project",
  "Initialize or load a SMAPS project. Must be called before using other tools.",
  {
    name: z.string().describe("Project name to create or load"),
    create: z
      .boolean()
      .optional()
      .default(true)
      .describe("Create a new project (true) or load existing (false)"),
  },
  async ({ name, create }) => {
    try {
      if (create) {
        const project = await smaps.createProject(name);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  status: "created",
                  projectId: project["@id"],
                  branchId: smaps.branchId,
                },
                null,
                2
              ),
            },
          ],
        };
      }

      const projects = await smaps.listProjects();
      const found = projects.find((p) => p.name === name);
      if (!found) {
        return {
          content: [{ type: "text" as const, text: `Project "${name}" not found` }],
          isError: true,
        };
      }

      const project = await smaps.loadProject(found["@id"]);
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(
              {
                status: "loaded",
                projectId: project["@id"],
                branchId: smaps.branchId,
                headCommitId: smaps.headCommitId,
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

registerCreateElement(server, smaps);
registerQueryElements(server, smaps);
registerCreateRelationship(server, smaps);
registerQueryRelationships(server, smaps);
registerValidateModel(server, smaps);
registerExportSysml(server, smaps);
registerImportSysml(server, smaps);
registerGetProjectState(server, smaps);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch(console.error);
