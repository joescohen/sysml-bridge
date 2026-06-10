/**
 * audit-provenance.test.ts
 *
 * GATE-03 provenance-existence rule pack tests.
 *
 * Tests pure provenanceFindings(elements, resolutionSet) using:
 *   - Inline minimal Extracted fixture → buildResolutionSet (from corpus.ts)
 *   - Hand-built SysmlElement literals (no store/server needed)
 *
 * Behavior under test:
 *   - Laundered fake id "requirement-deadbeef" → GATE03-unresolvable-provenance, error
 *   - Known corpus id (in fixture) → zero findings
 *   - naturalKey "CC-1" in resolution set → zero findings
 *   - component name "Autopilot" in resolution set → zero findings
 *   - "model-asserted" → GATE03-model-asserted, severity info (never error)
 *   - Allowlist values (C&C, Demonstration, Test, Analysis, Inspection) → zero error findings
 *   - RequirementDefinition with missing provenanceSourceId → GATE03-missing-provenance, warning
 *   - RequirementDefinition with empty provenanceSourceId → GATE03-missing-provenance, warning
 */

import { describe, it, expect } from "vitest";
import type { SysmlElement } from "../types/sysml-elements.js";
import { provenanceFindings } from "../audit/provenance.js";
import { buildResolutionSet } from "../audit/corpus.js";
import type { Extracted } from "@sysml-bridge/ir";
import type { Finding } from "../audit/findings.js";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/** Minimal Extracted fixture with one entity per supported kind. */
const FIXTURE_CORPUS: Extracted = {
  schema_version: "1.0.0",
  subsystem: "test",
  needs: [{ id: "need-001", kind: "need", naturalKey: "N-1", name: "Be Safe" }],
  requirements: [
    {
      id: "requirement-abc",
      kind: "requirement",
      naturalKey: "CC-1",
      name: "Do Thing",
      statement: "The system shall do the thing.",
      needIds: ["need-001"],
    },
  ],
  functions: [
    { id: "function-001", kind: "function", naturalKey: "F-1", name: "Process Data", level: "1", owner: "Autopilot" },
  ],
  components: [{ id: "component-001", kind: "component", naturalKey: "COMP-1", name: "Autopilot" }],
  satisfies: [],
  allocations: [],
};

const RESOLUTION_SET = buildResolutionSet(FIXTURE_CORPUS);

/** Build a minimal SysmlElement with the given id/type/provenanceSourceId. */
function makeEl(
  id: string,
  type: string,
  provenanceSourceId?: string | null
): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name: `El-${id}`,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: provenanceSourceId !== undefined && provenanceSourceId !== null
      ? { provenanceSourceId }
      : {},
  };
}

function byRule(findings: Finding[], ruleId: string): Finding[] {
  return findings.filter((f) => f.ruleId === ruleId);
}

// ---------------------------------------------------------------------------
// Existence check (GATE03-unresolvable-provenance)
// ---------------------------------------------------------------------------

describe("provenanceFindings — GATE03-unresolvable-provenance (existence check)", () => {
  it("ROADMAP criterion 3: laundered fake id 'requirement-deadbeef' yields GATE03-unresolvable-provenance error", () => {
    const el = makeEl("el-1", "RequirementDefinition", "requirement-deadbeef");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const unresolvable = byRule(findings, "GATE03-unresolvable-provenance");
    expect(unresolvable).toHaveLength(1);
    expect(unresolvable[0].severity).toBe("error");
    expect(unresolvable[0].elementId).toBe("el-1");
    expect(unresolvable[0].message).toContain("requirement-deadbeef");
    expect(unresolvable[0].suggestedFix).toBeTruthy();
  });

  it("Error: offending value is included in the message", () => {
    const el = makeEl("el-x", "PartDefinition", "fake-corpus-id-xyz");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const f = byRule(findings, "GATE03-unresolvable-provenance");
    expect(f.length).toBeGreaterThan(0);
    expect(f[0].message).toContain("fake-corpus-id-xyz");
  });

  it("applies to ANY element type with an unresolvable provenanceSourceId", () => {
    // Not just RequirementDefinition — existence check is universal
    const el = makeEl("el-2", "ActionDefinition", "totally-fake-id-999");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const f = byRule(findings, "GATE03-unresolvable-provenance");
    expect(f).toHaveLength(1);
    expect(f[0].severity).toBe("error");
  });
});

// ---------------------------------------------------------------------------
// Known-good values → zero findings
// ---------------------------------------------------------------------------

describe("provenanceFindings — known-good corpus values produce zero error findings", () => {
  it("element with corpus id 'requirement-abc' → zero findings", () => {
    const el = makeEl("el-3", "RequirementDefinition", "requirement-abc");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(0);
  });

  it("element with naturalKey 'CC-1' → zero findings (defense-in-depth key)", () => {
    const el = makeEl("el-4", "RequirementDefinition", "CC-1");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(0);
  });

  it("element with component name 'Autopilot' → zero findings (defense-in-depth key)", () => {
    const el = makeEl("el-5", "PartDefinition", "Autopilot");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// model-asserted
// ---------------------------------------------------------------------------

describe("provenanceFindings — model-asserted", () => {
  it("'model-asserted' yields GATE03-model-asserted with severity 'info'", () => {
    const el = makeEl("el-6", "PartDefinition", "model-asserted");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const info = byRule(findings, "GATE03-model-asserted");
    expect(info).toHaveLength(1);
    expect(info[0].severity).toBe("info");
    expect(info[0].elementId).toBe("el-6");
  });

  it("'model-asserted' yields ZERO error findings (never error — locked)", () => {
    const el = makeEl("el-7", "PartDefinition", "model-asserted");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// Allowlist values (decision A4)
// ---------------------------------------------------------------------------

describe("provenanceFindings — allowlist values (decision A4)", () => {
  const ALLOWLIST_VALUES = ["C&C", "Demonstration", "Test", "Analysis", "Inspection"];

  for (const val of ALLOWLIST_VALUES) {
    it(`'${val}' → zero error-severity findings`, () => {
      const el = makeEl(`el-al-${val}`, "RequirementDefinition", val);
      const findings = provenanceFindings([el], RESOLUTION_SET);
      const errors = findings.filter((f) => f.severity === "error");
      expect(errors).toHaveLength(0);
    });
  }
});

// ---------------------------------------------------------------------------
// Presence check (GATE03-missing-provenance)
// ---------------------------------------------------------------------------

describe("provenanceFindings — GATE03-missing-provenance (presence check)", () => {
  it("RequirementDefinition with missing provenanceSourceId → GATE03-missing-provenance warning", () => {
    const el = makeEl("el-8", "RequirementDefinition"); // no provenanceSourceId key
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const missing = byRule(findings, "GATE03-missing-provenance");
    expect(missing).toHaveLength(1);
    expect(missing[0].severity).toBe("warning");
    expect(missing[0].elementId).toBe("el-8");
  });

  it("RequirementDefinition with empty string provenanceSourceId → GATE03-missing-provenance warning", () => {
    const el = makeEl("el-9", "RequirementDefinition", "");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const missing = byRule(findings, "GATE03-missing-provenance");
    expect(missing).toHaveLength(1);
    expect(missing[0].severity).toBe("warning");
  });

  it("PartDefinition with missing provenanceSourceId → GATE03-missing-provenance warning", () => {
    const el = makeEl("el-10", "PartDefinition"); // no provenanceSourceId key
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const missing = byRule(findings, "GATE03-missing-provenance");
    expect(missing).toHaveLength(1);
    expect(missing[0].severity).toBe("warning");
  });

  it("ActionDefinition with missing provenanceSourceId → GATE03-missing-provenance warning", () => {
    const el = makeEl("el-11", "ActionDefinition"); // no provenanceSourceId key
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const missing = byRule(findings, "GATE03-missing-provenance");
    expect(missing).toHaveLength(1);
    expect(missing[0].severity).toBe("warning");
  });

  it("PartUsage (non-legacy type) with missing provenanceSourceId → zero GATE03-missing-provenance findings (not widened)", () => {
    // Presence check is scoped to legacy types only: RequirementDefinition, PartDefinition, ActionDefinition
    const el = makeEl("el-12", "PartUsage"); // no provenanceSourceId
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const missing = byRule(findings, "GATE03-missing-provenance");
    expect(missing).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// Combined: GATE03-missing-provenance is warning-only, no error for missing
// ---------------------------------------------------------------------------

describe("provenanceFindings — missing provenance is warning, not error", () => {
  it("RequirementDefinition with no provenanceSourceId → warning but no error", () => {
    const el = makeEl("el-13", "RequirementDefinition");
    const findings = provenanceFindings([el], RESOLUTION_SET);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(0);
    const warnings = findings.filter((f) => f.severity === "warning");
    expect(warnings.length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// Every finding has non-empty required fields
// ---------------------------------------------------------------------------

describe("provenanceFindings — finding shape", () => {
  it("all findings have non-empty elementId, message, and suggestedFix", () => {
    const elements: SysmlElement[] = [
      makeEl("el-14", "RequirementDefinition", "requirement-deadbeef"), // error
      makeEl("el-15", "RequirementDefinition"), // warning (missing)
      makeEl("el-16", "PartDefinition", "model-asserted"), // info
    ];
    const findings = provenanceFindings(elements, RESOLUTION_SET);
    for (const f of findings) {
      expect(f.elementId).toBeTruthy();
      expect(f.message).toBeTruthy();
      expect(f.suggestedFix).toBeTruthy();
    }
  });
});
