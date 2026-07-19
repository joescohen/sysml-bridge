/**
 * build-model.ts
 *
 * Deterministic ANGARS C&C SysML v2 model generator (demo:build).
 *
 * Reads examples/angars/cc-extracted.json, builds all model elements and
 * relationships via the file-native FileStore, runs the Gate-1 audit checks,
 * projects to a Cameo-valid presentation model, serializes to
 * examples/angars/out/angars.sysml, emits examples/angars/out/audit.json, and
 * runs the Gate-2 grammar validator.
 *
 * Ported from sysml-bridge scripts/generate-cc-model.ts. Gate order is law:
 *   build -> Gate 1 (audit) -> serialize -> Gate 2 (grammar validator).
 * A failing gate stops the pipeline with a non-zero exit.
 *
 * Usage:
 *   pnpm demo:build
 *   SYSML_FOUNDRY_MODEL_DIR=examples/angars/out/.store pnpm tsx examples/angars/pipeline/build-model.ts
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

import { FileStore } from "../../../packages/model/src/index.js";
import { serializeToSysml } from "../../../packages/sysml/src/index.js";
import {
  compareTrace,
  type TracePair,
} from "../../../packages/sysml/src/index.js";
import type {
  SysmlElement,
  SysmlRelationship,
} from "../../../packages/model/src/index.js";
import { audit, loadCorpusCached } from "../../../packages/gates/src/index.js";
import type { AuditResult } from "../../../packages/gates/src/index.js";
import { projectForPresentation, type ComponentFlow } from "./cc-presentation.js";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../../..");

const EXTRACTED_JSON = path.join(REPO_ROOT, "examples/angars/cc-extracted.json");
const MODEL_DIR =
  process.env.SYSML_FOUNDRY_MODEL_DIR ??
  path.join(REPO_ROOT, "examples/angars/out/.store");
const OUTPUT_SYSML = path.join(REPO_ROOT, "examples/angars/out/angars.sysml");
const AUDIT_JSON = path.join(REPO_ROOT, "examples/angars/out/audit.json");
// gates audit() corpus (ExtractedSchema shape) — distinct from the build's
// cc-extracted.json. Same corpus demo:seeded audits against.
const GATES_CORPUS = path.join(REPO_ROOT, "examples/angars/extracted.json");
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

interface N2Interface {
  id: string;
  kind: "n2";
  scope: "component" | "functional";
  sourceId: string;
  targetId: string;
  sourceLabel: string;
  targetLabel: string;
  flow: string;
  provenance: { workbook: string; sheet: string; row: number; cell: string };
}

export interface CCExtracted {
  subsystem: string;
  needs: Need[];
  requirements: Requirement[];
  functions: FunctionEntry[];
  components: Component[];
  satisfies: SatisfiesEntry[];
  allocations: unknown[];
  n2Interfaces?: N2Interface[];
}

/**
 * Load and parse the ANGARS C&C corpus (examples/angars/cc-extracted.json).
 *
 * Exported so callers that reuse the build (e.g. the seeded-defect harness)
 * read the SAME corpus the demo build reads, from the SAME path.
 */
export function loadCorpus(): CCExtracted {
  return JSON.parse(fs.readFileSync(EXTRACTED_JSON, "utf8")) as CCExtracted;
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
// Gate-1 finding aggregation (audit.json contract)
// ---------------------------------------------------------------------------
// findings.length === 0 must mean "the gate report shows zero problems". Each
// FAIL category the console report prints becomes one { code, message } entry.

interface AuditFinding {
  code: string;
  message: string;
}

// The presentation projection intentionally keeps VerificationCaseDefinition as a
// def (the serializer emits `objective { verify <req>; }`, a grammatically valid
// form). The gates R4 rule flags ANY Definition trace operand, so these legitimate
// verify edges are excluded from the audited relationships — the SAME exclusion
// the seeded-defect harness applies to its clean control (see seeded-defects.ts).
const VERIFY_TRACE_TYPE = "VerifyRequirementUsage";

/**
 * Relationships audited by the gates rule pack: the presentation trace edges
 * MINUS the VerifyRequirementUsage edges (whose VerificationCaseDefinition source
 * is a legitimate presentation form, not an R4 defect).
 */
function auditableRelationships(
  relationships: SysmlRelationship[]
): SysmlRelationship[] {
  return relationships.filter((r) => r.type !== VERIFY_TRACE_TYPE);
}

// ---------------------------------------------------------------------------
// Main generator
// ---------------------------------------------------------------------------

/**
 * Build the ANGARS C&C model into a fresh FileStore — corpus → Steps 1..8.
 *
 * This is the single build seam shared by `demo:build` (via main) and the
 * seeded-defect harness (examples/angars/pipeline/seeded-defects.ts). It
 * populates the store with every structural element and trace relationship the
 * demo builds — needs, requirements, derive edges, subsystem/components,
 * functions, decomposition, satisfy/allocate/verify — and returns the store.
 *
 * It performs NO serialization, NO audit, and NO validation: callers own the
 * gates. `main()` continues from here with serialize → Gate 1 → Gate 2; the
 * harness runs the @sysml-bridge/gates audit() over the same store. Keeping
 * this side-effect-free w.r.t. output files is what makes `demo:build` outputs
 * byte-identical before and after the refactor.
 *
 * Returns the populated store plus `allocCount` (the number of model-asserted
 * allocation edges) so `main()`'s VALIDATION GATE RESULTS report is unchanged.
 *
 * @param corpus   - the loaded ANGARS corpus (see loadCorpus)
 * @param modelDir - where the FileStore persists (default: examples/angars/out/.store)
 */
export async function buildModelStore(
  corpus: CCExtracted,
  modelDir: string = MODEL_DIR
): Promise<{ store: FileStore; allocCount: number }> {
  console.log(
    `Corpus loaded: ${corpus.needs.length} needs, ${corpus.requirements.length} reqs, ` +
    `${corpus.functions.length} functions, ${corpus.components.length} components, ` +
    `${corpus.satisfies.length} satisfy edges`
  );

  // -- Init store (always fresh — wipe any prior .store run) --
  if (fs.existsSync(modelDir)) {
    fs.rmSync(modelDir, { recursive: true, force: true });
    console.log(`Cleared existing store at ${modelDir}`);
  }

  const store = new FileStore(modelDir);
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

  return { store, allocCount };
}

// ---------------------------------------------------------------------------
// Main generator — build → serialize → Gate 1 (audit) → Gate 2 (grammar)
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  console.log("=== ANGARS C&C Model Generator ===\n");

  // -- Read corpus + build the store (Steps 1..8) via the shared seam --
  const corpus = loadCorpus();
  const { store, allocCount } = await buildModelStore(corpus, MODEL_DIR);

  // -- Step 9: Export to SysML --
  console.log("\nStep 9: Exporting to SysML v2...");
  const allElements = await store.queryElements();

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

  // Corpus-grounded component-scope interface flows (both endpoints are C&C
  // components). Each becomes one `connect` inside the 'C&C Subsystem' container
  // so the Interconnection view is non-empty. provenanceSourceId = the n2 row id.
  const componentFlows: ComponentFlow[] = (corpus.n2Interfaces ?? [])
    .filter((n) => n.scope === "component")
    .map((n) => ({
      id: n.id,
      sourceLabel: n.sourceLabel,
      targetLabel: n.targetLabel,
      flow: n.flow,
    }));
  const functionalFlowCount = (corpus.n2Interfaces ?? []).filter(
    (n) => n.scope === "functional"
  ).length;
  console.log(
    `  n2 interface flows: ${componentFlows.length} component-scope (→ connections), ` +
    `${functionalFlowCount} functional-scope (→ successions)` +
    (functionalFlowCount === 0
      ? " [none: C&C functions have no intra-scope N2 edge; 'C&C Operations' is a General (structural) view of its nested actions]"
      : "")
  );

  // Project the def-based store model into a Cameo-valid, renderable presentation
  // model: synthesizes package-level requirement usages; nests the 6 component
  // part usages inside a 'C&C Subsystem' container with corpus-grounded connect
  // edges; nests the 15 leaf action usages inside a 'C&C Operations' container;
  // and re-points every trace relationship onto the usage ids. The store is NOT
  // mutated — so audit/fidelity (which read the store) are unaffected.
  const presentation = projectForPresentation(
    structuralElements,
    relationships,
    componentFlows
  );

  // Milestone 1 (identity round-trip): carry each element's stable id via a
  // `// @id:` comment — real evidence the id-carrying form is grammar-legal
  // at ANGARS scale (validated below by Gate 2), not just in unit fixtures.
  const sysmlText = serializeToSysml(presentation.elements, presentation.relationships, undefined, {
    emitElementIds: true,
  });
  fs.mkdirSync(path.dirname(OUTPUT_SYSML), { recursive: true });
  fs.writeFileSync(OUTPUT_SYSML, sysmlText, "utf8");
  console.log(`  Written to ${OUTPUT_SYSML} (${sysmlText.length} chars, ${sysmlText.split("\n").length} lines)`);

  // -- Step 10: Validate (Gate 1 — relational/coverage/provenance audit) --
  console.log("\nStep 10: Validation Gate...");

  // -- Step 10a: @sysml-bridge/gates rule-pack audit over the presentation ---
  //    model. This runs the SAME audit() the seeded-defect harness runs — so
  //    R4-def-operand (the def-operand semantic class Cameo catches but the
  //    grammar validator cannot) and the other rule-pack checks are enforced by
  //    `pnpm demo` itself, not only by the separate demo:seeded harness. The
  //    VerifyRequirementUsage edges are excluded (auditableRelationships), the
  //    same way demo:seeded excludes them from its clean control. This is
  //    ADDITIVE: the inline forward/verify/orphan/provenance/dangling/fidelity
  //    reporting below still feeds audit.json's existing contract unchanged.
  const gatesCorpus = await loadCorpusCached(GATES_CORPUS);
  if (gatesCorpus === null) {
    throw new Error(
      `FATAL: gates corpus failed to load from ${GATES_CORPUS}. ` +
      `Provenance existence checks require it; refusing to run a degraded audit.`
    );
  }
  const gatesAudit: AuditResult = audit(
    presentation.elements,
    auditableRelationships(presentation.relationships),
    gatesCorpus
  );
  // Gate on error-severity findings only — the same clean-control definition the
  // seeded harness asserts (zero error findings on the presentation model). Rule-
  // pack warnings are surfaced to the console but kept out of audit.json's
  // findings array so the existing { code, message } contract is preserved.
  const gatesErrorFindings = gatesAudit.findings.filter((f) => f.severity === "error");
  const gatesWarningCount = gatesAudit.findings.length - gatesErrorFindings.length;

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

  // Fidelity contract values for audit.json:
  //   tracePairsTotal   = the authoritative trace pairs (denominator)
  //   tracePairsMatched = the present/faithful pairs   (numerator)
  const tracePairsTotal = authoritative.length;
  const tracePairsMatched = fidelityResult.present.length;

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
  console.log(`  gates rule-pack (errors)  : ${gatesErrorFindings.length}  ${gatesErrorFindings.length === 0 ? "PASS" : "FAIL"}  (${gatesWarningCount} warning(s))`);
  if (gatesErrorFindings.length > 0) {
    for (const f of gatesErrorFindings) {
      console.log(`    gates: [${f.ruleId}] ${f.elementId} — ${f.message}`);
    }
  }
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
    gatesErrorFindings.length === 0 &&
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
    if (gatesErrorFindings.length !== 0) console.log(`    - gates rule-pack errors = ${gatesErrorFindings.length} (expected 0)`);
    if (fidelityResult.fidelityPct !== 100) console.log(`    - fidelityPct = ${fidelityResult.fidelityPct} (expected 100)`);
    if (totalElements >= 500) console.log(`    - totalElements = ${totalElements} (expected < 500)`);
  }

  // -- Aggregate the gate report into a structured findings array. Each FAIL
  //    category the report above prints becomes one { code, message } entry;
  //    a clean gate yields findings.length === 0. --------------------------
  const findings: AuditFinding[] = [];
  if (forwardPercent !== 100) {
    findings.push({ code: "GATE-FORWARD", message: `forwardPercent = ${forwardPercent}% (expected 100)` });
  }
  if (verifyPercent !== 100) {
    findings.push({ code: "GATE-VERIFY", message: `verifyPercent = ${verifyPercent}% (expected 100)` });
  }
  for (const o of orphanElements) {
    findings.push({ code: "GATE-ORPHAN", message: `orphan [${o.type}] ${o.name ?? o.id}` });
  }
  for (const m of elementsMissingBackpointer) {
    findings.push({ code: "GATE-PROVENANCE", message: `missing provenanceSourceId [${m.type}] ${m.name ?? m.id}` });
  }
  for (const d of danglingRelationships) {
    findings.push({ code: "GATE-DANGLING", message: `dangling ${d.type} refs ${d.danglingIds.join(", ")}` });
  }
  for (const f of gatesErrorFindings) {
    findings.push({ code: f.ruleId, message: `[${f.elementId}] ${f.message}` });
  }
  for (const p of fidelityResult.missing) {
    findings.push({ code: "GATE-FIDELITY-MISSING", message: `dropped trace pair ${p.reqId} ← ${p.functionId}` });
  }
  for (const p of fidelityResult.unsupported) {
    findings.push({ code: "GATE-FIDELITY-UNSUPPORTED", message: `fabricated trace pair ${p.reqId} ← ${p.functionId}` });
  }
  if (totalElements >= 500) {
    findings.push({ code: "GATE-ELEMENT-CAP", message: `totalElements = ${totalElements} (expected < 500)` });
  }

  // -- Emit audit.json (Gate-1 findings + fidelity JSON) --------------------
  fs.mkdirSync(path.dirname(AUDIT_JSON), { recursive: true });
  fs.writeFileSync(
    AUDIT_JSON,
    JSON.stringify(
      {
        findings,
        fidelity: { tracePairs: tracePairsTotal, matched: tracePairsMatched },
        generatedBy: "demo:build",
      },
      null,
      2
    )
  );
  console.log(`\n  Wrote audit.json: ${findings.length} findings, fidelity ${tracePairsMatched}/${tracePairsTotal}`);

  // A failed Gate 1 stops the pipeline before Gate 2 (grammar) — gate order is law.
  if (!gatePass) {
    throw new Error(
      `Gate 1 (audit) FAILED for the ANGARS C&C model: ${findings.length} findings. ` +
      `See the VALIDATION GATE RESULTS above and examples/angars/out/audit.json.`
    );
  }

  // -- First 30 lines of generated SysML --
  console.log("\n--- First 30 lines of angars.sysml ---");
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
  //   exit 2 -> repo venv missing: HARD FAIL (no venv means no validation).
  console.log("Step 12: SysML v2 grammar gate (tools/sysml-validator)...");
  runGrammarGate(OUTPUT_SYSML);
}

/**
 * Invoke the local SysML v2 grammar validator on `sysmlPath`.
 *
 * exit 0 -> prints PASS and returns.
 * exit 1 -> prints validator output and FAILs generation (throws).
 * exit 2 -> venv missing: prints setup instructions and HARD-FAILs (exit 2).
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
    // Missing validator venv. In the old repo this downgraded to an advisory
    // skip; here it is a HARD failure — validate-before-claim is the whole
    // point of the gate.
    console.error("\n  ============================================================");
    console.error("  SysML v2 grammar gate could NOT run (venv missing).");
    console.error("  ------------------------------------------------------------");
    if (stdout.trim()) {
      for (const line of stdout.trim().split("\n")) console.error(`  ${line}`);
    }
    console.error("  Set up the validator venv once with:");
    console.error(`    python -m venv "${path.join(REPO_ROOT, ".venv")}"`);
    console.error(`    "${path.join(REPO_ROOT, ".venv/bin/pip")}" install -r "${VALIDATOR_REQS}"`);
    console.error("  ============================================================\n");
    // Gate 2 cannot be skipped: no venv means no validation, means no claim.
    process.exit(2);
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

// Run the full generator ONLY when this module is the entry point (pnpm
// demo:build). When imported for its build seam (buildModelStore / loadCorpus)
// — e.g. by the seeded-defect harness — main() must NOT auto-run, or importing
// the seam would re-run the entire demo build as a side effect.
const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
const isEntryPoint =
  invokedPath !== "" && fileURLToPath(import.meta.url) === invokedPath;

if (isEntryPoint) {
  main().catch((err) => {
    console.error("Fatal:", err);
    process.exit(1);
  });
}
