// ---------------------------------------------------------------------------
// Demo generator for the extended serializer aspects.
//
// Constructs SysmlElement[] / SysmlRelationship[] for five demo subsystems and
// writes grammar-conformant .sysml using the CURRENT serializer source. Each
// output must pass tools/sysml-validator/run.sh with 0 errors.
//
//   structural-ibd.sysml      — power system: part defs + ports + connections
//   behavioral-activity.sysml — action def + sub-actions + first..then + flow
//   state-machine.sysml       — state def + states + transition first..then
//   crosscutting.sysml        — specialization (:>), multiplicity ([n]),
//                               redefinition (:>>)
//   kitchen-sink.sysml        — ONE combined model with ALL aspects (Cameo demo)
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
// 1. structural-ibd.sysml — power system IBD
// ---------------------------------------------------------------------------
function structuralIbd(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "PowerSystem" }),
    // part defs with ports
    E({ id: "batDef", type: "PartDefinition", name: "Battery", ownerId: "pkg" }),
    E({ id: "batPort", type: "PortUsage", name: "dcOut", ownerId: "batDef" }),
    E({ id: "invDef", type: "PartDefinition", name: "Inverter", ownerId: "pkg" }),
    E({ id: "invIn", type: "PortUsage", name: "dcIn", ownerId: "invDef" }),
    E({ id: "invOut", type: "PortUsage", name: "acOut", ownerId: "invDef" }),
    E({ id: "loadDef", type: "PartDefinition", name: "Load", ownerId: "pkg" }),
    E({ id: "loadIn", type: "PortUsage", name: "acIn", ownerId: "loadDef" }),
    // part usages (the IBD instances) with their own ports via typing
    E({
      id: "battery",
      type: "PartUsage",
      name: "battery",
      ownerId: "pkg",
      raw: { typeName: "Battery" },
    }),
    E({ id: "uBatOut", type: "PortUsage", name: "dcOut", ownerId: "battery" }),
    E({
      id: "inverter",
      type: "PartUsage",
      name: "inverter",
      ownerId: "pkg",
      raw: { typeName: "Inverter" },
    }),
    E({ id: "uInvIn", type: "PortUsage", name: "dcIn", ownerId: "inverter" }),
    E({ id: "uInvOut", type: "PortUsage", name: "acOut", ownerId: "inverter" }),
    E({
      id: "load",
      type: "PartUsage",
      name: "load",
      ownerId: "pkg",
      raw: { typeName: "Load" },
    }),
    E({ id: "uLoadIn", type: "PortUsage", name: "acIn", ownerId: "load" }),
  ];
  const rels: SysmlRelationship[] = [
    // connections between ports on sub-parts -> qualified refs, common owner pkg
    R({ id: "c1", type: "Connector", sourceIds: ["uBatOut"], targetIds: ["uInvIn"] }),
    R({ id: "c2", type: "Connector", sourceIds: ["uInvOut"], targetIds: ["uLoadIn"] }),
  ];
  return serializeToSysml(els, rels);
}

// ---------------------------------------------------------------------------
// 2. behavioral-activity.sysml — action def with first..then + flow
// ---------------------------------------------------------------------------
function behavioralActivity(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "Behavior" }),
    E({ id: "proc", type: "ActionDefinition", name: "ProcessSignal", ownerId: "pkg" }),
    E({ id: "acquire", type: "ActionUsage", name: "acquire", ownerId: "proc" }),
    E({ id: "filter", type: "ActionUsage", name: "filterStep", ownerId: "proc" }),
    E({ id: "publish", type: "ActionUsage", name: "publish", ownerId: "proc" }),
    // flow element (FlowConnectionUsage) with sourceEnd/targetEnd
    E({
      id: "fl1",
      type: "FlowConnectionUsage",
      name: null,
      ownerId: "proc",
      raw: { sourceEnd: "acquire", targetEnd: "filter" },
    }),
  ];
  const rels: SysmlRelationship[] = [
    R({ id: "s1", type: "Succession", sourceIds: ["acquire"], targetIds: ["filter"] }),
    R({ id: "s2", type: "Succession", sourceIds: ["filter"], targetIds: ["publish"] }),
  ];
  return serializeToSysml(els, rels);
}

// ---------------------------------------------------------------------------
// 3. state-machine.sysml — state def with transition first..then
// ---------------------------------------------------------------------------
function stateMachine(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "Modes" }),
    E({ id: "sm", type: "StateDefinition", name: "OperatingModes", ownerId: "pkg" }),
    E({ id: "off", type: "StateUsage", name: "off", ownerId: "sm" }),
    E({ id: "standby", type: "StateUsage", name: "standby", ownerId: "sm" }),
    E({ id: "active", type: "StateUsage", name: "active", ownerId: "sm" }),
    E({
      id: "t1",
      type: "TransitionUsage",
      name: null,
      ownerId: "sm",
      raw: { sourceEnd: "off", targetEnd: "standby" },
    }),
    E({
      id: "t2",
      type: "TransitionUsage",
      name: null,
      ownerId: "sm",
      raw: { sourceEnd: "standby", targetEnd: "active" },
    }),
  ];
  return serializeToSysml(els, []);
}

// ---------------------------------------------------------------------------
// 4. crosscutting.sysml — specialization + multiplicity + redefinition
// ---------------------------------------------------------------------------
function crosscutting(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "CrossCutting" }),
    E({ id: "vehicle", type: "PartDefinition", name: "Vehicle", ownerId: "pkg" }),
    E({ id: "wheelDef", type: "PartDefinition", name: "Wheel", ownerId: "pkg" }),
    // specialization: Car :> Vehicle
    E({ id: "car", type: "PartDefinition", name: "Car", ownerId: "pkg" }),
    // multiplicity: part wheels : Wheel[4]
    E({
      id: "wheels",
      type: "PartUsage",
      name: "wheels",
      ownerId: "car",
      raw: { typeName: "Wheel", multiplicity: "4" },
    }),
    // base def for redefinition
    E({ id: "baseDef", type: "PartDefinition", name: "BaseAssembly", ownerId: "pkg" }),
    E({ id: "baseX", type: "PartUsage", name: "x", ownerId: "baseDef" }),
    // derived def specializes base, redefines x via y
    E({ id: "derived", type: "PartDefinition", name: "DerivedAssembly", ownerId: "pkg" }),
    E({ id: "derivedY", type: "PartUsage", name: "y", ownerId: "derived" }),
  ];
  const rels: SysmlRelationship[] = [
    R({ id: "sp1", type: "Specialization", sourceIds: ["car"], targetIds: ["vehicle"] }),
    R({ id: "sp2", type: "Specialization", sourceIds: ["derived"], targetIds: ["baseDef"] }),
    R({ id: "rd1", type: "Redefinition", sourceIds: ["derivedY"], targetIds: ["baseX"] }),
  ];
  return serializeToSysml(els, rels);
}

// ---------------------------------------------------------------------------
// 5. kitchen-sink.sysml — ALL aspects combined (Cameo demo)
// ---------------------------------------------------------------------------
function kitchenSink(): string {
  const els: SysmlElement[] = [
    E({ id: "pkg", type: "Package", name: "KitchenSink" }),

    // --- structural: defs + ports ---
    E({ id: "batDef", type: "PartDefinition", name: "Battery", ownerId: "pkg" }),
    E({ id: "invDef", type: "PartDefinition", name: "Inverter", ownerId: "pkg" }),

    // --- crosscutting: specialization of a DEFINITION (Battery :> PowerSource) ---
    E({ id: "pwrDef", type: "PartDefinition", name: "PowerSource", ownerId: "pkg" }),
    // Battery :> PowerSource handled via relationship below (def -> def, valid)

    // --- structural usages with ports + multiplicity ---
    E({
      id: "battery",
      type: "PartUsage",
      name: "battery",
      ownerId: "pkg",
      raw: { typeName: "Battery" },
    }),
    E({ id: "uBatOut", type: "PortUsage", name: "dcOut", ownerId: "battery" }),
    E({
      id: "inverter",
      type: "PartUsage",
      name: "inverter",
      ownerId: "pkg",
      raw: { typeName: "Inverter" },
    }),
    E({ id: "uInvIn", type: "PortUsage", name: "dcIn", ownerId: "inverter" }),
    // multiplicity usage
    E({ id: "cellDef", type: "PartDefinition", name: "Cell", ownerId: "pkg" }),
    E({
      id: "cells",
      type: "PartUsage",
      name: "cells",
      ownerId: "battery",
      raw: { typeName: "Cell", multiplicity: "8" },
    }),

    // --- crosscutting: SUBSETTING on a usage (usage -> usage, `:>`) ---
    // part def Pack { part primaryCell : Cell; part backupCell :> primaryCell; }
    E({ id: "packDef", type: "PartDefinition", name: "Pack", ownerId: "pkg" }),
    E({
      id: "primaryCell",
      type: "PartUsage",
      name: "primaryCell",
      ownerId: "packDef",
      raw: { typeName: "Cell" },
    }),
    E({
      id: "backupCell",
      type: "PartUsage",
      name: "backupCell",
      ownerId: "packDef",
    }),

    // --- behavioral: action def + sub-actions + flow ---
    E({ id: "proc", type: "ActionDefinition", name: "ConvertPower", ownerId: "pkg" }),
    E({ id: "sense", type: "ActionUsage", name: "sense", ownerId: "proc" }),
    E({ id: "convert", type: "ActionUsage", name: "convert", ownerId: "proc" }),
    E({ id: "deliver", type: "ActionUsage", name: "deliver", ownerId: "proc" }),
    E({
      id: "fl1",
      type: "FlowConnectionUsage",
      name: null,
      ownerId: "proc",
      raw: { sourceEnd: "sense", targetEnd: "convert" },
    }),

    // --- state machine: state def + transitions ---
    E({ id: "sm", type: "StateDefinition", name: "ConverterModes", ownerId: "pkg" }),
    E({ id: "idle", type: "StateUsage", name: "idle", ownerId: "sm" }),
    E({ id: "running", type: "StateUsage", name: "running", ownerId: "sm" }),
    E({
      id: "tr1",
      type: "TransitionUsage",
      name: null,
      ownerId: "sm",
      raw: { sourceEnd: "idle", targetEnd: "running" },
    }),

    // --- crosscutting: redefinition on a usage ---
    E({ id: "baseDef", type: "PartDefinition", name: "BaseModule", ownerId: "pkg" }),
    E({ id: "baseX", type: "PartUsage", name: "x", ownerId: "baseDef" }),
    E({ id: "derived", type: "PartDefinition", name: "DerivedModule", ownerId: "pkg" }),
    E({ id: "derivedY", type: "PartUsage", name: "y", ownerId: "derived" }),
  ];

  const rels: SysmlRelationship[] = [
    // connection between ports on sub-parts (qualified refs, owner = pkg)
    R({ id: "c1", type: "Connector", sourceIds: ["uBatOut"], targetIds: ["uInvIn"] }),
    // succession in the action body
    R({ id: "s1", type: "Succession", sourceIds: ["sense"], targetIds: ["convert"] }),
    R({ id: "s2", type: "Succession", sourceIds: ["convert"], targetIds: ["deliver"] }),
    // specialization on a DEFINITION (def -> def). Battery :> PowerSource.
    R({ id: "sp1", type: "Specialization", sourceIds: ["batDef"], targetIds: ["pwrDef"] }),
    R({ id: "sp2", type: "Specialization", sourceIds: ["derived"], targetIds: ["baseDef"] }),
    // subsetting on a USAGE (usage -> usage). backupCell :> primaryCell.
    R({ id: "ss1", type: "Subsetting", sourceIds: ["backupCell"], targetIds: ["primaryCell"] }),
    // redefinition on a usage. y :>> x.
    R({ id: "rd1", type: "Redefinition", sourceIds: ["derivedY"], targetIds: ["baseX"] }),
  ];

  return serializeToSysml(els, rels);
}

write("structural-ibd.sysml", structuralIbd());
write("behavioral-activity.sysml", behavioralActivity());
write("state-machine.sysml", stateMachine());
write("crosscutting.sysml", crosscutting());
write("kitchen-sink.sysml", kitchenSink());
console.log("done");
