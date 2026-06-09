import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import * as path from "node:path";
import { z } from "zod";

import { SmapsClient } from "./smaps-client.js";
import { FileStore } from "./file-store.js";
import type { ModelStore } from "./store.js";
import { registerCreateElement } from "./tools/create-element.js";
import { registerQueryElements } from "./tools/query-elements.js";
import { registerCreateRelationship } from "./tools/create-relationship.js";
import { registerQueryRelationships } from "./tools/query-relationships.js";
import { registerValidateModel } from "./tools/validate-model.js";
import { registerExportSysml } from "./tools/export-sysml.js";
import { registerImportSysml } from "./tools/import-sysml.js";
import { registerGetProjectState } from "./tools/get-project-state.js";

// ---------------------------------------------------------------------------
// Backend selection
//
//   SYSML_BRIDGE_BACKEND=file   (default) → file-native .sysml/JSON store, no server
//   SYSML_BRIDGE_BACKEND=smaps            → live SMAPS REST backend (Pilot / Cameo)
//
// Per RESEARCH.md the consensus is file-native, so it is the default. SMAPS
// remains available behind the same ModelStore interface for the live-model /
// Cameo-server path. The 8 tools below are backend-agnostic.
// ---------------------------------------------------------------------------

const BACKEND = (process.env.SYSML_BRIDGE_BACKEND ?? "file").toLowerCase();
const SMAPS_ENDPOINT = process.env.SMAPS_ENDPOINT ?? "http://localhost:9000";
const MODEL_DIR =
  process.env.SYSML_BRIDGE_MODEL_DIR ?? path.join(process.cwd(), ".sysml-bridge");

const server = new McpServer({
  name: "sysml-bridge",
  version: "0.1.0",
});

const store: ModelStore =
  BACKEND === "smaps" ? new SmapsClient(SMAPS_ENDPOINT) : new FileStore(MODEL_DIR);

server.tool(
  "init_project",
  "Initialize or load a project. Must be called before using other tools.",
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
        const project = await store.createProject(name);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  status: "created",
                  backend: BACKEND,
                  projectId: project["@id"],
                  branchId: store.branchId,
                },
                null,
                2
              ),
            },
          ],
        };
      }

      const projects = await store.listProjects();
      const found = projects.find((p) => p.name === name);
      if (!found) {
        return {
          content: [{ type: "text" as const, text: `Project "${name}" not found` }],
          isError: true,
        };
      }

      const project = await store.loadProject(found["@id"]);
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(
              {
                status: "loaded",
                backend: BACKEND,
                projectId: project["@id"],
                branchId: store.branchId,
                headCommitId: store.headCommitId,
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

registerCreateElement(server, store);
registerQueryElements(server, store);
registerCreateRelationship(server, store);
registerQueryRelationships(server, store);
registerValidateModel(server, store);
registerExportSysml(server, store);
registerImportSysml(server, store);
registerGetProjectState(server, store);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch(console.error);
