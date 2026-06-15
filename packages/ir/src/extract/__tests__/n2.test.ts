import { describe, it, expect } from "vitest";
import {
  extractN2Triples,
  assertSpotCheck,
  N2_ROW_IS_SOURCE,
} from "../n2.js";

// 3x3 fixture: rows = [header, A, B, C]
// A→B: "x, y" (comma-split → 2 triples)
// B→A: "z"
// B→C: "-" (skip)
// C→A: "NA" (skip)
// C→B: "\r\n" (skip)
// diagonals: "" (skip)
const fixture: unknown[][] = [
  ["hdr", "A", "B", "C"],
  ["A", "", "x, y", ""],
  ["B", "z", "", "-"],
  ["C", "NA", "\r\n", ""],
];

describe("N2_ROW_IS_SOURCE", () => {
  it("is pinned to true", () => {
    expect(N2_ROW_IS_SOURCE).toBe(true);
  });
});

describe("extractN2Triples", () => {
  it("returns exactly 3 triples from the 3x3 fixture", () => {
    const triples = extractN2Triples(fixture);
    expect(triples).toHaveLength(3);
  });

  it("interprets ROW as source and COLUMN as target", () => {
    const triples = extractN2Triples(fixture);
    // A→B "x" and A→B "y" (comma-split)
    const ab = triples.filter(
      (t) => t.sourceLabel === "A" && t.targetLabel === "B"
    );
    expect(ab).toHaveLength(2);
    expect(ab.map((t) => t.flow).sort()).toEqual(["x", "y"]);
    // B→A "z"
    const ba = triples.filter(
      (t) => t.sourceLabel === "B" && t.targetLabel === "A"
    );
    expect(ba).toHaveLength(1);
    expect(ba[0].flow).toBe("z");
  });

  it("skips diagonal cells positionally", () => {
    const triples = extractN2Triples(fixture);
    // No self-flows: (A,A), (B,B), (C,C) should not appear
    const diag = triples.filter((t) => t.sourceLabel === t.targetLabel);
    expect(diag).toHaveLength(0);
  });

  it("skips empty, '-', 'NA', and \\r\\n-only cells", () => {
    const triples = extractN2Triples(fixture);
    // Only valid flows survive
    const flows = triples.map((t) => t.flow);
    expect(flows).not.toContain("-");
    expect(flows).not.toContain("NA");
    expect(flows).not.toContain("");
  });

  it("splits comma-separated cell content into multiple triples", () => {
    const triples = extractN2Triples(fixture);
    const ab = triples.filter(
      (t) => t.sourceLabel === "A" && t.targetLabel === "B"
    );
    // "x, y" splits into two separate triples
    expect(ab).toHaveLength(2);
  });

  it("records correct rowIndex and colIndex for provenance", () => {
    const triples = extractN2Triples(fixture);
    // A→B triple: data row 0 (rows[1]), col 2 (B is header[2])
    const ab = triples.filter(
      (t) => t.sourceLabel === "A" && t.targetLabel === "B"
    );
    expect(ab[0].rowIndex).toBe(0);
    expect(ab[0].colIndex).toBe(2);
    // B→A triple: data row 1 (rows[2]), col 1 (A is header[1])
    const ba = triples.filter(
      (t) => t.sourceLabel === "B" && t.targetLabel === "A"
    );
    expect(ba[0].rowIndex).toBe(1);
    expect(ba[0].colIndex).toBe(1);
  });

  it("emits triples in row-major order", () => {
    const triples = extractN2Triples(fixture);
    // Expected order: (A,B,"x"), (A,B,"y"), (B,A,"z")
    expect(triples[0]).toMatchObject({
      sourceLabel: "A",
      targetLabel: "B",
      flow: "x",
    });
    expect(triples[1]).toMatchObject({
      sourceLabel: "A",
      targetLabel: "B",
      flow: "y",
    });
    expect(triples[2]).toMatchObject({
      sourceLabel: "B",
      targetLabel: "A",
      flow: "z",
    });
  });
});

describe("assertSpotCheck", () => {
  it("returns void when a matching triple exists", () => {
    const triples = extractN2Triples(fixture);
    expect(() => assertSpotCheck(triples, "A", "B", "x")).not.toThrow();
  });

  it("throws [ETL-02] when no matching triple exists", () => {
    const triples = extractN2Triples(fixture);
    expect(() => assertSpotCheck(triples, "A", "B", "missing")).toThrow(
      /\[ETL-02\]/
    );
  });

  it("throws [ETL-02] on the transposed fixture (direction inversion)", () => {
    // Transposed: put "x" at (B,A) instead of (A,B)
    const transposed: unknown[][] = [
      ["hdr", "A", "B", "C"],
      ["A", "", "z", "NA"],     // was B row
      ["B", "x, y", "", "-"],  // was A row
      ["C", "\r\n", "", ""],
    ];
    const triples = extractN2Triples(transposed);
    // spot-check: expect (A,B,"x") but in the transposed fixture it's (B,A,"x")
    expect(() => assertSpotCheck(triples, "A", "B", "x")).toThrow(/\[ETL-02\]/);
  });
});
