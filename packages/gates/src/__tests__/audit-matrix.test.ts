/**
 * audit-matrix.test.ts
 *
 * TDD tests for GATE-06 coverage matrix — one row per system requirement with
 * satisfied/verified/derived booleans. Uses hand-built element/relationship
 * literals (faster, no temp dir).
 *
 * Fixture:
 *   - R1 (RequirementDefinition, systemReq) — fully traced (satisfy + verify + derive)
 *   - R2 (RequirementDefinition, systemReq) — allocation-only (AllocationUsage = forward, no verify/derive)
 *   - N1 (RequirementDefinition, stakeholderNeed) — EXEMPT from matrix
 *   - Rels: SatisfyReq(→R1), VerifyReq(→R1), DeriveReq(→R1), AllocationUsage(→R2)
 *
 * Acceptance criteria:
 *   - Matrix has exactly 2 rows (R1 and R2) — N1 is excluded
 *   - R1 row: {satisfied:true, verified:true, derived:true}
 *   - R2 row: {satisfied:true, verified:false, derived:false}
 *   - N1 id appears in NO row
 *   - Empty model → empty matrix (no throw)
 */

import { describe, it, expect } from "vitest";
import type { SysmlElement, SysmlRelationship } from "@sysml-bridge/model";
import { coverageMatrix } from "../matrix.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeReq(
  id: string,
  name: string,
  stakeholderNeed?: boolean
): SysmlElement {
  return {
    id,
    elementId: id,
    type: "RequirementDefinition",
    name,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: stakeholderNeed === true ? { stakeholderNeed: true } : {},
  };
}

function makeRel(
  id: string,
  type: string,
  sourceIds: string[],
  targetIds: string[]
): SysmlRelationship {
  return { id, type, sourceIds, targetIds, raw: {} };
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

const R1 = makeReq("req-r1", "Aircraft ID Verification");
const R2 = makeReq("req-r2", "Generate Schedule");
const N1 = makeReq("req-n1", "Stakeholder Need Alpha", true);

const relSatisfyR1 = makeRel("rel-sat-r1", "SatisfyRequirementUsage", ["elem-part"], ["req-r1"]);
const relVerifyR1 = makeRel("rel-ver-r1", "VerifyRequirementUsage", ["elem-test"], ["req-r1"]);
const relDeriveR1 = makeRel("rel-der-r1", "DeriveRequirementUsage", ["req-r1"], ["req-n1"]);
const relAllocR2 = makeRel("rel-alloc-r2", "AllocationUsage", ["elem-func"], ["req-r2"]);

const FULL_ELEMENTS: SysmlElement[] = [R1, R2, N1];
const FULL_RELS: SysmlRelationship[] = [relSatisfyR1, relVerifyR1, relDeriveR1, relAllocR2];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("coverageMatrix — GATE-06", () => {
  it("returns one row per system requirement (Needs exempt)", () => {
    const rows = coverageMatrix(FULL_ELEMENTS, FULL_RELS);
    expect(rows).toHaveLength(2);
    const ids = rows.map((r) => r.reqId);
    expect(ids).toContain("req-r1");
    expect(ids).toContain("req-r2");
    expect(ids).not.toContain("req-n1");
  });

  it("fully-traced req row is all-true", () => {
    const rows = coverageMatrix(FULL_ELEMENTS, FULL_RELS);
    const r1Row = rows.find((r) => r.reqId === "req-r1");
    expect(r1Row).toBeDefined();
    expect(r1Row!.satisfied).toBe(true);
    expect(r1Row!.verified).toBe(true);
    expect(r1Row!.derived).toBe(true);
  });

  it("allocation-only req row: satisfied=true, verified=false, derived=false", () => {
    const rows = coverageMatrix(FULL_ELEMENTS, FULL_RELS);
    const r2Row = rows.find((r) => r.reqId === "req-r2");
    expect(r2Row).toBeDefined();
    expect(r2Row!.satisfied).toBe(true);
    expect(r2Row!.verified).toBe(false);
    expect(r2Row!.derived).toBe(false);
  });

  it("stakeholder Need id appears in NO row", () => {
    const rows = coverageMatrix(FULL_ELEMENTS, FULL_RELS);
    const needRow = rows.find((r) => r.reqId === "req-n1");
    expect(needRow).toBeUndefined();
  });

  it("row carries reqName from element name", () => {
    const rows = coverageMatrix(FULL_ELEMENTS, FULL_RELS);
    const r1Row = rows.find((r) => r.reqId === "req-r1");
    expect(r1Row!.reqName).toBe("Aircraft ID Verification");
  });

  it("empty model returns empty matrix without throwing", () => {
    const rows = coverageMatrix([], []);
    expect(rows).toEqual([]);
  });

  it("model with only Needs and no systemReqs returns empty matrix", () => {
    const rows = coverageMatrix([N1], FULL_RELS);
    expect(rows).toEqual([]);
  });

  it("edge touching req.id in sourceIds (DeriveRequirementUsage from req) sets derived=true", () => {
    // DeriveRequirementUsage has req as SOURCE (req derives from need)
    const rows = coverageMatrix(FULL_ELEMENTS, FULL_RELS);
    const r1Row = rows.find((r) => r.reqId === "req-r1");
    expect(r1Row!.derived).toBe(true);
  });

  it("CR-02: req that is only the TARGET of a DeriveRequirementUsage has derived=false (chained derivation)", () => {
    // rTarget is the TARGET of a DeriveRequirementUsage from rSource.
    // With the CR-02 fix, being a derive target does NOT count as backtraced.
    const rTarget = makeReq("req-target", "R-Target");
    const rSource = makeReq("req-source", "R-Source");
    // DeriveRequirementUsage: rSource → rTarget (rTarget is the TARGET)
    const relChained = makeRel(
      "rel-chained",
      "DeriveRequirementUsage",
      ["req-source"],
      ["req-target"]
    );
    const rows = coverageMatrix([rTarget, rSource], [relChained]);
    const targetRow = rows.find((r) => r.reqId === "req-target");
    expect(targetRow).toBeDefined();
    // rTarget is the DERIVE TARGET — must NOT be marked as derived (no outgoing derive)
    expect(targetRow!.derived).toBe(false);
    // rSource is the DERIVE SOURCE — it IS marked as derived (it has an outgoing derive)
    const sourceRow = rows.find((r) => r.reqId === "req-source");
    expect(sourceRow).toBeDefined();
    expect(sourceRow!.derived).toBe(true);
  });
});
