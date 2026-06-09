/**
 * validate-coverage.test.ts
 *
 * Tests for the extended validate_model tool: traceability coverage axes
 * (forward-union, verify-either-dir, backward, orphan), provenance gate, and
 * dangling-endpoint detection.
 *
 * Fixture (main describe):
 *   - Req A — SatisfyRequirementUsage (forward ✓), VerifyRequirementUsage (verify ✓), provenanceSourceId set
 *   - Req B — AllocationUsage only (forward ✓ via union), no verify, no provenanceSourceId
 *   - PartA — SOURCE of SatisfyRequirementUsage (→ ReqA) and AllocationUsage (→ ReqB);
 *             it is the satisfier/allocator, so it participates in trace edges → NOT an orphan
 *   - TracedComponent — TARGET of an AllocationUsage (a function allocated to it) → NOT an orphan
 *   - TracedFunction  — SOURCE of a SatisfyRequirementUsage (satisfies a req) → NOT an orphan
 *   - OrphanPart — PartDefinition with ZERO trace edges (no satisfy, no allocate, no derive,
 *                  in either direction) → IS an orphan
 *   - GhostRel — relationship whose targetId does not exist → dangling endpoint
 *
 * Assertions:
 *   forwardPercent === 100 (A via Satisfy, B via Allocation)
 *   verifyPercent  === 50  (A verified, B not)
 *   B appears in elementsMissingBackpointer
 *   OrphanPart appears in orphanElements; PartA, TracedComponent, TracedFunction do NOT
 *   GhostRel appears in danglingRelationships
 *
 * Additional describe blocks:
 *   - Need coverage semantics (derive-covered Needs, uncovered Needs, Need exemption from forward/verify)
 *   - Container exemption from orphan check (FeatureMembership children)
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

interface NeedCoverage {
  percent: number;
  total: number;
  covered: number;
  uncovered: Array<{ id: string; name: string | null }>;
}

interface CoverageResult {
  summary: unknown;
  issues: string[];
  coverage: {
    forwardPercent: number;
    verifyPercent: number;
    backwardPercent: number;
    needCoverage: NeedCoverage;
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

    // ── TracedComponent: PartDefinition that is the TARGET of an AllocationUsage ──
    // A function is allocated TO this component → it participates as target of a trace edge.
    // Correct behavior: NOT an orphan (it has an inbound AllocationUsage edge).
    const tracedComponent = await store.createElement("PartDefinition", "TracedComponent", {
      provenanceSourceId: "corpus-design-4.0",
    });
    const tracedFunction = await store.createElement("ActionDefinition", "TracedFunction", {
      provenanceSourceId: "corpus-design-4.1",
    });
    // TracedFunction → (SatisfyRequirementUsage) → ReqA  (TracedFunction is SOURCE)
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": tracedFunction.id }],
      target: [{ "@id": reqA.id }],
    });
    // TracedFunction → (AllocationUsage) → TracedComponent  (TracedComponent is TARGET)
    await store.createElement("AllocationUsage", "", {
      source: [{ "@id": tracedFunction.id }],
      target: [{ "@id": tracedComponent.id }],
    });

    // ── OrphanPart: PartDefinition with ZERO trace edges in either direction ──
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

  it("OrphanPart appears in orphanElements (zero trace edges); traced elements do NOT", async () => {
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;
    const orphans = parsed.coverage.orphanElements;

    // Negative case: element with zero trace edges is flagged as orphan
    expect(orphans.some((e) => e.name === "OrphanPart")).toBe(true);

    // Positive controls: elements that participate in trace edges (as source OR target)
    // must NOT appear in orphanElements.
    // PartA is source of SatisfyRequirementUsage and AllocationUsage → not an orphan
    expect(orphans.some((e) => e.name === "PartA")).toBe(false);
    // TracedFunction is source of SatisfyRequirementUsage (satisfies a req) → not an orphan
    expect(orphans.some((e) => e.name === "TracedFunction")).toBe(false);
    // TracedComponent is target of AllocationUsage (a function is allocated to it) → not an orphan
    expect(orphans.some((e) => e.name === "TracedComponent")).toBe(false);
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

// ---------------------------------------------------------------------------
// Stakeholder Need semantics
// ---------------------------------------------------------------------------

describe("validate_model — stakeholder Need coverage semantics", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("(a) a Need with an incoming DeriveRequirementUsage is COVERED and NOT counted in forward/verify", async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-need-a-"));
    store = new FileStore(dir);
    await store.createProject("Need Coverage Test A");

    // One Need (stakeholderNeed=true), one system Requirement
    const need = await store.createElement("RequirementDefinition", "NeedCovered", {
      provenanceSourceId: "N1",
      stakeholderNeed: true,
    });
    const sysReq = await store.createElement("RequirementDefinition", "SysReq1", {
      provenanceSourceId: "R1",
    });

    // System req derives from need (req is SOURCE, need is TARGET)
    await store.createElement("DeriveRequirementUsage", "", {
      source: [{ "@id": sysReq.id }],
      target: [{ "@id": need.id }],
    });

    // System req also has forward + verify edges so it passes all checks
    const fn = await store.createElement("ActionDefinition", "Fn1", { provenanceSourceId: "F1" });
    const vc = await store.createElement("PartDefinition", "Verify1", { provenanceSourceId: "VC1" });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": fn.id }],
      target: [{ "@id": sysReq.id }],
    });
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": vc.id }],
      target: [{ "@id": sysReq.id }],
    });

    ({ client, cleanup } = await buildTestPair(store));
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;

    // forward/verify are computed over system requirements ONLY (1 system req, fully traced)
    expect(parsed.coverage.forwardPercent).toBe(100);
    expect(parsed.coverage.verifyPercent).toBe(100);

    // Need is covered
    expect(parsed.coverage.needCoverage.total).toBe(1);
    expect(parsed.coverage.needCoverage.covered).toBe(1);
    expect(parsed.coverage.needCoverage.percent).toBe(100);
    expect(parsed.coverage.needCoverage.uncovered).toHaveLength(0);
  });

  it("(b) a Need with NO incoming DeriveRequirementUsage is flagged in needCoverage.uncovered", async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-need-b-"));
    store = new FileStore(dir);
    await store.createProject("Need Coverage Test B");

    // One covered need, one uncovered need
    const coveredNeed = await store.createElement("RequirementDefinition", "NeedCovered", {
      provenanceSourceId: "N1",
      stakeholderNeed: true,
    });
    await store.createElement("RequirementDefinition", "NeedUncovered", {
      provenanceSourceId: "N2",
      stakeholderNeed: true,
    });

    // One system req that derives from coveredNeed only
    const sysReq = await store.createElement("RequirementDefinition", "SysReq1", {
      provenanceSourceId: "R1",
    });
    await store.createElement("DeriveRequirementUsage", "", {
      source: [{ "@id": sysReq.id }],
      target: [{ "@id": coveredNeed.id }],
    });

    // System req gets forward + verify so it doesn't pollute other checks
    const fn = await store.createElement("ActionDefinition", "Fn1", { provenanceSourceId: "F1" });
    const vc = await store.createElement("PartDefinition", "Verify1", { provenanceSourceId: "VC1" });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": fn.id }],
      target: [{ "@id": sysReq.id }],
    });
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": vc.id }],
      target: [{ "@id": sysReq.id }],
    });

    ({ client, cleanup } = await buildTestPair(store));
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;

    expect(parsed.coverage.needCoverage.total).toBe(2);
    expect(parsed.coverage.needCoverage.covered).toBe(1);
    expect(parsed.coverage.needCoverage.percent).toBe(50);
    expect(parsed.coverage.needCoverage.uncovered.some((n) => n.name === "NeedUncovered")).toBe(true);
    expect(parsed.coverage.needCoverage.uncovered.some((n) => n.name === "NeedCovered")).toBe(false);

    // Issue should mention the uncovered need
    const allIssues = parsed.issues.join("\n");
    expect(allIssues).toMatch(/NeedUncovered/);
  });
});

// ---------------------------------------------------------------------------
// Container exemption from orphan check
// ---------------------------------------------------------------------------

describe("validate_model — container PartDefinition exemption from orphan check", () => {
  let dir: string;
  let store: FileStore;
  let client: Client;
  let cleanup: () => Promise<void>;

  afterEach(async () => {
    await cleanup();
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("(c) a container PartDefinition with a FeatureMembership child is NOT an orphan even with no satisfy edge", async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-container-c-"));
    store = new FileStore(dir);
    await store.createProject("Container Test C");

    // A container part with one child (via FeatureMembership) and no direct trace edge
    const container = await store.createElement("PartDefinition", "ContainerPart", {
      provenanceSourceId: "subsystem-1",
    });
    const child = await store.createElement("PartDefinition", "ChildPart", {
      provenanceSourceId: "comp-1",
    });
    // FeatureMembership: container → child
    await store.createElement("FeatureMembership", "", {
      source: [{ "@id": container.id }],
      target: [{ "@id": child.id }],
    });

    // Child has a real trace edge so it is not orphaned either
    const req = await store.createElement("RequirementDefinition", "Req1", {
      provenanceSourceId: "R1",
    });
    const fn = await store.createElement("ActionDefinition", "Fn1", { provenanceSourceId: "F1" });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": fn.id }],
      target: [{ "@id": req.id }],
    });
    await store.createElement("AllocationUsage", "", {
      source: [{ "@id": fn.id }],
      target: [{ "@id": child.id }],
    });
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": child.id }],
      target: [{ "@id": req.id }],
    });

    ({ client, cleanup } = await buildTestPair(store));
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;

    const orphans = parsed.coverage.orphanElements;
    // Container is NOT an orphan (it owns a child via FeatureMembership)
    expect(orphans.some((e) => e.name === "ContainerPart")).toBe(false);
    // Child is NOT an orphan (AllocationUsage inbound)
    expect(orphans.some((e) => e.name === "ChildPart")).toBe(false);
  });

  it("(d) a leaf PartDefinition with no trace edge IS an orphan", async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-leaf-d-"));
    store = new FileStore(dir);
    await store.createProject("Leaf Orphan Test D");

    // A leaf part: no FeatureMembership children, no trace edges
    await store.createElement("PartDefinition", "LeafOrphan", {
      provenanceSourceId: "leaf-1",
    });

    // Add a traced requirement so the model is not completely empty
    const req = await store.createElement("RequirementDefinition", "Req1", {
      provenanceSourceId: "R1",
    });
    const fn = await store.createElement("ActionDefinition", "Fn1", { provenanceSourceId: "F1" });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": fn.id }],
      target: [{ "@id": req.id }],
    });
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": fn.id }],
      target: [{ "@id": req.id }],
    });

    ({ client, cleanup } = await buildTestPair(store));
    const result = await client.callTool({ name: "validate_model", arguments: {} });
    const text = (result.content as Array<{ type: string; text: string }>)[0].text;
    const parsed = JSON.parse(text) as CoverageResult;

    const orphans = parsed.coverage.orphanElements;
    // Leaf with no trace edge and no FeatureMembership children IS an orphan
    expect(orphans.some((e) => e.name === "LeafOrphan")).toBe(true);
  });
});
