import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "../file-store.js";
import { registerQueryRelationships } from "../tools/query-relationships.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function buildTestPair(
  store: FileStore
): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  registerQueryRelationships(server, store);

  const [clientTransport, serverTransport] =
    InMemoryTransport.createLinkedPair();
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
// Tests
// ---------------------------------------------------------------------------

describe("query_relationships tool — type filter", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-qrel-test-"));
    store = new FileStore(dir);
    await store.createProject("Query Rel Test");
    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("returns only relationships matching the requested type when model has mixed types", async () => {
    // Build three elements and two relationships of different types.
    const part = await store.createElement("PartDefinition", "Pump");
    const req1 = await store.createElement("RequirementDefinition", "FlowReq");
    const req2 = await store.createElement("RequirementDefinition", "SafetyReq");

    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": part.id }],
      target: [{ "@id": req1.id }],
    });
    await store.createElement("AllocationUsage", "", {
      source: [{ "@id": part.id }],
      target: [{ "@id": req2.id }],
    });

    // Without filter: both relationships returned.
    const allResult = await client.callTool({
      name: "query_relationships",
      arguments: {},
    });
    expect(allResult.isError).toBeFalsy();
    const allParsed = JSON.parse(
      (allResult.content as Array<{ type: string; text: string }>)[0].text
    );
    expect(allParsed.count).toBe(2);

    // With type filter: only SatisfyRequirementUsage returned.
    const filteredResult = await client.callTool({
      name: "query_relationships",
      arguments: { type: "SatisfyRequirementUsage" },
    });
    expect(filteredResult.isError).toBeFalsy();
    const filteredParsed = JSON.parse(
      (filteredResult.content as Array<{ type: string; text: string }>)[0].text
    );
    expect(filteredParsed.count).toBe(1);
    expect(filteredParsed.relationships).toHaveLength(1);
    expect(filteredParsed.relationships[0].type).toBe("SatisfyRequirementUsage");

    // Filtering for a type not present returns empty.
    const emptyResult = await client.callTool({
      name: "query_relationships",
      arguments: { type: "DeriveRequirementUsage" },
    });
    expect(emptyResult.isError).toBeFalsy();
    const emptyParsed = JSON.parse(
      (emptyResult.content as Array<{ type: string; text: string }>)[0].text
    );
    expect(emptyParsed.count).toBe(0);
    expect(emptyParsed.relationships).toHaveLength(0);
  });
});
