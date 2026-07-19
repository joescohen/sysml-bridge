/**
 * write-path-coupling.test.ts
 *
 * Two describe blocks:
 *   1. "pure-function structural check" — unit tests for structural.ts
 *      (no MCP server/client, no network I/O)
 *   2. "MCP round-trip coupling" — end-to-end proofs through the three
 *      mutating tools (create_element, create_relationship, import_sysml)
 *      that the gate rejects before persisting (Task 3).
 *
 * This file is the authoritative GATE-05 coupling evidence for ROADMAP criterion 5.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";

import { structuralCheck, checkBatch, clearCorpusCache } from "@sysml-bridge/gates";
import type { Candidate, Finding } from "@sysml-bridge/gates";
import { hasFinding } from "@sysml-bridge/invariants";
import type { SysmlElement } from "@sysml-bridge/model";
import { FileStore } from "@sysml-bridge/model";
import { registerCreateElement } from "../tools/create-element.js";
import { registerCreateRelationship } from "../tools/create-relationship.js";
import { registerImportSysml } from "../tools/import-sysml.js";

// ---------------------------------------------------------------------------
// Helper: build a minimal SysmlElement for the "existing" set
// ---------------------------------------------------------------------------
function mkEl(id: string, type: string, name?: string): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name: name ?? null,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
  };
}

// ---------------------------------------------------------------------------
// 1. Pure-function structural check unit tests
// ---------------------------------------------------------------------------

describe("pure-function structural check", () => {
  // ── R4 def-operand ──

  it("R4: SatisfyRequirementUsage with RequirementDefinition source → R4-def-operand error", () => {
    const reqDef = mkEl("req-1", "RequirementDefinition", "SysTechReq");
    const partUsage = mkEl("part-1", "PartUsage", "SubSys");

    const candidate: Candidate = {
      type: "SatisfyRequirementUsage",
      sourceIds: ["req-1"],
      targetIds: ["part-1"],
    };

    const findings = structuralCheck(candidate, [reqDef, partUsage], null);
    expect(hasFinding(findings, { ruleId: "R4-def-operand" })).toBe(true);
    const r4 = findings.find((f) => f.ruleId === "R4-def-operand")!;
    expect(r4.severity).toBe("error");
  });

  // ── GATE02-dangling-endpoint ──

  it("Dangling: targetId absent from existing → GATE02-dangling-endpoint error", () => {
    const partUsage = mkEl("part-1", "PartUsage");
    const candidate: Candidate = {
      type: "SatisfyRequirementUsage",
      sourceIds: ["part-1"],
      targetIds: ["ghost"],  // not in existing
    };

    const findings = structuralCheck(candidate, [partUsage], null);
    expect(hasFinding(findings, { ruleId: "GATE02-dangling-endpoint" })).toBe(true);
    const dangling = findings.find((f) => f.ruleId === "GATE02-dangling-endpoint")!;
    expect(dangling.severity).toBe("error");
  });

  // ── GATE03-unresolvable-provenance ──

  it("Provenance: non-null resolutionSet lacking provenanceSourceId → GATE03-unresolvable-provenance error", () => {
    const candidate: Candidate = {
      type: "PartDefinition",
      sourceIds: [],
      targetIds: [],
      provenanceSourceId: "fake-xyz",
    };

    const resolutionSet = new Set(["real-corpus-id"]);
    const findings = structuralCheck(candidate, [], resolutionSet);
    expect(hasFinding(findings, { ruleId: "GATE03-unresolvable-provenance" })).toBe(true);
    const f = findings.find((f) => f.ruleId === "GATE03-unresolvable-provenance")!;
    expect(f.severity).toBe("error");
  });

  it("Provenance: resolutionSet === null (corpus unavailable) → NO provenance finding (gate degrades)", () => {
    const candidate: Candidate = {
      type: "PartDefinition",
      sourceIds: [],
      targetIds: [],
      provenanceSourceId: "fake-xyz",
    };

    // null means corpus is unavailable — provenance existence check is skipped
    const findings = structuralCheck(candidate, [], null);
    expect(hasFinding(findings, { ruleId: "GATE03-unresolvable-provenance" })).toBe(false);
  });

  it("Provenance: model-asserted → GATE03-model-asserted info finding (never blocks)", () => {
    const candidate: Candidate = {
      type: "PartDefinition",
      sourceIds: [],
      targetIds: [],
      provenanceSourceId: "model-asserted",
    };

    // resolution set contains "model-asserted" (from ALLOWLIST)
    const resolutionSet = new Set(["model-asserted"]);
    const findings = structuralCheck(candidate, [], resolutionSet);
    expect(hasFinding(findings, { ruleId: "GATE03-model-asserted" })).toBe(true);
    const infoF = findings.find((f) => f.ruleId === "GATE03-model-asserted")!;
    expect(infoF.severity).toBe("info");
    // must NOT produce an error for model-asserted
    expect(findings.filter((f) => f.severity === "error").length).toBe(0);
  });

  it("No provenanceSourceId → no provenance error (missing provenance is completeness, never pre-add reject)", () => {
    const candidate: Candidate = {
      type: "RequirementDefinition",
      sourceIds: [],
      targetIds: [],
      // provenanceSourceId intentionally absent
    };

    const resolutionSet = new Set(["something"]);
    const findings = structuralCheck(candidate, [], resolutionSet);
    // No provenance error (presence check is completeness, not pre-add structural)
    expect(findings.filter((f) => f.ruleId === "GATE03-unresolvable-provenance").length).toBe(0);
  });

  // ── structuralCheck NEVER returns completeness ruleIds ──

  it("structuralCheck never returns completeness ruleIds on a satisfy-less requirement candidate", () => {
    const candidate: Candidate = {
      type: "RequirementDefinition",
      sourceIds: [],
      targetIds: [],
    };

    const findings = structuralCheck(candidate, [], null);
    const completenessRuleIds = [
      "GATE02-unsatisfied",
      "GATE02-unverified",
      "GATE02-unbacktraced",
      "GATE02-orphan",
      "GATE02-uncovered-need",
    ];
    for (const ruleId of completenessRuleIds) {
      expect(hasFinding(findings, { ruleId: ruleId })).toBe(false);
    }
  });

  // ── checkBatch ──

  it("checkBatch: third candidate targets first candidate id → no dangling error (cumulative set)", () => {
    // candidate 1: PartUsage "p1" (has an id, can be referenced)
    // candidate 2: RequirementUsage "r1"
    // candidate 3: SatisfyRequirementUsage referencing p1 and r1 (both previous candidates)
    const c1: Candidate = { id: "p1", type: "PartUsage", sourceIds: [], targetIds: [] };
    const c2: Candidate = { id: "r1", type: "RequirementUsage", sourceIds: [], targetIds: [] };
    const c3: Candidate = {
      type: "SatisfyRequirementUsage",
      sourceIds: ["p1"],   // references c1's id
      targetIds: ["r1"],   // references c2's id
    };

    const findings = checkBatch([c1, c2, c3], [], null);
    // c3 targets c1 and c2 which are in the cumulative set — should NOT be dangling
    expect(hasFinding(findings, { ruleId: "GATE02-dangling-endpoint" })).toBe(false);
  });

  it("checkBatch: if any candidate has an error finding, errors array is non-empty (all-or-nothing signal)", () => {
    const reqDef = mkEl("req-existing", "RequirementDefinition");
    // candidate with dangling target — will cause an error
    const badCandidate: Candidate = {
      type: "SatisfyRequirementUsage",
      sourceIds: ["req-existing"],
      targetIds: ["no-such-target"],  // dangling
    };
    const goodCandidate: Candidate = {
      id: "clean",
      type: "PartUsage",
      sourceIds: [],
      targetIds: [],
    };

    const findings = checkBatch([goodCandidate, badCandidate], [reqDef], null);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors.length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// 2. MCP round-trip coupling tests
// ---------------------------------------------------------------------------

// Minimal corpus fixture that the provenance tests can point at.
// Contains one requirement with id "corpus-req-001".
// All required fields per ExtractedSchema: statement, needIds are required on requirements.
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

/**
 * Build an MCP server+client pair with all three mutating tools registered.
 * Mirrors create-relationship.test.ts buildTestPair pattern.
 */
async function buildTestPair(store: FileStore): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const server = new McpServer({ name: "test", version: "0.0.1" });
  registerCreateElement(server, store);
  registerCreateRelationship(server, store);
  registerImportSysml(server, store);

  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: "test-client", version: "0.0.1" });
  await server.connect(serverTransport);
  await client.connect(clientTransport);
  return { client, cleanup: async () => { await client.close(); } };
}

describe("MCP round-trip coupling", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: (() => Promise<void>) | undefined;

  // Save/restore SYSML_BRIDGE_CORPUS_PATH env var + clear corpus cache before each test
  // to guarantee each test has a clean corpus state.
  let savedCorpusPath: string | undefined;

  beforeEach(async () => {
    savedCorpusPath = process.env.SYSML_BRIDGE_CORPUS_PATH;
    clearCorpusCache();

    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-coupling-"));
    store = new FileStore(dir);
    await store.createProject("CouplingTest");
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

  // ── R4 reject ──

  it("R4 reject: create_relationship SatisfyRequirementUsage between Def operands → isError, store unchanged", async () => {
    const reqDef = await store.createElement("RequirementDefinition", "SysTechReq");
    const partDef = await store.createElement("PartDefinition", "SysArch");

    const beforeElements = await store.queryElements();
    const beforeRelationships = await store.queryRelationships();

    const result = await client.callTool({
      name: "create_relationship",
      arguments: {
        type: "SatisfyRequirementUsage",
        source_id: reqDef.id,
        target_id: partDef.id,
      },
    });

    expect(result.isError).toBe(true);
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.rejected).toBe(true);
    expect(hasFinding((parsed.findings as Finding[]), { ruleId: "R4-def-operand" })).toBe(true);

    // Store must be unchanged (ROADMAP criterion 5)
    const afterElements = await store.queryElements();
    const afterRelationships = await store.queryRelationships();
    expect(afterElements.length).toBe(beforeElements.length);
    expect(afterRelationships.length).toBe(beforeRelationships.length);
  });

  // ── allow_invalid bypass ──

  it("allow_invalid: true bypasses R4 and relationship is persisted", async () => {
    const reqDef = await store.createElement("RequirementDefinition", "SysTechReq");
    const partDef = await store.createElement("PartDefinition", "SysArch");

    const result = await client.callTool({
      name: "create_relationship",
      arguments: {
        type: "SatisfyRequirementUsage",
        source_id: reqDef.id,
        target_id: partDef.id,
        allow_invalid: true,
      },
    });

    // Should NOT be an error
    expect(result.isError).toBeFalsy();

    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    // Response shape B2: findings are included (auditable bypass)
    expect(parsed.element).toBeDefined();
    expect(hasFinding((parsed.findings as Finding[]), { ruleId: "R4-def-operand" })).toBe(true);

    // Relationship was persisted
    const relationships = await store.queryRelationships();
    expect(relationships.length).toBeGreaterThan(0);
    expect(relationships.some((r) => r.type === "SatisfyRequirementUsage")).toBe(true);
  });

  // ── Dangling reject ──

  it("Dangling reject: create_relationship with nonexistent target_id → rejected, store unchanged", async () => {
    const partUsage = await store.createElement("PartUsage", "SubSys");

    const beforeRelationships = await store.queryRelationships();

    const result = await client.callTool({
      name: "create_relationship",
      arguments: {
        type: "SatisfyRequirementUsage",
        source_id: partUsage.id,
        target_id: "no-such-id",
      },
    });

    expect(result.isError).toBe(true);
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.rejected).toBe(true);
    expect(hasFinding((parsed.findings as Finding[]), { ruleId: "GATE02-dangling-endpoint" })).toBe(true);

    const afterRelationships = await store.queryRelationships();
    expect(afterRelationships.length).toBe(beforeRelationships.length);
  });

  // ── Provenance reject (with corpus fixture) ──

  it("Provenance reject: create_element PartUsage with fake provenanceSourceId → GATE03 error", async () => {
    // Write a minimal corpus fixture and point the env var at it
    const corpusFile = path.join(dir, "extracted.json");
    await fs.writeFile(corpusFile, MINIMAL_CORPUS, "utf8");
    process.env.SYSML_BRIDGE_CORPUS_PATH = corpusFile;
    clearCorpusCache(); // reset so resolveGateCorpus picks up the new path

    const result = await client.callTool({
      name: "create_element",
      arguments: {
        type: "PartUsage",
        name: "FakePart",
        attributes: { provenanceSourceId: "fake-xyz" },
      },
    });

    expect(result.isError).toBe(true);
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    expect(parsed.rejected).toBe(true);
    expect(hasFinding((parsed.findings as Finding[]), { ruleId: "GATE03-unresolvable-provenance" })).toBe(true);
  });

  it("Provenance pass: create_element with valid corpus id succeeds", async () => {
    const corpusFile = path.join(dir, "extracted.json");
    await fs.writeFile(corpusFile, MINIMAL_CORPUS, "utf8");
    process.env.SYSML_BRIDGE_CORPUS_PATH = corpusFile;
    clearCorpusCache();

    const result = await client.callTool({
      name: "create_element",
      arguments: {
        type: "PartUsage",
        name: "RealPart",
        attributes: { provenanceSourceId: "corpus-req-001" },  // valid corpus id
      },
    });

    expect(result.isError).toBeFalsy();
    // No GATE03 error finding in response
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    if (parsed.findings) {
      expect((parsed.findings as Finding[]).filter((f) => f.severity === "error").length).toBe(0);
    }
  });

  // ── Corpus-absent grace ──

  it("Corpus-absent grace: unresolvable provenanceSourceId with no corpus env var SUCCEEDS", async () => {
    delete process.env.SYSML_BRIDGE_CORPUS_PATH;
    clearCorpusCache();
    // Point at a non-existent path so loadCorpusCached returns null
    process.env.SYSML_BRIDGE_CORPUS_PATH = path.join(dir, "does-not-exist.json");
    clearCorpusCache();

    const result = await client.callTool({
      name: "create_element",
      arguments: {
        type: "PartUsage",
        name: "UnprovenancedPart",
        attributes: { provenanceSourceId: "totally-fake-id" },
      },
    });

    // Gate degrades: no corpus → no provenance rejection
    expect(result.isError).toBeFalsy();
    const elements = await store.queryElements();
    expect(elements.some((e) => e.name === "UnprovenancedPart")).toBe(true);
  });

  // ── import_sysml clean regression ──

  it("import_sysml: small valid package imports with legacy success shape", async () => {
    const sysmlText = `package TestPkg {
  part def Fuselage;
  part def Wing;
}`;

    const result = await client.callTool({
      name: "import_sysml",
      arguments: { sysml_text: sysmlText },
    });

    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text);
    // Legacy shape must be preserved
    expect(parsed.success).toBe(true);
    expect(typeof parsed.elementsImported).toBe("number");
    expect(Array.isArray(parsed.elements)).toBe(true);
    expect(parsed.elementsImported).toBeGreaterThan(0);
  });
});
