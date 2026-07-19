/**
 * pass-record.test.ts — audit summary, warnings delta, convergence gate, and the
 * pass-record@1 envelope (spec §5, §8 W3).
 *
 *   - All SIX record fields survive a round-trip; a record missing any field or
 *     a wrong schema throws on load.
 *   - Convergence is a HARD gate: errors-increased → fail; ends-with-errors →
 *     fail; zero-and-not-increased → ok.
 *   - Warnings delta is per rule id (SOFT signal).
 */
import { describe, it, expect } from "vitest";
import {
  summarizeAudit,
  computeWarningsDelta,
  evaluateConvergence,
  serializePassRecord,
  parsePassRecord,
  passFileName,
  PASS_RECORD_SCHEMA,
  type PassRecord,
  type WeaveFinding,
} from "../index.js";

function f(ruleId: string, severity: WeaveFinding["severity"]): WeaveFinding {
  return { ruleId, severity, elementId: "e", message: "m", suggestedFix: "s" };
}

describe("summarizeAudit", () => {
  it("counts by severity and per rule id (sorted keys)", () => {
    const s = summarizeAudit([
      f("GATE02-unsatisfied", "warning"),
      f("GATE02-orphan", "warning"),
      f("INFER-unpremised", "error"),
    ]);
    expect(s.errorCount).toBe(1);
    expect(s.warningCount).toBe(2);
    expect(Object.keys(s.byRuleId)).toEqual([
      "GATE02-orphan",
      "GATE02-unsatisfied",
      "INFER-unpremised",
    ]);
    expect(s.byRuleId["INFER-unpremised"]).toEqual({ error: 1, warning: 0, info: 0 });
  });
});

describe("computeWarningsDelta (SOFT signal, per rule id)", () => {
  it("reports before/after/delta per warning rule, sorted", () => {
    const before = summarizeAudit([f("GATE02-orphan", "warning")]);
    const after = summarizeAudit([
      f("GATE02-orphan", "warning"),
      f("GATE02-orphan", "warning"),
      f("GATE02-unsatisfied", "warning"),
    ]);
    expect(computeWarningsDelta(before, after)).toEqual([
      { ruleId: "GATE02-orphan", before: 1, after: 2, delta: 1 },
      { ruleId: "GATE02-unsatisfied", before: 0, after: 1, delta: 1 },
    ]);
  });
});

describe("evaluateConvergence (HARD gate)", () => {
  it("ok when zero errors and none introduced", () => {
    const v = evaluateConvergence(summarizeAudit([]), summarizeAudit([f("GATE02-orphan", "warning")]));
    expect(v.ok).toBe(true);
  });

  it("FAILS when error findings increased", () => {
    const before = summarizeAudit([f("INFER-unpremised", "error")]);
    const after = summarizeAudit([f("INFER-unpremised", "error"), f("ENT-dangling-mention-ref", "error")]);
    const v = evaluateConvergence(before, after);
    expect(v.errorsIncreased).toBe(true);
    expect(v.ok).toBe(false);
  });

  it("FAILS when the pass ends with any error (even if not increased)", () => {
    const before = summarizeAudit([f("INFER-unpremised", "error"), f("INFER-unpremised", "error")]);
    const after = summarizeAudit([f("INFER-unpremised", "error")]);
    const v = evaluateConvergence(before, after);
    expect(v.errorsIncreased).toBe(false);
    expect(v.endsWithErrors).toBe(true);
    expect(v.ok).toBe(false);
  });
});

describe("pass-record@1 envelope", () => {
  const record: PassRecord = {
    auditBefore: summarizeAudit([f("GATE02-orphan", "warning")]),
    queries: [
      {
        findingRuleId: "GATE02-orphan",
        gapElementId: "part-1",
        gapElementName: "Fuel Pump",
        family: "allocation",
        bm25Query: "Fuel Pump",
      },
    ],
    candidatesProposed: [
      {
        id: "cooccur-abc",
        relationFamily: "allocation",
        sourceId: "entity-a",
        targetId: "entity-b",
        targetsGapElementIds: ["part-1"],
      },
    ],
    dispositionsApplied: [],
    auditAfter: summarizeAudit([f("GATE02-orphan", "warning")]),
    warningsDelta: [{ ruleId: "GATE02-orphan", before: 1, after: 1, delta: 0 }],
  };

  it("round-trips with all six fields", () => {
    const json = serializePassRecord(3, record, new Date("2026-07-14T00:00:00Z"));
    const parsed = parsePassRecord(json);
    expect(parsed.schema).toBe(PASS_RECORD_SCHEMA);
    expect(parsed.passNumber).toBe(3);
    for (const key of [
      "auditBefore",
      "queries",
      "candidatesProposed",
      "dispositionsApplied",
      "auditAfter",
      "warningsDelta",
    ] as const) {
      expect(parsed.record[key]).toBeDefined();
    }
    expect(parsed.record.candidatesProposed[0].targetsGapElementIds).toEqual(["part-1"]);
    expect(passFileName(3)).toBe("pass-003.json");
  });

  it("throws on a wrong schema tag", () => {
    const json = serializePassRecord(1, record).replace(PASS_RECORD_SCHEMA, "sysml-foundry/wrong@9");
    expect(() => parsePassRecord(json)).toThrow(/unexpected schema/);
  });

  it("throws when a record field is missing (all six required)", () => {
    const obj = JSON.parse(serializePassRecord(1, record));
    delete obj.record.warningsDelta;
    expect(() => parsePassRecord(JSON.stringify(obj))).toThrow(/warningsDelta is missing/);
  });

  it("throws on malformed JSON", () => {
    expect(() => parsePassRecord("{ not json")).toThrow(/not valid JSON/);
  });
});
