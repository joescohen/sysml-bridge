/**
 * validate-model-findings.test.ts
 *
 * Integration test: findings[] surface through the validate_model MCP tool.
 * ROADMAP criterion 1 evidence — seeded-defect model + structured report inspection
 * through the real tool surface.
 *
 * Fixture: seeded-defect store with three defect classes:
 *   1. def-operand (R4-def-operand error): SatisfyRequirementUsage pointing at a
 *      RequirementDefinition instead of a RequirementUsage
 *   2. dangling endpoint (GATE02-dangling-endpoint error): relationship whose target
 *      id does not exist in the model
 *   3. laundered provenance (GATE03-unresolvable-provenance error): element with
 *      provenanceSourceId "requirement-deadbeef" that is absent from the corpus
 *
 * Also tests:
 *   - matrix has a row for the seeded systemReq
 *   - fidelity.fabrications is non-empty (laundered id)
 *   - write_report=true with SYSML_BRIDGE_AUDITS_DIR → both markdown files created
 *   - corpus unavailable (SYSML_BRIDGE_CORPUS_PATH unset, non-existent path) →
 *     GATE03-corpus-unavailable warning + legacy coverage keys present
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
import { clearCorpusCache } from "../audit/index.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Finding {
  elementId: string;
  ruleId: string;
  message: string;
  severity: string;
  suggestedFix: string;
}

interface MatrixRow {
  reqId: string;
  reqName: string | null;
  satisfied: boolean;
  verified: boolean;
  derived: boolean;
}

interface FidelityRow {
  corpusId: string;
  corpusName: string;
  kind: string;
}

interface NearMatch extends FidelityRow {
  modelElementId: string;
  modelName: string;
  similarity: number;
  band: string;
}

interface FindingsResult {
  summary: unknown;
  issues: string[];
  coverage: unknown;
  findings: Finding[];
  fidelity: {
    drops: FidelityRow[];
    fabrications: FidelityRow[];
    nearMatches: NearMatch[];
  };
  matrix: MatrixRow[];
  reportPaths?: { matrixPath: string; fidelityPath: string };
}

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

/**
 * Minimal valid Extracted fixture JSON.
 * schema_version "1.0.0" + the six required arrays + one requirement whose id
 * matches a seeded element's provenanceSourceId so exact-match works.
 */
function makeExtractedFixture(corpusReqId: string): object {
  return {
    schema_version: "1.0.0",
    subsystem: "test-subsystem",
    needs: [
      {
        id: "need-001",
        kind: "need",
        naturalKey: "N.001",
        name: "Test Need",
      },
    ],
    requirements: [
      {
        id: corpusReqId,
        kind: "requirement",
        naturalKey: `${corpusReqId}.nk`,
        name: "System Requirement One",
        statement: "The system shall do something.",
        needIds: ["need-001"],
      },
    ],
    functions: [],
    components: [],
    satisfies: [],
    allocations: [],
  };
}

// ---------------------------------------------------------------------------
// Seeded-defect fixture
//
// Defect 1: def-operand  — SatisfyRequirementUsage → RequirementDefinition (not Usage)
// Defect 2: dangling     — relationship pointing at a non-existent target id
// Defect 3: laundered    — element with provenanceSourceId "requirement-deadbeef"
// ---------------------------------------------------------------------------

describe("validate_model — findings surface through MCP tool (ROADMAP criterion 1)", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;
  let corpusPath: string;
  let origCorpusEnv: string | undefined;
  let origAuditsEnv: string | undefined;

  // The corpus requirement id used for exact-match provenance
  const CORPUS_REQ_ID = "corpus-requirement-001";

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-findings-"));
    store = new FileStore(dir);
    await store.createProject("Findings Test");

    // Write the corpus fixture to a temp file
    corpusPath = path.join(dir, "extracted.json");
    await fs.writeFile(corpusPath, JSON.stringify(makeExtractedFixture(CORPUS_REQ_ID)), "utf8");

    // Save + set env vars
    origCorpusEnv = process.env.SYSML_BRIDGE_CORPUS_PATH;
    origAuditsEnv = process.env.SYSML_BRIDGE_AUDITS_DIR;
    process.env.SYSML_BRIDGE_CORPUS_PATH = corpusPath;
    // SYSML_BRIDGE_AUDITS_DIR is NOT set here — each test sets it if needed

    // Clear the cache so this test's fixture is picked up fresh
    clearCorpusCache();

    // ── Seed the defect model ────────────────────────────────────────────
    // Defect 1: systemReq is a RequirementDefinition (not Usage) and is the
    // TARGET of a SatisfyRequirementUsage → fires R4-def-operand
    const systemReq = await store.createElement("RequirementDefinition", "SystemReq", {
      provenanceSourceId: CORPUS_REQ_ID, // exact match — will resolve in corpus
    });

    // Satisfier part — also a Definition; when used as source of Satisfy it fires R4
    const satisfierPart = await store.createElement("PartDefinition", "SatisfierPart", {
      provenanceSourceId: CORPUS_REQ_ID, // resolves
    });

    // Defect 1: SatisfyRequirementUsage between two Definitions → R4-def-operand fires
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": satisfierPart.id }],
      target: [{ "@id": systemReq.id }],
    });

    // Defect 2: relationship with a non-existent target (dangling endpoint)
    const GHOST_ID = "00000000-dead-beef-0000-000000000000";
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": satisfierPart.id }],
      target: [{ "@id": GHOST_ID }],
    });

    // Defect 3: element with laundered provenance "requirement-deadbeef"
    await store.createElement("RequirementDefinition", "LaunderedReq", {
      provenanceSourceId: "requirement-deadbeef",
    });

    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });

    // Restore env vars
    if (origCorpusEnv === undefined) {
      delete process.env.SYSML_BRIDGE_CORPUS_PATH;
    } else {
      process.env.SYSML_BRIDGE_CORPUS_PATH = origCorpusEnv;
    }
    if (origAuditsEnv === undefined) {
      delete process.env.SYSML_BRIDGE_AUDITS_DIR;
    } else {
      process.env.SYSML_BRIDGE_AUDITS_DIR = origAuditsEnv;
    }

    clearCorpusCache();
  });

  it("findings[] contains R4-def-operand with severity 'error'", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    const r4 = parsed.findings.filter((f) => f.ruleId === "R4-def-operand");
    expect(r4.length).toBeGreaterThan(0);
    expect(r4[0].severity).toBe("error");
  });

  it("findings[] contains GATE02-dangling-endpoint", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    const dangling = parsed.findings.filter((f) => f.ruleId === "GATE02-dangling-endpoint");
    expect(dangling.length).toBeGreaterThan(0);
  });

  it("findings[] contains GATE03-unresolvable-provenance (laundered id)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    const unresolvable = parsed.findings.filter(
      (f) => f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(unresolvable.length).toBeGreaterThan(0);
    // The laundered id should appear in the message
    expect(unresolvable.some((f) => f.message.includes("requirement-deadbeef"))).toBe(true);
  });

  it("every finding has all five required keys (elementId, ruleId, message, severity, suggestedFix)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    expect(parsed.findings.length).toBeGreaterThan(0);
    for (const f of parsed.findings) {
      expect(typeof f.elementId).toBe("string");
      expect(typeof f.ruleId).toBe("string");
      expect(typeof f.message).toBe("string");
      expect(typeof f.severity).toBe("string");
      expect(typeof f.suggestedFix).toBe("string");
      // None of the five keys should be empty
      expect(f.elementId.length).toBeGreaterThan(0);
      expect(f.ruleId.length).toBeGreaterThan(0);
      expect(f.message.length).toBeGreaterThan(0);
      expect(f.severity.length).toBeGreaterThan(0);
      expect(f.suggestedFix.length).toBeGreaterThan(0);
    }
  });

  it("matrix has a row for the seeded SystemReq", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    expect(Array.isArray(parsed.matrix)).toBe(true);
    const row = parsed.matrix.find((r) => r.reqName === "SystemReq");
    expect(row).toBeDefined();
    // SystemReq has a SatisfyRequirementUsage → satisfied=true
    expect(row?.satisfied).toBe(true);
  });

  it("fidelity.fabrications is non-empty (laundered provenance id)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    expect(Array.isArray(parsed.fidelity.fabrications)).toBe(true);
    expect(parsed.fidelity.fabrications.length).toBeGreaterThan(0);
    // The laundered id should appear as the corpusId of the fabrication row
    expect(
      parsed.fidelity.fabrications.some((f) => f.corpusId === "requirement-deadbeef")
    ).toBe(true);
  });

  it("response always includes legacy coverage keys (envelope intact)", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    // Legacy keys must be present (GATE: envelope intact)
    expect(typeof (parsed.coverage as Record<string, unknown>).forwardPercent).toBe("number");
    expect(typeof (parsed.coverage as Record<string, unknown>).verifyPercent).toBe("number");
    expect(typeof (parsed.coverage as Record<string, unknown>).backwardPercent).toBe("number");
    expect(Array.isArray(parsed.issues)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// GATE-06: report artifact emission
// ---------------------------------------------------------------------------

describe("validate_model — GATE-06 report artifact emission (write_report=true)", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;
  let auditsDir: string;
  let corpusPath: string;
  let origCorpusEnv: string | undefined;
  let origAuditsEnv: string | undefined;

  const CORPUS_REQ_ID = "corpus-requirement-audit-001";

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-reports-"));
    auditsDir = path.join(dir, "audits");
    store = new FileStore(dir);
    await store.createProject("Reports Test");

    corpusPath = path.join(dir, "extracted.json");
    await fs.writeFile(corpusPath, JSON.stringify(makeExtractedFixture(CORPUS_REQ_ID)), "utf8");

    origCorpusEnv = process.env.SYSML_BRIDGE_CORPUS_PATH;
    origAuditsEnv = process.env.SYSML_BRIDGE_AUDITS_DIR;
    process.env.SYSML_BRIDGE_CORPUS_PATH = corpusPath;
    process.env.SYSML_BRIDGE_AUDITS_DIR = auditsDir;
    clearCorpusCache();

    // Seed one system requirement so the matrix has content
    await store.createElement("RequirementDefinition", "AuditReq", {
      provenanceSourceId: CORPUS_REQ_ID,
    });

    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });

    if (origCorpusEnv === undefined) {
      delete process.env.SYSML_BRIDGE_CORPUS_PATH;
    } else {
      process.env.SYSML_BRIDGE_CORPUS_PATH = origCorpusEnv;
    }
    if (origAuditsEnv === undefined) {
      delete process.env.SYSML_BRIDGE_AUDITS_DIR;
    } else {
      process.env.SYSML_BRIDGE_AUDITS_DIR = origAuditsEnv;
    }

    clearCorpusCache();
  });

  it("write_report=true: coverage-matrix.md and fidelity-report.md exist in audits dir", async () => {
    const result = await client.callTool({
      name: "validate_model",
      arguments: { write_report: true },
    });
    expect(result.isError).toBeFalsy();

    // Both files must exist
    const matrixFile = path.join(auditsDir, "coverage-matrix.md");
    const fidelityFile = path.join(auditsDir, "fidelity-report.md");
    await expect(fs.access(matrixFile)).resolves.toBeUndefined();
    await expect(fs.access(fidelityFile)).resolves.toBeUndefined();
  });

  it("write_report=true: coverage-matrix.md contains the seeded requirement name", async () => {
    await client.callTool({ name: "validate_model", arguments: { write_report: true } });

    const matrixContent = await fs.readFile(
      path.join(auditsDir, "coverage-matrix.md"),
      "utf8"
    );
    expect(matrixContent).toContain("AuditReq");
    expect(matrixContent).toContain("# Traceability Coverage Matrix");
  });

  it("write_report=false (default): audits dir is NOT created", async () => {
    await client.callTool({ name: "validate_model", arguments: {} });

    // The audits dir should not exist because write_report defaults to false
    let exists = true;
    try {
      await fs.access(auditsDir);
    } catch {
      exists = false;
    }
    expect(exists).toBe(false);
  });

  it("write_report=true: response includes reportPaths with matrixPath and fidelityPath", async () => {
    const result = await client.callTool({
      name: "validate_model",
      arguments: { write_report: true },
    });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    expect(parsed.reportPaths).toBeDefined();
    expect(typeof parsed.reportPaths?.matrixPath).toBe("string");
    expect(typeof parsed.reportPaths?.fidelityPath).toBe("string");
    expect(parsed.reportPaths?.matrixPath).toContain("coverage-matrix.md");
    expect(parsed.reportPaths?.fidelityPath).toContain("fidelity-report.md");
  });
});

// ---------------------------------------------------------------------------
// Corpus unavailable degradation
// ---------------------------------------------------------------------------

describe("validate_model — corpus unavailable degrades gracefully", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;
  let origCorpusEnv: string | undefined;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-no-corpus-"));
    store = new FileStore(dir);
    await store.createProject("No Corpus Test");

    // Save and unset corpus env (point to a non-existent file via arg instead)
    origCorpusEnv = process.env.SYSML_BRIDGE_CORPUS_PATH;
    // Delete env so the default path resolution is also unavailable
    delete process.env.SYSML_BRIDGE_CORPUS_PATH;
    clearCorpusCache();

    // Seed a minimal system requirement
    await store.createElement("RequirementDefinition", "ReqNoCorpus", {
      provenanceSourceId: "some-prov",
    });

    ({ client, cleanup } = await buildTestPair(store));
  });

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });

    if (origCorpusEnv === undefined) {
      delete process.env.SYSML_BRIDGE_CORPUS_PATH;
    } else {
      process.env.SYSML_BRIDGE_CORPUS_PATH = origCorpusEnv;
    }

    clearCorpusCache();
  });

  it("corpus unavailable: findings contains GATE03-corpus-unavailable warning", async () => {
    // Point corpus_path to a definitely non-existent file
    const result = await client.callTool({
      name: "validate_model",
      arguments: { corpus_path: "/tmp/does-not-exist-xyzzy/extracted.json" },
    });
    expect(result.isError).toBeFalsy();
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    const unavailable = parsed.findings.filter(
      (f) => f.ruleId === "GATE03-corpus-unavailable"
    );
    expect(unavailable.length).toBeGreaterThan(0);
    expect(unavailable[0].severity).toBe("warning");
  });

  it("corpus unavailable: legacy coverage keys still present", async () => {
    const result = await client.callTool({
      name: "validate_model",
      arguments: { corpus_path: "/tmp/does-not-exist-xyzzy/extracted.json" },
    });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    // Legacy keys must survive corpus failure
    expect(typeof (parsed.coverage as Record<string, unknown>).forwardPercent).toBe("number");
    expect(Array.isArray(parsed.issues)).toBe(true);
    expect(Array.isArray(parsed.matrix)).toBe(true);
  });

  it("corpus unavailable: fidelity has empty drops/fabrications/nearMatches", async () => {
    const result = await client.callTool({
      name: "validate_model",
      arguments: { corpus_path: "/tmp/does-not-exist-xyzzy/extracted.json" },
    });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as FindingsResult;

    expect(parsed.fidelity.drops).toHaveLength(0);
    expect(parsed.fidelity.fabrications).toHaveLength(0);
    expect(parsed.fidelity.nearMatches).toHaveLength(0);
  });
});
