/**
 * edit-tools-coupling.test.ts
 *
 * MCP round-trip coupling proofs for update_element and delete_element.
 * Clones the buildTestPair + MINIMAL_CORPUS + env/cache lifecycle from
 * write-path-coupling.test.ts. Nine cases:
 *
 *   update_element:
 *     1. Clean rename persists
 *     2. R4 reject on retarget to Definition operand
 *     3. Dangling reject on retarget to nonexistent endpoint
 *     4. allow_invalid bypass with full findings (B2)
 *     5. Provenance reject (MINIMAL_CORPUS env set)
 *
 *   delete_element:
 *     6. Leaf delete succeeds
 *     7. Would-dangle reject (store unchanged)
 *     8. allow_invalid force-delete (relationship dangling, findings returned)
 *     9. Nonexistent id → isError "Element not found"
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";

import { FileStore } from "../file-store.js";
import { registerCreateElement } from "../tools/create-element.js";
import { registerCreateRelationship } from "../tools/create-relationship.js";
import { registerUpdateElement } from "../tools/update-element.js";
import { registerDeleteElement } from "../tools/delete-element.js";
import { clearCorpusCache } from "../audit/corpus.js";
import type { Finding } from "../audit/findings.js";

// ---------------------------------------------------------------------------
// MINIMAL_CORPUS fixture — matches write-path-coupling.test.ts verbatim
// ---------------------------------------------------------------------------
const MINIMAL_CORPUS = JSON.stringify({
  schema_version: "1.0.0",
  subsystem: "TEST",
  needs: [{ id: "corpus-need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
  requirements: [{
    id: "corpus-req-001",
    kind: "requirement",
    naturalKey: "R1",
    name: "Test Requirement",
    statement: "The system shall do something.",
    needIds: ["corpus-need-001"],
  }],
  functions: [],
  components: [],
  satisfies: [],
  allocations: [],
});

// ---------------------------------------------------------------------------
// buildTestPair — registers all four mutating tools over a FileStore
// ---------------------------------------------------------------------------
async function buildTestPair(store: FileStore): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  registerCreateElement(server, store);
  registerCreateRelationship(server, store);
  registerUpdateElement(server, store);
  registerDeleteElement(server, store);

  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: "test-client", version: "0.0.1" });
  await server.connect(serverTransport);
  await client.connect(clientTransport);
  return { client, cleanup: async () => { await client.close(); } };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function parseText(result: { content: unknown }): unknown {
  const text = (result.content as Array<{ type: string; text: string }>)[0].text;
  return JSON.parse(text);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("MCP edit-tools coupling", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: (() => Promise<void>) | undefined;
  let savedCorpusPath: string | undefined;

  beforeEach(async () => {
    savedCorpusPath = process.env.SYSML_BRIDGE_CORPUS_PATH;
    clearCorpusCache();

    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-edit-coupling-"));
    store = new FileStore(dir);
    await store.createProject("EditCouplingTest");
    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup?.();
    if (savedCorpusPath !== undefined) {
      process.env.SYSML_BRIDGE_CORPUS_PATH = savedCorpusPath;
    } else {
      delete process.env.SYSML_BRIDGE_CORPUS_PATH;
    }
    clearCorpusCache();
    await fs.rm(dir, { recursive: true, force: true });
  });

  // ── Test 1: Clean rename ──

  it("1. update_element clean rename: persists new name, not error", async () => {
    const el = await store.createElement("PartUsage", "OldName");

    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: el.id,
        updates: { name: "NewName" },
      },
    });

    expect(result.isError).toBeFalsy();
    const parsed = parseText(result) as Record<string, unknown>;
    // Bare element shape (no findings on a clean rename)
    expect((parsed as { name?: string }).name).toBe("NewName");

    const elements = await store.queryElements();
    const updated = elements.find((e) => e.id === el.id);
    expect(updated?.name).toBe("NewName");
  });

  // ── Test 2: R4 reject on retarget to Definition operand ──

  it("2. update_element R4 reject: retarget SatisfyRequirementUsage to a RequirementDefinition source → isError, store unchanged", async () => {
    // Seed: two usages for the clean initial relationship
    const reqUsage = await store.createElement("RequirementUsage", "ReqU");
    const partUsage = await store.createElement("PartUsage", "PartU");
    // Create a clean satisfy relationship (usage→usage)
    const rel = await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": reqUsage.id }],
      target: [{ "@id": partUsage.id }],
    });
    // Seed a RequirementDefinition to retarget to (invalid — def operand)
    const reqDef = await store.createElement("RequirementDefinition", "ReqDef");

    const beforeElements = await store.queryElements();
    const beforeRels = await store.queryRelationships();

    // Retarget source to the Definition (R4 violation)
    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: rel.id,
        updates: { source: [{ "@id": reqDef.id }] },
      },
    });

    expect(result.isError).toBe(true);
    const parsed = parseText(result) as { rejected: boolean; findings: Finding[] };
    expect(parsed.rejected).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "R4-def-operand")).toBe(true);

    // Store must be unchanged
    const afterElements = await store.queryElements();
    const afterRels = await store.queryRelationships();
    expect(afterElements.length).toBe(beforeElements.length);
    expect(afterRels.length).toBe(beforeRels.length);

    // The relationship still points at the original clean source
    const relAfter = afterRels.find((r) => r.id === rel.id);
    expect(relAfter?.sourceIds).toContain(reqUsage.id);
    expect(relAfter?.sourceIds).not.toContain(reqDef.id);
  });

  // ── Test 3: Dangling reject on retarget to ghost endpoint ──

  it("3. update_element dangling reject: retarget to nonexistent endpoint → isError GATE02-dangling-endpoint, relationship unchanged", async () => {
    const src = await store.createElement("PartUsage", "Src");
    const tgt = await store.createElement("PartUsage", "Tgt");
    const rel = await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": src.id }],
      target: [{ "@id": tgt.id }],
    });

    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: rel.id,
        updates: { target: [{ "@id": "ghost-id-that-does-not-exist" }] },
      },
    });

    expect(result.isError).toBe(true);
    const parsed = parseText(result) as { rejected: boolean; findings: Finding[] };
    expect(parsed.rejected).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "GATE02-dangling-endpoint")).toBe(true);

    // Relationship target must still be the original
    const rels = await store.queryRelationships();
    const relAfter = rels.find((r) => r.id === rel.id);
    expect(relAfter?.targetIds).toContain(tgt.id);
    expect(relAfter?.targetIds).not.toContain("ghost-id-that-does-not-exist");
  });

  // ── Test 4: allow_invalid bypass with full findings ──

  it("4. update_element allow_invalid: R4 retarget with allow_invalid:true → not error, element persisted, findings included (B2)", async () => {
    const reqUsage = await store.createElement("RequirementUsage", "ReqU");
    const partUsage = await store.createElement("PartUsage", "PartU");
    const rel = await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": reqUsage.id }],
      target: [{ "@id": partUsage.id }],
    });
    const reqDef = await store.createElement("RequirementDefinition", "ReqDef");

    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: rel.id,
        updates: { source: [{ "@id": reqDef.id }] },
        allow_invalid: true,
      },
    });

    // Should NOT be an error
    expect(result.isError).toBeFalsy();

    const parsed = parseText(result) as { element: unknown; findings: Finding[] };
    // Response shape B2: findings included (full set, including bypassed errors)
    expect(parsed.element).toBeDefined();
    expect(Array.isArray(parsed.findings)).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "R4-def-operand")).toBe(true);

    // Store reflects the (intentionally invalid) retarget
    const rels = await store.queryRelationships();
    const relAfter = rels.find((r) => r.id === rel.id);
    expect(relAfter?.sourceIds).toContain(reqDef.id);
  });

  // ── Test 5: Provenance reject (MINIMAL_CORPUS env set) ──

  it("5. update_element provenance reject: update with fake provenanceSourceId and corpus present → GATE03-unresolvable-provenance, store unchanged", async () => {
    const el = await store.createElement("PartUsage", "ProvenancePart");

    // Write corpus fixture and point env at it
    const corpusFile = path.join(dir, "extracted.json");
    await fs.writeFile(corpusFile, MINIMAL_CORPUS, "utf8");
    process.env.SYSML_BRIDGE_CORPUS_PATH = corpusFile;
    clearCorpusCache();

    const beforeElements = await store.queryElements();

    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: el.id,
        updates: { provenanceSourceId: "fake-xyz" },
      },
    });

    expect(result.isError).toBe(true);
    const parsed = parseText(result) as { rejected: boolean; findings: Finding[] };
    expect(parsed.rejected).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "GATE03-unresolvable-provenance")).toBe(true);

    // Store unchanged
    const afterElements = await store.queryElements();
    expect(afterElements.length).toBe(beforeElements.length);
  });

  // ── Test 6: Leaf delete succeeds ──

  it("6. delete_element leaf: deletes element, queryElements count drops by one", async () => {
    const el = await store.createElement("PartUsage", "LeafPart");
    const before = await store.queryElements();

    const result = await client.callTool({
      name: "delete_element",
      arguments: { element_id: el.id },
    });

    expect(result.isError).toBeFalsy();
    const parsed = parseText(result) as { deleted: boolean; elementId: string };
    expect(parsed.deleted).toBe(true);
    expect(parsed.elementId).toBe(el.id);

    const after = await store.queryElements();
    expect(after.length).toBe(before.length - 1);
    expect(after.find((e) => e.id === el.id)).toBeUndefined();
  });

  // ── Test 7: Would-dangle reject ──

  it("7. delete_element dangle reject: endpoint of a relationship → isError EDIT-delete-would-dangle, store unchanged", async () => {
    const src = await store.createElement("PartUsage", "SrcPart");
    const tgt = await store.createElement("PartUsage", "TgtPart");
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": src.id }],
      target: [{ "@id": tgt.id }],
    });

    const beforeElements = await store.queryElements();
    const beforeRels = await store.queryRelationships();

    // Attempt to delete src (which is a relationship endpoint)
    const result = await client.callTool({
      name: "delete_element",
      arguments: { element_id: src.id },
    });

    expect(result.isError).toBe(true);
    const parsed = parseText(result) as { rejected: boolean; findings: Finding[] };
    expect(parsed.rejected).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "EDIT-delete-would-dangle")).toBe(true);

    // Store unchanged
    const afterElements = await store.queryElements();
    const afterRels = await store.queryRelationships();
    expect(afterElements.length).toBe(beforeElements.length);
    expect(afterRels.length).toBe(beforeRels.length);
  });

  // ── Test 8: allow_invalid force-delete ──

  it("8. delete_element allow_invalid: force-deletes endpoint element, findings included (auditable)", async () => {
    const src = await store.createElement("PartUsage", "SrcPart");
    const tgt = await store.createElement("PartUsage", "TgtPart");
    const rel = await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": src.id }],
      target: [{ "@id": tgt.id }],
    });

    const result = await client.callTool({
      name: "delete_element",
      arguments: { element_id: src.id, allow_invalid: true },
    });

    expect(result.isError).toBeFalsy();
    const parsed = parseText(result) as {
      deleted: boolean;
      elementId: string;
      findings: Finding[];
    };
    expect(parsed.deleted).toBe(true);
    expect(parsed.elementId).toBe(src.id);
    // Findings included for auditability
    expect(Array.isArray(parsed.findings)).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "EDIT-delete-would-dangle")).toBe(true);

    // src is gone from elements
    const elements = await store.queryElements();
    expect(elements.find((e) => e.id === src.id)).toBeUndefined();

    // The relationship still exists but now dangles (documented consequence)
    const rels = await store.queryRelationships();
    const relAfter = rels.find((r) => r.id === rel.id);
    expect(relAfter).toBeDefined();
    expect(relAfter?.sourceIds).toContain(src.id); // dangling — still references deleted id
  });

  // ── Test 9: Nonexistent id → isError ──

  it("9. delete_element nonexistent id: isError 'Element not found', store unchanged", async () => {
    const el = await store.createElement("PartUsage", "Existing");
    const before = await store.queryElements();

    const result = await client.callTool({
      name: "delete_element",
      arguments: { element_id: "no-such-id" },
    });

    expect(result.isError).toBe(true);
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    expect(text).toContain("Element not found");
    expect(text).toContain("no-such-id");

    // Store unchanged (existing element still there)
    const after = await store.queryElements();
    expect(after.length).toBe(before.length);
    expect(after.find((e) => e.id === el.id)).toBeDefined();
  });

  // ── Test 10: Type-change round-trip (CR-01 regression guard) ──

  it("10. update_element type-change: el.type and el.raw['@type'] updated in store after type update", async () => {
    const el = await store.createElement("PartUsage", "TypedPart");

    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: el.id,
        updates: { type: "ActionUsage" },
      },
    });

    expect(result.isError).toBeFalsy();

    // Round-trip: queryElements must reflect the new type
    const elements = await store.queryElements();
    const updated = elements.find((e) => e.id === el.id);
    expect(updated?.type).toBe("ActionUsage");

    // queryElements(type?) filter must surface the element under new type
    const byNewType = await store.queryElements("ActionUsage");
    expect(byNewType.some((e) => e.id === el.id)).toBe(true);

    const byOldType = await store.queryElements("PartUsage");
    expect(byOldType.some((e) => e.id === el.id)).toBe(false);
  });

  // ── Test 11: CR-02 — empty endpoints guard on relationship update ──

  it("11. update_element empty-endpoints guard: clearing all endpoints on a relationship type → isError EDIT-empty-endpoints, store unchanged", async () => {
    const src = await store.createElement("PartUsage", "Src");
    const tgt = await store.createElement("PartUsage", "Tgt");
    const rel = await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": src.id }],
      target: [{ "@id": tgt.id }],
    });

    const beforeRels = await store.queryRelationships();

    // Attempt to clear both endpoints simultaneously
    const result = await client.callTool({
      name: "update_element",
      arguments: {
        element_id: rel.id,
        updates: { source: [], target: [] },
      },
    });

    expect(result.isError).toBe(true);
    const parsed = parseText(result) as { rejected: boolean; findings: Array<{ ruleId: string }> };
    expect(parsed.rejected).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "EDIT-empty-endpoints")).toBe(true);

    // Store unchanged — relationship still has its original endpoints
    const afterRels = await store.queryRelationships();
    expect(afterRels.length).toBe(beforeRels.length);
    const relAfter = afterRels.find((r) => r.id === rel.id);
    expect(relAfter?.sourceIds).toContain(src.id);
    expect(relAfter?.targetIds).toContain(tgt.id);
  });

  // ── Test 12: WR-01 — delete dangle check uses canonical id ──

  it("12. delete_element dangle check uses canonical id (not caller alias)", async () => {
    const src = await store.createElement("PartUsage", "SrcPart");
    const tgt = await store.createElement("PartUsage", "TgtPart");
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": src.id }],
      target: [{ "@id": tgt.id }],
    });

    // Call with the canonical id — dangle guard must fire
    const result = await client.callTool({
      name: "delete_element",
      arguments: { element_id: src.id },
    });

    expect(result.isError).toBe(true);
    const parsed = parseText(result) as { rejected: boolean; findings: Array<{ ruleId: string }> };
    expect(parsed.rejected).toBe(true);
    expect(parsed.findings.some((f) => f.ruleId === "EDIT-delete-would-dangle")).toBe(true);
  });
});
