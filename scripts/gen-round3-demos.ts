// ---------------------------------------------------------------------------
// Round-3 demo generator: attribute values, enumerations, constraints, typed
// item flow, interfaces (ends + interface connect), use cases (actor + include).
//
// Constructs SysmlElement[] / SysmlRelationship[] and writes grammar-conformant
// .sysml using the CURRENT serializer source. Each output must pass
// tools/sysml-validator/run.sh with 0 errors.
//
//   round3-data.sysml          — typed attr values + enum def + constraint def + assert
//   round3-interfaces.sysml    — port defs, interface def with ends, interface connect
//   round3-flows-usecase.sysml — typed `flow of <Item>` + use case def + actor + include
//   round3-kitchen-sink.sysml  — ALL round-3 aspects combined (Cameo demo)
// ---------------------------------------------------------------------------

import * as fs from "node:fs";
import * as path from "node:path";
import { serializeToSysml } from "../packages/mcp-server/src/utils/sysml-serializer.js";
import type {
  SysmlElement,
  SysmlRelationship,
} from "../packages/mcp-server/src/types/sysml-elements.js";

const OUT_DIR = "/tmp/rubric-anchored-recursion/tool-validation/subsystems";

function E(o: Partial<SysmlElement>): SysmlElement {
  return {
    id: o.id!,
    elementId: o.id!,
    type: o.type ?? "PartDefinition",
    name: o.name ?? null,
    shortName: o.shortName ?? null,
    qualifiedName: null,
    ownerId: o.ownerId ?? null,
    ownedElementIds: [],
    raw: o.raw ?? {},
  };
}

function R(o: Partial<SysmlRelationship>): SysmlRelationship {
  return {
    id: o.id!,
    type: o.type!,
    sourceIds: o.sourceIds ?? [],
    targetIds: o.targetIds ?? [],
    raw: o.raw ?? {},
  };
}

function write(file: string, text: string): void {
  const p = path.join(OUT_DIR, file);
  fs.writeFileSync(p, text, "utf8");
  console.log(`wrote ${p} (${text.split("\n").length} lines)`);
}

// ---------------------------------------------------------------------------
// 1. round3-data.sysml — attribute values + enum + constraint + assert
// ---------------------------------------------------------------------------
function dataDemo(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "DataDemo" }),

    // enum def with bare literals
    E({ id: "color", type: "EnumerationDefinition", name: "CellChemistry", ownerId: "pkg" }),
    E({ id: "liion", type: "EnumerationUsage", name: "LiIon", ownerId: "color" }),
    E({ id: "nimh", type: "EnumerationUsage", name: "NiMH", ownerId: "color" }),
    E({ id: "leadacid", type: "EnumerationUsage", name: "LeadAcid", ownerId: "color" }),

    // constraint def with a BOUND parameter (TF-11): `in capacity : Real;`
    // makes the constraint resolvable in Cameo.
    E({
      id: "cdef",
      type: "ConstraintDefinition",
      name: "CapacityPositive",
      ownerId: "pkg",
      raw: { expression: "in capacity : Real; capacity > 0" },
    }),

    // part def with typed attribute values (capacity typed Real so it binds
    // cleanly to the constraint parameter)
    E({ id: "batDef", type: "PartDefinition", name: "Battery", ownerId: "pkg" }),
    E({
      id: "capacity",
      type: "AttributeUsage",
      name: "capacity",
      ownerId: "batDef",
      raw: { typeName: "Real", value: "100" },
    }),
    E({
      id: "voltage",
      type: "AttributeUsage",
      name: "voltage",
      ownerId: "batDef",
      raw: { typeName: "Real", value: "48.0" },
    }),
    // asserted constraint usage referencing the constraint def
    E({
      id: "chk",
      type: "ConstraintUsage",
      name: "capacityCheck",
      ownerId: "batDef",
      raw: { typeName: "CapacityPositive", asserted: true },
    }),
  ];
  return serializeToSysml(els, []);
}

// ---------------------------------------------------------------------------
// 2. round3-interfaces.sysml — port defs, interface def with ends, connect
// ---------------------------------------------------------------------------
function interfacesDemo(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "Interfaces" }),

    E({ id: "ppDef", type: "PortDefinition", name: "PowerPort", ownerId: "pkg" }),

    // interface def with end members
    E({ id: "ifDef", type: "InterfaceDefinition", name: "PowerInterface", ownerId: "pkg" }),
    E({
      id: "supply",
      type: "PortUsage",
      name: "supply",
      ownerId: "ifDef",
      raw: { typeName: "PowerPort", end: true },
    }),
    E({
      id: "demand",
      type: "PortUsage",
      name: "demand",
      ownerId: "ifDef",
      raw: { typeName: "PowerPort", end: true },
    }),

    // two parts with ports
    E({ id: "srcDef", type: "PartDefinition", name: "Source", ownerId: "pkg" }),
    E({ id: "srcP", type: "PortUsage", name: "p", ownerId: "srcDef", raw: { typeName: "PowerPort" } }),
    E({ id: "snkDef", type: "PartDefinition", name: "Sink", ownerId: "pkg" }),
    E({ id: "snkQ", type: "PortUsage", name: "q", ownerId: "snkDef", raw: { typeName: "PowerPort" } }),

    E({ id: "src", type: "PartUsage", name: "src", ownerId: "pkg", raw: { typeName: "Source" } }),
    E({ id: "uSrcP", type: "PortUsage", name: "p", ownerId: "src", raw: { typeName: "PowerPort" } }),
    E({ id: "snk", type: "PartUsage", name: "snk", ownerId: "pkg", raw: { typeName: "Sink" } }),
    E({ id: "uSnkQ", type: "PortUsage", name: "q", ownerId: "snk", raw: { typeName: "PowerPort" } }),

    // interface usage connecting the two ports (typed by PowerInterface)
    E({
      id: "powerLink",
      type: "InterfaceUsage",
      name: "powerLink",
      ownerId: "pkg",
      raw: { typeName: "PowerInterface", sourceEnd: "uSrcP", targetEnd: "uSnkQ" },
    }),
  ];
  return serializeToSysml(els, []);
}

// ---------------------------------------------------------------------------
// 3. round3-flows-usecase.sysml — typed flow + use case + actor + include
// ---------------------------------------------------------------------------
function flowsUseCaseDemo(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "FlowsUseCase" }),

    // item def used as the flow payload type
    E({ id: "sigDef", type: "ItemDefinition", name: "Signal", ownerId: "pkg" }),

    // two parts with ports + a typed item flow between them. NOTE: `in` / `out`
    // are reserved (flow-direction) keywords, so the ports are named outPort /
    // inPort.
    E({ id: "aDef", type: "PartDefinition", name: "Producer", ownerId: "pkg" }),
    E({ id: "aOut", type: "PortUsage", name: "outPort", ownerId: "aDef" }),
    E({ id: "bDef", type: "PartDefinition", name: "Consumer", ownerId: "pkg" }),
    E({ id: "bIn", type: "PortUsage", name: "inPort", ownerId: "bDef" }),
    E({ id: "a", type: "PartUsage", name: "a", ownerId: "pkg", raw: { typeName: "Producer" } }),
    E({ id: "uAOut", type: "PortUsage", name: "outPort", ownerId: "a" }),
    E({ id: "b", type: "PartUsage", name: "b", ownerId: "pkg", raw: { typeName: "Consumer" } }),
    E({ id: "uBIn", type: "PortUsage", name: "inPort", ownerId: "b" }),
    // typed item flow: `flow of Signal from a.out to b.in;`
    E({
      id: "fl",
      type: "FlowConnectionUsage",
      name: null,
      ownerId: "pkg",
      raw: { sourceEnd: "uAOut", targetEnd: "uBIn", payloadType: "Signal" },
    }),

    // use case def with an actor + an included use case
    E({ id: "pilotDef", type: "PartDefinition", name: "Pilot", ownerId: "pkg" }),
    E({ id: "authUC", type: "UseCaseDefinition", name: "Authenticate", ownerId: "pkg" }),
    E({ id: "refuelUC", type: "UseCaseDefinition", name: "Refuel", ownerId: "pkg" }),
    E({
      id: "operator",
      type: "PartUsage",
      name: "operator",
      ownerId: "refuelUC",
      raw: { typeName: "Pilot", actor: true },
    }),
  ];
  const rels: SysmlRelationship[] = [
    R({ id: "inc1", type: "IncludeUseCase", sourceIds: ["refuelUC"], targetIds: ["authUC"] }),
  ];
  return serializeToSysml(els, rels);
}

// ---------------------------------------------------------------------------
// 4. round3-kitchen-sink.sysml — ALL round-3 aspects combined
// ---------------------------------------------------------------------------
function kitchenSink(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "Round3KitchenSink" }),

    // --- enumeration ---
    E({ id: "chem", type: "EnumerationDefinition", name: "CellChemistry", ownerId: "pkg" }),
    E({ id: "liion", type: "EnumerationUsage", name: "LiIon", ownerId: "chem" }),
    E({ id: "nimh", type: "EnumerationUsage", name: "NiMH", ownerId: "chem" }),

    // --- constraint def with a BOUND parameter (TF-11) ---
    E({
      id: "cdef",
      type: "ConstraintDefinition",
      name: "CapacityPositive",
      ownerId: "pkg",
      raw: { expression: "in capacity : Real; capacity > 0" },
    }),

    // --- item def used as flow payload ---
    E({ id: "sigDef", type: "ItemDefinition", name: "PowerSignal", ownerId: "pkg" }),

    // --- port def + interface def with ends ---
    E({ id: "ppDef", type: "PortDefinition", name: "PowerPort", ownerId: "pkg" }),
    E({ id: "ifDef", type: "InterfaceDefinition", name: "PowerInterface", ownerId: "pkg" }),
    E({
      id: "supply",
      type: "PortUsage",
      name: "supply",
      ownerId: "ifDef",
      raw: { typeName: "PowerPort", end: true },
    }),
    E({
      id: "demand",
      type: "PortUsage",
      name: "demand",
      ownerId: "ifDef",
      raw: { typeName: "PowerPort", end: true },
    }),

    // --- part def with typed attribute values + asserted constraint ---
    E({ id: "batDef", type: "PartDefinition", name: "Battery", ownerId: "pkg" }),
    E({
      id: "capacity",
      type: "AttributeUsage",
      name: "capacity",
      ownerId: "batDef",
      raw: { typeName: "Real", value: "100" },
    }),
    E({
      id: "voltage",
      type: "AttributeUsage",
      name: "voltage",
      ownerId: "batDef",
      raw: { typeName: "Real", value: "48.0" },
    }),
    E({
      id: "chk",
      type: "ConstraintUsage",
      name: "capacityCheck",
      ownerId: "batDef",
      raw: { typeName: "CapacityPositive", asserted: true },
    }),
    E({ id: "batOut", type: "PortUsage", name: "powerOut", ownerId: "batDef", raw: { typeName: "PowerPort" } }),

    E({ id: "loadDef", type: "PartDefinition", name: "Load", ownerId: "pkg" }),
    E({ id: "loadIn", type: "PortUsage", name: "powerIn", ownerId: "loadDef", raw: { typeName: "PowerPort" } }),

    // --- part usages + ports for interface connect + typed flow ---
    E({ id: "battery", type: "PartUsage", name: "battery", ownerId: "pkg", raw: { typeName: "Battery" } }),
    E({ id: "uBatOut", type: "PortUsage", name: "powerOut", ownerId: "battery", raw: { typeName: "PowerPort" } }),
    E({ id: "load", type: "PartUsage", name: "load", ownerId: "pkg", raw: { typeName: "Load" } }),
    E({ id: "uLoadIn", type: "PortUsage", name: "powerIn", ownerId: "load", raw: { typeName: "PowerPort" } }),

    // interface usage (typed) connecting the two ports
    E({
      id: "powerLink",
      type: "InterfaceUsage",
      name: "powerLink",
      ownerId: "pkg",
      raw: { typeName: "PowerInterface", sourceEnd: "uBatOut", targetEnd: "uLoadIn" },
    }),
    // typed item flow between the same ports
    E({
      id: "fl",
      type: "FlowConnectionUsage",
      name: null,
      ownerId: "pkg",
      raw: { sourceEnd: "uBatOut", targetEnd: "uLoadIn", payloadType: "PowerSignal" },
    }),

    // --- use case def with actor + include ---
    E({ id: "pilotDef", type: "PartDefinition", name: "Operator", ownerId: "pkg" }),
    E({ id: "authUC", type: "UseCaseDefinition", name: "Authenticate", ownerId: "pkg" }),
    E({ id: "refuelUC", type: "UseCaseDefinition", name: "OperatePowerSystem", ownerId: "pkg" }),
    E({
      id: "operator",
      type: "PartUsage",
      name: "operator",
      ownerId: "refuelUC",
      raw: { typeName: "Operator", actor: true },
    }),
  ];

  const rels: SysmlRelationship[] = [
    R({ id: "inc1", type: "IncludeUseCase", sourceIds: ["refuelUC"], targetIds: ["authUC"] }),
  ];

  return serializeToSysml(els, rels);
}

write("round3-data.sysml", dataDemo());
write("round3-interfaces.sysml", interfacesDemo());
write("round3-flows-usecase.sysml", flowsUseCaseDemo());
write("round3-kitchen-sink.sysml", kitchenSink());
console.log("done");
