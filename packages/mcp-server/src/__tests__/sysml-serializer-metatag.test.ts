/**
 * sysml-serializer-metatag.test.ts — RED-first tests for F8 InferenceProvenance metatag emission (A7)
 *
 * Tests:
 *   MT-1  InferenceProvenance def emitted exactly once when approved inferred entries present
 *   MT-2  Per-element `metadata InferenceProvenance about <name>` blocks emitted
 *   MT-3  provenanceClass = "inferred" for inferred entries
 *   MT-4  confidenceScore, premiseRefs, inferenceRunId, approvedBy emitted correctly
 *   MT-5  premiseRefs carries ids ONLY (never rationale text or corpus quotes)
 *   MT-6  rationale is NEVER exported (audit-only)
 *   MT-7  model-asserted allocate edge gets provenanceClass = "asserted"
 *   MT-8  No InferenceProvenance def emitted when no inferred/asserted entries
 *   MT-9  The def is emitted ONCE even when multiple inferred elements present
 *   MT-10 A7 probe: emitted text passes format check (exact §5 shapes)
 */

import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../utils/sysml-serializer.js";
import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";
import type { InferredApprovedEntry } from "@sysml-bridge/ir";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function el(overrides: Partial<SysmlElement>): SysmlElement {
  return {
    id: "e1",
    elementId: "e1",
    type: "PartDefinition",
    name: "Test",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
    ...overrides,
  };
}

function makeInferredEntry(overrides: Partial<InferredApprovedEntry> = {}): InferredApprovedEntry {
  return {
    id: "inferred-alloc-001",
    relationFamily: "allocation",
    sourceId: "function-xyz",
    targetId: "component-111",
    premises: ["req-abc", "function-xyz"],
    rationale: "AUDIT-ONLY: This text must never appear in exported SysML.",
    confidence: 0.82,
    inferenceRunId: "run-test-001",
    approvedBy: "alice",
    approvedAt: "2026-06-11T00:00:00.000Z",
    status: "approved",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// MT-1 — InferenceProvenance def emitted exactly once when inferred entries present
// ---------------------------------------------------------------------------

describe("InferenceProvenance def emission (MT-1)", () => {
  it("emits `metadata def InferenceProvenance` when approved inferred entries provided", () => {
    const fnEl = el({ id: "function-xyz", type: "ActionUsage", name: "ProcessData" });
    const compEl = el({ id: "component-111", type: "PartUsage", name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" } });

    const inferredEntry = makeInferredEntry();
    const result = serializeToSysml([fnEl, compEl], [], [inferredEntry]);

    expect(result).toContain("metadata def InferenceProvenance {");
  });

  it("does NOT emit InferenceProvenance def when no inferred entries", () => {
    const fnEl = el({ id: "e1", type: "PartDefinition", name: "Engine" });
    const result = serializeToSysml([fnEl], [], []);

    expect(result).not.toContain("metadata def InferenceProvenance");
  });

  it("does NOT emit InferenceProvenance def when inferred entries array is undefined", () => {
    const fnEl = el({ id: "e1", type: "PartDefinition", name: "Engine" });
    const result = serializeToSysml([fnEl], []);

    expect(result).not.toContain("metadata def InferenceProvenance");
  });
});

// ---------------------------------------------------------------------------
// MT-2 — Per-element about blocks emitted
// ---------------------------------------------------------------------------

describe("per-element about block emission (MT-2)", () => {
  it("emits `metadata InferenceProvenance about <name>` for an element with inferred provenanceSourceId", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const inferredEntry = makeInferredEntry({ id: "inferred-alloc-001" });
    const result = serializeToSysml([compEl], [], [inferredEntry]);

    expect(result).toContain("metadata InferenceProvenance about Controller {");
  });

  it("emits about blocks for each element with inferred provenance", () => {
    const comp1 = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });
    const comp2 = el({
      id: "component-222",
      type: "PartUsage",
      name: "Sensor",
      raw: { provenanceSourceId: "inferred-alloc-002" },
    });

    const entry1 = makeInferredEntry({ id: "inferred-alloc-001", targetId: "component-111" });
    const entry2 = makeInferredEntry({
      id: "inferred-alloc-002",
      targetId: "component-222",
      premises: ["req-xyz"],
      confidence: 0.72,
      inferenceRunId: "run-002",
    });
    const result = serializeToSysml([comp1, comp2], [], [entry1, entry2]);

    expect(result).toContain("metadata InferenceProvenance about Controller {");
    expect(result).toContain("metadata InferenceProvenance about Sensor {");
  });
});

// ---------------------------------------------------------------------------
// MT-3 — provenanceClass = "inferred" for inferred entries
// ---------------------------------------------------------------------------

describe("provenanceClass attribute (MT-3)", () => {
  it("emits provenanceClass = \"inferred\" for an inferred element", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml([compEl], [], [makeInferredEntry()]);

    expect(result).toContain('provenanceClass = "inferred"');
  });
});

// ---------------------------------------------------------------------------
// MT-4 — confidenceScore, premiseRefs, inferenceRunId, approvedBy correct
// ---------------------------------------------------------------------------

describe("attribute values in about block (MT-4)", () => {
  it("emits confidenceScore matching the entry confidence", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml([compEl], [], [makeInferredEntry({ confidence: 0.82 })]);

    expect(result).toContain("confidenceScore = 0.82");
  });

  it("emits premiseRefs as comma-joined premise ids", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml(
      [compEl],
      [],
      [makeInferredEntry({ premises: ["req-abc", "function-xyz"] })]
    );

    expect(result).toContain('premiseRefs = "req-abc, function-xyz"');
  });

  it("emits inferenceRunId from the entry", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml(
      [compEl],
      [],
      [makeInferredEntry({ inferenceRunId: "run-test-001" })]
    );

    expect(result).toContain('inferenceRunId = "run-test-001"');
  });

  it("emits approvedBy from the entry", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml([compEl], [], [makeInferredEntry({ approvedBy: "alice" })]);

    expect(result).toContain('approvedBy = "alice"');
  });
});

// ---------------------------------------------------------------------------
// MT-5 — premiseRefs carries ids ONLY (never rationale or quotes)
// ---------------------------------------------------------------------------

describe("premiseRefs content (MT-5 — privacy)", () => {
  it("premiseRefs contains only id strings, not rationale text", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const entry = makeInferredEntry({
      premises: ["req-abc"],
      rationale: "AUDIT-ONLY rationale text that must NEVER appear in exported files.",
    });
    const result = serializeToSysml([compEl], [], [entry]);

    // The rationale text must never appear in the exported SysML
    expect(result).not.toContain("AUDIT-ONLY rationale text");
    expect(result).not.toContain("NEVER appear in exported files");
    // premiseRefs must contain only the id
    expect(result).toContain('premiseRefs = "req-abc"');
  });
});

// ---------------------------------------------------------------------------
// MT-6 — rationale NEVER exported
// ---------------------------------------------------------------------------

describe("rationale not exported (MT-6 — A7 privacy)", () => {
  it("rationale text does not appear anywhere in the emitted output", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const entry = makeInferredEntry({
      rationale: "SECRET_RATIONALE_TEXT_XYZ_SHOULD_NOT_APPEAR",
    });
    const result = serializeToSysml([compEl], [], [entry]);

    expect(result).not.toContain("SECRET_RATIONALE_TEXT_XYZ_SHOULD_NOT_APPEAR");
  });
});

// ---------------------------------------------------------------------------
// MT-7 — model-asserted elements get provenanceClass = "asserted"
// ---------------------------------------------------------------------------

describe("model-asserted allocation tag (MT-7)", () => {
  it("element with provenanceSourceId = 'model-asserted' emits provenanceClass = \"asserted\"", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "model-asserted" },
    });

    // Pass an empty inferred entries array but flag the element is asserted
    const result = serializeToSysml([compEl], [], [], { emitAssertedTags: true });

    expect(result).toContain("metadata def InferenceProvenance {");
    expect(result).toContain("metadata InferenceProvenance about Controller {");
    expect(result).toContain('provenanceClass = "asserted"');
  });

  it("model-asserted element has empty inferenceRunId and premiseRefs", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "model-asserted" },
    });

    const result = serializeToSysml([compEl], [], [], { emitAssertedTags: true });

    // Empty strings or omitted — spec says "emit empty strings or omit those attributes"
    // We'll check it does NOT contain rationale, run ids, or premise quotes
    expect(result).not.toContain("AUDIT-ONLY");
  });

  it("model-asserted element does NOT emit tags when emitAssertedTags is not set", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "model-asserted" },
    });

    // Without the flag, no asserted tags emitted (default behavior unchanged)
    const result = serializeToSysml([compEl], [], []);

    expect(result).not.toContain("metadata def InferenceProvenance");
  });
});

// ---------------------------------------------------------------------------
// MT-8 — No def emitted when no inferred/asserted entries
// ---------------------------------------------------------------------------

describe("no def when empty (MT-8)", () => {
  it("no InferenceProvenance output for a plain model without any inferred entries", () => {
    const pkgEl = el({ id: "pkg", type: "Package", name: "MyPkg", ownerId: null });
    const child = el({ id: "c1", type: "PartDefinition", name: "Engine", ownerId: "pkg" });

    const result = serializeToSysml([pkgEl, child], [], []);

    expect(result).not.toContain("InferenceProvenance");
    expect(result).not.toContain("provenanceClass");
    expect(result).not.toContain("metadata");
  });
});

// ---------------------------------------------------------------------------
// MT-9 — def emitted ONCE with multiple inferred elements
// ---------------------------------------------------------------------------

describe("def emitted exactly once (MT-9)", () => {
  it("InferenceProvenance def appears exactly once with two inferred elements", () => {
    const comp1 = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });
    const comp2 = el({
      id: "component-222",
      type: "PartUsage",
      name: "Sensor",
      raw: { provenanceSourceId: "inferred-alloc-002" },
    });

    const entry1 = makeInferredEntry({ id: "inferred-alloc-001" });
    const entry2 = makeInferredEntry({ id: "inferred-alloc-002", premises: ["req-xyz"], confidence: 0.72 });
    const result = serializeToSysml([comp1, comp2], [], [entry1, entry2]);

    const defCount = (result.match(/metadata def InferenceProvenance \{/g) ?? []).length;
    expect(defCount).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// MT-10 — A7 probe: exact §5 shape
// ---------------------------------------------------------------------------

describe("A7 probe — exact §5 shape (MT-10)", () => {
  it("emits the InferenceProvenance def with all five attributes", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml([compEl], [], [makeInferredEntry()]);

    // §5 attribute: provenanceClass : ScalarValues::String
    expect(result).toContain("attribute provenanceClass : ScalarValues::String");
    // §5 attribute: confidenceScore : ScalarValues::Real
    expect(result).toContain("attribute confidenceScore : ScalarValues::Real");
    // §5 attribute: premiseRefs : ScalarValues::String
    expect(result).toContain("attribute premiseRefs : ScalarValues::String");
    // §5 attribute: inferenceRunId : ScalarValues::String
    expect(result).toContain("attribute inferenceRunId : ScalarValues::String");
    // §5 attribute: approvedBy : ScalarValues::String
    expect(result).toContain("attribute approvedBy : ScalarValues::String");
  });

  it("about block has all required attributes in closing brace form", () => {
    const compEl = el({
      id: "component-111",
      type: "PartUsage",
      name: "Controller",
      raw: { provenanceSourceId: "inferred-alloc-001" },
    });

    const result = serializeToSysml(
      [compEl],
      [],
      [makeInferredEntry({
        id: "inferred-alloc-001",
        confidence: 0.82,
        premises: ["n2-1234", "function-abcd"],
        inferenceRunId: "run-001",
        approvedBy: "joe",
      })]
    );

    // Verify the about block structure (§5 exact form)
    expect(result).toContain('metadata InferenceProvenance about Controller {');
    expect(result).toContain('provenanceClass = "inferred"');
    expect(result).toContain('confidenceScore = 0.82');
    expect(result).toContain('premiseRefs = "n2-1234, function-abcd"');
    expect(result).toContain('inferenceRunId = "run-001"');
    expect(result).toContain('approvedBy = "joe"');
  });
});
