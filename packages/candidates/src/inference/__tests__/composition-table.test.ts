/**
 * composition-table.test.ts — the explicit composition table + its §9 growth
 * policy (registry-completeness check).
 *
 * §9 (spec): a table entry without a paired type-gate test MUST fail a
 * registry-completeness check (mirror the query-keys pattern). Here the "registry"
 * is `COMPOSITION_KEYS` (derived from the LIVE table); `TESTED_COMPOSITIONS` is
 * derived from the paired type-gate tests declared below. The completeness test
 * asserts the two sets are EQUAL — so:
 *   - adding a row to COMPOSITION_TABLE without a paired test → key missing from
 *     TESTED_COMPOSITIONS → completeness FAILS;
 *   - removing a row while leaving a stale test → extra key in TESTED → FAILS.
 */

import { describe, it, expect } from "vitest";
import {
  COMPOSITION_TABLE,
  COMPOSITION_KEYS,
  compositionKey,
  composeChain,
  isLegalComposition,
  applyChainTypeGate,
  chainStableId,
  type RawChainCandidate,
} from "../composition-table.js";
import type { RelationFamily } from "../types.js";

// ── Paired type-gate tests, one per table entry ───────────────────────────────
//
// Each object is BOTH a runnable paired test AND the source of a
// TESTED_COMPOSITIONS key. To add a new composition you MUST add its row here
// (and to COMPOSITION_TABLE) — otherwise the completeness check below fails.
const PAIRED_TYPE_GATE_TESTS: ReadonlyArray<{
  left: string;
  right: string;
  result: RelationFamily;
}> = [
  { left: "allocation", right: "containment", result: "allocation" },
  { left: "flowTyping", right: "interfaceAggregation", result: "flowTyping" },
];

const TESTED_COMPOSITIONS: ReadonlySet<string> = new Set(
  PAIRED_TYPE_GATE_TESTS.map((t) => compositionKey(t.left, t.right)),
);

function rawChain(left: string, right: string): RawChainCandidate {
  return {
    stableId: chainStableId(left, right, "A", "B", "C"),
    leftFamily: left,
    rightFamily: right,
    sourceId: "A",
    middleId: "B",
    targetId: "C",
    premiseIds: ["rel-1", "rel-2"],
  };
}

describe("composition-table — paired type-gate test per entry", () => {
  for (const t of PAIRED_TYPE_GATE_TESTS) {
    it(`${compositionKey(t.left, t.right)} → ${t.result} passes the chain type gate`, () => {
      expect(composeChain(t.left, t.right)).toBe(t.result);
      expect(isLegalComposition(t.left, t.right)).toBe(true);
      const { accepted, rejected } = applyChainTypeGate([rawChain(t.left, t.right)]);
      expect(rejected).toHaveLength(0);
      expect(accepted).toHaveLength(1);
      expect(accepted[0]!.relationFamily).toBe(t.result);
    });
  }

  it("FAIL-ABLE control: an unlisted composition is illegal and rejected", () => {
    expect(composeChain("allocation", "flowTyping")).toBeNull();
    expect(isLegalComposition("allocation", "flowTyping")).toBe(false);
    const { accepted, rejected } = applyChainTypeGate([rawChain("allocation", "flowTyping")]);
    expect(accepted).toHaveLength(0);
    expect(rejected).toHaveLength(1);
  });
});

// ── §9 registry-completeness check ────────────────────────────────────────────

describe("composition-table — §9 registry-completeness (table ↔ tests)", () => {
  it("every COMPOSITION_TABLE entry has a paired type-gate test (and vice versa)", () => {
    // Registry (live table) key set.
    const tableKeys = [...COMPOSITION_KEYS].sort();
    // Tested key set.
    const testedKeys = [...TESTED_COMPOSITIONS].sort();
    expect(testedKeys).toEqual(tableKeys);
  });

  it("COMPOSITION_KEYS is derived from the live table (no drift)", () => {
    const derived = new Set(COMPOSITION_TABLE.map((e) => compositionKey(e.left, e.right)));
    expect([...COMPOSITION_KEYS].sort()).toEqual([...derived].sort());
  });

  it("results are always pipeline families (routable downstream)", () => {
    const pipeline: ReadonlySet<string> = new Set([
      "allocation",
      "modeMembership",
      "flowTyping",
      "controlJoin",
    ]);
    for (const e of COMPOSITION_TABLE) {
      expect(pipeline.has(e.result)).toBe(true);
    }
  });
});
