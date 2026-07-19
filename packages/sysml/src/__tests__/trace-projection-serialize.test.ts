/**
 * trace-projection-serialize.test.ts — the crux mapping serializes.
 *
 * Proves that an APPROVED inferred trace entry (satisfy / derive) projected via
 * `projectInferredTraceRelationships` serializes to a USAGE-correct SysML v2 trace
 * statement (R4). The generated file for satisfy is separately confirmed
 * validator-clean via tools/sysml-validator/run.sh (see the build report).
 */

import { describe, it, expect } from "vitest";
import {
  projectInferredTraceRelationships,
  type InferredApprovedEntry,
  type SysmlElement,
} from "@sysml-bridge/model";
import { serializeToSysml } from "../sysml-serializer.js";

const reqUsage: SysmlElement = {
  id: "req-001", elementId: "req-001", type: "RequirementUsage", name: "R_FuelCommand",
  shortName: null, qualifiedName: null, ownerId: null, ownedElementIds: [], raw: {},
};
const partUsage: SysmlElement = {
  id: "part-fcm", elementId: "part-fcm", type: "PartUsage", name: "FuelControlModule",
  shortName: null, qualifiedName: null, ownerId: null, ownedElementIds: [], raw: {},
};
const needUsage: SysmlElement = {
  id: "need-001", elementId: "need-001", type: "RequirementUsage", name: "N_AutonomousRefuel",
  shortName: null, qualifiedName: null, ownerId: null, ownedElementIds: [], raw: {},
};

function entry(over: Partial<InferredApprovedEntry> & Pick<InferredApprovedEntry, "id" | "relationFamily" | "sourceId" | "targetId">): InferredApprovedEntry {
  return {
    premises: ["chunk-1"], rationale: "audit-only", confidence: 0.9,
    inferenceRunId: "run-1", approvedBy: "tester", approvedAt: "2026-07-15T00:00:00.000Z",
    status: "approved", ...over,
  };
}

describe("trace projection → serializer (crux)", () => {
  it("approved satisfy → `satisfy <req> by <element>;` (usage operands, R4)", () => {
    const rels = projectInferredTraceRelationships([
      entry({ id: "s1", relationFamily: "satisfy", sourceId: "part-fcm", targetId: "req-001" }),
    ]);
    expect(rels[0]!.type).toBe("SatisfyRequirementUsage");
    const sysml = serializeToSysml([reqUsage, partUsage], rels);
    expect(sysml).toContain("satisfy R_FuelCommand by FuelControlModule;");
    // Operands are the package-level usages (never definitions).
    expect(sysml).toContain("requirement R_FuelCommand;");
    expect(sysml).toContain("part FuelControlModule;");
  });

  it("approved derive → `dependency from <req> to <need>;`", () => {
    const rels = projectInferredTraceRelationships([
      entry({ id: "d1", relationFamily: "derive", sourceId: "req-001", targetId: "need-001" }),
    ]);
    expect(rels[0]!.type).toBe("DeriveRequirementUsage");
    const sysml = serializeToSysml([reqUsage, needUsage], rels);
    expect(sysml).toContain("dependency from R_FuelCommand to N_AutonomousRefuel;");
  });

  it("both together serialize inside the 'C&C Trace' package", () => {
    const rels = projectInferredTraceRelationships([
      entry({ id: "s1", relationFamily: "satisfy", sourceId: "part-fcm", targetId: "req-001" }),
      entry({ id: "d1", relationFamily: "derive", sourceId: "req-001", targetId: "need-001" }),
    ]);
    const sysml = serializeToSysml([reqUsage, partUsage, needUsage], rels);
    expect(sysml).toContain("package 'C&C Trace' {");
    expect(sysml).toContain("satisfy R_FuelCommand by FuelControlModule;");
    expect(sysml).toContain("dependency from R_FuelCommand to N_AutonomousRefuel;");
  });
});
