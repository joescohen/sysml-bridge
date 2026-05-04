import { describe, it, expect } from "vitest";
import { parseSysml } from "../utils/sysml-parser.js";
import type { ParsedElement, ParseResult } from "../utils/sysml-parser.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function findElement(result: ParseResult, name: string): ParsedElement | undefined {
  function search(elements: ParsedElement[]): ParsedElement | undefined {
    for (const el of elements) {
      if (el.name === name) return el;
      const found = search(el.children);
      if (found) return found;
    }
    return undefined;
  }
  return search(result.elements);
}

// ---------------------------------------------------------------------------
// 1. Parses a package
// ---------------------------------------------------------------------------

describe("parseSysml — package", () => {
  it("parses a package declaration", () => {
    const result = parseSysml("package VehicleModel;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("Package");
    expect(result.elements[0].name).toBe("VehicleModel");
  });

  it("parses a package with a body", () => {
    const result = parseSysml(`package VehicleModel {
}`);
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("Package");
    expect(result.elements[0].name).toBe("VehicleModel");
  });
});

// ---------------------------------------------------------------------------
// 2. Parses part definitions
// ---------------------------------------------------------------------------

describe("parseSysml — part definitions", () => {
  it("parses a basic part def", () => {
    const result = parseSysml("part def Engine;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("PartDefinition");
    expect(result.elements[0].name).toBe("Engine");
  });

  it("parses multiple part defs", () => {
    const input = `part def Engine;
part def Transmission;`;
    const result = parseSysml(input);
    const names = result.elements.map((e) => e.name);
    expect(names).toContain("Engine");
    expect(names).toContain("Transmission");
    expect(result.elements.every((e) => e.type === "PartDefinition")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// 3. Parses part usages with typing (`: Engine`)
// ---------------------------------------------------------------------------

describe("parseSysml — part usages with typing", () => {
  it("parses a part usage with a type", () => {
    const result = parseSysml("part engine : Engine;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("PartUsage");
    expect(result.elements[0].name).toBe("engine");
    expect(result.elements[0].typedBy).toBe("Engine");
  });

  it("typedBy is undefined when no typing annotation", () => {
    const result = parseSysml("part myPart;");
    expect(result.elements[0].typedBy).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// 4. Parses requirement definitions
// ---------------------------------------------------------------------------

describe("parseSysml — requirement definitions", () => {
  it("parses a requirement def", () => {
    const result = parseSysml("requirement def MaxMass;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("RequirementDefinition");
    expect(result.elements[0].name).toBe("MaxMass");
  });

  it("parses a requirement usage", () => {
    const result = parseSysml("requirement massReq : MaxMass;");
    expect(result.elements[0].type).toBe("RequirementUsage");
    expect(result.elements[0].typedBy).toBe("MaxMass");
  });
});

// ---------------------------------------------------------------------------
// 5. Parses short names in angle brackets (`<'SYS-001'>`)
// ---------------------------------------------------------------------------

describe("parseSysml — short names", () => {
  it("extracts shortName from angle bracket syntax", () => {
    const result = parseSysml("requirement def <'SYS-001'> MaxMass;");
    expect(result.elements[0].type).toBe("RequirementDefinition");
    expect(result.elements[0].name).toBe("MaxMass");
    expect(result.elements[0].shortName).toBe("SYS-001");
  });

  it("works with shortName on part def", () => {
    const result = parseSysml("part def <'P-01'> Engine;");
    expect(result.elements[0].type).toBe("PartDefinition");
    expect(result.elements[0].name).toBe("Engine");
    expect(result.elements[0].shortName).toBe("P-01");
  });
});

// ---------------------------------------------------------------------------
// 6. Parses specialization with `:>`
// ---------------------------------------------------------------------------

describe("parseSysml — specialization", () => {
  it("parses :> specialization on part def", () => {
    const result = parseSysml("part def SportsCar :> Vehicle;");
    expect(result.elements[0].type).toBe("PartDefinition");
    expect(result.elements[0].name).toBe("SportsCar");
    expect(result.elements[0].specializes).toBe("Vehicle");
  });

  it("specializes is undefined when not present", () => {
    const result = parseSysml("part def Engine;");
    expect(result.elements[0].specializes).toBeUndefined();
  });

  it("parses both typedBy and specialization (: ... :> not common but test robustness)", () => {
    const result = parseSysml("part def ElectricCar :> Car;");
    expect(result.elements[0].specializes).toBe("Car");
  });
});

// ---------------------------------------------------------------------------
// 7. Parses nested elements as children
// ---------------------------------------------------------------------------

describe("parseSysml — nested elements (children)", () => {
  it("assigns nested elements as children of the parent", () => {
    const input = `package VehicleModel {
  part def Engine;
  part def Transmission;
}`;
    const result = parseSysml(input);
    expect(result.elements).toHaveLength(1);
    const pkg = result.elements[0];
    expect(pkg.type).toBe("Package");
    expect(pkg.children).toHaveLength(2);
    expect(pkg.children[0].type).toBe("PartDefinition");
    expect(pkg.children[0].name).toBe("Engine");
    expect(pkg.children[1].type).toBe("PartDefinition");
    expect(pkg.children[1].name).toBe("Transmission");
  });

  it("handles deeply nested elements", () => {
    const input = `package Outer {
  package Inner {
    part def Widget;
  }
}`;
    const result = parseSysml(input);
    expect(result.elements).toHaveLength(1);
    const outer = result.elements[0];
    expect(outer.children).toHaveLength(1);
    const inner = outer.children[0];
    expect(inner.type).toBe("Package");
    expect(inner.children).toHaveLength(1);
    expect(inner.children[0].type).toBe("PartDefinition");
    expect(inner.children[0].name).toBe("Widget");
  });

  it("top-level elements have empty children array", () => {
    const result = parseSysml("part def Engine;");
    expect(result.elements[0].children).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// 8. Parses satisfy relationships
// ---------------------------------------------------------------------------

describe("parseSysml — relationships", () => {
  it("parses a satisfy relationship", () => {
    const result = parseSysml("satisfy massReq by vehicle;");
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].type).toBe("satisfy");
    expect(result.relationships[0].requirement).toBe("massReq");
    expect(result.relationships[0].by).toBe("vehicle");
  });

  it("parses a verify relationship", () => {
    const result = parseSysml("verify massReq by testPlan;");
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].type).toBe("verify");
    expect(result.relationships[0].requirement).toBe("massReq");
    expect(result.relationships[0].by).toBe("testPlan");
  });

  it("parses an allocate relationship", () => {
    const result = parseSysml("allocate l.component to p.assembly.element;");
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].type).toBe("allocate");
    expect(result.relationships[0].from).toBe("l.component");
    expect(result.relationships[0].to).toBe("p.assembly.element");
  });

  it("parses a dependency relationship", () => {
    const result = parseSysml("dependency from x to y;");
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].type).toBe("dependency");
    expect(result.relationships[0].from).toBe("x");
    expect(result.relationships[0].to).toBe("y");
  });
});

// ---------------------------------------------------------------------------
// 9. Parses imports
// ---------------------------------------------------------------------------

describe("parseSysml — imports", () => {
  it("parses a wildcard import", () => {
    const result = parseSysml("import ISQ::*;");
    expect(result.imports).toContain("ISQ::*");
  });

  it("parses a specific import", () => {
    const result = parseSysml("import SI::kg;");
    expect(result.imports).toContain("SI::kg");
  });

  it("parses a private import", () => {
    const result = parseSysml("private import SI::kg;");
    expect(result.imports).toContain("SI::kg");
  });

  it("collects multiple imports", () => {
    const input = `import ISQ::*;
import SI::kg;
private import Units::m;`;
    const result = parseSysml(input);
    expect(result.imports).toHaveLength(3);
    expect(result.imports).toContain("ISQ::*");
    expect(result.imports).toContain("SI::kg");
    expect(result.imports).toContain("Units::m");
  });
});

// ---------------------------------------------------------------------------
// 10. Parses `verification def` → VerificationCaseDefinition
// ---------------------------------------------------------------------------

describe("parseSysml — verification def", () => {
  it("parses verification def as VerificationCaseDefinition", () => {
    const result = parseSysml("verification def MassTest;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("VerificationCaseDefinition");
    expect(result.elements[0].name).toBe("MassTest");
  });

  it("parses verification usage as VerificationCaseUsage", () => {
    const result = parseSysml("verification massTest : MassTest;");
    expect(result.elements[0].type).toBe("VerificationCaseUsage");
    expect(result.elements[0].typedBy).toBe("MassTest");
  });
});

// ---------------------------------------------------------------------------
// 11. Parses `analysis def` → AnalysisCaseDefinition
// ---------------------------------------------------------------------------

describe("parseSysml — analysis def", () => {
  it("parses analysis def as AnalysisCaseDefinition", () => {
    const result = parseSysml("analysis def ThermalAnalysis;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("AnalysisCaseDefinition");
    expect(result.elements[0].name).toBe("ThermalAnalysis");
  });

  it("parses analysis usage as AnalysisCaseUsage", () => {
    const result = parseSysml("analysis thermalRun : ThermalAnalysis;");
    expect(result.elements[0].type).toBe("AnalysisCaseUsage");
    expect(result.elements[0].typedBy).toBe("ThermalAnalysis");
  });
});

// ---------------------------------------------------------------------------
// 12. Parses `enum def` → EnumerationDefinition
// ---------------------------------------------------------------------------

describe("parseSysml — enum def", () => {
  it("parses enum def as EnumerationDefinition", () => {
    const result = parseSysml("enum def Status;");
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("EnumerationDefinition");
    expect(result.elements[0].name).toBe("Status");
  });

  it("parses enum usage as EnumerationUsage", () => {
    const result = parseSysml("enum currentStatus : Status;");
    expect(result.elements[0].type).toBe("EnumerationUsage");
  });
});

// ---------------------------------------------------------------------------
// 13. Returns errors for unparseable lines
// ---------------------------------------------------------------------------

describe("parseSysml — error handling", () => {
  it("records an error for a truly unparseable line", () => {
    const result = parseSysml("$$invalid sysml gibberish$$");
    expect(result.errors.length).toBeGreaterThan(0);
  });

  it("does not error on known-skip keywords", () => {
    const input = `first step1 then step2;
flow of signal to port;
connect a to b;`;
    const result = parseSysml(input);
    expect(result.errors).toHaveLength(0);
  });

  it("does not error on comments", () => {
    const result = parseSysml("// this is a comment");
    expect(result.errors).toHaveLength(0);
    expect(result.elements).toHaveLength(0);
  });

  it("does not error on block comments", () => {
    const result = parseSysml("/* block comment */");
    expect(result.errors).toHaveLength(0);
  });

  it("does not error on empty lines or closing braces", () => {
    const result = parseSysml("   \n}\n};");
    expect(result.errors).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// 14. Handles empty input
// ---------------------------------------------------------------------------

describe("parseSysml — empty input", () => {
  it("returns empty result for empty string", () => {
    const result = parseSysml("");
    expect(result.elements).toHaveLength(0);
    expect(result.relationships).toHaveLength(0);
    expect(result.imports).toHaveLength(0);
    expect(result.errors).toHaveLength(0);
  });

  it("returns empty result for whitespace-only input", () => {
    const result = parseSysml("   \n   \n   ");
    expect(result.elements).toHaveLength(0);
    expect(result.relationships).toHaveLength(0);
    expect(result.imports).toHaveLength(0);
    expect(result.errors).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// Additional integration: mixed content
// ---------------------------------------------------------------------------

describe("parseSysml — integration", () => {
  it("parses a realistic SysML v2 snippet", () => {
    const input = `package VehicleSystem {
  import ISQ::*;

  part def Vehicle;
  part def Engine :> PowerSource;

  requirement def <'SYS-001'> MaxMass;

  part vehicle : Vehicle {
    part eng : Engine;
  }

  satisfy massReq by vehicle;
}`;
    const result = parseSysml(input);

    // Top-level package
    expect(result.elements).toHaveLength(1);
    const pkg = result.elements[0];
    expect(pkg.type).toBe("Package");
    expect(pkg.name).toBe("VehicleSystem");

    // Import
    expect(result.imports).toContain("ISQ::*");

    // Children of package
    const childTypes = pkg.children.map((c) => c.type);
    expect(childTypes).toContain("PartDefinition");
    expect(childTypes).toContain("RequirementDefinition");

    // Engine has specialization
    const engine = findElement(result, "Engine");
    expect(engine).toBeDefined();
    expect(engine!.specializes).toBe("PowerSource");

    // RequirementDefinition has shortName
    const maxMass = findElement(result, "MaxMass");
    expect(maxMass).toBeDefined();
    expect(maxMass!.shortName).toBe("SYS-001");

    // Satisfy relationship
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].type).toBe("satisfy");
  });
});
