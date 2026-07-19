import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "@sysml-bridge/model";
import { registerExportSysml } from "../tools/export-sysml.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function buildTestPair(
  store: FileStore
): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  registerExportSysml(server, store);

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

describe("export_sysml tool — traceability regression", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-export-test-"));
    store = new FileStore(dir);
    await store.createProject("Export Test");
    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("emits satisfy relationship exactly once and not as a standalone element declaration", async () => {
    // Build a minimal model: one RequirementDefinition, one PartDefinition,
    // and one SatisfyRequirementUsage between them.
    const req = await store.createElement("RequirementDefinition", "MassReq");
    const part = await store.createElement("PartDefinition", "Engine");
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": part.id }],
      target: [{ "@id": req.id }],
    });

    const result = await client.callTool({
      name: "export_sysml",
      arguments: {},
    });

    expect(result.isError).toBeFalsy();

    const text = (
      result.content as Array<{ type: string; text: string }>
    )[0].text;

    // The traceability statement must appear exactly once.
    const satisfyMatches = (text.match(/\bsatisfy\b/g) ?? []).length;
    expect(satisfyMatches).toBe(1);

    // The relationship element must NOT appear as a standalone element
    // declaration (i.e. no "SatisfyRequirementUsage" keyword emitted as a
    // structural block or inline element line).
    expect(text).not.toMatch(/SatisfyRequirementUsage/);
  });
});
