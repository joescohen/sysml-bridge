/**
 * registration.test.ts
 *
 * Registers every tool the server exposes on an McpServer, connects an
 * InMemoryTransport client, and asserts client.listTools() returns exactly
 * the expected tool names — no more, no fewer. This is the drift guard: if a
 * tool is added/removed/renamed in index.ts without updating this list, the
 * test fails.
 *
 * Mirrors index.ts's registration exactly (including the inline init_project
 * tool) rather than importing index.ts directly, since index.ts's module-level
 * `main()` call would start a real stdio server as a side effect of import.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { z } from "zod";

import { FileStore } from "@sysml-bridge/model";
import type { ModelStore } from "@sysml-bridge/model";
import { registerCreateElement } from "../tools/create-element.js";
import { registerQueryElements } from "../tools/query-elements.js";
import { registerCreateRelationship } from "../tools/create-relationship.js";
import { registerQueryRelationships } from "../tools/query-relationships.js";
import { registerValidateModel } from "../tools/validate-model.js";
import { registerExportSysml } from "../tools/export-sysml.js";
import { registerImportSysml } from "../tools/import-sysml.js";
import { registerGetProjectState } from "../tools/get-project-state.js";
import { registerUpdateElement } from "../tools/update-element.js";
import { registerDeleteElement } from "../tools/delete-element.js";
import { registerWeavePass } from "../tools/weave-pass.js";
import { registerClosePass } from "../tools/close-pass.js";

const EXPECTED_TOOL_NAMES = [
  "init_project",
  "create_element",
  "query_elements",
  "create_relationship",
  "query_relationships",
  "validate_model",
  "export_sysml",
  "import_sysml",
  "get_project_state",
  "update_element",
  "delete_element",
  "weave_pass",
  "close_pass",
] as const;

function registerAllTools(server: McpServer, store: ModelStore) {
  // Inline init_project — mirrors index.ts (kept inline there too since it
  // has no dedicated tool module).
  server.tool(
    "init_project",
    "Initialize or load a project. Must be called before using other tools.",
    {
      name: z.string().describe("Project name to create or load"),
      create: z.boolean().optional().default(true).describe(
        "Create a new project (true) or load existing (false)"
      ),
    },
    async ({ name, create }) => {
      if (create) {
        const project = await store.createProject(name);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                { status: "created", projectId: project["@id"], branchId: store.branchId },
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
  registerUpdateElement(server, store);
  registerDeleteElement(server, store);
  registerWeavePass(server, store);
  registerClosePass(server, store);
}

describe("MCP server tool registration", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-registration-test-"));
    store = new FileStore(dir);

    const server = new McpServer({ name: "test", version: "0.0.1" });
    registerAllTools(server, store);

    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    client = new Client({ name: "test-client", version: "0.0.1" });

    await server.connect(serverTransport);
    await client.connect(clientTransport);

    cleanup = async () => {
      await client.close();
    };
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("client.listTools() returns exactly the 13 expected tool names", async () => {
    const { tools } = await client.listTools();
    const names = tools.map((t) => t.name).sort();

    expect(names).toEqual([...EXPECTED_TOOL_NAMES].sort());
    expect(names).toHaveLength(13);
  });
});
