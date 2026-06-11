/**
 * prose-gate.test.ts
 *
 * RED-first tests for G-B gate extension:
 *   C8  — Gate-1 prose-id resolution:
 *           - element with provenanceSourceId pointing at an APPROVED prose id passes Gate 1
 *           - element with provenanceSourceId pointing at a candidate id (not approved)
 *             FAILS with GATE03-unresolvable-provenance
 *           - store is UNCHANGED on reject
 *   C9  — PROSE-suspect-source warning:
 *           - composeIR entries with status:suspect cause audit() to emit
 *             PROSE-suspect-source warning-severity findings listing the suspect ids
 *           - entries still compose (not an error, not blocked)
 *   C13 — corpus.ts buildResolutionSet with a ProseComposedIR includes approved prose ids
 *   C14 — the n2-precedent narrowness: only approved prose ids are added, not candidate ids
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import {
  buildResolutionSet,
  buildResolutionSetFromComposed,
  clearCorpusCache,
} from "../audit/corpus.js";
import { provenanceFindings } from "../audit/provenance.js";
import { audit } from "../audit/index.js";
import type { Extracted, ProseComposedIR } from "@sysml-bridge/ir";
import type { SysmlElement } from "../types/sysml-elements.js";

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

// A minimal ProseComposedIR with one approved prose entry
function makeComposedIR(
  approvedIds: string[],
  suspectIds: string[] = []
): ProseComposedIR {
  const allEntries = [
    ...approvedIds.map((id) => ({
      id,
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
      approvedAt: "2026-06-10T00:00:00.000Z",
      candidateId: `cand-${id}`,
      status: "approved" as const,
    })),
    ...suspectIds.map((id) => ({
      id,
      kind: "requirement" as const,
      fields: {},
      citation: {
        docId: "doc-001",
        docSha256: "bb".repeat(32),
        chunkId: "chunk-001",
        sectionPath: "Section 1",
        quote: "The system shall do a suspect thing.",
      },
      approvedBy: "test",
      approvedAt: "2026-06-10T00:00:00.000Z",
      candidateId: `cand-suspect-${id}`,
      status: "suspect" as const,
    })),
  ];

  return {
    extracted: MINIMAL_EXTRACTED,
    proseEntries: allEntries,
    approvedProseIds: new Set(approvedIds),
  };
}

// ---------------------------------------------------------------------------
// C13 — buildResolutionSetFromComposed includes approved prose ids
// ---------------------------------------------------------------------------

describe("buildResolutionSetFromComposed — C13", () => {
  it("approved prose id is included in the resolution set", () => {
    const approvedId = "prose-aabbcc00deadbeef";
    const composed = makeComposedIR([approvedId]);
    const s = buildResolutionSetFromComposed(composed);
    expect(s.has(approvedId)).toBe(true);
  });

  it("multiple approved prose ids are all included", () => {
    const ids = ["prose-id-001", "prose-id-002", "prose-id-003"];
    const composed = makeComposedIR(ids);
    const s = buildResolutionSetFromComposed(composed);
    for (const id of ids) {
      expect(s.has(id)).toBe(true);
    }
  });

  it("existing corpus entity ids are still included when prose ids are added", () => {
    const approvedId = "prose-aabbcc00deadbeef";
    const composed = makeComposedIR([approvedId]);
    const s = buildResolutionSetFromComposed(composed);
    // Existing corpus entity must still resolve
    expect(s.has("requirement-abc")).toBe(true);
    expect(s.has("need-001")).toBe(true);
  });

  it("ALLOWLIST values are still included", () => {
    const composed = makeComposedIR(["prose-001"]);
    const s = buildResolutionSetFromComposed(composed);
    expect(s.has("model-asserted")).toBe(true);
    expect(s.has("Test")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// C14 — narrowness: candidate ids (not in approvedProseIds) do NOT resolve
// ---------------------------------------------------------------------------

describe("buildResolutionSetFromComposed — C14 narrowness", () => {
  it("a candidate id (not in approvedProseIds) does NOT appear in resolution set", () => {
    const approvedId = "prose-approved-001";
    const candidateId = "candidate-xyz-unreviewed"; // not in approvedProseIds
    const composed = makeComposedIR([approvedId]);
    const s = buildResolutionSetFromComposed(composed);
    // The candidate id must NOT be in the set
    expect(s.has(candidateId)).toBe(false);
  });

  it("suspect entry id is NOT in the resolution set (suspect != approved)", () => {
    const approvedId = "prose-approved-001";
    const suspectId = "prose-suspect-001";
    const composed = makeComposedIR([approvedId], [suspectId]);
    const s = buildResolutionSetFromComposed(composed);
    // suspect entry is NOT in approvedProseIds → must not resolve
    expect(s.has(suspectId)).toBe(false);
    // approved entry IS
    expect(s.has(approvedId)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// C8 — provenanceFindings: approved prose id passes, candidate id → GATE03 error
// ---------------------------------------------------------------------------

describe("provenanceFindings — C8 gate pass / reject", () => {
  it("element with provenanceSourceId = approved prose id emits NO GATE03 error", () => {
    const approvedId = "prose-approved-001";
    const composed = makeComposedIR([approvedId]);
    const resolutionSet = buildResolutionSetFromComposed(composed);

    const el = mkEl("el-001", "RequirementDefinition", approvedId);
    const findings = provenanceFindings([el], resolutionSet);
    const errors = findings.filter(
      (f) => f.severity === "error" && f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(errors).toHaveLength(0);
  });

  it("element with provenanceSourceId = candidate id (not approved) emits GATE03-unresolvable-provenance error", () => {
    const approvedId = "prose-approved-001";
    const candidateId = "candidate-unreviewed-xyz";
    const composed = makeComposedIR([approvedId]);
    const resolutionSet = buildResolutionSetFromComposed(composed);

    const el = mkEl("el-002", "RequirementDefinition", candidateId);
    const findings = provenanceFindings([el], resolutionSet);
    const errors = findings.filter(
      (f) => f.severity === "error" && f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toContain(candidateId);
  });

  it("element with provenanceSourceId = unknown prose id (not in set) emits GATE03-unresolvable-provenance", () => {
    const resolutionSet = buildResolutionSet(MINIMAL_EXTRACTED);
    const fakeProseId = "prose-completely-unknown-id";

    const el = mkEl("el-003", "PartDefinition", fakeProseId);
    const findings = provenanceFindings([el], resolutionSet);
    const errors = findings.filter(
      (f) => f.severity === "error" && f.ruleId === "GATE03-unresolvable-provenance"
    );
    expect(errors).toHaveLength(1);
  });
});

// ---------------------------------------------------------------------------
// C9 — audit() emits PROSE-suspect-source warning for suspect entries
// ---------------------------------------------------------------------------

describe("audit() — C9 PROSE-suspect-source warning", () => {
  it("suspect prose entry in composed IR causes PROSE-suspect-source warning finding", () => {
    const approvedId = "prose-approved-001";
    const suspectId = "prose-suspect-001";
    const composed = makeComposedIR([approvedId], [suspectId]);

    // Run audit with the composed IR (passing the proseComposedIR to audit)
    const result = audit([], [], composed);

    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "PROSE-suspect-source"
    );
    expect(suspectFindings.length).toBeGreaterThan(0);
    // Must be warning severity (not error)
    expect(suspectFindings.every((f) => f.severity === "warning")).toBe(true);
    // Must list the suspect id
    expect(suspectFindings.some((f) => f.message.includes(suspectId))).toBe(true);
  });

  it("no suspect entries → no PROSE-suspect-source findings", () => {
    const approvedId = "prose-approved-001";
    const composed = makeComposedIR([approvedId]);

    const result = audit([], [], composed);

    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "PROSE-suspect-source"
    );
    expect(suspectFindings).toHaveLength(0);
  });

  it("suspect entries still compose (audit does not reject them — not an error)", () => {
    const suspectId = "prose-suspect-001";
    const composed = makeComposedIR([], [suspectId]);

    // Must not throw — suspect entries compose with a warning, not a hard error
    expect(() => audit([], [], composed)).not.toThrow();

    const result = audit([], [], composed);
    // The warning is emitted
    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "PROSE-suspect-source"
    );
    expect(suspectFindings.length).toBeGreaterThan(0);
    // No errors from PROSE-suspect-source
    const suspectErrors = suspectFindings.filter((f) => f.severity === "error");
    expect(suspectErrors).toHaveLength(0);
  });

  it("audit() with plain Extracted (null prose) still works — no PROSE-suspect-source", () => {
    // Backward compat: calling audit() with null corpus (existing behavior)
    const result = audit([], [], null);
    const suspectFindings = result.findings.filter(
      (f) => f.ruleId === "PROSE-suspect-source"
    );
    expect(suspectFindings).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// C8 structural gate: write-path gate uses prose resolution set
// ---------------------------------------------------------------------------

describe("structural gate — C8 write-path with prose ids", () => {
  it("structuralCheck with prose-extended resolutionSet: approved prose id passes", async () => {
    const { structuralCheck } = await import("../audit/structural.js");
    const approvedId = "prose-approved-gate-001";
    const composed = makeComposedIR([approvedId]);
    const resolutionSet = buildResolutionSetFromComposed(composed);

    const candidate = {
      id: "new-el-001",
      type: "RequirementUsage",
      name: "NewEl",
      sourceIds: [],
      targetIds: [],
      provenanceSourceId: approvedId,
    };
    const findings = structuralCheck(candidate, [], resolutionSet);
    const errors = findings.filter(
      (f) => f.ruleId === "GATE03-unresolvable-provenance" && f.severity === "error"
    );
    expect(errors).toHaveLength(0);
  });

  it("structuralCheck: candidate prose id (not approved) → GATE03 error, store unchanged signal", async () => {
    const { structuralCheck } = await import("../audit/structural.js");
    const candidateId = "candidate-prose-not-approved";
    const resolutionSet = buildResolutionSet(MINIMAL_EXTRACTED);

    const candidate = {
      id: "new-el-002",
      type: "RequirementUsage",
      name: "NewEl",
      sourceIds: [],
      targetIds: [],
      provenanceSourceId: candidateId,
    };
    const findings = structuralCheck(candidate, [], resolutionSet);
    const errors = findings.filter(
      (f) => f.ruleId === "GATE03-unresolvable-provenance" && f.severity === "error"
    );
    expect(errors).toHaveLength(1);
  });
});
