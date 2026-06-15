import { describe, it, expect } from "vitest";
import { stableId } from "../stable-id.js";

describe("stableId", () => {
  it("is identical regardless of input row order (shuffle test / IR-02)", () => {
    const keys = ["ANGARS-10", "ANGARS-4", "ANGARS-14", "ANGARS-62"];
    const ordered = keys.map((k) => stableId("requirement", k));
    const shuffled = [...keys].reverse().map((k) => stableId("requirement", k));
    keys.forEach((_k, i) => {
      const j = keys.length - 1 - i;
      expect(shuffled[j]).toBe(ordered[i]);
    });
  });

  it("is deterministic across calls", () => {
    expect(stableId("requirement", "ANGARS-10")).toBe(
      stableId("requirement", "ANGARS-10")
    );
  });

  it("differs across namespaces for the same natural key", () => {
    expect(stableId("requirement", "X")).not.toBe(stableId("need", "X"));
  });

  it("includes the namespace prefix in the returned ID", () => {
    expect(stableId("requirement", "ANGARS-10")).toMatch(
      /^requirement-[0-9a-f]{16}$/
    );
  });
});
