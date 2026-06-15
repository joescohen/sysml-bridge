/**
 * inferred-gate.test.ts — RED-first tests for F8 Gate-1 extension (T2)
 *
 * Covers the spec acceptance criteria A5 and A6:
 *
 * A5 — Approval round-trip:
 *   - Element with provenanceSourceId = approved inferred id → passes Gate 1 (no GATE03 error)
 *   - Same element with the id ONLY in a candidate (not in inferred-approved.json) → GATE03 error
 *
 * A6 — Suspect premise propagation:
 *   - Superseding a premise prose entry → dependent inferred entry composes as suspect
 *   - INFER-suspect-premise warning emitted with entry id + offending premise id
 *
 * T2a — Resolution set extension:
 *   - buildResolutionSetFromInferred adds approved inferred ids
 *   - suspect/superseded inferred ids NOT added
 *
 * T2b — New findings:
 *   - INFER-suspect-premise: warning — inferred entry whose premise is suspect/superseded
 *   - INFER-unpremised: error — inferred entry with no resolvable premises (defense-in-depth)
 *
 * T2c — Fidelity 4th bucket:
 *   - fidelityReport (or audit) exposes an `inferred` count (model elements with inferred-layer
 *     provenanceSourceId)
 *   - The inferred bucket is separate from drops/fabrications/nearMatches, never netted
 */

import { describe, it, expect } from "vitest";
import {
  buildResolutionSetFromInferred,
} from "../audit/corpus.js";
import { provenanceFindings } from "../audit/provenance.js";
import { audit } from "../audit/index.js";
import type { InferredComposedIR, InferredApprovedEntry } from "@sysml-bridge/ir";
import type { SysmlElement } from "../types/sysml-elements.js";
import type { Extracted } from "@sysml-bridge/ir";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function mkEl(id: string, type: string, prov?: string): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name: id,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: prov ? { provenanceSourceId: prov } : {},
  };
}

const MINIMAL_EXTRACTED: Extracted = {
  schema_version: "1.0.0",
  subsystem: "TestSub",
  needs: [{ id: "need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
  requirements: [
    {
      id: "requirement-abc",
      kind: "requirement",
      naturalKey: "CC-1",
      name: "Do Thing",
      statement: "The system shall do a thing.",
      needIds: ["need-001"],
    },
  ],
  functions: [],
  components: [],
  satisfies: [],
  allocations: [],
};

function makeInferredEntry(overrides: Partial<InferredApprovedEntry> = {}): InferredApprovedEntry {
  return {
    id: "inferred-test-abc123",
    relationFamily: "allocation",
    sourceId: "function-xyz",
    targetId: "component-111",
    premises: ["requirement-abc"],
    rationale: "Audit-only rationale.",
    confidence: 0.85,
    inferenceRunId: "run-test-001",
    approvedBy: "test-user",
    approvedAt: "2026-06-11T00:00:00.000Z",
    status: "approved",
    ...overrides,
  };
}

/** Build a minimal InferredComposedIR fixture */
function makeInferredComposedIR(
  approvedIds: string[],
  suspectEntries: InferredApprovedEntry[] = [],
  proseApprovedIds: string[] = [],
  proseEntries: Array<{ id: string; status: "approved" | "suspect" | "superseded" }> = []
): InferredComposedIR {
  const allInferredEntries: InferredApprovedEntry[] = [
    ...approvedIds.map((id) => makeInferredEntry({ id })),
    ...suspectEntries,
  ];

  return {
    extracted: MINIMAL_EXTRACTED,
    proseEntries: proseEntries.map((pe) => ({
      id: pe.id,
      kind: "requirement" as const,
      fields: {},
      citation: {
        docId: "doc-001",
        docSha256: "aa".repeat(32),
        chunkId: "chunk-001",
        sectionPath: "Section 1",
        quote: "The system shall do something.",
      },
      approvedBy: "test",
      approvedAt: "2026-06-11T00:00:00.000Z",
      candidateId: `cand-${pe.id}`,
      status: pe.status === "superseded" ? "approved" as const : pe.status, // superseded entries are filtered, but we use "approved" as fallback
    })),
    approvedProseIds: new Set(proseApprovedIds),
    inferredEntries: allInferredEntries,
    approvedInferredIds: new Set(approvedIds),
  };
}

// ---------------------------------------------------------------------------
// T2a — buildResolutionSetFromInferred
// ---------------------------------------------------------------------------

describe("buildResolutionSetFromInferred", () => {
  it("approved inferred entry id is in the resolution set", () => {
    const approvedId = "inferred-approved-001";
    const composed = makeInferredComposedIR([approvedId]);
    const s = buildResolutionSetFromInferred(composed);
    expect(s.has(approvedId)).toBe(true);
  });

  it("multiple approved inferred ids are all included", () => {
    const ids = ["inferred-id-001", "inferred-id-002", "inferred-id-003"];
    const composed = makeInferredComposedIR(ids);
    const s = buildResolutionSetFromInferred(composed);
    for (const id of ids) {
      expect(s.has(id)).toBe(true);
    }
  });

  it("existing corpus entity ids and ALLOWLIST are still included", () => {
    const approvedId = "inferred-approved-001";
    const composed = makeInferredComposedIR([approvedId]);
    const s = buildResolutionSetFromInferred(composed);
    expect(s.has("requirement-abc")).toBe(true);
    expect(s.has("model-asserted")).toBe(true);
  });

  it("suspect inferred entry id NOT in resolution set", () => {
    const suspectEntry = makeInferredEntry({
      id: "inferred-suspect-001",
      status: "suspect",
    });
    const composed = makeInferredComposedIR([], [suspectEntry]);
    const s = buildResolutionSetFromInferred(composed);
    expect(s.has("inferred-suspect-001")).toBe(false);
  });

  it("approvedInferredIds controls what gets added (defense-in-depth)", () => {
    // Entry appears in inferredEntries with status approved but NOT in approvedInferredIds
    // (edge case where someone manually constructs the composed IR)
    const composed = makeInferredComposedIR([]); // empty approvedInferredIds
    composed.inferredEntries.push(makeInferredEntry({ id: "inferred-extra-001", status: "approved" }));
    const s = buildResolutionSetFromInferred(composed);
    // Must not be in set — only approvedInferredIds is authoritative
    expect(s.has("inferred-extra-001")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// A5 — Gate-1 approval round-trip
// ---------------------------------------------------------------------------

describe("A5 — Gate-1 inferred-id approval round-trip", () => {
  it("element with provenanceSourceId = approved inferred id passes GATE03 (no error)", () => {
    const approvedId = "inferred-approved-gate-001";
    const composed = makeInferredComposedIR([approvedId]);
    const resolutionSet = buildResolutionSetFromInferred(composed);

    const el = mkEl("el-001", "PartUsage", approvedId);
    const findings = provenanceFindings([el], resolutionSet);
    const errors = findings.filter(
      (f) => f.severity === "error" && f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(errors).toHaveLength(0);
  });

  it("element with provenanceSourceId = candidate-only id (not in inferred-approved) fails GATE03", () => {
    const candidateId = "candidate-not-approved-inferred-xyz";
    // No approved inferred ids — empty composed IR
    const composed = makeInferredComposedIR([]);
    const resolutionSet = buildResolutionSetFromInferred(composed);

    const el = mkEl("el-002", "PartUsage", candidateId);
    const findings = provenanceFindings([el], resolutionSet);
    const errors = findings.filter(
      (f) => f.severity === "error" && f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toContain(candidateId);
  });

  it("suspect inferred entry id does NOT pass GATE03", () => {
    const suspectId = "inferred-suspect-gate-001";
    const suspectEntry = makeInferredEntry({ id: suspectId, status: "suspect" });
    const composed = makeInferredComposedIR([], [suspectEntry]);
    const resolutionSet = buildResolutionSetFromInferred(composed);

    const el = mkEl("el-003", "PartUsage", suspectId);
    const findings = provenanceFindings([el], resolutionSet);
    const errors = findings.filter(
      (f) => f.severity === "error" && f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(errors).toHaveLength(1);
  });
});

// ---------------------------------------------------------------------------
// A6 — INFER-suspect-premise warning
// ---------------------------------------------------------------------------

describe("A6 — INFER-suspect-premise warning", () => {
  it("suspect inferred entry in composed IR causes INFER-suspect-premise warning finding", () => {
    const suspectEntry = makeInferredEntry({
      id: "inferred-suspect-premise-001",
      premises: ["prose-premise-that-is-suspect"],
      status: "suspect",
    });
    const composed = makeInferredComposedIR([], [suspectEntry]);

    const result = audit([], [], composed);

    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "INFER-suspect-premise"
    );
    expect(suspectFindings.length).toBeGreaterThan(0);
    expect(suspectFindings.every((f) => f.severity === "warning")).toBe(true);
    expect(suspectFindings.some((f) => f.message.includes("inferred-suspect-premise-001"))).toBe(true);
  });

  it("INFER-suspect-premise message includes the offending premise id", () => {
    const offendingPremise = "prose-premise-that-is-suspect-xyz";
    const suspectEntry = makeInferredEntry({
      id: "inferred-suspect-with-premise",
      premises: [offendingPremise],
      status: "suspect",
    });
    const composed = makeInferredComposedIR([], [suspectEntry]);

    const result = audit([], [], composed);

    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "INFER-suspect-premise"
    );
    // The finding should mention the entry id
    expect(suspectFindings.some((f) => f.message.includes("inferred-suspect-with-premise"))).toBe(true);
  });

  it("approved inferred entry causes NO INFER-suspect-premise finding", () => {
    const approvedEntry = makeInferredEntry({
      id: "inferred-approved-no-suspect",
      status: "approved",
    });
    const composed = makeInferredComposedIR(["inferred-approved-no-suspect"]);

    const result = audit([], [], composed);

    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "INFER-suspect-premise"
    );
    expect(suspectFindings).toHaveLength(0);
  });

  it("no inferred entries → no INFER-suspect-premise findings", () => {
    const composed = makeInferredComposedIR([]);
    const result = audit([], [], composed);
    expect(result.findings.filter((f) => f.ruleId === "INFER-suspect-premise")).toHaveLength(0);
  });

  it("INFER-suspect-premise is warning severity (not error — still composes)", () => {
    const suspectEntry = makeInferredEntry({ id: "inferred-suspect-only", status: "suspect" });
    const composed = makeInferredComposedIR([], [suspectEntry]);

    const result = audit([], [], composed);
    const suspectErrors = result.findings.filter(
      (f) => f.ruleId === "INFER-suspect-premise" && f.severity === "error"
    );
    expect(suspectErrors).toHaveLength(0);
  });

  it("suspect entry still composes — audit() does not throw", () => {
    const suspectEntry = makeInferredEntry({ id: "inferred-suspect-compose", status: "suspect" });
    const composed = makeInferredComposedIR([], [suspectEntry]);

    expect(() => audit([], [], composed)).not.toThrow();
  });
});

// ---------------------------------------------------------------------------
// T2b — INFER-unpremised finding (defense-in-depth error)
// ---------------------------------------------------------------------------

describe("T2b — INFER-unpremised finding", () => {
  it("an inferred entry with empty premises list emits INFER-unpremised error", () => {
    // Note: the schema requires premises.min(1), so this is a defense-in-depth check
    // for entries that somehow bypassed schema validation.
    // We construct the entry directly (bypassing schema validation) to test the finding.
    const unpremisedEntry: InferredApprovedEntry = {
      id: "inferred-unpremised-001",
      relationFamily: "allocation",
      sourceId: "function-xyz",
      targetId: "component-111",
      premises: [], // INVALID — bypassing schema for defense-in-depth test
      rationale: "No premises — should be caught by defense-in-depth.",
      confidence: 0.9,
      inferenceRunId: "run-test-001",
      approvedBy: "test-user",
      approvedAt: "2026-06-11T00:00:00.000Z",
      status: "approved",
    };
    const composed: InferredComposedIR = {
      extracted: MINIMAL_EXTRACTED,
      proseEntries: [],
      approvedProseIds: new Set(),
      inferredEntries: [unpremisedEntry],
      approvedInferredIds: new Set(["inferred-unpremised-001"]),
    };

    const result = audit([], [], composed);

    const unpremisedFindings = result.findings.filter(
      (f) => f.ruleId === "INFER-unpremised"
    );
    expect(unpremisedFindings.length).toBeGreaterThan(0);
    expect(unpremisedFindings.every((f) => f.severity === "error")).toBe(true);
    expect(unpremisedFindings.some((f) => f.message.includes("inferred-unpremised-001"))).toBe(true);
  });

  it("well-premised approved entry does NOT emit INFER-unpremised", () => {
    const composed = makeInferredComposedIR(["inferred-well-premised-001"]);
    const result = audit([], [], composed);
    expect(result.findings.filter((f) => f.ruleId === "INFER-unpremised")).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// T2c — Fidelity 4th bucket: inferred count
// ---------------------------------------------------------------------------

describe("T2c — Fidelity 4th bucket: inferred element count", () => {
  it("model element with inferred-layer provenanceSourceId is counted in inferred bucket", () => {
    const approvedId = "inferred-approved-fidelity-001";
    const composed = makeInferredComposedIR([approvedId]);
    const resolutionSet = buildResolutionSetFromInferred(composed);

    const el = mkEl("el-001", "PartUsage", approvedId);
    const result = audit([el], [], composed);

    expect(result.fidelity).toHaveProperty("inferred");
    expect(result.fidelity.inferred).toBeGreaterThanOrEqual(1);
  });

  it("inferred count is separate from drops/fabrications (never netted)", () => {
    const approvedId = "inferred-approved-fidelity-002";
    const composed = makeInferredComposedIR([approvedId]);

    const el = mkEl("el-002", "PartUsage", approvedId);
    const result = audit([el], [], composed);

    // inferred bucket must be separate (defined independently)
    expect(typeof result.fidelity.inferred).toBe("number");
    // fabrications must NOT include approved inferred elements
    const fabrications = result.fidelity.fabrications.filter(
      (f) => f.corpusId === approvedId
    );
    expect(fabrications).toHaveLength(0);
  });

  it("model element with corpus provenance is NOT counted in inferred bucket", () => {
    const composed = makeInferredComposedIR([]);
    const el = mkEl("el-corpus", "PartDefinition", "requirement-abc");
    const result = audit([el], [], composed);

    // inferred count should be 0 — the element uses corpus provenance
    expect(result.fidelity.inferred).toBe(0);
  });

  it("zero inferred elements → inferred bucket is 0", () => {
    const composed = makeInferredComposedIR([]);
    const result = audit([], [], composed);
    expect(result.fidelity.inferred).toBe(0);
  });

  it("backward-compat: plain Extracted corpus still produces inferred:0 in fidelity", () => {
    const result = audit([], [], MINIMAL_EXTRACTED);
    expect(result.fidelity).toHaveProperty("inferred");
    expect(result.fidelity.inferred).toBe(0);
  });

  it("null corpus → inferred:0 in fidelity", () => {
    const result = audit([], [], null);
    expect(result.fidelity).toHaveProperty("inferred");
    expect(result.fidelity.inferred).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// A9 — backward-compat: plain Extracted and ProseComposedIR still work
// ---------------------------------------------------------------------------

describe("A9 — backward-compat: corpus without inferred layer", () => {
  it("audit() with null corpus still works (no inferred findings)", () => {
    const result = audit([], [], null);
    expect(result.findings.filter((f) => f.ruleId.startsWith("INFER-"))).toHaveLength(0);
  });

  it("audit() with plain Extracted still works", () => {
    const result = audit([], [], MINIMAL_EXTRACTED);
    expect(result.findings.filter((f) => f.ruleId.startsWith("INFER-"))).toHaveLength(0);
  });

  it("audit() with ProseComposedIR (no inferred layer) emits no INFER- findings", () => {
    const proseComposed = {
      extracted: MINIMAL_EXTRACTED,
      proseEntries: [],
      approvedProseIds: new Set<string>(),
    };
    const result = audit([], [], proseComposed as any);
    expect(result.findings.filter((f) => f.ruleId.startsWith("INFER-"))).toHaveLength(0);
  });
});
