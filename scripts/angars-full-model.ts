// ---------------------------------------------------------------------------
// ANGARS C&C — FULL all-pillars demo model (single source of truth).
//
// Builds one in-memory SysML v2 model spanning ALL THREE pillars + cross-pillar
// traceability, grounded in the REAL ANGARS C&C content (the same 6 components,
// 15 functions, 34 requirements/needs, and the satisfy/allocate/derive/verify
// web as the corpus-derived model) — then ADDS the structural pillar (ports +
// IBD connections) and behavioral pillar (action flow + state machine) that the
// requirements corpus does not contain.
//
// Two emitters consume this one model:
//   - serializeToSysml(...)  -> examples/angars/model/cc-subsystem-full.sysml
//                               (validated; fed to DeciSym's export_figures)
//   - scripts/angars-to-reactflow.ts maps the same elements/relationships ->
//     sysml-reactflow factory specs.
//
// No Cameo 500-element cap applies to either target, so this is the FULL
// def/usage model (not the lean projection).
// ---------------------------------------------------------------------------

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { serializeToSysml } from "../packages/mcp-server/src/utils/sysml-serializer.js";
import type {
  SysmlElement,
  SysmlRelationship,
} from "../packages/mcp-server/src/types/sysml-elements.js";

// ---- tiny builders --------------------------------------------------------

let counter = 0;
const rid = () => `r${++counter}`;

function el(over: Partial<SysmlElement> & { id: string; type: string }): SysmlElement {
  return {
    elementId: over.id,
    name: null,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
    ...over,
  } as SysmlElement;
}

function rel(
  type: string,
  sourceIds: string[],
  targetIds: string[],
  raw: Record<string, unknown> = {}
): SysmlRelationship {
  return { id: rid(), type, sourceIds, targetIds, raw } as SysmlRelationship;
}

function slug(s: string): string {
  return s.replace(/[^a-zA-Z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "x";
}

// ---- the model ------------------------------------------------------------

export interface AngarsModel {
  elements: SysmlElement[];
  relationships: SysmlRelationship[];
}

export function buildAngarsModel(): AngarsModel {
  counter = 0;
  const E: SysmlElement[] = [];
  const R: SysmlRelationship[] = [];

  // === REQUIREMENTS PILLAR ================================================
  // Needs (stakeholder needs) + requirements, both as RequirementUsage with a
  // short name (the ANGARS / N id).
  const needs = ["N1", "N2", "N7", "N12", "N15", "N16"];
  const requirements: Array<[string, string]> = [
    ["ANGARS-4", "Aircraft ID Verification"],
    ["ANGARS-10", "Multi-Platform Refueling"],
    ["ANGARS-14", "Fuel Capacity Check"],
    ["ANGARS-62", "Mission Success Rate"],
    ["ANGARS-67", "Post-Mission Reports"],
    ["ANGARS-103", "HMI Mission Display"],
    ["ANGARS-104", "Manual Override Speed"],
    ["ANGARS-105", "Emergency Shutdown"],
    ["ANGARS-106", "Tactile Alerts"],
    ["ANGARS-107", "Voice Command Support"],
    ["ANGARS-108", "HMI Refresh Rate"],
    ["ANGARS-109", "Subsystem Health Display"],
    ["ANGARS-110", "Multilingual HMI"],
    ["ANGARS-111", "Operator Action Logging"],
    ["ANGARS-112", "Haptic Emergency Alerts"],
    ["ANGARS-113", "HMI Update Speed"],
    ["ANGARS-114", "Ground Dashboard Integration"],
    ["ANGARS-115", "Critical Control Lockdown"],
    ["ANGARS-116", "Manual Override Instructions"],
    ["ANGARS-117", "Refueling Queue Reprioritization"],
    ["ANGARS-141", "Fuel Schedule Optimization"],
    ["ANGARS-147", "Low-Fuel Priority"],
    ["ANGARS-149", "Refueling Request Processing"],
    ["ANGARS-150", "Refueling Priority Logic"],
    ["ANGARS-151", "Schedule Generation Speed"],
    ["ANGARS-152", "Dynamic Schedule Updates"],
    ["ANGARS-153", "Post-Mission Reporting"],
    ["ANGARS-154", "Mission Status Updates"],
  ];

  const reqId = new Map<string, string>(); // name -> element id
  for (const n of needs) {
    const id = `need_${n}`;
    E.push(el({ id, type: "RequirementUsage", name: n, shortName: n, raw: { provenanceSourceId: n } }));
    reqId.set(n, id);
  }
  for (const [short, name] of requirements) {
    const id = `req_${slug(short)}`;
    E.push(el({ id, type: "RequirementUsage", name, shortName: short, raw: { provenanceSourceId: short } }));
    reqId.set(name, id);
  }

  // Verification cases (each owns an objective { verify <req>; } body, emitted
  // by the serializer from VerifyRequirementUsage relationships).
  const verifications: Array<[string, string[]]> = [
    ["Verify_Demonstration", [
      "Aircraft ID Verification", "Fuel Capacity Check", "HMI Mission Display",
      "Manual Override Speed", "Multilingual HMI", "Haptic Emergency Alerts",
      "Ground Dashboard Integration", "Refueling Queue Reprioritization",
      "Schedule Generation Speed", "Post-Mission Reporting", "Mission Status Updates",
    ]],
    ["Verify_Test", [
      "Multi-Platform Refueling", "Emergency Shutdown", "Tactile Alerts",
      "Voice Command Support", "HMI Refresh Rate", "HMI Update Speed",
      "Critical Control Lockdown", "Refueling Request Processing", "Dynamic Schedule Updates",
    ]],
    ["Verify_Analysis", [
      "Mission Success Rate", "Manual Override Instructions", "Fuel Schedule Optimization",
      "Low-Fuel Priority", "Refueling Priority Logic",
    ]],
    ["Verify_Inspection", [
      "Post-Mission Reports", "Subsystem Health Display", "Operator Action Logging",
    ]],
  ];
  for (const [vname, reqs] of verifications) {
    const vid = `ver_${slug(vname)}`;
    E.push(el({ id: vid, type: "VerificationCaseDefinition", name: vname }));
    for (const rq of reqs) {
      R.push(rel("VerifyRequirementUsage", [vid], [reqId.get(rq)!]));
    }
  }

  // === STRUCTURAL PILLAR ==================================================
  // A `C&C Architecture` package holds: component part DEFS (with attributes)
  // that are the BDD nodes; a `C&C Subsystem` block whose component USAGES are
  // TYPED by those defs (so composition subsystem ◆-> component renders in the
  // BDD) and carry ports + connections for the IBD. The usages are the
  // R4-correct trace operands for `allocate`.
  const pkgId = "pkg_arch";
  E.push(el({ id: pkgId, type: "Package", name: "C&C Architecture" }));
  for (const pt of ["PowerPort", "DataPort", "CtrlPort"]) {
    E.push(el({ id: `portdef_${pt}`, type: "PortDefinition", name: pt, ownerId: pkgId }));
  }
  // Component DEFINITIONS (BDD nodes) with attributes.
  const componentDefs: Array<[string, Array<[string, string]>]> = [
    ["C&C Power Module", [["voltage", "Real"], ["capacity", "Real"]]],
    ["Flight Control Module", [["clockRate", "Real"]]],
    ["Operator Control Plane", [["throughput", "Real"]]],
    ["Operator Console Module", [["refreshRate", "Real"]]],
    ["HMI Panel & Displays", [["resolution", "Real"]]],
    ["Haptic Alert Unit", [["latency", "Real"]]],
  ];
  for (const [comp, attrs] of componentDefs) {
    const did = `compdef_${slug(comp)}`;
    E.push(el({ id: did, type: "PartDefinition", name: comp, ownerId: pkgId }));
    for (const [an, at] of attrs) {
      E.push(el({ id: `${did}_${an}`, type: "AttributeUsage", name: an, ownerId: did, raw: { typeName: at } }));
    }
  }
  const subsysId = "sub_cc";
  E.push(el({ id: subsysId, type: "PartDefinition", name: "C&C Subsystem", ownerId: pkgId }));

  const compId = new Map<string, string>(); // component name -> USAGE id (trace operand)
  const portId = new Map<string, string>(); // `${comp}|${port}` -> port id
  const componentPorts: Array<[string, Array<[string, string]>]> = [
    ["C&C Power Module", [["pwrOut", "PowerPort"], ["telemOut", "DataPort"]]],
    ["Flight Control Module", [["pwrIn", "PowerPort"], ["navOut", "DataPort"]]],
    ["Operator Control Plane", [["pwrIn", "PowerPort"], ["telemIn", "DataPort"], ["cmdOut", "CtrlPort"]]],
    ["Operator Console Module", [["cmdIn", "CtrlPort"], ["uiOut", "DataPort"]]],
    ["HMI Panel & Displays", [["uiIn", "DataPort"]]],
    ["Haptic Alert Unit", [["alertIn", "DataPort"]]],
  ];
  for (const [comp, ports] of componentPorts) {
    const cid = `comp_${slug(comp)}`;
    // typed usage: `part 'C&C Power Module' : 'C&C Power Module'` (typed by its def)
    E.push(el({ id: cid, type: "PartUsage", name: comp, ownerId: subsysId, raw: { typeName: comp } }));
    compId.set(comp, cid);
    for (const [pn, pt] of ports) {
      const pid = `${cid}_${pn}`;
      E.push(el({ id: pid, type: "PortUsage", name: pn, ownerId: cid, raw: { typeName: pt } }));
      portId.set(`${comp}|${pn}`, pid);
    }
  }
  const connections: Array<[string, string, string, string]> = [
    ["C&C Power Module", "pwrOut", "Flight Control Module", "pwrIn"],
    ["C&C Power Module", "pwrOut", "Operator Control Plane", "pwrIn"],
    ["Flight Control Module", "navOut", "Operator Control Plane", "telemIn"],
    ["Operator Control Plane", "cmdOut", "Operator Console Module", "cmdIn"],
    ["Operator Console Module", "uiOut", "HMI Panel & Displays", "uiIn"],
    ["C&C Power Module", "telemOut", "Haptic Alert Unit", "alertIn"],
  ];
  for (const [ca, pa, cb, pb] of connections) {
    R.push(rel("Connector", [portId.get(`${ca}|${pa}`)!], [portId.get(`${cb}|${pb}`)!]));
  }

  // === BEHAVIORAL PILLAR ==================================================
  // (a) Action flow: the 15 functions as action usages inside a C&C Operations
  //     action def, wired with first/then successions + a couple typed flows.
  const opsId = "act_ops";
  E.push(el({ id: opsId, type: "ActionDefinition", name: "C&C Operations" }));
  const functions: Array<[string, string]> = [
    ["F1.1", "Receive & Authenticate Request"],
    ["F1.2", "Validate Fuel Capacity"],
    ["F1.3", "Prioritize Requests"],
    ["F1.4", "Generate Schedule"],
    ["F1.5", "Update Schedule Dynamically"],
    ["F1.6", "Transmit Status & Reports"],
    ["F8.1", "Display Mission Data"],
    ["F8.2", "Receive Operator Input"],
    ["F8.3", "Process Manual Override"],
    ["F8.4", "Execute Emergency Controls"],
    ["F8.5", "Provide Alerts & Feedback"],
    ["F8.6", "Update HMI Displays"],
    ["F8.7", "Subsystem Health & Multilingual Interface Support"],
    ["F8.8", "Logging & Dashboard Integration"],
    ["F8.9", "Reprioritize Refueling Queue"],
  ];
  const fnId = new Map<string, string>();
  for (const [src, name] of functions) {
    const id = `fn_${slug(src)}`;
    E.push(el({ id, type: "ActionUsage", name, ownerId: opsId, raw: { provenanceSourceId: src } }));
    fnId.set(name, id);
  }
  const successions: Array<[string, string]> = [
    ["Receive & Authenticate Request", "Validate Fuel Capacity"],
    ["Validate Fuel Capacity", "Prioritize Requests"],
    ["Prioritize Requests", "Generate Schedule"],
    ["Generate Schedule", "Update Schedule Dynamically"],
    ["Update Schedule Dynamically", "Transmit Status & Reports"],
    ["Receive Operator Input", "Process Manual Override"],
    ["Process Manual Override", "Execute Emergency Controls"],
    ["Display Mission Data", "Update HMI Displays"],
    ["Update HMI Displays", "Provide Alerts & Feedback"],
    ["Reprioritize Refueling Queue", "Prioritize Requests"],
  ];
  for (const [a, b] of successions) {
    R.push(rel("Succession", [fnId.get(a)!], [fnId.get(b)!]));
  }
  // Typed SIGNAL item flows between functions — the data the activities exchange.
  // Each function is itself satisfy-linked to a requirement, so every flow sits
  // on a function -> requirement trace chain.
  const flows: Array<[string, string, string]> = [
    ["Receive & Authenticate Request", "Validate Fuel Capacity", "AuthToken"],
    ["Validate Fuel Capacity", "Prioritize Requests", "FuelData"],
    ["Prioritize Requests", "Generate Schedule", "PriorityList"],
    ["Generate Schedule", "Update Schedule Dynamically", "Schedule"],
    ["Generate Schedule", "Display Mission Data", "Schedule"],
    ["Update Schedule Dynamically", "Transmit Status & Reports", "ScheduleUpdate"],
    ["Transmit Status & Reports", "Logging & Dashboard Integration", "Report"],
    ["Receive Operator Input", "Process Manual Override", "OperatorCmd"],
    ["Process Manual Override", "Execute Emergency Controls", "OverrideCmd"],
    ["Provide Alerts & Feedback", "Update HMI Displays", "AlertSignal"],
    ["Subsystem Health & Multilingual Interface Support", "Logging & Dashboard Integration", "HealthStatus"],
  ];
  for (const [a, b, payload] of flows) {
    E.push(
      el({
        id: `flow_${slug(a)}_${slug(b)}`,
        type: "FlowConnectionUsage",
        name: null,
        ownerId: opsId,
        raw: { sourceEnd: fnId.get(a)!, targetEnd: fnId.get(b)!, payloadType: payload },
      })
    );
  }

  // (b) State machine: C&C operating modes.
  const smId = "sm_cc";
  E.push(el({ id: smId, type: "StateDefinition", name: "C&C Mode" }));
  const states = ["Idle", "Authenticating", "Scheduling", "Refueling", "Reporting", "Emergency"];
  const stateId = new Map<string, string>();
  for (const s of states) {
    const id = `st_${slug(s)}`;
    E.push(el({ id, type: "StateUsage", name: s, ownerId: smId }));
    stateId.set(s, id);
  }
  const transitions: Array<[string, string]> = [
    ["Idle", "Authenticating"],
    ["Authenticating", "Scheduling"],
    ["Scheduling", "Refueling"],
    ["Refueling", "Reporting"],
    ["Reporting", "Idle"],
    ["Refueling", "Emergency"],
    ["Emergency", "Idle"],
  ];
  for (const [a, b] of transitions) {
    E.push(
      el({
        id: `tr_${slug(a)}_${slug(b)}`,
        type: "TransitionUsage",
        name: null,
        ownerId: smId,
        raw: { sourceEnd: stateId.get(a)!, targetEnd: stateId.get(b)! },
      })
    );
  }

  // === CROSS-PILLAR TRACEABILITY =========================================
  // derive: requirement -> need
  const derive: Array<[string, string]> = [
    ["Aircraft ID Verification", "N1"], ["Multi-Platform Refueling", "N1"],
    ["Multi-Platform Refueling", "N2"], ["Fuel Capacity Check", "N2"],
    ["Mission Success Rate", "N7"], ["Post-Mission Reports", "N7"],
    ["HMI Mission Display", "N12"], ["Manual Override Speed", "N12"],
    ["Emergency Shutdown", "N12"], ["Tactile Alerts", "N12"],
    ["Voice Command Support", "N12"], ["HMI Refresh Rate", "N12"],
    ["Subsystem Health Display", "N12"], ["Multilingual HMI", "N12"],
    ["Operator Action Logging", "N12"], ["Haptic Emergency Alerts", "N12"],
    ["HMI Update Speed", "N12"], ["Ground Dashboard Integration", "N12"],
    ["Critical Control Lockdown", "N12"], ["Manual Override Instructions", "N12"],
    ["Refueling Queue Reprioritization", "N12"], ["Fuel Schedule Optimization", "N15"],
    ["Low-Fuel Priority", "N15"], ["Refueling Request Processing", "N16"],
    ["Refueling Priority Logic", "N16"], ["Schedule Generation Speed", "N16"],
    ["Dynamic Schedule Updates", "N16"], ["Post-Mission Reporting", "N16"],
    ["Mission Status Updates", "N16"],
  ];
  for (const [req, need] of derive) {
    R.push(rel("DeriveRequirementUsage", [reqId.get(req)!], [reqId.get(need)!]));
  }

  // satisfy: function -> requirement (serializer emits `satisfy <req> by <fn>`)
  const satisfy: Array<[string, string]> = [
    ["Receive & Authenticate Request", "Aircraft ID Verification"],
    ["Receive & Authenticate Request", "Multi-Platform Refueling"],
    ["Validate Fuel Capacity", "Fuel Capacity Check"],
    ["Transmit Status & Reports", "Mission Success Rate"],
    ["Transmit Status & Reports", "Post-Mission Reports"],
    ["Display Mission Data", "HMI Mission Display"],
    ["Process Manual Override", "Manual Override Speed"],
    ["Execute Emergency Controls", "Emergency Shutdown"],
    ["Provide Alerts & Feedback", "Tactile Alerts"],
    ["Receive Operator Input", "Voice Command Support"],
    ["Update HMI Displays", "HMI Refresh Rate"],
    ["Subsystem Health & Multilingual Interface Support", "Subsystem Health Display"],
    ["Subsystem Health & Multilingual Interface Support", "Multilingual HMI"],
    ["Logging & Dashboard Integration", "Operator Action Logging"],
    ["Provide Alerts & Feedback", "Haptic Emergency Alerts"],
    ["Update HMI Displays", "HMI Update Speed"],
    ["Logging & Dashboard Integration", "Ground Dashboard Integration"],
    ["Execute Emergency Controls", "Critical Control Lockdown"],
    ["Process Manual Override", "Manual Override Instructions"],
    ["Reprioritize Refueling Queue", "Refueling Queue Reprioritization"],
    ["Prioritize Requests", "Fuel Schedule Optimization"],
    ["Prioritize Requests", "Low-Fuel Priority"],
    ["Receive & Authenticate Request", "Refueling Request Processing"],
    ["Prioritize Requests", "Refueling Priority Logic"],
    ["Generate Schedule", "Schedule Generation Speed"],
    ["Update Schedule Dynamically", "Dynamic Schedule Updates"],
    ["Transmit Status & Reports", "Post-Mission Reporting"],
    ["Transmit Status & Reports", "Mission Status Updates"],
  ];
  for (const [fn, req] of satisfy) {
    R.push(rel("SatisfyRequirementUsage", [fnId.get(fn)!], [reqId.get(req)!]));
  }

  // allocate: function -> component (serializer emits `allocate <fn> to <comp>`)
  const allocate: Array<[string, string]> = [
    ["Receive & Authenticate Request", "Flight Control Module"],
    ["Validate Fuel Capacity", "Flight Control Module"],
    ["Prioritize Requests", "C&C Power Module"],
    ["Generate Schedule", "C&C Power Module"],
    ["Update Schedule Dynamically", "C&C Power Module"],
    ["Transmit Status & Reports", "Operator Control Plane"],
    ["Display Mission Data", "HMI Panel & Displays"],
    ["Receive Operator Input", "Operator Console Module"],
    ["Process Manual Override", "Operator Control Plane"],
    ["Execute Emergency Controls", "Operator Control Plane"],
    ["Provide Alerts & Feedback", "Haptic Alert Unit"],
    ["Update HMI Displays", "HMI Panel & Displays"],
    ["Subsystem Health & Multilingual Interface Support", "HMI Panel & Displays"],
    ["Logging & Dashboard Integration", "Operator Control Plane"],
    ["Reprioritize Refueling Queue", "C&C Power Module"],
  ];
  for (const [fn, comp] of allocate) {
    R.push(rel("AllocationUsage", [fnId.get(fn)!], [compId.get(comp)!]));
  }

  return { elements: E, relationships: R };
}

// ---- run: write the .sysml ------------------------------------------------

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1]);
if (isMain) {
  const { elements, relationships } = buildAngarsModel();
  const sysml = serializeToSysml(elements, relationships);
  const here = path.dirname(fileURLToPath(import.meta.url));
  const out = path.resolve(here, "../examples/angars/model/cc-subsystem-full.sysml");
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, sysml);
  const kinds = new Map<string, number>();
  for (const e of elements) kinds.set(e.type, (kinds.get(e.type) ?? 0) + 1);
  const rkinds = new Map<string, number>();
  for (const r of relationships) rkinds.set(r.type, (rkinds.get(r.type) ?? 0) + 1);
  console.log(`wrote ${out}`);
  console.log(`elements: ${elements.length} ${JSON.stringify(Object.fromEntries(kinds))}`);
  console.log(`relationships: ${relationships.length} ${JSON.stringify(Object.fromEntries(rkinds))}`);
}
