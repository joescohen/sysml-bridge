/**
 * tools-e2e.test.ts
 *
 * Happy-path callTool coverage for tools not otherwise exercised via
 * client.callTool() in the ported suites (verified by grepping the test dir
 * for `name: "<tool>"` occurrences — see Task 2 verification record).
 *
 * As of this writing, init_project, query_elements, and get_project_state
 * have no callTool coverage elsewhere; create_element, create_relationship,
 * query_relationships, update_element, delete_element, export_sysml,
 * import_sysml, and validate_model are already covered by
 * write-path-coupling.test.ts, edit-tools-coupling.test.ts,
 * create-relationship.test.ts, query-relationships.test.ts,
 * export-sysml.test.ts, and validate-model-findings.test.ts /
 * validate-coverage.test.ts.
 *
 * Pattern modeled on create-relationship.test.ts's buildTestPair helper.
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
import { registerQueryElements } from "../tools/query-elements.js";
import { registerGetProjectState } from "../tools/get-project-state.js";
import { registerCreateElement } from "../tools/create-element.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Registers the same inline init_project tool index.ts defines, plus
 * whichever other tools a given test needs, against the supplied store.
 */
function registerInitProject(server: McpServer, store: ModelStore) {
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
      try {
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
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}

async function buildTestPair(
  register: (server: McpServer) => void
): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  register(server);

  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: "test-client", version: "0.0.1" });

  await server.connect(serverTransport);
  await client.connect(clientTransport);

  return {
    client,
    cleanup: async () => {
      await client.close();
    },
  };
}

// ---------------------------------------------------------------------------
// init_project
// ---------------------------------------------------------------------------

describe("init_project tool — happy path", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-init-project-test-"));
    store = new FileStore(dir);
    ({ client, cleanup } = await buildTestPair((server) => registerInitProject(server, store)));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("create:true creates a new project and returns status/projectId/branchId", async () => {
    const result = await client.callTool({
      name: "init_project",
      arguments: { name: "E2E Test Project", create: true },
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.status).toBe("created");
    expect(typeof parsed.projectId).toBe("string");
    expect(parsed.projectId.length).toBeGreaterThan(0);
    expect(typeof parsed.branchId).toBe("string");
  });

  it("create:false loads a previously created project by name", async () => {
    await store.createProject("Existing Project");

    const result = await client.callTool({
      name: "init_project",
      arguments: { name: "Existing Project", create: false },
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.status).toBe("loaded");
    expect(typeof parsed.projectId).toBe("string");
  });

  it("create:false with an unknown name → isError, project not found", async () => {
    const result = await client.callTool({
      name: "init_project",
      arguments: { name: "Nonexistent Project", create: false },
    });

    expect(result.isError).toBe(true);
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    expect(text).toContain("Nonexistent Project");
    expect(text).toContain("not found");
  });
});

// ---------------------------------------------------------------------------
// query_elements
// ---------------------------------------------------------------------------

describe("query_elements tool — happy path", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-query-elements-test-"));
    store = new FileStore(dir);
    await store.createProject("Query Elements Test");
    ({ client, cleanup } = await buildTestPair((server) => registerQueryElements(server, store)));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("returns all elements when called with no filters", async () => {
    await store.createElement("PartDefinition", "Engine");
    await store.createElement("RequirementDefinition", "MassReq");

    const result = await client.callTool({
      name: "query_elements",
      arguments: {},
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.count).toBe(2);
    expect(parsed.elements).toHaveLength(2);
  });

  it("filters by type", async () => {
    await store.createElement("PartDefinition", "Engine");
    await store.createElement("RequirementDefinition", "MassReq");

    const result = await client.callTool({
      name: "query_elements",
      arguments: { type: "PartDefinition" },
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.count).toBe(1);
    expect(parsed.elements[0].type).toBe("PartDefinition");
    expect(parsed.elements[0].name).toBe("Engine");
  });

  it("filters by name_pattern (substring match)", async () => {
    await store.createElement("PartDefinition", "Engine");
    await store.createElement("PartDefinition", "Wing");

    const result = await client.callTool({
      name: "query_elements",
      arguments: { name_pattern: "Eng" },
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.count).toBe(1);
    expect(parsed.elements[0].name).toBe("Engine");
  });
});

// ---------------------------------------------------------------------------
// get_project_state
// ---------------------------------------------------------------------------

describe("get_project_state tool — happy path", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-project-state-test-"));
    store = new FileStore(dir);
    await store.createProject("Project State Test");
    ({ client, cleanup } = await buildTestPair((server) => {
      registerGetProjectState(server, store);
      registerCreateElement(server, store);
    }));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("reports element counts by type and identifying ids", async () => {
    await client.callTool({
      name: "create_element",
      arguments: { type: "PartDefinition", name: "Engine" },
    });
    await client.callTool({
      name: "create_element",
      arguments: { type: "PartDefinition", name: "Wing" },
    });
    await client.callTool({
      name: "create_element",
      arguments: { type: "RequirementDefinition", name: "MassReq" },
    });

    const result = await client.callTool({
      name: "get_project_state",
      arguments: {},
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);

    expect(parsed.totalElements).toBe(3);
    expect(parsed.elementCountsByType.PartDefinition).toBe(2);
    expect(parsed.elementCountsByType.RequirementDefinition).toBe(1);
    expect(typeof parsed.projectId).toBe("string");
    expect(typeof parsed.branchId).toBe("string");
    expect(typeof parsed.commitId).toBe("string");
  });
});
