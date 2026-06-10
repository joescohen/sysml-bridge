// ---------------------------------------------------------------------------
// Adapter: ANGARS model (our SysmlElement/Relationship graph) -> sysml-reactflow
// factory specs. Emits a self-contained TS data module (literal arrays, no
// node imports) that a Storybook story imports and renders via
// layoutAndRouteFromSpecs / createStateNode.
//
// Run:  pnpm tsx scripts/angars-to-reactflow.ts
// Out:  <hollando-repo>/src/stories/angars-data.ts
// ---------------------------------------------------------------------------

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { buildAngarsModel } from "./angars-full-model.js";
import type { SysmlElement } from "../packages/mcp-server/src/types/sysml-elements.js";

const OUT =
  process.argv[2] ?? "/tmp/hollando-spike/src/stories/angars-data.ts";

const { elements, relationships } = buildAngarsModel();
const byId = new Map(elements.map((e) => [e.id, e]));
const nm = (id: string) => byId.get(id)?.name ?? id;
const childrenOf = (ownerId: string) =>
  elements.filter((e) => e.ownerId === ownerId);

const rels = (type: string) => relationships.filter((r) => r.type === type);
const portDir = (p: string) =>
  /Out$/.test(p) ? "out" : /In$/.test(p) ? "in" : "inout";

// ---- node id sets ---------------------------------------------------------
const needIds = elements.filter((e) => e.type === "RequirementUsage" && /^need_/.test(e.id));
const reqIds = elements.filter((e) => e.type === "RequirementUsage" && /^req_/.test(e.id));
const compEls = elements.filter((e) => e.type === "PartUsage" && /^comp_/.test(e.id));
const fnEls = elements.filter((e) => e.type === "ActionUsage");
const stateEls = elements.filter((e) => e.type === "StateUsage");
const verEls = elements.filter((e) => e.type === "VerificationCaseDefinition");
const subsys = elements.find((e) => e.type === "PartDefinition")!;

// requirement -> status, derived from which verification case covers it
const caseStatus: Record<string, string> = {
  Verify_Demonstration: "reviewed",
  Verify_Test: "draft",
  Verify_Analysis: "approved",
  Verify_Inspection: "reviewed",
};
const reqStatus = new Map<string, string>();
for (const r of rels("VerifyRequirementUsage")) {
  const status = caseStatus[nm(r.sourceIds[0])] ?? "reviewed";
  reqStatus.set(r.targetIds[0], status);
}
// requirement -> first need it derives to (for the `definition` field)
const reqToNeed = new Map<string, string>();
for (const r of rels("DeriveRequirementUsage")) {
  if (!reqToNeed.has(r.sourceIds[0])) reqToNeed.set(r.sourceIds[0], r.targetIds[0]);
}

const partSpec = (e: SysmlElement, withPorts: boolean) => {
  const ports = childrenOf(e.id)
    .filter((c) => c.type === "PortUsage")
    .map((p) => ({ name: p.name!, type: String(p.raw?.typeName ?? "Port"), direction: portDir(p.name!) }));
  return {
    kind: "part-definition",
    spec: {
      id: e.id,
      name: e.name,
      ...(withPorts && ports.length ? { ports } : {}),
    },
  };
};

// ===== VIEW 1: BDD — C&C Subsystem decomposition ==========================
const bdd = {
  specs: [
    { kind: "part-definition", spec: { id: subsys.id, name: subsys.name, description: "ANGARS Command & Control subsystem" } },
    ...compEls.map((c) => partSpec(c, false)),
  ],
  relationships: compEls.map((c, i) => ({
    id: `bdd_e${i}`,
    type: "composition",
    source: subsys.id,
    target: c.id,
    label: "contains",
  })),
};

// ===== VIEW 2: IBD — components + ports + connection web ===================
const portOwner = new Map<string, string>(); // portId -> component id
for (const c of compEls)
  for (const p of childrenOf(c.id)) portOwner.set(p.id, c.id);
const ibd = {
  specs: compEls.map((c) => partSpec(c, true)),
  relationships: rels("Connector").map((r, i) => ({
    id: `ibd_e${i}`,
    type: "flow-connection",
    source: portOwner.get(r.sourceIds[0])!,
    target: portOwner.get(r.targetIds[0])!,
    label: `${nm(r.sourceIds[0])} → ${nm(r.targetIds[0])}`,
  })),
};

// ===== VIEW 3: ACTIVITY — function flow ===================================
const opsDef = elements.find((e) => e.type === "ActionDefinition")!;
const flowEls = elements.filter((e) => e.type === "FlowConnectionUsage");
const activity = {
  specs: [
    { kind: "action-definition", spec: { id: opsDef.id, name: opsDef.name, description: "C&C operational behavior" } },
    ...fnEls.map((f) => ({ kind: "action-usage", spec: { id: f.id, name: f.name, definition: opsDef.id } })),
  ],
  relationships: [
    ...rels("Succession").map((r, i) => ({ id: `succ${i}`, type: "succession", source: r.sourceIds[0], target: r.targetIds[0], label: "then" })),
    ...flowEls.map((f, i) => ({ id: `flow${i}`, type: "control-flow", source: String(f.raw!.sourceEnd), target: String(f.raw!.targetEnd), label: `flow: ${String(f.raw!.payloadType)}` })),
  ],
};

// ===== VIEW 4: STATE MACHINE ==============================================
const stateTrigger: Record<string, string> = {
  Authenticating: "requestReceived",
  Scheduling: "authenticated",
  Refueling: "scheduleReady",
  Reporting: "refuelComplete",
  Emergency: "faultDetected",
};
const stateView = {
  states: stateEls.map((s) => ({
    id: s.id,
    name: s.name,
    ...(s.name === "Refueling" ? { doActivity: "executeRefuel()" } : {}),
    ...(s.name === "Emergency" ? { entryAction: "triggerAlarm()" } : {}),
  })),
  transitions: elements
    .filter((e) => e.type === "TransitionUsage")
    .map((t, i) => ({
      id: `tr${i}`,
      source: String(t.raw!.sourceEnd),
      target: String(t.raw!.targetEnd),
      ...(stateTrigger[nm(String(t.raw!.targetEnd))] ? { trigger: stateTrigger[nm(String(t.raw!.targetEnd))] } : {}),
      ...(nm(String(t.raw!.targetEnd)) === "Idle" ? { trigger: "reset" } : {}),
    })),
};

// ===== VIEW 5: REQUIREMENTS — needs + reqs + derive tree ==================
const requirements = {
  specs: [
    ...needIds.map((n) => ({ kind: "requirement-definition", spec: { id: n.id, name: n.name, text: `Stakeholder need ${n.name}` } })),
    ...reqIds.map((r) => ({
      kind: "requirement-usage",
      spec: {
        id: r.id,
        name: r.name,
        ...(reqToNeed.get(r.id) ? { definition: reqToNeed.get(r.id) } : {}),
        text: `${r.shortName}: ${r.name}`,
        status: reqStatus.get(r.id) ?? "reviewed",
      },
    })),
  ],
  relationships: rels("DeriveRequirementUsage").map((r, i) => ({
    id: `der${i}`,
    type: "specialization",
    source: r.sourceIds[0],
    target: r.targetIds[0],
    label: "derives",
  })),
};

// ===== VIEW 6: FULL TRACEABILITY (the "see it all" web) ===================
const traceability = {
  specs: [
    ...needIds.map((n) => ({ kind: "requirement-definition", spec: { id: n.id, name: n.name } })),
    ...reqIds.map((r) => ({ kind: "requirement-usage", spec: { id: r.id, name: r.name, status: reqStatus.get(r.id) ?? "reviewed" } })),
    ...fnEls.map((f) => ({ kind: "action-usage", spec: { id: f.id, name: f.name } })),
    ...compEls.map((c) => ({ kind: "part-definition", spec: { id: c.id, name: c.name } })),
    ...verEls.map((v) => ({ kind: "verification-case-definition", spec: { id: v.id, name: v.name } })),
  ],
  relationships: [
    ...rels("DeriveRequirementUsage").map((r, i) => ({ id: `td${i}`, type: "specialization", source: r.sourceIds[0], target: r.targetIds[0], label: "derives" })),
    ...rels("SatisfyRequirementUsage").map((r, i) => ({ id: `ts${i}`, type: "satisfy", source: r.sourceIds[0], target: r.targetIds[0], label: "satisfies" })),
    ...rels("AllocationUsage").map((r, i) => ({ id: `ta${i}`, type: "allocate", source: r.sourceIds[0], target: r.targetIds[0], label: "allocated to" })),
    ...rels("VerifyRequirementUsage").map((r, i) => ({ id: `tv${i}`, type: "verify", source: r.sourceIds[0], target: r.targetIds[0], label: "verifies" })),
  ],
};

// ---- emit -----------------------------------------------------------------
const data = { bdd, ibd, activity, state: stateView, requirements, traceability };
const banner = `// AUTO-GENERATED by scripts/angars-to-reactflow.ts — do not edit by hand.\n// ANGARS C&C full model -> sysml-reactflow specs (6 views).\n`;
const body = `export const angars = ${JSON.stringify(data, null, 2)} as const;\n`;
fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, banner + body);

const counts = Object.fromEntries(
  Object.entries(data).map(([k, v]: [string, any]) => [
    k,
    v.specs ? `${v.specs.length} nodes / ${v.relationships.length} edges` : `${v.states.length} states / ${v.transitions.length} transitions`,
  ])
);
console.log(`wrote ${OUT}`);
console.log(JSON.stringify(counts, null, 2));
void fileURLToPath; // keep import used
