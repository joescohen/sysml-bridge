/**
 * audit-relational.test.ts
 *
 * Seeded-defect fixture proving GATE-02 relational rule pack fires distinctly
 * for each rule class (ROADMAP criteria 1-2 evidence).
 *
 * Fixture defects seeded:
 *   Defect 1 (R4): SatisfyRequirementUsage whose operands are Definitions
 *   Defect 2 (dangling): relationship with target id "ghost-id" not in model
 *   Defect 3 (orphan): leaf PartDefinition with no trace edge
 *   Defect 4 (satisfy<->verify gap): req with satisfy but no verify; req with verify but no satisfy
 *   Defect 5 (duplicate id): two elements with identical @id attribute
 *   Defect 6 (uncovered-need): Need with no inbound DeriveRequirementUsage
 *   Defect 7 (unbacktraced): systemReq with no outbound DeriveRequirementUsage
 *
 *   Clean control: fully-traced req (satisfy+verify+derive, Usage operands) → ZERO findings
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "@sysml-bridge/model";
import { relationalFindings, TRACE_TYPES } from "../relational.js";
import type { Finding } from "../findings.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function byRule(findings: Finding[], ruleId: string): Finding[] {
  return findings.filter((f) => f.ruleId === ruleId);
}

// ---------------------------------------------------------------------------
// Main seeded-defect describe
// ---------------------------------------------------------------------------

describe("relationalFindings — seeded-defect fixture (GATE-02 ROADMAP criterion 2)", () => {
  let dir: string;
  let store: FileStore;

  // Element ids captured for clean-control assertion
  let cleanReqId: string;
  let cleanPartId: string;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-audit-rel-"));
    store = new FileStore(dir);
    await store.createProject("Relational Test");

    // ── Defect 1 (R4): SatisfyRequirementUsage with Definition operands ──
    // Source = PartDefinition (not a Usage), Target = RequirementDefinition (not a Usage)
    const defPart = await store.createElement("PartDefinition", "DefPart", {
      provenanceSourceId: "corp-1",
    });
    const defReq = await store.createElement("RequirementDefinition", "DefReq", {
      provenanceSourceId: "corp-2",
    });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": defPart.id }],
      target: [{ "@id": defReq.id }],
    });

    // ── Defect 2 (dangling endpoint): relationship with a ghost target id ──
    const realPart = await store.createElement("PartDefinition", "RealPart", {
      provenanceSourceId: "corp-3",
    });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": realPart.id }],
      target: [{ "@id": "ghost-id" }],
    });

    // ── Defect 3 (orphan): leaf PartDefinition with no trace edge, no children ──
    await store.createElement("PartDefinition", "OrphanPart", {
      provenanceSourceId: "corp-4",
    });

    // ── Defect 4 (satisfy<->verify gap) ──
    // satisfiedReq: has SatisfyRequirementUsage but NO verify edge
    const satisfiedReq = await store.createElement("RequirementDefinition", "SatisfiedReq", {
      provenanceSourceId: "corp-5",
    });
    // verifiedReq: has VerifyRequirementUsage but NO satisfy edge
    const verifiedReq = await store.createElement("RequirementDefinition", "VerifiedReq", {
      provenanceSourceId: "corp-6",
    });
    const satisfierPart = await store.createElement("PartUsage", "SatisfierPart", {
      provenanceSourceId: "corp-7",
    });
    const verifierPart = await store.createElement("PartUsage", "VerifierPart", {
      provenanceSourceId: "corp-8",
    });
    // satisfiedReq gets a satisfy but no verify
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": satisfierPart.id }],
      target: [{ "@id": satisfiedReq.id }],
    });
    // verifiedReq gets a verify but no satisfy
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": verifierPart.id }],
      target: [{ "@id": verifiedReq.id }],
    });

    // ── Defect 5 (duplicate id): two elements with the same @id ──
    await store.createElement("PartDefinition", "DupA", { "@id": "dup-1" });
    await store.createElement("PartDefinition", "DupB", { "@id": "dup-1" });

    // ── Defect 6 (uncovered-need): Need with no inbound DeriveRequirementUsage ──
    await store.createElement("RequirementDefinition", "UncoveredNeed", {
      provenanceSourceId: "corp-11",
      stakeholderNeed: true,
    });

    // ── Defect 7 (unbacktraced): systemReq with no outbound DeriveRequirementUsage ──
    await store.createElement("RequirementDefinition", "UnbacktracedReq", {
      provenanceSourceId: "corp-12",
    });

    // ── Clean control: fully-traced req (Usage operands only, satisfy+verify+derive) ──
    // Both source and target are Usages so R4 does NOT fire.
    // RequirementUsage as the requirement, PartUsage as the satisfying element.
    const cleanReq = await store.createElement("RequirementUsage", "CleanReq", {
      provenanceSourceId: "corp-9",
    });
    const cleanPart = await store.createElement("PartUsage", "CleanPart", {
      provenanceSourceId: "corp-10",
    });
    const cleanNeed = await store.createElement("RequirementUsage", "CleanNeed", {
      provenanceSourceId: "corp-13",
      stakeholderNeed: true,
    });
    // satisfy (PartUsage → RequirementUsage): both are Usages → no R4 finding
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": cleanPart.id }],
      target: [{ "@id": cleanReq.id }],
    });
    // verify (PartUsage → RequirementUsage): both are Usages → no R4 finding
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": cleanPart.id }],
      target: [{ "@id": cleanReq.id }],
    });
    // derive (RequirementUsage → RequirementUsage-need): both are Usages → no R4 finding
    await store.createElement("DeriveRequirementUsage", "", {
      source: [{ "@id": cleanReq.id }],
      target: [{ "@id": cleanNeed.id }],
    });
    // CleanNeed now has an inbound derive so it is covered

    cleanReqId = cleanReq.id;
    cleanPartId = cleanPart.id;
  });

  afterEach(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  // --------------------------------------------------------------------------
  // Module-level export checks
  // --------------------------------------------------------------------------

  it("TRACE_TYPES exports the five expected trace relationship types", () => {
    const expected = [
      "SatisfyRequirementUsage",
      "AllocationUsage",
      "VerifyRequirementUsage",
      "DeriveRequirementUsage",
      "TraceRequirementUsage",
    ];
    for (const t of expected) {
      expect(TRACE_TYPES.has(t)).toBe(true);
    }
  });

  // --------------------------------------------------------------------------
  // Defect 1: R4-def-operand
  // --------------------------------------------------------------------------

  it("Defect 1 (R4): fires R4-def-operand with severity error for Definition operands", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const r4 = byRule(findings, "R4-def-operand");
    expect(r4.length).toBeGreaterThan(0);
    expect(r4[0].severity).toBe("error");
    // elementId should be one of the Definition operand ids
    for (const f of r4) {
      expect(f.elementId).toBeTruthy();
      expect(f.message).toBeTruthy();
      expect(f.suggestedFix).toBeTruthy();
    }
  });

  it("Defect 1 (R4): suggestedFix names the Usage replacement type", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const r4 = byRule(findings, "R4-def-operand");
    expect(r4.length).toBeGreaterThan(0);
    // At least one suggestedFix should mention a Usage type
    const anyFix = r4.some((f) => f.suggestedFix.includes("Usage"));
    expect(anyFix).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Defect 2: GATE02-dangling-endpoint
  // --------------------------------------------------------------------------

  it("Defect 2 (dangling): fires GATE02-dangling-endpoint with severity error for ghost-id", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const dangling = byRule(findings, "GATE02-dangling-endpoint");
    expect(dangling.length).toBeGreaterThan(0);
    expect(dangling[0].severity).toBe("error");

    // The finding's message should mention ghost-id
    const mentionsGhost = dangling.some((f) => f.message.includes("ghost-id"));
    expect(mentionsGhost).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Defect 3: GATE02-orphan
  // --------------------------------------------------------------------------

  it("Defect 3 (orphan): fires GATE02-orphan with severity warning for OrphanPart", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const orphans = byRule(findings, "GATE02-orphan");
    expect(orphans.length).toBeGreaterThan(0);
    expect(orphans[0].severity).toBe("warning");

    const mentionsOrphan = orphans.some((f) => f.message.includes("OrphanPart"));
    expect(mentionsOrphan).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Defect 4: satisfy<->verify gap
  // --------------------------------------------------------------------------

  it("Defect 4 (gap): fires GATE02-unverified warning for req with satisfy but no verify", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const unverified = byRule(findings, "GATE02-unverified");
    expect(unverified.length).toBeGreaterThan(0);
    expect(unverified[0].severity).toBe("warning");

    const mentionsSatisfied = unverified.some((f) => f.message.includes("SatisfiedReq"));
    expect(mentionsSatisfied).toBe(true);
  });

  it("Defect 4 (gap): fires GATE02-unsatisfied warning for req with verify but no satisfy", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const unsatisfied = byRule(findings, "GATE02-unsatisfied");
    expect(unsatisfied.length).toBeGreaterThan(0);
    expect(unsatisfied[0].severity).toBe("warning");

    const mentionsVerified = unsatisfied.some((f) => f.message.includes("VerifiedReq"));
    expect(mentionsVerified).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Defect 5: GATE02-id-duplicate
  // --------------------------------------------------------------------------

  it("Defect 5 (duplicate id): fires GATE02-id-duplicate with severity error for dup-1", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const dups = byRule(findings, "GATE02-id-duplicate");
    expect(dups.length).toBeGreaterThan(0);
    expect(dups[0].severity).toBe("error");

    const mentionsDup = dups.some((f) => f.message.includes("dup-1"));
    expect(mentionsDup).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Defect 6: GATE02-uncovered-need
  // --------------------------------------------------------------------------

  it("Defect 6 (uncovered need): fires GATE02-uncovered-need warning for UncoveredNeed", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const uncoveredNeeds = byRule(findings, "GATE02-uncovered-need");
    expect(uncoveredNeeds.length).toBeGreaterThan(0);
    expect(uncoveredNeeds[0].severity).toBe("warning");

    const mentionsUncovered = uncoveredNeeds.some((f) => f.message.includes("UncoveredNeed"));
    expect(mentionsUncovered).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Defect 7: GATE02-unbacktraced
  // --------------------------------------------------------------------------

  it("Defect 7 (unbacktraced): fires GATE02-unbacktraced warning for UnbacktracedReq", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const unbacktraced = byRule(findings, "GATE02-unbacktraced");
    expect(unbacktraced.length).toBeGreaterThan(0);
    expect(unbacktraced[0].severity).toBe("warning");

    const mentionsUnbacktraced = unbacktraced.some((f) => f.message.includes("UnbacktracedReq"));
    expect(mentionsUnbacktraced).toBe(true);
  });

  // --------------------------------------------------------------------------
  // Clean control
  // --------------------------------------------------------------------------

  it("Clean control: CleanReq and CleanPart ids appear in ZERO findings", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const cleanFindings = findings.filter(
      (f) => f.elementId === cleanReqId || f.elementId === cleanPartId
    );
    expect(cleanFindings).toHaveLength(0);
  });

  // --------------------------------------------------------------------------
  // Every finding has non-empty required fields
  // --------------------------------------------------------------------------

  it("Every finding has non-empty elementId, message, and suggestedFix", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    for (const f of findings) {
      expect(f.elementId).toBeTruthy();
      expect(f.message).toBeTruthy();
      expect(f.suggestedFix).toBeTruthy();
    }
  });

  // --------------------------------------------------------------------------
  // Distinct rule classes (ROADMAP criterion 2)
  // --------------------------------------------------------------------------

  it("ROADMAP criterion 2: all five core rule classes produce DISTINCT ruleIds", async () => {
    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    const ruleIds = new Set(findings.map((f) => f.ruleId));
    expect(ruleIds.has("R4-def-operand")).toBe(true);
    expect(ruleIds.has("GATE02-dangling-endpoint")).toBe(true);
    expect(ruleIds.has("GATE02-orphan")).toBe(true);
    expect(ruleIds.has("GATE02-unverified")).toBe(true);
    expect(ruleIds.has("GATE02-unsatisfied")).toBe(true);
    expect(ruleIds.has("GATE02-id-duplicate")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// CR-02: chained-derivation direction test
// R2 derives FROM R1 (R2 is SOURCE, R1 is TARGET). R1 must NOT be counted as
// backtraced — being the TARGET of a derive edge is NOT the same as having an
// outgoing derive to a stakeholder Need.
// ---------------------------------------------------------------------------

describe("relationalFindings — CR-02 backward-trace direction (chained derivation)", () => {
  let dir: string;
  let store: FileStore;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-cr02-"));
    store = new FileStore(dir);
    await store.createProject("CR-02 Direction Test");
  });

  afterEach(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("CR-02: R1 that is ONLY the TARGET of a DeriveRequirementUsage still fires GATE02-unbacktraced", async () => {
    // R1 (system req) — has no outgoing DeriveRequirementUsage
    const r1 = await store.createElement("RequirementDefinition", "R1", {
      provenanceSourceId: "R1-prov",
    });
    // R2 (system req) — derives FROM R1 (R2=SOURCE, R1=TARGET)
    const r2 = await store.createElement("RequirementDefinition", "R2", {
      provenanceSourceId: "R2-prov",
    });
    // DeriveRequirementUsage: R2 → R1 (R2 is source, R1 is target)
    await store.createElement("DeriveRequirementUsage", "", {
      source: [{ "@id": r2.id }],
      target: [{ "@id": r1.id }],
    });

    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    // R1 is the TARGET of a derive edge — it must still fire GATE02-unbacktraced
    // because it has no OUTGOING derive trace to a stakeholder Need.
    const unbacktraced = findings.filter((f) => f.ruleId === "GATE02-unbacktraced");
    const r1Unbacktraced = unbacktraced.some((f) => f.elementId === r1.id);
    expect(r1Unbacktraced).toBe(true);
  });

  it("CR-02: R2 that is the SOURCE of a DeriveRequirementUsage to a Need does NOT fire GATE02-unbacktraced", async () => {
    // Need (stakeholderNeed)
    const need = await store.createElement("RequirementDefinition", "Need1", {
      provenanceSourceId: "N1-prov",
      stakeholderNeed: true,
    });
    // R2 (system req) — derives to Need (R2=SOURCE, Need=TARGET) → backtraced
    const r2 = await store.createElement("RequirementDefinition", "R2", {
      provenanceSourceId: "R2-prov",
    });
    // DeriveRequirementUsage: R2 → Need
    await store.createElement("DeriveRequirementUsage", "", {
      source: [{ "@id": r2.id }],
      target: [{ "@id": need.id }],
    });
    // Give R2 satisfy+verify so only the backward check is in scope
    const part = await store.createElement("PartUsage", "Part1", { provenanceSourceId: "P1" });
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": part.id }],
      target: [{ "@id": r2.id }],
    });
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": part.id }],
      target: [{ "@id": r2.id }],
    });

    const elements = await store.queryElements();
    const relationships = await store.queryRelationships();
    const findings = relationalFindings(elements, relationships);

    // R2 is the SOURCE of a derive to a Need — must NOT fire GATE02-unbacktraced
    const r2Unbacktraced = findings.some(
      (f) => f.ruleId === "GATE02-unbacktraced" && f.elementId === r2.id
    );
    expect(r2Unbacktraced).toBe(false);
  });
});
