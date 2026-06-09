import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../utils/sysml-serializer.js";
import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

function el(overrides: Partial<SysmlElement>): SysmlElement {
  return {
    id: "e1",
    elementId: "e1",
    type: "PartDefinition",
    name: "Test",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("serializeToSysml", () => {
  it("serializes a part definition", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "Engine" })],
      []
    );
    expect(result).toBe("part def Engine;\n");
  });

  it("serializes a requirement definition", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "RequirementDefinition", name: "MassReq" })],
      []
    );
    expect(result).toBe("requirement def MassReq;\n");
  });

  it("serializes a package with children", () => {
    const parent = el({ id: "pkg1", type: "Package", name: "SystemPkg", ownerId: null });
    const child = el({ id: "e2", type: "PartDefinition", name: "Engine", ownerId: "pkg1" });
    const result = serializeToSysml([parent, child], []);
    expect(result).toBe("package SystemPkg {\n  part def Engine;\n}\n");
  });

  it("serializes VerificationCaseDefinition with keyword 'verification def'", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "VerificationCaseDefinition", name: "MassTest" })],
      []
    );
    expect(result).toBe("verification def MassTest;\n");
  });

  it("serializes AnalysisCaseDefinition with keyword 'analysis def'", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "AnalysisCaseDefinition", name: "FuelAnalysis" })],
      []
    );
    expect(result).toBe("analysis def FuelAnalysis;\n");
  });

  it("serializes EnumerationDefinition with keyword 'enum def'", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "EnumerationDefinition", name: "FuelKind" })],
      []
    );
    expect(result).toBe("enum def FuelKind;\n");
  });

  it("serializes elements with short names", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "RequirementDefinition", name: "MaxMass", shortName: "SYS-001" })],
      []
    );
    expect(result).toBe("requirement def <'SYS-001'> MaxMass;\n");
  });

  it("quotes names that are not valid identifiers", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "My Engine" })],
      []
    );
    expect(result).toBe("part def 'My Engine';\n");
  });

  it("does not quote plain valid identifier names", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "Engine" })],
      []
    );
    expect(result).not.toContain("'Engine'");
    expect(result).toContain("Engine;");
  });

  it("treats elements whose ownerId is not in the element set as root", () => {
    // ownerId points to something outside the element array → treated as root
    const child = el({ id: "e1", type: "PartDefinition", name: "Engine", ownerId: "missing-parent" });
    const result = serializeToSysml([child], []);
    expect(result).toBe("part def Engine;\n");
  });

  it("handles deeply nested children", () => {
    const pkg = el({ id: "pkg1", type: "Package", name: "TopPkg", ownerId: null });
    const part = el({ id: "p1", type: "PartDefinition", name: "EngineDef", ownerId: "pkg1" });
    const port = el({ id: "port1", type: "PortDefinition", name: "FuelPort", ownerId: "p1" });
    const result = serializeToSysml([pkg, part, port], []);
    expect(result).toBe(
      "package TopPkg {\n  part def EngineDef {\n    port def FuelPort;\n  }\n}\n"
    );
  });

  it("handles a null name by omitting the name portion", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: null })],
      []
    );
    // Should produce: "part def;" (no name) — just verify it doesn't crash and has the keyword
    expect(result).toContain("part def");
  });

  it("outputs a trailing newline", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "Engine" })],
      []
    );
    expect(result.endsWith("\n")).toBe(true);
  });

  it("emits a satisfy statement for SatisfyRequirementUsage", () => {
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification" });
    const act = el({ id: "a1", type: "ActionDefinition", name: "ReceiveAuthenticateRequest" });
    const rel: SysmlRelationship = { id: "rel1", type: "SatisfyRequirementUsage", sourceIds: ["a1"], targetIds: ["r1"], raw: {} };
    expect(serializeToSysml([req, act], [rel])).toContain("satisfy AircraftIDVerification by ReceiveAuthenticateRequest;");
  });
  it("emits an allocate statement for AllocationUsage", () => {
    const fn = el({ id: "f1", type: "ActionDefinition", name: "ReceiveAuthenticateRequest" });
    const comp = el({ id: "c1", type: "PartDefinition", name: "FlightControlModule" });
    const rel: SysmlRelationship = { id: "rel2", type: "AllocationUsage", sourceIds: ["f1"], targetIds: ["c1"], raw: {} };
    expect(serializeToSysml([fn, comp], [rel])).toContain("allocate ReceiveAuthenticateRequest to FlightControlModule;");
  });
  it("emits verify as a nested objective body in the verification def (not top-level)", () => {
    const req = el({ id: "r1", type: "RequirementUsage", name: "AircraftIDVerification" });
    const ver = el({ id: "v1", type: "VerificationCaseDefinition", name: "AuthVerification" });
    const rel: SysmlRelationship = { id: "rel3", type: "VerifyRequirementUsage", sourceIds: ["v1"], targetIds: ["r1"], raw: {} };
    const out = serializeToSysml([req, ver], [rel]);

    // Nested form: verification def { objective { verify <reqUsage>; } }
    expect(out).toContain("verification def AuthVerification {");
    expect(out).toContain("objective {");
    expect(out).toContain("verify AircraftIDVerification;");

    // Negative: there must be NO top-level `verify ... by ...` line, and no
    // package-level `verify` (only the indented one inside objective {}).
    expect(out).not.toMatch(/^\s*verify .* by /m);
    expect(out).not.toMatch(/^verify /m);
  });
  it("emits a dependency statement for DeriveRequirementUsage (Need->Req)", () => {
    const need = el({ id: "n1", type: "RequirementDefinition", name: "N4_AuthenticationSecurity" });
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification" });
    const rel: SysmlRelationship = { id: "rel4", type: "DeriveRequirementUsage", sourceIds: ["r1"], targetIds: ["n1"], raw: {} };
    expect(serializeToSysml([need, req], [rel])).toContain("dependency from AircraftIDVerification to N4_AuthenticationSecurity;");
  });
  it("skips relationships whose endpoints are not in the element set", () => {
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification" });
    const rel: SysmlRelationship = { id: "rel5", type: "SatisfyRequirementUsage", sourceIds: ["missing"], targetIds: ["r1"], raw: {} };
    expect(serializeToSysml([req], [rel])).not.toContain("satisfy");
  });
  it("emits a provenance comment when an element carries a source id", () => {
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification", raw: { provenanceSourceId: "ANGARS-4" } });
    expect(serializeToSysml([req], [])).toContain("// @source: ANGARS-4");
  });
});
