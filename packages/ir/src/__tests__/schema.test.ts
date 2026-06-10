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
