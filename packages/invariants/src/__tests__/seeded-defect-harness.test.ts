import { describe, it, expect } from "vitest";
import { seededDefectHarness } from "../seeded-defect-harness.js";

// A toy model + auditor: the "audit" flags any element carrying a `bad` marker.
interface Elem {
  id: string;
  bad?: string; // ruleId this element should trip, when planted
}
interface Model {
  elements: Elem[];
}

function audit(model: Model): Array<{ ruleId: string; elementId: string; severity: string }> {
  return model.elements
    .filter((e) => e.bad)
    .map((e) => ({ ruleId: e.bad!, elementId: e.id, severity: "error" }));
}

const isError = (f: { severity: string }) => f.severity === "error";
const CLEAN: Model = { elements: [{ id: "clean-1" }, { id: "clean-2" }] };

const silent = () => {};

describe("seededDefectHarness", () => {
  it("ok when clean control is clean and every defect is caught by rule + element", async () => {
    const result = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "plant R4",
          plant: (b) => ({ elements: [...b.elements, { id: "seeded-r4", bad: "R4-def-operand" }] }),
          expectRule: "R4-def-operand",
          expectElementId: "seeded-r4",
        },
        {
          defect: "custom grammar control",
          check: () => true,
        },
      ],
    });
    expect(result.cleanControlOk).toBe(true);
    expect(result.caughtCount).toBe(2);
    expect(result.defectCount).toBe(2);
    expect(result.ok).toBe(true);
  });

  it("POSITIVE CONTROL: a defect NO gate catches → ok is false", async () => {
    const result = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          // The plant adds an element that trips a DIFFERENT rule than expected,
          // so the audit never produces the expected finding → not caught.
          defect: "expected rule never fires",
          plant: (b) => ({ elements: [...b.elements, { id: "seeded", bad: "SOME-OTHER-RULE" }] }),
          expectRule: "R4-def-operand",
          expectElementId: "seeded",
        },
      ],
    });
    expect(result.caughtCount).toBe(0);
    expect(result.ok).toBe(false);
  });

  it("POSITIVE CONTROL: right rule but WRONG element id → not caught", async () => {
    const result = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "rule fires on a different element",
          plant: (b) => ({ elements: [...b.elements, { id: "actual", bad: "R4-def-operand" }] }),
          expectRule: "R4-def-operand",
          expectElementId: "expected-different-id",
        },
      ],
    });
    expect(result.ok).toBe(false);
  });

  it("POSITIVE CONTROL: a non-vacuous clean control fails when the base already has an error", async () => {
    const dirtyBase: Model = { elements: [{ id: "already-bad", bad: "R4-def-operand" }] };
    const result = await seededDefectHarness({
      base: dirtyBase,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "caught fine",
          plant: (b) => ({ elements: [...b.elements, { id: "seeded", bad: "GATE-X" }] }),
          expectRule: "GATE-X",
          expectElementId: "seeded",
        },
      ],
    });
    // The defect itself is caught, but the clean control is DIRTY → overall not ok.
    expect(result.cleanControlOk).toBe(false);
    expect(result.caughtCount).toBe(1);
    expect(result.ok).toBe(false);
  });

  it("POSITIVE CONTROL: a custom-check defect that returns false → ok is false", async () => {
    const result = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [{ defect: "grammar control missed", check: () => false }],
    });
    expect(result.ok).toBe(false);
  });

  it("soleError: passes when the expected finding is the ONLY error", async () => {
    const result = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "isolated error seed",
          plant: (b) => ({ elements: [...b.elements, { id: "seeded-sole", bad: "R4-def-operand" }] }),
          expectRule: "R4-def-operand",
          expectElementId: "seeded-sole",
          soleError: true,
        },
      ],
    });
    expect(result.caughtCount).toBe(1);
    expect(result.ok).toBe(true);
  });

  it("POSITIVE CONTROL: soleError fails when the seed cross-triggers a SECOND error", async () => {
    // The plant adds the expected R4 element AND a second element that trips a
    // different error rule. Without soleError this would pass (the expected
    // finding is present); with soleError the extra error makes it NOT caught.
    const twoErrorPlant = (b: Model): Model => ({
      elements: [
        ...b.elements,
        { id: "seeded-sole", bad: "R4-def-operand" },
        { id: "seeded-extra", bad: "GATE02-id-duplicate" },
      ],
    });

    const withoutSole = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "expected present but a second error is also raised",
          plant: twoErrorPlant,
          expectRule: "R4-def-operand",
          expectElementId: "seeded-sole",
        },
      ],
    });
    // Existence-only: the expected finding IS present, so it passes vacuously.
    expect(withoutSole.ok).toBe(true);

    const withSole = await seededDefectHarness({
      base: CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "expected present but a second error is also raised",
          plant: twoErrorPlant,
          expectRule: "R4-def-operand",
          expectElementId: "seeded-sole",
          soleError: true,
        },
      ],
    });
    // soleError catches the cross-trigger the existence check missed.
    expect(withSole.caughtCount).toBe(0);
    expect(withSole.ok).toBe(false);
    expect(withSole.rows[1].detail).toContain("CROSS-TRIGGERED");
  });

  it("supports an async base factory and async plant", async () => {
    const result = await seededDefectHarness({
      base: async () => CLEAN,
      audit,
      isError,
      logger: silent,
      defects: [
        {
          defect: "async plant",
          plant: async (b) => ({ elements: [...b.elements, { id: "s", bad: "GATE-Y" }] }),
          expectRule: "GATE-Y",
        },
      ],
    });
    expect(result.ok).toBe(true);
  });
});
