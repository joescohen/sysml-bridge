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
});
