import { describe, it, expect } from "vitest";
import { findFinding, hasFinding, findingMatches } from "../findings.js";

const FINDINGS = [
  { ruleId: "R4-def-operand", elementId: "SeededDefPart", severity: "error" },
  { ruleId: "GATE03-missing-provenance", elementId: "seeded-uncited-part", severity: "warning" },
  { ruleId: "GATE02-dangling-endpoint", elementId: "ghost", severity: "error" },
];

describe("findings matcher", () => {
  it("hasFinding matches by ruleId alone", () => {
    expect(hasFinding(FINDINGS, { ruleId: "R4-def-operand" })).toBe(true);
    expect(hasFinding(FINDINGS, { ruleId: "NOPE" })).toBe(false);
  });

  it("findFinding matches by ruleId + elementId (both must hold)", () => {
    expect(findFinding(FINDINGS, { ruleId: "R4-def-operand", elementId: "SeededDefPart" })).toBeDefined();
    // right rule, wrong element → no match
    expect(findFinding(FINDINGS, { ruleId: "R4-def-operand", elementId: "OtherPart" })).toBeUndefined();
    // right element, wrong rule → no match
    expect(findFinding(FINDINGS, { ruleId: "GATE02-dangling-endpoint", elementId: "SeededDefPart" })).toBeUndefined();
  });

  it("matches by severity", () => {
    expect(hasFinding(FINDINGS, { severity: "error" })).toBe(true);
    expect(findFinding(FINDINGS, { ruleId: "GATE03-missing-provenance", severity: "error" })).toBeUndefined();
  });

  it("an empty query matches the first finding (no constraints)", () => {
    expect(findingMatches(FINDINGS[0], {})).toBe(true);
  });
});
