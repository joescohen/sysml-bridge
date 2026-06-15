/**
 * audit-fidelity.test.ts
 *
 * TDD tests for GATE-04 fidelity three-bucket reconciliation (drops / fabrications /
 * near-matches). Uses the exact fixture from the plan's <behavior> block.
 *
 * Fixture:
 *   Corpus:
 *     R1 = { id:"requirement-aaa", naturalKey:"CC-1", name:"Aircraft ID Verification" }
 *     R2 = { id:"requirement-bbb", naturalKey:"CC-2", name:"Generate Schedule" }
 *     F1 = { id:"function-ccc", naturalKey:"F1.1", name:"Manage Refueling Requests" }
 *   Model elements (non-relationship):
 *     M1 — provenanceSourceId:"requirement-bbb", name:"Generate Schedule"  (exact match → R2)
 *     M2 — provenanceSourceId:"fake-xyz",        name:"ID Verification Aircraft" (token-reorder of R1)
 *
 * Expected behavior:
 *   drops       ← R1 and F1 (no model element resolves to them); NOT R2
 *   fabrications ← M2 (present-but-unresolvable provenance "fake-xyz"); NOT M1
 *   nearMatches  ← (R1, M2) pair with similarity >= 0.90 and band "confident"
 *   R1 STAYS in drops (never auto-merged even though it near-matches M2)
 *   M2 STAYS in fabrications (never auto-merged)
 *   F1 vs M2 NOT in nearMatches ("Manage Refueling Requests" vs "ID Verification Aircraft" = unmatched band)
 *   R2/M1 (exact-matched) appear in NO fuzzy comparison output
 *
 * Anti-laundering invariant (locked):
 *   near-matches are surfaced for human review ONLY — never auto-merged
 */

import { describe, it, expect } from "vitest";
import type { SysmlElement } from "../types/sysml-elements.js";
import type { Extracted } from "@sysml-bridge/ir";
import { fidelityReport } from "../audit/fidelity.js";
import { buildResolutionSet } from "../audit/corpus.js";

// ---------------------------------------------------------------------------
// Corpus fixture — minimal Extracted conforming to ExtractedSchema
// ---------------------------------------------------------------------------

const CORPUS: Extracted = {
  schema_version: "1.0.0",
  subsystem: "CC",
  needs: [],
  satisfies: [],
  allocations: [],
  requirements: [
    {
      id: "requirement-aaa",
      kind: "requirement",
      naturalKey: "CC-1",
      name: "Aircraft ID Verification",
      statement: "The system shall verify aircraft identity.",
      needIds: [],
    },
    {
      id: "requirement-bbb",
      kind: "requirement",
      naturalKey: "CC-2",
      name: "Generate Schedule",
      statement: "The system shall generate a schedule.",
      needIds: [],
    },
  ],
  functions: [
    {
      id: "function-ccc",
      kind: "function",
      naturalKey: "F1.1",
      name: "Manage Refueling Requests",
      level: "1",
      owner: "CC",
    },
  ],
  components: [],
};

// ---------------------------------------------------------------------------
// Model element helpers
// ---------------------------------------------------------------------------

function makeElement(
  id: string,
  type: string,
  name: string | null,
  provenanceSourceId?: string
): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: provenanceSourceId !== undefined ? { provenanceSourceId } : {},
  };
}

// M1 — exact provenance match to R2
const M1 = makeElement("model-m1", "RequirementDefinition", "Generate Schedule", "requirement-bbb");
// M2 — unresolvable provenance + token-reorder name (near-match to R1)
const M2 = makeElement("model-m2", "PartDefinition", "ID Verification Aircraft", "fake-xyz");

// ---------------------------------------------------------------------------
// Build resolution set from corpus
// ---------------------------------------------------------------------------

const RESOLUTION_SET = buildResolutionSet(CORPUS);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("fidelityReport — GATE-04 three-bucket reconciliation", () => {
  it("drops contains R1 and F1 — not R2", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    const dropIds = result.drops.map((d) => d.corpusId);
    expect(dropIds).toContain("requirement-aaa"); // R1 dropped
    expect(dropIds).toContain("function-ccc"); // F1 dropped
    expect(dropIds).not.toContain("requirement-bbb"); // R2 is matched by M1
  });

  it("fabrications contains M2 — not M1", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    const fabIds = result.fabrications.map((f) => f.corpusId);
    // M2's offending provenance is "fake-xyz"
    expect(fabIds).toContain("fake-xyz");
    // M1's provenance "requirement-bbb" is in resolution set → NOT a fabrication
    const m1Fab = result.fabrications.find((f) => f.corpusId === "requirement-bbb");
    expect(m1Fab).toBeUndefined();
  });

  it("nearMatches contains the (R1, M2) token-reorder pair with similarity >= 0.90 and band confident", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    const nm = result.nearMatches.find(
      (n) => n.corpusId === "requirement-aaa" && n.modelElementId === "model-m2"
    );
    expect(nm).toBeDefined();
    expect(nm!.similarity).toBeGreaterThanOrEqual(0.90);
    expect(nm!.band).toBe("confident");
  });

  it("R1 STAYS in drops even though it near-matches M2 (no auto-merge — anti-laundering)", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    const r1Drop = result.drops.find((d) => d.corpusId === "requirement-aaa");
    expect(r1Drop).toBeDefined(); // R1 remains in drops
  });

  it("M2 STAYS in fabrications even though it near-matches R1 (no auto-merge — anti-laundering)", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    const m2Fab = result.fabrications.find((f) => f.corpusId === "fake-xyz");
    expect(m2Fab).toBeDefined(); // M2 remains in fabrications
  });

  it("F1 vs M2 does NOT appear in nearMatches (unmatched band)", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    const f1M2 = result.nearMatches.find(
      (n) => n.corpusId === "function-ccc" && n.modelElementId === "model-m2"
    );
    expect(f1M2).toBeUndefined();
  });

  it("R2/M1 (exact-matched pair) do NOT appear in nearMatches (fuzzy ran on residual only)", () => {
    const result = fidelityReport([M1, M2], CORPUS, RESOLUTION_SET);
    // M1 exactly matched R2 — neither should appear in nearMatches as a residual comparison
    const r2InNear = result.nearMatches.find(
      (n) => n.corpusId === "requirement-bbb" && n.modelElementId === "model-m1"
    );
    expect(r2InNear).toBeUndefined();
  });

  it("empty model: all corpus entities are drops, no fabrications, no nearMatches", () => {
    const result = fidelityReport([], CORPUS, RESOLUTION_SET);
    expect(result.drops).toHaveLength(3); // R1, R2, F1
    expect(result.fabrications).toHaveLength(0);
    expect(result.nearMatches).toHaveLength(0);
  });

  it("element with no provenanceSourceId (empty string) is treated as missing — not a fabrication", () => {
    const noProvEl = makeElement("model-noprov", "PartDefinition", "Some Part", "");
    const result = fidelityReport([noProvEl], CORPUS, RESOLUTION_SET);
    // empty string provenance should not produce a fabrication entry
    const fabIds = result.fabrications.map((f) => f.corpusId);
    expect(fabIds).not.toContain("");
  });

  it("relationship-type elements are excluded from the model side scan", () => {
    const relEl = makeElement("rel-001", "SatisfyRequirementUsage", "Satisfy R1", "requirement-bbb");
    // With M1 (matches R2) and relEl (should be skipped as relationship type):
    const result = fidelityReport([M1, relEl], CORPUS, RESOLUTION_SET);
    // R2 matched by M1 → not in drops; R1 and F1 still in drops
    const dropIds = result.drops.map((d) => d.corpusId);
    expect(dropIds).toContain("requirement-aaa");
    expect(dropIds).not.toContain("requirement-bbb");
    // relEl provenance should not be treated as a fabrication (it's skipped)
    const fabIds = result.fabrications.map((f) => f.corpusId);
    expect(fabIds).not.toContain("requirement-bbb");
  });
});
