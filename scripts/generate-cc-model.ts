/**
 * generate-cc-model.ts
 *
 * Deterministic ANGARS C&C SysML v2 model generator.
 *
 * Reads examples/angars/model/cc-extracted.json, builds all model elements
 * and relationships via the file-native FileStore, exports to
 * examples/angars/model/cc-subsystem.sysml, and reports a full validation gate.
 *
 * Usage:
 *   SYSML_BRIDGE_MODEL_DIR=examples/angars/model/.store pnpm tsx scripts/generate-cc-model.ts
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

import { FileStore } from "../packages/mcp-server/src/file-store.js";
import { serializeToSysml } from "../packages/mcp-server/src/utils/sysml-serializer.js";
import { projectForPresentation } from "../packages/mcp-server/src/utils/cc-presentation.js";
import {
  compareTrace,
  type TracePair,
} from "../packages/mcp-server/src/utils/trace-compare.js";
import type { SysmlElement, SysmlRelationship } from "../packages/mcp-server/src/types/sysml-elements.js";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..");

const EXTRACTED_JSON = path.join(REPO_ROOT, "examples/angars/model/cc-extracted.json");
const MODEL_DIR =
  process.env.SYSML_BRIDGE_MODEL_DIR ??
  path.join(REPO_ROOT, "examples/angars/model/.store");
const OUTPUT_SYSML = path.join(REPO_ROOT, "examples/angars/model/cc-subsystem.sysml");
const VALIDATOR_SH = path.join(REPO_ROOT, "tools/sysml-validator/run.sh");
const VALIDATOR_REQS = path.join(REPO_ROOT, "tools/sysml-validator/requirements.txt");

// ---------------------------------------------------------------------------
// Corpus types
// ---------------------------------------------------------------------------

interface Need {
  id: string;
  name: string;
}

interface Requirement {
  id: string;
  name: string;
  statement: string;
  needIds: string[];
  verifyMethod: string;
}

interface FunctionEntry {
  id: string;
  name: string;
  level: string;
  owner: string;
}

interface Component {
  name: string;
}

interface SatisfiesEntry {
  reqId: string;
  functionId: string;
}

interface CCExtracted {
  subsystem: string;
  needs: Need[];
  requirements: Requirement[];
  functions: FunctionEntry[];
  components: Component[];
  satisfies: SatisfiesEntry[];
  allocations: unknown[];
}

// ---------------------------------------------------------------------------
// Keyword heuristic for model-asserted Func→Comp allocations
// ---------------------------------------------------------------------------

const COMPONENT_NAMES = [
  "C&C Power Module",
  "Operator Control Plane",
  "Operator Console Module",
  "HMI Panel & Displays",
  "Haptic Alert Unit",
  "Flight Control Module",
] as const;

type ComponentName = typeof COMPONENT_NAMES[number];

function allocateLeafFunction(fn: FunctionEntry): ComponentName {
  const combined = (fn.id + " " + fn.name).toLowerCase();

  // authenticate/validate fuel → Flight Control Module (protocol + fuel validation)
  if (
    combined.includes("authenticat") ||
    combined.includes("validate fuel")
  ) {
    return "Flight Control Module";
  }

  // prioritize/schedule (request handling) → C&C Power Module
  // (priority and schedule computation is a power-/compute-intensive C2 core function)
  if (
    combined.includes("priorit") ||
    combined.includes("generate schedule") ||
    combined.includes("update schedule")
  ) {
    return "C&C Power Module";
  }

  // display/update hmi/health/multilingual → HMI Panel & Displays
  if (
    combined.includes("display") ||
    combined.includes("update hmi") ||
    combined.includes("health") ||
    combined.includes("multilingual")
  ) {
    return "HMI Panel & Displays";
  }

  // haptic/alert/feedback → Haptic Alert Unit
  if (combined.includes("haptic") || combined.includes("alert") || combined.includes("feedback")) {
    return "Haptic Alert Unit";
  }

  // operator input/receive operator/queue/reprioritize → Operator Console Module
  if (
    combined.includes("operator input") ||
    combined.includes("receive operator") ||
    combined.includes("queue") ||
    combined.includes("reprioritize")
  ) {
    return "Operator Console Module";
  }

  // logging/dashboard/report/transmit/status/override/emergency/shutdown → Operator Control Plane
  if (
    combined.includes("log") ||
    combined.includes("dashboard") ||
    combined.includes("report") ||
    combined.includes("transmit") ||
    combined.includes("status") ||
    combined.includes("override") ||
    combined.includes("emergency") ||
    combined.includes("shutdown")
  ) {
    return "Operator Control Plane";
  }

  // default → Flight Control Module
  return "Flight Control Module";
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function isLeafFunction(fn: FunctionEntry): boolean {
  // Leaf = has a dot in the id (F1.1, F8.3, etc.)
  return fn.id.includes(".");
}

// ---------------------------------------------------------------------------
// Main generator
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  console.log("=== ANGARS C&C Model Generator ===\n");

  // -- Read corpus --
  const corpus = JSON.parse(fs.readFileSync(EXTRACTED_JSON, "utf8")) as CCExtracted;
  console.log(
    `Corpus loaded: ${corpus.needs.length} needs, ${corpus.requirements.length} reqs, ` +
    `${corpus.functions.length} functions, ${corpus.components.length} components, ` +
    `${corpus.satisfies.length} satisfy edges`
  );

  // -- Init store (always fresh — wipe any prior .store run) --
  if (fs.existsSync(MODEL_DIR)) {
    fs.rmSync(MODEL_DIR, { recursive: true, force: true });
    console.log(`Cleared existing store at ${MODEL_DIR}`);
  }

  const store = new FileStore(MODEL_DIR);
  await store.createProject("ANGARS C&C Subsystem");
  console.log(`Project created: ANGARS C&C Subsystem\n`);

  // Tracking maps: corpus id → store element id
  const needIdToElemId = new Map<string, string>();
  const reqIdToElemId = new Map<string, string>();
  const fnIdToElemId = new Map<string, string>();
  const compNameToElemId = new Map<string, string>();
  const verifyMethodToElemId = new Map<string, string>();

  // -- Step 1: Needs — tagged with stakeholderNeed: true --
  // Needs are RequirementDefinitions, but they are NOT system requirements.
  // They are covered when a system requirement derives from them (DeriveRequirementUsage,
  // req→need). The validator distinguishes Needs from system Requirements via this flag.
  console.log("Step 1: Creating Needs (tagged stakeholderNeed: true)...");
  for (const need of corpus.needs) {
    const el = await store.createElement("RequirementDefinition", need.name, {
      provenanceSourceId: need.id,
      stakeholderNeed: true,
    });
    needIdToElemId.set(need.id, el.id);
  }
  console.log(`  Created ${corpus.needs.length} needs.`);

  // -- Step 2: Requirements --
  console.log("Step 2: Creating Requirements...");
  for (const req of corpus.requirements) {
    const el = await store.createElement("RequirementDefinition", req.name, {
      provenanceSourceId: req.id,
    });
    reqIdToElemId.set(req.id, el.id);
  }
  console.log(`  Created ${corpus.requirements.length} requirements.`);

  // -- Step 3: Need → Req derive relationships --
  // Each system requirement declares which stakeholder needs it traces to via
  // DeriveRequirementUsage. Source = system requirement, target = stakeholder need.
  // This is the correct backward-trace edge AND the mechanism by which needs are covered.
  console.log("Step 3: Creating Req→Need DeriveRequirementUsage edges...");
  let deriveCount = 0;
  for (const req of corpus.requirements) {
    const reqElemId = reqIdToElemId.get(req.id);
    if (!reqElemId) throw new Error(`No element ID for req ${req.id}`);
    for (const needId of req.needIds) {
      const needElemId = needIdToElemId.get(needId);
      if (!needElemId) throw new Error(`No element ID for need ${needId}`);
      await store.createElement("DeriveRequirementUsage", "", {
        source: [{ "@id": reqElemId }],
        target: [{ "@id": needElemId }],
      });
      deriveCount++;
    }
  }
  console.log(`  Created ${deriveCount} derive edges.`);

  // -- Step 4: BDD — subsystem + components + FeatureMembership --
  // The subsystem owns all components via FeatureMembership — this makes it a structural
  // container, exempt from the orphan check even without its own direct trace edge.
  console.log("Step 4: Creating BDD (subsystem + components)...");
  const subsystem = await store.createElement("PartDefinition", "Command & Control Subsystem", {
    provenanceSourceId: "C&C",
  });
  console.log(`  Created subsystem: Command & Control Subsystem (${subsystem.id})`);

  for (const comp of corpus.components) {
    const compEl = await store.createElement("PartDefinition", comp.name, {
      provenanceSourceId: comp.name,
    });
    compNameToElemId.set(comp.name, compEl.id);
    // containment via FeatureMembership (subsystem owns component)
    await store.createElement("FeatureMembership", "", {
      source: [{ "@id": subsystem.id }],
      target: [{ "@id": compEl.id }],
    });
  }
  console.log(`  Created ${corpus.components.length} components + ${corpus.components.length} FeatureMembership edges.`);

  // -- Step 5: Functions --
  console.log("Step 5: Creating Functions (ActionDefinition)...");
  for (const fn of corpus.functions) {
    const el = await store.createElement("ActionDefinition", fn.name, {
      provenanceSourceId: fn.id,
    });
    fnIdToElemId.set(fn.id, el.id);
  }
  console.log(`  Created ${corpus.functions.length} function elements.`);

  // -- Step 5b: Functional decomposition via FeatureMembership --
  // F1 and F8 are top-level functions that own leaf sub-functions.
  // Without FeatureMembership edges they would appear as leaf elements with no trace
  // (since only F1.x/F8.x functions satisfy requirements directly).
  // Adding FeatureMembership F1→F1.x and F8→F8.x makes F1/F8 containers,
  // exempt from the orphan check per the corrected validator semantics.
  console.log("Step 5b: Creating functional decomposition (FeatureMembership F1→F1.x, F8→F8.x)...");
  const topLevelFnIds = ["F1", "F8"];
  let fnDecompCount = 0;
  for (const parentId of topLevelFnIds) {
    const parentElemId = fnIdToElemId.get(parentId);
    if (!parentElemId) {
      console.warn(`  WARN: No element for parent function ${parentId}`);
      continue;
    }
    // Find all children of this parent (dot-notation: F1.1, F1.2, F8.1, etc.)
    const children = corpus.functions.filter(
      (fn) => fn.id.startsWith(parentId + ".") && fn.id.split(".").length === 2
    );
    for (const child of children) {
      const childElemId = fnIdToElemId.get(child.id);
      if (!childElemId) {
        console.warn(`  WARN: No element for child function ${child.id}`);
        continue;
      }
      await store.createElement("FeatureMembership", "", {
        source: [{ "@id": parentElemId }],
        target: [{ "@id": childElemId }],
      });
      fnDecompCount++;
      console.log(`    ${parentId} → ${child.id} (${child.name})`);
    }
  }
  console.log(`  Created ${fnDecompCount} functional decomposition edges.`);

  // -- Step 6: Satisfy (Req→Function, source=function, target=req) --
  console.log("Step 6: Creating SatisfyRequirementUsage edges...");
  let satisfyCount = 0;
  for (const sat of corpus.satisfies) {
    const fnElemId = fnIdToElemId.get(sat.functionId);
    const reqElemId = reqIdToElemId.get(sat.reqId);
    if (!fnElemId) {
      console.warn(`  WARN: No element for functionId ${sat.functionId}, skipping satisfy edge`);
      continue;
    }
    if (!reqElemId) {
      console.warn(`  WARN: No element for reqId ${sat.reqId}, skipping satisfy edge`);
      continue;
    }
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": fnElemId }],
      target: [{ "@id": reqElemId }],
    });
    satisfyCount++;
  }
  console.log(`  Created ${satisfyCount} satisfy edges.`);

  // -- Step 7: Allocate (model-asserted, leaf functions only) --
  // Only leaf functions (F1.x, F8.x) get direct AllocationUsage edges to components.
  // Top-level functions (F1, F8) are containers via FeatureMembership — they do NOT
  // need allocation edges and MUST NOT get fake allocations.
  // The subsystem is a container via FeatureMembership to its components — no allocation needed.
  console.log("Step 7: Creating model-asserted AllocationUsage edges (leaf functions only)...");
  const leafFunctions = corpus.functions.filter(isLeafFunction);
  let allocCount = 0;
  const unallocatedComponents = new Set(corpus.components.map((c) => c.name));

  for (const fn of leafFunctions) {
    const fnElemId = fnIdToElemId.get(fn.id);
    if (!fnElemId) {
      console.warn(`  WARN: No element for function ${fn.id}`);
      continue;
    }
    const targetCompName = allocateLeafFunction(fn);
    const compElemId = compNameToElemId.get(targetCompName);
    if (!compElemId) {
      console.warn(`  WARN: No element for component "${targetCompName}"`);
      continue;
    }
    await store.createElement("AllocationUsage", "", {
      source: [{ "@id": fnElemId }],
      target: [{ "@id": compElemId }],
      provenanceSourceId: "model-asserted",
    });
    allocCount++;
    unallocatedComponents.delete(targetCompName);
    console.log(`    ${fn.id} (${fn.name}) → ${targetCompName}`);
  }

  console.log(`  Created ${allocCount} model-asserted allocation edges.`);

  // Report any legitimately unallocated components (real gaps, not papered over)
  if (unallocatedComponents.size > 0) {
    console.log(`\n  NOTE — legitimately unallocated components (no leaf function maps to them):`);
    for (const name of unallocatedComponents) {
      console.log(`    UNALLOCATED: ${name}`);
    }
    console.log(`  These components are structural containers owned by the subsystem via`);
    console.log(`  FeatureMembership. They are NOT orphans (the subsystem is their container).`);
    console.log(`  Their unallocated status is a real model gap, not masked.\n`);
  }

  // -- Step 8: Verify (VerificationCaseDefinition + VerifyRequirementUsage) --
  // Verification cases are created for SYSTEM REQUIREMENTS ONLY.
  // Stakeholder Needs are NOT verified — they are covered by derivation, not verification.
  console.log("Step 8: Creating VerificationCaseDefinition + VerifyRequirementUsage edges...");
  const distinctMethods = [...new Set(corpus.requirements.map((r) => r.verifyMethod))];
  for (const method of distinctMethods) {
    const el = await store.createElement("VerificationCaseDefinition", `Verify_${method}`, {
      provenanceSourceId: method,
    });
    verifyMethodToElemId.set(method, el.id);
  }
  console.log(`  Created ${distinctMethods.length} VerificationCaseDefinition elements: ${distinctMethods.join(", ")}`);

  let verifyEdgeCount = 0;
  for (const req of corpus.requirements) {
    const verCaseId = verifyMethodToElemId.get(req.verifyMethod);
    const reqElemId = reqIdToElemId.get(req.id);
    if (!verCaseId || !reqElemId) {
      console.warn(`  WARN: Missing IDs for verify edge on ${req.id}`);
      continue;
    }
    await store.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": verCaseId }],
      target: [{ "@id": reqElemId }],
    });
    verifyEdgeCount++;
  }
  console.log(`  Created ${verifyEdgeCount} verify edges (system requirements only).`);

  // -- Step 9: Export to SysML --
  console.log("\nStep 9: Exporting to SysML v2...");
  const allElements = await store.queryElements();
  const allRels = await store.queryRelationships();

  const TRACE_REL_TYPES = new Set([
    "SatisfyRequirementUsage",
    "VerifyRequirementUsage",
    "DeriveRequirementUsage",
    "AllocationUsage",
    "FeatureMembership",
  ]);

  const structuralElements = allElements.filter(
    (e) => !TRACE_REL_TYPES.has(e.type)
  );

  const relElements = allElements.filter((e) => TRACE_REL_TYPES.has(e.type));
  const relationships: SysmlRelationship[] = relElements.map((e) => ({
    id: e.id,
    type: e.type,
    sourceIds: idsFrom(e.raw.source),
    targetIds: idsFrom(e.raw.target),
    raw: e.raw,
  }));

  // Project the def-based store model into a Cameo-valid presentation model:
  // synthesizes package-level usages for requirements/components/leaf functions,
  // nests part/action usages under the subsystem and F1/F8 defs, and re-points
  // every trace relationship onto the usage ids. The store is NOT mutated.
  const presentation = projectForPresentation(structuralElements, relationships);

  const sysmlText = serializeToSysml(presentation.elements, presentation.relationships);
  fs.mkdirSync(path.dirname(OUTPUT_SYSML), { recursive: true });
  fs.writeFileSync(OUTPUT_SYSML, sysmlText, "utf8");
  console.log(`  Written to ${OUTPUT_SYSML} (${sysmlText.length} chars, ${sysmlText.split("\n").length} lines)`);

  // -- Step 10: Validate --
  console.log("\nStep 10: Validation Gate...");

  const requirements = await store.queryElements("RequirementDefinition");
  const parts = await store.queryElements("PartDefinition");
  const actions = await store.queryElements("ActionDefinition");
  const allElementsFresh = await store.queryElements();
  const allRelsFresh = await store.queryRelationships();

  const allElementIds = new Set(allElementsFresh.map((e) => e.id));

  // Separate needs from system requirements
  const needElements = requirements.filter((r) => r.raw.stakeholderNeed === true);
  const systemReqs = requirements.filter((r) => r.raw.stakeholderNeed !== true);

  const FORWARD_TYPES = new Set(["SatisfyRequirementUsage", "AllocationUsage"]);
  const VERIFY_TYPES = new Set(["VerifyRequirementUsage", "RequirementVerificationMembership"]);
  const BACKWARD_TYPES = new Set(["DeriveRequirementUsage"]);
  const ORPHAN_TRACE_TYPES = new Set([
    "SatisfyRequirementUsage",
    "AllocationUsage",
    "DeriveRequirementUsage",
  ]);

  const forwardTracedIds = new Set<string>();
  const verifiedIds = new Set<string>();
  const backwardTracedIds = new Set<string>();

  for (const req of systemReqs) {
    const rels = await store.queryRelationships(req.id, "both");
    if (rels.some((r) => FORWARD_TYPES.has(r.type))) forwardTracedIds.add(req.id);
    if (rels.some((r) => VERIFY_TYPES.has(r.type))) verifiedIds.add(req.id);
    if (rels.some((r) => BACKWARD_TYPES.has(r.type))) backwardTracedIds.add(req.id);
  }

  const totalSystemReqs = systemReqs.length;
  const forwardPercent = totalSystemReqs > 0 ? Math.round((forwardTracedIds.size / totalSystemReqs) * 100) : 0;
  const verifyPercent = totalSystemReqs > 0 ? Math.round((verifiedIds.size / totalSystemReqs) * 100) : 0;
  const backwardPercent = totalSystemReqs > 0 ? Math.round((backwardTracedIds.size / totalSystemReqs) * 100) : 0;

  // Need coverage
  const coveredNeedIds = new Set<string>();
  for (const need of needElements) {
    const rels = await store.queryRelationships(need.id, "in");
    if (rels.some((r) => BACKWARD_TYPES.has(r.type))) coveredNeedIds.add(need.id);
  }
  const needCoveragePercent = needElements.length > 0
    ? Math.round((coveredNeedIds.size / needElements.length) * 100)
    : 100;
  const uncoveredNeeds = needElements.filter((n) => !coveredNeedIds.has(n.id));

  // Orphans: leaf design elements (no trace edge AND no FeatureMembership children)
  const designElements = [...parts, ...actions];
  const orphanElements: Array<{ id: string; name: string | null; type: string }> = [];

  for (const el of designElements) {
    const rels = await store.queryRelationships(el.id, "both");
    const hasTraceEdge = rels.some((r) => ORPHAN_TRACE_TYPES.has(r.type));
    if (hasTraceEdge) continue;
    // Check if it is a container (SOURCE of FeatureMembership)
    const isContainer = rels.some(
      (r) => r.type === "FeatureMembership" && r.sourceIds.includes(el.id)
    );
    if (!isContainer) {
      orphanElements.push({ id: el.id, name: el.name, type: el.type });
    }
  }

  const provenanceElements = [...requirements, ...parts, ...actions];
  const elementsMissingBackpointer: Array<{ id: string; name: string | null; type: string }> = [];
  for (const el of provenanceElements) {
    const prov = el.raw.provenanceSourceId;
    if (!prov || (typeof prov === "string" && prov.trim() === "")) {
      elementsMissingBackpointer.push({ id: el.id, name: el.name, type: el.type });
    }
  }

  const danglingRelationships: Array<{ id: string; type: string; danglingIds: string[] }> = [];
  for (const rel of allRelsFresh) {
    const danglingIds: string[] = [];
    for (const sid of rel.sourceIds) {
      if (!allElementIds.has(sid)) danglingIds.push(sid);
    }
    for (const tid of rel.targetIds) {
      if (!allElementIds.has(tid)) danglingIds.push(tid);
    }
    if (danglingIds.length > 0) {
      danglingRelationships.push({ id: rel.id, type: rel.type, danglingIds });
    }
  }

  const state = await store.getProjectState();
  const totalElements = state.totalElements;

  // -- Step 11: Fidelity --
  console.log("\nStep 11: Fidelity Check (compareTrace)...");

  const authoritative: TracePair[] = corpus.satisfies.map((s) => ({
    reqId: s.reqId,
    functionId: s.functionId,
  }));

  const provenanceById = new Map<string, string>();
  for (const el of allElementsFresh) {
    const prov = el.raw.provenanceSourceId;
    if (typeof prov === "string" && prov) {
      provenanceById.set(el.id, prov);
    }
  }

  const generatedPairs: TracePair[] = [];
  const satisfyEdges = allElementsFresh.filter((e) => e.type === "SatisfyRequirementUsage");
  for (const edge of satisfyEdges) {
    const srcIds = idsFrom(edge.raw.source);
    const tgtIds = idsFrom(edge.raw.target);
    for (const srcId of srcIds) {
      for (const tgtId of tgtIds) {
        const functionId = provenanceById.get(srcId);
        const reqId = provenanceById.get(tgtId);
        if (functionId && reqId) {
          generatedPairs.push({ reqId, functionId });
        }
      }
    }
  }

  const fidelityResult = compareTrace(authoritative, generatedPairs);

  // ---------------------------------------------------------------------------
  // Report
  // ---------------------------------------------------------------------------

  console.log("\n");
  console.log("==========================================================");
  console.log("  VALIDATION GATE RESULTS");
  console.log("==========================================================");
  console.log(`  totalElements             : ${totalElements}`);
  console.log(`  System requirements       : ${totalSystemReqs}`);
  console.log(`  Stakeholder needs         : ${needElements.length}`);
  console.log(`  forwardPercent (sys reqs) : ${forwardPercent}%  ${forwardPercent === 100 ? "PASS" : "FAIL"}`);
  console.log(`  verifyPercent  (sys reqs) : ${verifyPercent}%  ${verifyPercent === 100 ? "PASS" : "FAIL"}`);
  console.log(`  backwardPercent(sys reqs) : ${backwardPercent}%  (informational)`);
  console.log(`  needCoverage              : ${needCoveragePercent}%  (${coveredNeedIds.size}/${needElements.length} needs covered)`);
  if (uncoveredNeeds.length > 0) {
    console.log(`    Uncovered needs: ${uncoveredNeeds.map((n) => n.name ?? n.id).join(", ")}`);
  }
  console.log(`  orphanElements            : ${orphanElements.length}  ${orphanElements.length === 0 ? "PASS" : "FAIL"}`);
  if (orphanElements.length > 0) {
    for (const o of orphanElements) {
      console.log(`    orphan: [${o.type}] ${o.name ?? o.id}`);
    }
  }
  console.log(`  elementsMissingBackpointer: ${elementsMissingBackpointer.length}  ${elementsMissingBackpointer.length === 0 ? "PASS" : "FAIL"}`);
  if (elementsMissingBackpointer.length > 0) {
    for (const m of elementsMissingBackpointer) {
      console.log(`    missing prov: [${m.type}] ${m.name ?? m.id}`);
    }
  }
  console.log(`  danglingRelationships     : ${danglingRelationships.length}  ${danglingRelationships.length === 0 ? "PASS" : "FAIL"}`);
  console.log(`  fidelityPct               : ${fidelityResult.fidelityPct}%  ${fidelityResult.fidelityPct === 100 ? "PASS" : "FAIL"}`);
  console.log(`    present (faithful)      : ${fidelityResult.present.length}`);
  console.log(`    missing (dropped)       : ${fidelityResult.missing.length}`);
  console.log(`    unsupported (fabricated): ${fidelityResult.unsupported.length}`);
  console.log(`  model-asserted allocs     : ${allocCount}`);
  console.log("==========================================================");

  // Overall gate
  const gatePass =
    forwardPercent === 100 &&
    verifyPercent === 100 &&
    orphanElements.length === 0 &&
    elementsMissingBackpointer.length === 0 &&
    danglingRelationships.length === 0 &&
    fidelityResult.fidelityPct === 100 &&
    totalElements < 500;

  console.log(`\n  OVERALL GATE: ${gatePass ? "PASS" : "FAIL"}`);
  if (!gatePass) {
    console.log("\n  Failed gate criteria:");
    if (forwardPercent !== 100) console.log(`    - forwardPercent = ${forwardPercent} (expected 100)`);
    if (verifyPercent !== 100) console.log(`    - verifyPercent = ${verifyPercent} (expected 100)`);
    if (orphanElements.length !== 0) console.log(`    - orphanElements = ${orphanElements.length} (expected 0)`);
    if (elementsMissingBackpointer.length !== 0) console.log(`    - elementsMissingBackpointer = ${elementsMissingBackpointer.length} (expected 0)`);
    if (danglingRelationships.length !== 0) console.log(`    - danglingRelationships = ${danglingRelationships.length} (expected 0)`);
    if (fidelityResult.fidelityPct !== 100) console.log(`    - fidelityPct = ${fidelityResult.fidelityPct} (expected 100)`);
    if (totalElements >= 500) console.log(`    - totalElements = ${totalElements} (expected < 500)`);
  }

  // -- First 30 lines of generated SysML --
  console.log("\n--- First 30 lines of cc-subsystem.sysml ---");
  const sysmlLines = sysmlText.split("\n");
  for (let i = 0; i < Math.min(30, sysmlLines.length); i++) {
    console.log(sysmlLines[i]);
  }
  console.log("--- end excerpt ---\n");

  // -- Step 12: SysML v2 grammar gate (hard) --------------------------------
  // Run the local ANTLR-based validator on the freshly written .sysml. This is
  // the durable "never guess, catch locally" control: a grammar-invalid model
  // can never be silently produced.
  //   exit 0 -> PASS, continue.
  //   exit 1 -> grammar errors: print them and FAIL the generation.
  //   exit 2 -> repo venv missing: loud, actionable warning, but do NOT hard-fail
  //             (missing tooling is an env problem, not a model defect).
  console.log("Step 12: SysML v2 grammar gate (tools/sysml-validator)...");
  runGrammarGate(OUTPUT_SYSML);
}

/**
 * Invoke the local SysML v2 grammar validator on `sysmlPath`.
 *
 * exit 0 -> prints PASS and returns.
 * exit 1 -> prints validator output and FAILs generation (throws).
 * exit 2 -> venv missing: prints a loud, actionable warning and sets an
 *           advisory non-zero exitCode, but does NOT throw (env, not defect).
 */
function runGrammarGate(sysmlPath: string): void {
  let stdout = "";
  let exitCode = 0;
  try {
    stdout = execFileSync("bash", [VALIDATOR_SH, sysmlPath], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
  } catch (err) {
    const e = err as { status?: number; stdout?: string; stderr?: string };
    exitCode = typeof e.status === "number" ? e.status : 1;
    stdout = (e.stdout ?? "") + (e.stderr ?? "");
  }

  if (exitCode === 0) {
    console.log(`  SysML v2 grammar gate: PASS (0 errors)`);
    if (stdout.trim()) console.log(`  ${stdout.trim()}`);
    return;
  }

  if (exitCode === 2) {
    // Environment problem (no python venv), not a model defect. Warn loudly but
    // do not hard-fail the whole generation.
    console.warn("\n  ============================================================");
    console.warn("  WARNING: SysML v2 grammar gate could NOT run (venv missing).");
    console.warn("  ------------------------------------------------------------");
    if (stdout.trim()) {
      for (const line of stdout.trim().split("\n")) console.warn(`  ${line}`);
    }
    console.warn("  Set up the validator venv once with:");
    console.warn(`    python -m venv "${path.join(REPO_ROOT, ".venv")}"`);
    console.warn(`    "${path.join(REPO_ROOT, ".venv/bin/pip")}" install -r "${VALIDATOR_REQS}"`);
    console.warn("  The grammar gate is ADVISORY-SKIPPED this run (env not set up).");
    console.warn("  ============================================================\n");
    // Advisory: surface a non-zero note without failing on tooling absence.
    process.exitCode = process.exitCode ?? 3;
    return;
  }

  // exit 1 (or any other non-zero, non-2): grammar errors -> hard fail.
  console.error("\n  ============================================================");
  console.error("  SysML v2 grammar gate: FAIL");
  console.error("  ------------------------------------------------------------");
  for (const line of stdout.trim().split("\n")) console.error(`  ${line}`);
  console.error("  ============================================================\n");
  throw new Error(
    `SysML v2 grammar gate FAILED (validator exit ${exitCode}) for ${sysmlPath}. ` +
    `A grammar-invalid model must never be produced.`
  );
}

// ---------------------------------------------------------------------------
// Utility: extract IDs from source/target arrays
// ---------------------------------------------------------------------------

function idsFrom(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    if (typeof item === "string") {
      out.push(item);
    } else if (item && typeof item === "object" && "@id" in (item as object)) {
      const id = (item as { "@id"?: unknown })["@id"];
      if (typeof id === "string") out.push(id);
    }
  }
  return out;
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
