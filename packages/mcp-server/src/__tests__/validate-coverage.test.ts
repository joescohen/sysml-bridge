/**
 * validate-coverage.test.ts
 *
 * Tests for the extended validate_model tool: traceability coverage axes
 * (forward-union, verify-either-dir, backward, orphan), provenance gate, and
 * dangling-endpoint detection.
 *
 * Fixture:
 *   - Req A — SatisfyRequirementUsage (forward ✓), VerifyRequirementUsage (verify ✓), provenanceSourceId set
 *   - Req B — AllocationUsage only (forward ✓ via union), no verify, no provenanceSourceId
 *   - OrphanPart — PartDefinition with no inbound satisfy or derive edge
 *   - GhostRel — relationship whose targetId does not exist → dangling endpoint
 *
 * Assertions:
 *   forwardPercent === 100 (A via Satisfy, B via Allocation)
 *   verifyPercent  === 50  (A verified, B not)
 *   B appears in elementsMissingBackpointer
 *   OrphanPart appears in orphanElements
 *   GhostRel appears in danglingRelationships
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "../file-store.js";
import { registerValidateModel } from "../tools/validate-model.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function buildTestPair(
  store: FileStore
): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  registerValidateModel(server, store);

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

interface CoverageResult {
  summary: unknown;
  issues: string[];
  coverage: {
    forwardPercent: number;
    verifyPercent: number;
    backwardPercent: number;
    orphanElements: Array<{ id: string; name: string | null; type: string }>;
    provenanceCoverage: number;
    elementsMissingBackpointer: Array<{ id: string; name: string | null; type: string }>;
    danglingRelationships: Array<{ id: string; type: string; danglingIds: string[] }>;
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("validate_model — traceability coverage + provenance + dangling", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-validate-cov-"));
    store = new FileStore(dir);
    await store.createProject("Coverage Test");

    // ── Req A: SatisfyRequirementUsage + VerifyRequirementUsage + provenance ──
    const reqA = await store.createElement("RequirementDefinition", "ReqA", {
      provenanceSourceId: "corpus-section-1.2",
    });
    const partA = await store.createElement("PartDefinition", "PartA", {
      provenanceSourceId: "corpus-design-3.1",
    });

    // Forward edge for Req A: source=partA, target=reqA (incoming to req)
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": partA.id }],
      target: [{ "@id": reqA.id }],
    });

    // Verify edge for Req A: source=partA, target=reqA
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": partA.id }],
      target: [{ "@id": reqA.id }],
    });

    // ── Req B: AllocationUsage ONLY (no verify, no provenance) ──
    const reqB = await store.createElement("RequirementDefinition", "ReqB");
    // ReqB has no provenanceSourceId — should appear in elementsMissingBackpointer

    // Forward edge for Req B: AllocationUsage proves the UNION (AllocationUsage counts)
    await store.createElement("AllocationUsage", "", {
      source: [{ "@id": partA.id }],
      target: [{ "@id": reqB.id }],
    });

    // ── OrphanPart: PartDefinition with no inbound satisfy or derive ──
    await store.createElement("PartDefinition", "OrphanPart", {
      provenanceSourceId: "corpus-design-5.0",
    });

    // ── GhostRel: relationship with a non-existent target id ──
    const GHOST_TARGET_ID = "00000000-dead-beef-0000-000000000000";
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": partA.id }],
      target: [{ "@id": GHOST_TARGET_ID }],
    });

    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("returns the coverage envelope (not the old requirementCoverage field)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    expect(parsed.coverage).toBeDefined();
    expect((parsed as unknown as Record<string, unknown>).requirementCoverage).toBeUndefined();
  });

  it("forwardPercent === 100: both A (Satisfy) and B (Allocation) count as forward-traced", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    expect(parsed.coverage.forwardPercent).toBe(100);
  });

  it("verifyPercent === 50: only A has a VerifyRequirementUsage edge", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    expect(parsed.coverage.verifyPercent).toBe(50);
  });

  it("ReqB appears in elementsMissingBackpointer (no provenanceSourceId)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    const missing = parsed.coverage.elementsMissingBackpointer;
    expect(missing.some((e) => e.name === "ReqB")).toBe(true);
    // ReqA and PartA have provenanceSourceId — should NOT appear
    expect(missing.some((e) => e.name === "ReqA")).toBe(false);
  });

  it("OrphanPart appears in orphanElements (no inbound satisfy/derive)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    const orphans = parsed.coverage.orphanElements;
    expect(orphans.some((e) => e.name === "OrphanPart")).toBe(true);
    // PartA has an outbound satisfy edge — but the check is INBOUND to PartA
    // PartA is a source on satisfy/allocation edges, not a target, so it IS an orphan too
    // (the spec §4 checks PartDefinition for inbound satisfy/derive pointing AT it)
    // Both are orphan parts here; the key assertion is OrphanPart is included
  });

  it("danglingRelationships contains the ghost-target relationship", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    const dangling = parsed.coverage.danglingRelationships;
    expect(dangling.length).toBeGreaterThan(0);
    expect(
      dangling.some((r) =>
        r.danglingIds.includes("00000000-dead-beef-0000-000000000000")
      )
    ).toBe(true);
  });

  it("issues array contains human-readable lines for failing axes", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    // Should have at least: unverified (ReqB), missing-provenance (ReqB), orphan parts, dangling
    expect(parsed.issues.length).toBeGreaterThan(0);
    const allIssues = parsed.issues.join("\n");
    expect(allIssues).toMatch(/verified|VerifyRequirement/i);
    expect(allIssues).toMatch(/provenance/i);
    expect(allIssues).toMatch(/dangling|endpoint/i);
  });
});
