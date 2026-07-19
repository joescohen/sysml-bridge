import { describe, it, expect } from "vitest";
import { pairedControl } from "../paired-control.js";

/**
 * A paired control's whole job is to catch a gate that lets a bad input through.
 * These tests include the POSITIVE CONTROLS proving the control itself can fail:
 * if the "bad" input passes the check, `ok` must be false.
 */
describe("pairedControl", () => {
  it("ok when good passes (exit 0) and bad is rejected (exit != 0)", async () => {
    const r = await pairedControl({
      good: "clean",
      bad: "poisoned",
      run: (input) => (input === "clean" ? 0 : 1),
    });
    expect(r.goodPassed).toBe(true);
    expect(r.badPassed).toBe(false);
    expect(r.ok).toBe(true);
  });

  it("POSITIVE CONTROL: a gate that ALSO passes the bad input → ok is false", async () => {
    // The check is broken — it never rejects anything.
    const r = await pairedControl({
      good: "clean",
      bad: "poisoned",
      run: () => 0, // always "passes"
    });
    expect(r.badPassed).toBe(true);
    expect(r.ok).toBe(false);
  });

  it("POSITIVE CONTROL: a gate that rejects the GOOD input → ok is false", async () => {
    const r = await pairedControl({
      good: "clean",
      bad: "poisoned",
      run: () => 1, // always "rejects"
    });
    expect(r.goodPassed).toBe(false);
    expect(r.ok).toBe(false);
  });

  it("supports a custom passes() predicate and async run()", async () => {
    const r = await pairedControl({
      good: { code: "OK" },
      bad: { code: "ERR" },
      run: async (i) => i,
      passes: (res) => res.code === "OK",
    });
    expect(r.ok).toBe(true);
  });

  it("default passes() treats boolean true as a pass", async () => {
    const r = await pairedControl({
      good: true,
      bad: false,
      run: (i) => i,
    });
    expect(r.ok).toBe(true);
  });
});
