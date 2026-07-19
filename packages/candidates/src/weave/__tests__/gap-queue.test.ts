/**
 * gap-queue.test.ts — the finding→query table (spec §8 W3 done-criterion).
 *
 *   - Table covers GATE02-unsatisfied / -orphan / -uncovered-need.
 *   - Unknown finding ids are REPORTED (unmappedFindings), not silently skipped.
 *   - Mapping is pure/deterministic and carries the gap element id + a BM25 query.
 */
import { describe, it, expect } from "vitest";
import {
  GAP_QUERY_TABLE,
  MAPPED_FINDING_RULE_IDS,
  planQueries,
  type WeaveFinding,
  type GapContext,
} from "../gap-queue.js";

function finding(ruleId: string, elementId: string): WeaveFinding {
  return { ruleId, elementId, message: "m", severity: "warning", suggestedFix: "f" };
}

const RESOLVE = (id: string): GapContext => {
  const table: Record<string, GapContext> = {
    "req-1": { name: "Fuel Pump Control", text: "The system shall control the pump." },
    "part-1": { name: "Fuel Pump", text: "" },
    "need-1": { name: "Refuel Safely", text: "Operator need." },
  };
  return table[id] ?? { name: null, text: "" };
};

describe("GAP_QUERY_TABLE covers the three completeness findings", () => {
  it("maps unsatisfied/orphan/uncovered-need to satisfy/allocation/derive", () => {
    expect(GAP_QUERY_TABLE["GATE02-unsatisfied"]?.family).toBe("satisfy");
    expect(GAP_QUERY_TABLE["GATE02-orphan"]?.family).toBe("allocation");
    expect(GAP_QUERY_TABLE["GATE02-uncovered-need"]?.family).toBe("derive");
    expect(new Set(MAPPED_FINDING_RULE_IDS)).toEqual(
      new Set(["GATE02-unsatisfied", "GATE02-orphan", "GATE02-uncovered-need"]),
    );
  });
});

describe("planQueries", () => {
  it("maps each known finding to a query carrying its gap element id + BM25 text", () => {
    const { queries, unmappedFindings } = planQueries(
      [
        finding("GATE02-unsatisfied", "req-1"),
        finding("GATE02-orphan", "part-1"),
        finding("GATE02-uncovered-need", "need-1"),
      ],
      RESOLVE,
    );
    expect(unmappedFindings).toEqual([]);
    expect(queries).toHaveLength(3);

    const unsat = queries.find((q) => q.findingRuleId === "GATE02-unsatisfied")!;
    expect(unsat.gapElementId).toBe("req-1");
    expect(unsat.family).toBe("satisfy");
    expect(unsat.bm25Query).toContain("Fuel Pump Control");
    expect(unsat.bm25Query).toContain("control the pump");
  });

  it("REPORTS unknown finding ids (never silently skipped)", () => {
    const { queries, unmappedFindings } = planQueries(
      [
        finding("GATE02-unsatisfied", "req-1"),
        finding("GATE02-unverified", "req-1"), // known-shape, no query strategy yet
        finding("GATE02-made-up-rule", "x-9"), // wholly unknown
      ],
      RESOLVE,
    );
    expect(queries).toHaveLength(1);
    expect(unmappedFindings).toEqual([
      { ruleId: "GATE02-unverified", elementId: "req-1" },
      { ruleId: "GATE02-made-up-rule", elementId: "x-9" },
    ]);
  });

  it("is deterministic: same input → identical output", () => {
    const input = [finding("GATE02-orphan", "part-1"), finding("GATE02-unsatisfied", "req-1")];
    const a = planQueries(input, RESOLVE);
    const b = planQueries(input, RESOLVE);
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});
