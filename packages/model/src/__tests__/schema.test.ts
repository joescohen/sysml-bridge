import { describe, it, expect } from "vitest";
import { ExtractedSchema, SCHEMA_VERSION } from "../schema.js";

describe("ExtractedSchema", () => {
  it("rejects a fixture with a wrong schema_version (version-aware error)", () => {
    const bad = {
      schema_version: "0.0.0",
      subsystem: "C&C",
      needs: [],
      requirements: [],
      functions: [],
      components: [],
      satisfies: [],
      allocations: [],
    };
    const r = ExtractedSchema.safeParse(bad);
    expect(r.success).toBe(false);
    if (!r.success) {
      const issue = r.error.issues.find((i) => i.path[0] === "schema_version");
      expect(issue).toBeDefined();
    }
  });

  it("rejects a fixture missing required fields", () => {
    const r = ExtractedSchema.safeParse({ schema_version: SCHEMA_VERSION });
    expect(r.success).toBe(false);
  });

  it("round-trips a valid fixture clean (success criterion 3)", () => {
    const good = makeValidFixture();
    const parsed = ExtractedSchema.parse(good);
    const reparsed = ExtractedSchema.parse(JSON.parse(JSON.stringify(parsed)));
    expect(reparsed).toEqual(parsed);
  });

  it("accepts all four Phase-2 seam arrays with provenance (seam acceptance)", () => {
    const provenance = { workbook: "Interface Data N2.xlsx", sheet: "ANGARS SS" };
    const good = {
      ...makeValidFixture(),
      subsystems: [
        {
          id: "subsystem-aaa111",
          kind: "subsystem" as const,
          naturalKey: "C&C",
          name: "Command & Control",
          componentIds: ["component-bbb222"],
          provenance,
        },
      ],
      n2Interfaces: [
        {
          id: "n2-ccc333",
          kind: "n2" as const,
          scope: "subsystem" as const,
          sourceId: "subsystem-aaa111",
          targetId: "subsystem-ddd444",
          sourceLabel: "C&C",
          targetLabel: "Power",
          flow: "Control Signals",
          provenance,
        },
      ],
      kpps: [
        {
          id: "kpp-eee555",
          kind: "kpp" as const,
          naturalKey: "ANGARS-2",
          title: "Reliability",
          provenance,
        },
      ],
      behaviorDecomp: [
        {
          id: "behaviorDecomp-fff666",
          kind: "behaviorDecomp" as const,
          naturalKey: "F1.1",
          level: "L3",
          name: "Receive & Authenticate Request",
          provenance,
        },
      ],
    };
    const r = ExtractedSchema.safeParse(good);
    expect(r.success).toBe(true);
  });

  it("rejects an n2Interfaces entry missing sourceId (malformed seam)", () => {
    const provenance = { workbook: "Interface Data N2.xlsx", sheet: "ANGARS SS" };
    const bad = {
      ...makeValidFixture(),
      n2Interfaces: [
        {
          id: "n2-ccc333",
          kind: "n2" as const,
          scope: "subsystem" as const,
          // sourceId intentionally omitted
          targetId: "subsystem-ddd444",
          sourceLabel: "C&C",
          targetLabel: "Power",
          flow: "Control Signals",
          provenance,
        },
      ],
    };
    const r = ExtractedSchema.safeParse(bad);
    expect(r.success).toBe(false);
    if (!r.success) {
      const issue = r.error.issues.find((i) => i.path[0] === "n2Interfaces");
      expect(issue).toBeDefined();
    }
  });
});

// Fixture helper — inline in test file, not a separate file
function makeValidFixture() {
  return {
    schema_version: SCHEMA_VERSION,
    subsystem: "C&C",
    needs: [{ id: "need-abc123", kind: "need" as const, naturalKey: "N1", name: "Refuel" }],
    requirements: [],
    functions: [],
    components: [],
    satisfies: [],
    allocations: [],
  };
}
