/**
 * traceability-compare.test.ts
 *
 * Unit tests for the pure compareTrace() diff function and normalizeFunctionId()
 * helper — the testable core of the IEEE 15288 §6.3.3 traceability fidelity
 * comparator.
 *
 * The pure logic lives in src/utils/trace-compare.ts.  The CLI script
 * (scripts/traceability-compare.ts) imports from there and handles file I/O.
 * The generated model does not exist yet (Task 11 is pending), so we test only
 * the pure function with fixtures — no file I/O, no xlsx reads.
 */

import { describe, it, expect } from "vitest";
import {
  compareTrace,
  normalizeFunctionId,
  type TracePair,
} from "../utils/trace-compare.js";

// ---------------------------------------------------------------------------
// normalizeFunctionId
// ---------------------------------------------------------------------------

describe("normalizeFunctionId", () => {
  it("returns id unchanged when there is no colon", () => {
    expect(normalizeFunctionId("F1.1")).toBe("F1.1");
  });

  it("strips the suffix after the first colon", () => {
    expect(normalizeFunctionId("F1.1: Receive & Authenticate Request")).toBe("F1.1");
  });

  it("trims surrounding whitespace", () => {
    expect(normalizeFunctionId("  F8.3  ")).toBe("F8.3");
  });

  it("trims whitespace around the id before the colon", () => {
    expect(normalizeFunctionId("  F1.2 : Validate Fuel Capacity")).toBe("F1.2");
  });
});

// ---------------------------------------------------------------------------
// compareTrace — all-present (perfect fidelity)
// ---------------------------------------------------------------------------

describe("compareTrace — all present", () => {
  const auth: TracePair[] = [
    { reqId: "ANGARS-4", functionId: "F1.1" },
    { reqId: "ANGARS-14", functionId: "F1.2" },
    { reqId: "ANGARS-103", functionId: "F8.1" },
  ];

  const gen: TracePair[] = [
    { reqId: "ANGARS-4", functionId: "F1.1" },
    { reqId: "ANGARS-14", functionId: "F1.2" },
    { reqId: "ANGARS-103", functionId: "F8.1" },
  ];

  it("reports fidelityPct 100", () => {
    const result = compareTrace(auth, gen);
    expect(result.fidelityPct).toBe(100);
  });

  it("reports all pairs as present", () => {
    const result = compareTrace(auth, gen);
    expect(result.present).toHaveLength(3);
  });

  it("reports no missing pairs", () => {
    const result = compareTrace(auth, gen);
    expect(result.missing).toHaveLength(0);
  });

  it("reports no unsupported pairs", () => {
    const result = compareTrace(auth, gen);
    expect(result.unsupported).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// compareTrace — one authoritative pair absent from generated (DROPPED)
// ---------------------------------------------------------------------------

describe("compareTrace — one authoritative pair dropped", () => {
  const auth: TracePair[] = [
    { reqId: "ANGARS-4", functionId: "F1.1" },
    { reqId: "ANGARS-14", functionId: "F1.2" },
  ];

  // Generated is missing ANGARS-14/F1.2
  const gen: TracePair[] = [
    { reqId: "ANGARS-4", functionId: "F1.1" },
  ];

  it("reports fidelityPct less than 100", () => {
    const result = compareTrace(auth, gen);
    expect(result.fidelityPct).toBeLessThan(100);
  });

  it("puts the dropped pair in missing", () => {
    const result = compareTrace(auth, gen);
    expect(result.missing).toHaveLength(1);
    expect(result.missing[0]).toMatchObject({ reqId: "ANGARS-14", functionId: "F1.2" });
  });

  it("reports no unsupported", () => {
    const result = compareTrace(auth, gen);
    expect(result.unsupported).toHaveLength(0);
  });

  it("computes fidelityPct as 50 for 1-of-2 present", () => {
    const result = compareTrace(auth, gen);
    expect(result.fidelityPct).toBe(50);
  });
});

// ---------------------------------------------------------------------------
// compareTrace — one extra generated pair (FABRICATED)
// ---------------------------------------------------------------------------

describe("compareTrace — one fabricated pair in generated", () => {
  const auth: TracePair[] = [
    { reqId: "ANGARS-4", functionId: "F1.1" },
  ];

  // Generated has an extra pair not in authoritative
  const gen: TracePair[] = [
    { reqId: "ANGARS-4", functionId: "F1.1" },
    { reqId: "ANGARS-99", functionId: "F1.9" },
  ];

  it("puts the extra pair in unsupported", () => {
    const result = compareTrace(auth, gen);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]).toMatchObject({ reqId: "ANGARS-99", functionId: "F1.9" });
  });

  it("reports no missing", () => {
    const result = compareTrace(auth, gen);
    expect(result.missing).toHaveLength(0);
  });

  it("still reports fidelityPct 100 (fabricated pairs do not reduce authoritative fidelity)", () => {
    const result = compareTrace(auth, gen);
    expect(result.fidelityPct).toBe(100);
  });
});

// ---------------------------------------------------------------------------
// compareTrace — function-id normalization matching
// ---------------------------------------------------------------------------

describe("compareTrace — function-id normalization", () => {
  it('matches "F1.1: foo" in generated against "F1.1" in authoritative', () => {
    const auth: TracePair[] = [{ reqId: "ANGARS-4", functionId: "F1.1" }];
    const gen: TracePair[] = [
      { reqId: "ANGARS-4", functionId: "F1.1: Receive & Authenticate Request" },
    ];

    const result = compareTrace(auth, gen);
    expect(result.present).toHaveLength(1);
    expect(result.missing).toHaveLength(0);
    expect(result.unsupported).toHaveLength(0);
    expect(result.fidelityPct).toBe(100);
  });

  it("matches authoritative with colon suffix against normalized generated", () => {
    const auth: TracePair[] = [
      { reqId: "ANGARS-14", functionId: "F1.2: Validate Fuel Capacity" },
    ];
    const gen: TracePair[] = [{ reqId: "ANGARS-14", functionId: "F1.2" }];

    const result = compareTrace(auth, gen);
    expect(result.present).toHaveLength(1);
    expect(result.fidelityPct).toBe(100);
  });
});

// ---------------------------------------------------------------------------
// compareTrace — edge cases
// ---------------------------------------------------------------------------

describe("compareTrace — edge cases", () => {
  it("returns 100% fidelity for two empty inputs", () => {
    const result = compareTrace([], []);
    expect(result.fidelityPct).toBe(100);
    expect(result.present).toHaveLength(0);
    expect(result.missing).toHaveLength(0);
    expect(result.unsupported).toHaveLength(0);
  });

  it("puts all generated pairs in unsupported when authoritative is empty", () => {
    const gen: TracePair[] = [{ reqId: "ANGARS-1", functionId: "F1.1" }];
    const result = compareTrace([], gen);
    expect(result.unsupported).toHaveLength(1);
    expect(result.fidelityPct).toBe(100); // nothing authoritative → 0/0 = 100%
  });

  it("puts all authoritative pairs in missing when generated is empty", () => {
    const auth: TracePair[] = [
      { reqId: "ANGARS-4", functionId: "F1.1" },
      { reqId: "ANGARS-14", functionId: "F1.2" },
    ];
    const result = compareTrace(auth, []);
    expect(result.missing).toHaveLength(2);
    expect(result.present).toHaveLength(0);
    expect(result.fidelityPct).toBe(0);
  });
});
