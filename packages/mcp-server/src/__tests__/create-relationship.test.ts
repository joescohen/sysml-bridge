import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore, SYSML_RELATIONSHIP_TYPES } from "@sysml-bridge/model";
import { registerCreateRelationship } from "../tools/create-relationship.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function buildTestPair(store: FileStore): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  registerCreateRelationship(server, store);

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
// Tests
// ---------------------------------------------------------------------------

describe("create_relationship tool — type validation", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-rel-test-"));
    store = new FileStore(dir);
    await store.createProject("Test");
    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("rejects an unknown relationship type with isError=true", async () => {
    const result = await client.callTool({
      name: "create_relationship",
      arguments: {
        type: "Allocation",          // wrong — this is not a valid type
        source_id: "src-id",
        target_id: "tgt-id",
      },
    });

    expect(result.isError).toBe(true);
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    expect(text).toContain("unknown relationship type");
    expect(text).toContain("'Allocation'");
  });

  it("accepts AllocationUsage (a valid trace relationship type)", async () => {
    // Create two Usage elements (R4: trace operands must be Usages, not Definitions)
    const part = await store.createElement("PartUsage", "Widget");
    const req = await store.createElement("RequirementUsage", "AllocReq");

    const result = await client.callTool({
      name: "create_relationship",
      arguments: {
        type: "AllocationUsage",
        source_id: part.id,
        target_id: req.id,
      },
    });

    // Should succeed (no isError)
    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.type).toBe("AllocationUsage");
  });

  it("accepts all four new trace types without error", async () => {
    const newTypes = [
      "VerifyRequirementUsage",
      "DeriveRequirementUsage",
      "AllocationUsage",
      "TraceRequirementUsage",
    ] as const;

    for (const type of newTypes) {
      // R4: trace operands must be Usages, not Definitions
      const src = await store.createElement("PartUsage", `Src_${type}`);
      const tgt = await store.createElement("RequirementUsage", `Tgt_${type}`);

      const result = await client.callTool({
        name: "create_relationship",
        arguments: { type, source_id: src.id, target_id: tgt.id },
      });

      expect(result.isError).toBeFalsy();
    }
  });

  it("SYSML_RELATIONSHIP_TYPES includes all four new trace types", () => {
    const types = SYSML_RELATIONSHIP_TYPES as readonly string[];
    expect(types).toContain("VerifyRequirementUsage");
    expect(types).toContain("DeriveRequirementUsage");
    expect(types).toContain("AllocationUsage");
    expect(types).toContain("TraceRequirementUsage");
  });
});
