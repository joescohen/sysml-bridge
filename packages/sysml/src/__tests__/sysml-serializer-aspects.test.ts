import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../sysml-serializer.js";
import type { SysmlElement, SysmlRelationship } from "@sysml-bridge/model";

// ---------------------------------------------------------------------------
// Helpers
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

function rel(overrides: Partial<SysmlRelationship>): SysmlRelationship {
  return {
    id: "rel1",
    type: "Connector",
    sourceIds: [],
    targetIds: [],
    raw: {},
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// TC1 — Connections (ConnectionUsage + Connector → connect a to b;)
// ---------------------------------------------------------------------------

describe("serializer aspect: connections (TC1)", () => {
  it("emits `connect a to b;` for a Connector inside the common owner body", () => {
    // package Pkg { part a; part b; connect a to b; }
    const pkg = el({ id: "pkg", type: "Package", name: "Pkg", ownerId: null });
    const a = el({ id: "a", type: "PartUsage", name: "a", ownerId: "pkg" });
    const b = el({ id: "b", type: "PartUsage", name: "b", ownerId: "pkg" });
    const connector = rel({
      id: "c1",
      type: "Connector",
      sourceIds: ["a"],
      targetIds: ["b"],
      raw: {},
    });
    const out = serializeToSysml([pkg, a, b], [connector]);
    expect(out).toContain("connect a to b;");
  });

  it("emits a qualified port reference `connect battery.dcOut to inverter.dcIn;`", () => {
    // package Pkg { part battery { port dcOut; } part inverter { port dcIn; }
    //   connect battery.dcOut to inverter.dcIn; }
    const pkg = el({ id: "pkg", type: "Package", name: "Pkg", ownerId: null });
    const battery = el({
      id: "battery",
      type: "PartUsage",
      name: "battery",
      ownerId: "pkg",
    });
    const inverter = el({
      id: "inverter",
      type: "PartUsage",
      name: "inverter",
      ownerId: "pkg",
    });
    const dcOut = el({
      id: "dcOut",
      type: "PortUsage",
      name: "dcOut",
      ownerId: "battery",
    });
    const dcIn = el({
      id: "dcIn",
      type: "PortUsage",
      name: "dcIn",
      ownerId: "inverter",
    });
    const connector = rel({
      id: "c1",
      type: "Connector",
      sourceIds: ["dcOut"],
      targetIds: ["dcIn"],
      raw: {},
    });
    const out = serializeToSysml(
      [pkg, battery, inverter, dcOut, dcIn],
      [connector]
    );
    expect(out).toContain("connect battery.dcOut to inverter.dcIn;");
  });

  it("emits `connection L connect a to b;` for a named ConnectionUsage element", () => {
    const pkg = el({ id: "pkg", type: "Package", name: "Pkg", ownerId: null });
    const a = el({ id: "a", type: "PartUsage", name: "a", ownerId: "pkg" });
    const b = el({ id: "b", type: "PartUsage", name: "b", ownerId: "pkg" });
    const conn = el({
      id: "L",
      type: "ConnectionUsage",
      name: "L",
      ownerId: "pkg",
      raw: { sourceEnd: "a", targetEnd: "b" },
    });
    const out = serializeToSysml([pkg, a, b, conn], []);
    expect(out).toContain("connection L connect a to b;");
    // must NOT emit the old empty form `connection L;`
    expect(out).not.toMatch(/connection L;\s*$/m);
  });

  it("emits a BindingConnector as `bind a = b;`", () => {
    const pkg = el({ id: "pkg", type: "Package", name: "Pkg", ownerId: null });
    const a = el({ id: "a", type: "PartUsage", name: "a", ownerId: "pkg" });
    const b = el({ id: "b", type: "PartUsage", name: "b", ownerId: "pkg" });
    const binding = rel({
      id: "bc1",
      type: "BindingConnector",
      sourceIds: ["a"],
      targetIds: ["b"],
      raw: {},
    });
    const out = serializeToSysml([pkg, a, b], [binding]);
    expect(out).toContain("bind a = b;");
  });
});

// ---------------------------------------------------------------------------
// TC2 — Succession + Flow (first..then / flow from..to inside an action body)
// ---------------------------------------------------------------------------

describe("serializer aspect: succession + flow (TC2)", () => {
  it("emits `first stepA then stepB;` for a Succession inside the action body", () => {
    const proc = el({
      id: "proc",
      type: "ActionDefinition",
      name: "Process",
      ownerId: null,
    });
    const stepA = el({
      id: "sa",
      type: "ActionUsage",
      name: "stepA",
      ownerId: "proc",
    });
    const stepB = el({
      id: "sb",
      type: "ActionUsage",
      name: "stepB",
      ownerId: "proc",
    });
    const succ = rel({
      id: "s1",
      type: "Succession",
      sourceIds: ["sa"],
      targetIds: ["sb"],
      raw: {},
    });
    const out = serializeToSysml([proc, stepA, stepB], [succ]);
    expect(out).toContain("first stepA then stepB;");
  });

  it("emits `flow from stepA to stepB;` for a FlowConnectionUsage", () => {
    const proc = el({
      id: "proc",
      type: "ActionDefinition",
      name: "Process",
      ownerId: null,
    });
    const stepA = el({
      id: "sa",
      type: "ActionUsage",
      name: "stepA",
      ownerId: "proc",
    });
    const stepB = el({
      id: "sb",
      type: "ActionUsage",
      name: "stepB",
      ownerId: "proc",
    });
    const flow = el({
      id: "fl",
      type: "FlowConnectionUsage",
      name: null,
      ownerId: "proc",
      raw: { sourceEnd: "sa", targetEnd: "sb" },
    });
    const out = serializeToSysml([proc, stepA, stepB, flow], []);
    expect(out).toContain("flow from stepA to stepB;");
  });
});

// ---------------------------------------------------------------------------
// TC3 — Transition (transition first s1 then s2; inside a state body)
// ---------------------------------------------------------------------------

describe("serializer aspect: transition (TC3)", () => {
  it("emits `transition first s1 then s2;` for a TransitionUsage", () => {
    const modes = el({
      id: "modes",
      type: "StateDefinition",
      name: "Modes",
      ownerId: null,
    });
    const s1 = el({ id: "s1", type: "StateUsage", name: "s1", ownerId: "modes" });
    const s2 = el({ id: "s2", type: "StateUsage", name: "s2", ownerId: "modes" });
    const trans = el({
      id: "t1",
      type: "TransitionUsage",
      name: null,
      ownerId: "modes",
      raw: { sourceEnd: "s1", targetEnd: "s2" },
    });
    const out = serializeToSysml([modes, s1, s2, trans], []);
    expect(out).toContain("transition first s1 then s2;");
  });

  it("emits transition for a Transition relationship form too", () => {
    const modes = el({
      id: "modes",
      type: "StateDefinition",
      name: "Modes",
      ownerId: null,
    });
    const s1 = el({ id: "s1", type: "StateUsage", name: "s1", ownerId: "modes" });
    const s2 = el({ id: "s2", type: "StateUsage", name: "s2", ownerId: "modes" });
    const trans = rel({
      id: "t1",
      type: "Transition",
      sourceIds: ["s1"],
      targetIds: ["s2"],
      raw: {},
    });
    const out = serializeToSysml([modes, s1, s2], [trans]);
    expect(out).toContain("transition first s1 then s2;");
  });
});

// ---------------------------------------------------------------------------
// TC4 — Specialization (part def Car :> Vehicle;)
// ---------------------------------------------------------------------------

describe("serializer aspect: specialization (TC4)", () => {
  it("appends `:> Vehicle` to a definition header for a Specialization rel", () => {
    const vehicle = el({
      id: "veh",
      type: "PartDefinition",
      name: "Vehicle",
      ownerId: null,
    });
    const car = el({
      id: "car",
      type: "PartDefinition",
      name: "Car",
      ownerId: null,
    });
    const spec = rel({
      id: "sp1",
      type: "Specialization",
      sourceIds: ["car"],
      targetIds: ["veh"],
      raw: {},
    });
    const out = serializeToSysml([vehicle, car], [spec]);
    expect(out).toContain("part def Car :> Vehicle;");
  });

  it("SUPPRESSES `:>` when a USAGE source specializes a Definition (Cameo-invalid)", () => {
    // `part battery : Battery :> PowerSource` is grammar-valid but Cameo-invalid:
    // a usage cannot specialize a Definition. The emitter must drop the suffix.
    const powerSourceDef = el({
      id: "pwr",
      type: "PartDefinition",
      name: "PowerSource",
      ownerId: null,
    });
    const batteryDef = el({
      id: "batDef",
      type: "PartDefinition",
      name: "Battery",
      ownerId: null,
    });
    const battery = el({
      id: "battery",
      type: "PartUsage",
      name: "battery",
      ownerId: null,
      raw: { typeName: "Battery" },
    });
    const spec = rel({
      id: "sp2",
      type: "Specialization",
      sourceIds: ["battery"], // USAGE source
      targetIds: ["pwr"], // Definition target
      raw: {},
    });
    const out = serializeToSysml([powerSourceDef, batteryDef, battery], [spec]);
    // The typed usage is still emitted, but WITHOUT the invalid `:> PowerSource`.
    expect(out).toContain("part battery : Battery;");
    expect(out).not.toContain(":> PowerSource");
  });
});

// ---------------------------------------------------------------------------
// TC5 — Multiplicity (part wheels : Wheel[4];)
// ---------------------------------------------------------------------------

describe("serializer aspect: multiplicity (TC5)", () => {
  it("appends `[4]` after the type via raw.multiplicity", () => {
    const wheel = el({
      id: "wheelDef",
      type: "PartDefinition",
      name: "Wheel",
      ownerId: null,
    });
    const wheels = el({
      id: "wheels",
      type: "PartUsage",
      name: "wheels",
      ownerId: null,
      raw: { typeName: "Wheel", multiplicity: "4" },
    });
    const out = serializeToSysml([wheel, wheels], []);
    expect(out).toContain("part wheels : Wheel[4];");
  });

  it("appends `[mult]` after the name when there is no type", () => {
    const wheels = el({
      id: "wheels",
      type: "PartUsage",
      name: "wheels",
      ownerId: null,
      raw: { multiplicity: "1..*" },
    });
    const out = serializeToSysml([wheels], []);
    expect(out).toContain("part wheels[1..*];");
  });
});

// ---------------------------------------------------------------------------
// TC6 — Redefinition / Subsetting (part y :>> x; / attr :> base)
// ---------------------------------------------------------------------------

describe("serializer aspect: redefinition + subsetting (TC6)", () => {
  it("appends `:>> x` for a Redefinition rel on a usage", () => {
    const base = el({
      id: "base",
      type: "PartUsage",
      name: "x",
      ownerId: null,
    });
    const redef = el({
      id: "y",
      type: "PartUsage",
      name: "y",
      ownerId: null,
    });
    const r = rel({
      id: "rd1",
      type: "Redefinition",
      sourceIds: ["y"],
      targetIds: ["base"],
      raw: {},
    });
    const out = serializeToSysml([base, redef], [r]);
    expect(out).toContain("part y :>> x;");
  });

  it("appends `:> base` for a Subsetting rel on a usage", () => {
    const speed = el({
      id: "speed",
      type: "AttributeUsage",
      name: "speed",
      ownerId: null,
    });
    const fast = el({
      id: "fast",
      type: "AttributeUsage",
      name: "fastSpeed",
      ownerId: null,
    });
    const r = rel({
      id: "ss1",
      type: "Subsetting",
      sourceIds: ["fast"],
      targetIds: ["speed"],
      raw: {},
    });
    const out = serializeToSysml([speed, fast], [r]);
    expect(out).toContain("attribute fastSpeed :> speed;");
  });

  it("orders suffixes as `: Type [mult] :> Super` when combined", () => {
    const wheel = el({
      id: "wheelDef",
      type: "PartDefinition",
      name: "Wheel",
      ownerId: null,
    });
    const base = el({
      id: "base",
      type: "PartUsage",
      name: "base",
      ownerId: null,
    });
    const w = el({
      id: "w",
      type: "PartUsage",
      name: "w",
      ownerId: null,
      raw: { typeName: "Wheel", multiplicity: "4" },
    });
    const sub = rel({
      id: "ss2",
      type: "Subsetting",
      sourceIds: ["w"],
      targetIds: ["base"],
      raw: {},
    });
    const out = serializeToSysml([wheel, base, w], [sub]);
    expect(out).toContain("part w : Wheel[4] :> base;");
  });
});

// ===========================================================================
// ROUND 3 ASPECTS
// ===========================================================================

// ---------------------------------------------------------------------------
// R3-1 — Attribute values (raw.value → ` = <value>`)
// ---------------------------------------------------------------------------

describe("serializer aspect: attribute values (R3-1)", () => {
  it("appends ` = 100` for an untyped attribute value", () => {
    const a = el({
      id: "a",
      type: "AttributeUsage",
      name: "capacity",
      ownerId: null,
      raw: { value: 100 },
    });
    const out = serializeToSysml([a], []);
    expect(out).toContain("attribute capacity = 100;");
  });

  it("appends `: Real = 48.0` for a typed attribute value", () => {
    const a = el({
      id: "a",
      type: "AttributeUsage",
      name: "voltage",
      ownerId: null,
      raw: { typeName: "Real", value: 48.0 },
    });
    const out = serializeToSysml([a], []);
    expect(out).toContain("attribute voltage : Real = 48;");
  });
});

// ---------------------------------------------------------------------------
// R3-2 — Enumerations (literals emitted as bare `<name>;`)
// ---------------------------------------------------------------------------

describe("serializer aspect: enumerations (R3-2)", () => {
  it("emits enum literals as bare names (no `enum` keyword)", () => {
    const e = el({ id: "color", type: "EnumerationDefinition", name: "Color" });
    const red = el({ id: "red", type: "EnumerationUsage", name: "red", ownerId: "color" });
    const green = el({ id: "green", type: "EnumerationUsage", name: "green", ownerId: "color" });
    const blue = el({ id: "blue", type: "EnumerationUsage", name: "blue", ownerId: "color" });
    const out = serializeToSysml([e, red, green, blue], []);
    expect(out).toContain("enum def Color {");
    expect(out).toContain("red;");
    expect(out).toContain("green;");
    expect(out).toContain("blue;");
    // literals are bare — there must be no `enum red;` inside the body
    expect(out).not.toContain("enum red;");
  });
});

// ---------------------------------------------------------------------------
// R3-3 — Constraints (def expression body + assert constraint usage)
// ---------------------------------------------------------------------------

describe("serializer aspect: constraints (R3-3)", () => {
  it("emits a constraint def with an expression body", () => {
    const c = el({
      id: "c",
      type: "ConstraintDefinition",
      name: "C",
      raw: { expression: "capacity > 0" },
    });
    const out = serializeToSysml([c], []);
    expect(out).toContain("constraint def C {");
    expect(out).toContain("capacity > 0");
  });

  it("emits a plain `constraint def C;` when there is no expression", () => {
    const c = el({ id: "c", type: "ConstraintDefinition", name: "C" });
    const out = serializeToSysml([c], []);
    expect(out).toContain("constraint def C;");
  });

  it("emits `assert constraint <name> : <Type>;` for an asserted usage", () => {
    const cu = el({
      id: "cu",
      type: "ConstraintUsage",
      name: "chk",
      raw: { typeName: "C", asserted: true },
    });
    const out = serializeToSysml([cu], []);
    expect(out).toContain("assert constraint chk : C;");
  });

  it("emits a plain `constraint <name> : <Type>;` when not asserted", () => {
    const cu = el({
      id: "cu",
      type: "ConstraintUsage",
      name: "chk",
      raw: { typeName: "C" },
    });
    const out = serializeToSysml([cu], []);
    expect(out).toContain("constraint chk : C;");
    expect(out).not.toContain("assert");
  });
});

// ---------------------------------------------------------------------------
// R3-4 — Typed item flow (`flow of <Type> from <src> to <tgt>;`)
// ---------------------------------------------------------------------------

describe("serializer aspect: typed item flow (R3-4)", () => {
  it("emits `flow of Signal from a to b;` for a payload-typed flow element", () => {
    const proc = el({ id: "proc", type: "ActionDefinition", name: "Proc", ownerId: null });
    const a = el({ id: "a", type: "ActionUsage", name: "a", ownerId: "proc" });
    const b = el({ id: "b", type: "ActionUsage", name: "b", ownerId: "proc" });
    const flow = el({
      id: "fl",
      type: "FlowConnectionUsage",
      name: null,
      ownerId: "proc",
      raw: { sourceEnd: "a", targetEnd: "b", payloadType: "Signal" },
    });
    const out = serializeToSysml([proc, a, b, flow], []);
    expect(out).toContain("flow of Signal from a to b;");
  });
});

// ---------------------------------------------------------------------------
// R3-5 — Interfaces (end members + interface usage connect)
// ---------------------------------------------------------------------------

describe("serializer aspect: interfaces (R3-5)", () => {
  it("emits `end <name> : <Type>;` for end-tagged ports in an interface def", () => {
    const iface = el({ id: "if", type: "InterfaceDefinition", name: "PowerIF" });
    const supply = el({
      id: "supply",
      type: "PortUsage",
      name: "supply",
      ownerId: "if",
      raw: { typeName: "PowerPort", end: true },
    });
    const out = serializeToSysml([iface, supply], []);
    expect(out).toContain("interface def PowerIF {");
    expect(out).toContain("end supply : PowerPort;");
  });

  it("emits `interface <name> connect a.p to b.q;` for an InterfaceUsage", () => {
    const pkg = el({ id: "pkg", type: "Package", name: "Pkg" });
    const src = el({ id: "src", type: "PartUsage", name: "src", ownerId: "pkg" });
    const snk = el({ id: "snk", type: "PartUsage", name: "snk", ownerId: "pkg" });
    const p = el({ id: "p", type: "PortUsage", name: "p", ownerId: "src" });
    const q = el({ id: "q", type: "PortUsage", name: "q", ownerId: "snk" });
    const iface = el({
      id: "link",
      type: "InterfaceUsage",
      name: "link",
      ownerId: "pkg",
      raw: { sourceEnd: "p", targetEnd: "q" },
    });
    const out = serializeToSysml([pkg, src, snk, p, q, iface], []);
    expect(out).toContain("interface link connect src.p to snk.q;");
  });
});

// ---------------------------------------------------------------------------
// R3-6 — Use cases + actors + include
// ---------------------------------------------------------------------------

describe("serializer aspect: use cases + actors (R3-6)", () => {
  it("emits `actor <name> : <Type>;` for an actor-tagged usage in a use case def", () => {
    const uc = el({ id: "uc", type: "UseCaseDefinition", name: "Refuel" });
    const actor = el({
      id: "op",
      type: "PartUsage",
      name: "operator",
      ownerId: "uc",
      raw: { typeName: "Pilot", actor: true },
    });
    const out = serializeToSysml([uc, actor], []);
    expect(out).toContain("use case def Refuel {");
    expect(out).toContain("actor operator : Pilot;");
  });

  it("emits `include use case <name>;` for an IncludeUseCase relationship", () => {
    const uc = el({ id: "uc", type: "UseCaseDefinition", name: "Refuel" });
    const auth = el({ id: "auth", type: "UseCaseDefinition", name: "Authenticate" });
    const inc: SysmlRelationship = {
      id: "inc1",
      type: "IncludeUseCase",
      sourceIds: ["uc"],
      targetIds: ["auth"],
      raw: {},
    };
    const out = serializeToSysml([uc, auth], [inc]);
    expect(out).toContain("include use case Authenticate;");
  });
});

// ---------------------------------------------------------------------------
// R3-7 — Standard-library import for scalar types (TF-10)
// ---------------------------------------------------------------------------

describe("serializer aspect: ScalarValues import (TF-10)", () => {
  it("emits `import ScalarValues::*;` at the TOP when a scalar type is referenced", () => {
    const part = el({ id: "p", type: "PartDefinition", name: "Battery" });
    const v = el({
      id: "v",
      type: "AttributeUsage",
      name: "voltage",
      ownerId: "p",
      raw: { typeName: "Real", value: "48.0" },
    });
    const out = serializeToSysml([part, v], []);
    // Must be the FIRST line of output.
    expect(out.startsWith("import ScalarValues::*;\n")).toBe(true);
    // exactly one import line
    expect(out.match(/import ScalarValues::\*;/g)?.length).toBe(1);
  });

  it("does NOT emit the import when only user-defined types are referenced", () => {
    const wheelDef = el({ id: "wd", type: "PartDefinition", name: "Wheel" });
    const wheels = el({
      id: "w",
      type: "PartUsage",
      name: "wheels",
      ownerId: null,
      raw: { typeName: "Wheel" },
    });
    const out = serializeToSysml([wheelDef, wheels], []);
    expect(out).not.toContain("import ScalarValues");
  });

  it("detects scalar types other than Real (Integer, Boolean, ...)", () => {
    const a = el({
      id: "a",
      type: "AttributeUsage",
      name: "count",
      ownerId: null,
      raw: { typeName: "Integer", value: "3" },
    });
    const out = serializeToSysml([a], []);
    expect(out.startsWith("import ScalarValues::*;\n")).toBe(true);
  });
});
