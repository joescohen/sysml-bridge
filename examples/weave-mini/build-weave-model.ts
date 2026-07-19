/**
 * build-weave-model.ts
 *
 * Deterministic WEAVER-representative SysML v2 model generator for the
 * weave-mini corpus (W4). Mirrors the STYLE and gate order of
 * `examples/angars/pipeline/build-model.ts`:
 *
 *   ingest (mentions) -> autoCluster (entities) -> build (satisfy/derive/
 *   containment-fed allocation chain) -> serialize -> Gate 1 (audit) ->
 *   Gate 2 (grammar validator).
 *
 * Unlike ANGARS (a hand-extracted structured corpus), weave-mini is a
 * 3-document PROSE corpus. This generator runs the REAL weaver pipeline over
 * it, zero API key, byte-identical every run:
 *
 *   1. parseDocument (real .md/.docx/.xlsx parsers) + runIngestPipeline
 *      against a FIXTURE LlmProvider (recorded, verbatim-checked responses
 *      from fixture-responses.json) -> MentionRecord[] (W0).
 *   2. autoCluster(mentions) -> EntityRecord[] (W1, deterministic band-1
 *      clustering only — no fuzzy/LLM merge is ever auto-applied here).
 *   3. enumerateCooccurrence / suggestMerges are ALSO exercised (W2/W1
 *      band-2) but their output is PROPOSALS — logged as evidence the
 *      enumerators run over this corpus, never written into the model
 *      (no-auto-approve: a proposal is not a fact until a human approves it).
 *   4. enumerateChains + applyChainTypeGate compose the ONE 2-hop
 *      `allocation ∘ containment -> allocation` chain from two
 *      hand-authored, already-ACCEPTED relations (the same accepted pair
 *      pinned in examples/weave-mini/answer-key.json's `chain` block) —
 *      this is the containment-fed allocation chain the artifact exercises.
 *   5. The resulting elements/relationships are serialized to SysML v2 and
 *      run through the SAME two gates ANGARS uses.
 *
 * IMPORTANT — this is an EXAMPLE/DEMO generator (like build-model.ts), NOT a
 * production path. It constructs the "accepted" model directly in-script for
 * the artifact; it MUST NOT call the production approval writers
 * (appendApproval / appendInferredApproval / appendEntityMerge) and MUST NOT
 * be added to any ratchet allowlist. It never writes prose-approved.json,
 * inferred-approved.json, entity-approved.json, or any review-queue file.
 *
 * Gate order is law: build -> Gate 1 (audit) -> serialize -> Gate 2 (grammar
 * validator). A failing gate stops the pipeline with a non-zero exit.
 *
 * Usage:
 *   pnpm tsx examples/weave-mini/build-weave-model.ts
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";

import { runIngestPipeline } from "../../packages/candidates/src/prose/ingest-pipeline.js";
import { parseDocument } from "../../packages/candidates/src/prose/parsers/dispatch.js";
import type {
  CandidateProposal,
  LlmProvider,
} from "../../packages/candidates/src/prose/llm-provider.js";
import {
  autoCluster,
  entityIdFor,
  suggestMerges,
} from "../../packages/candidates/src/entities/index.js";
import type { EntityRecord } from "../../packages/candidates/src/entities/index.js";
import type { MentionRecord } from "../../packages/candidates/src/mentions/index.js";
import {
  enumerateCooccurrence,
  enumerateChains,
  applyChainTypeGate,
  chainStableId,
} from "../../packages/candidates/src/inference/index.js";
import type {
  AcceptedRelation,
  TypedChainCandidate,
} from "../../packages/candidates/src/inference/index.js";

import type {
  SysmlElement,
  SysmlRelationship,
} from "../../packages/model/src/index.js";
import { serializeToSysml } from "../../packages/sysml/src/index.js";
import { audit } from "../../packages/gates/src/index.js";
import type { AuditResult } from "../../packages/gates/src/index.js";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../..");
const WEAVE_MINI_DIR = __dirname; // examples/weave-mini
const CORPUS_DIR = path.join(WEAVE_MINI_DIR, "corpus");
const FIXTURES_PATH = path.join(WEAVE_MINI_DIR, "fixture-responses.json");

// examples/*/out/ is gitignored (.gitignore) -- write the working copy there ...
const OUT_DIR = path.join(WEAVE_MINI_DIR, "out");
const OUTPUT_SYSML_GITIGNORED = path.join(OUT_DIR, "weave-model.sysml");
const AUDIT_JSON = path.join(OUT_DIR, "audit.json");
// ... AND a COMMITTED copy outside out/ so the importable artifact ships in
// the repo (the human-import hand-off this generator exists to produce).
const OUTPUT_SYSML_COMMITTED = path.join(WEAVE_MINI_DIR, "weave-model.sysml");

const VALIDATOR_SH = path.join(REPO_ROOT, "tools/sysml-validator/run.sh");
const VALIDATOR_REQS = path.join(REPO_ROOT, "tools/sysml-validator/requirements.txt");

// ---------------------------------------------------------------------------
// Step 1 — deterministic fixture-driven ingestion
//
// Mirrors packages/candidates/fixtures/wm-compute-answer-key.ts EXACTLY (same
// parser calls, same chunk options, same FixtureProvider shape) so the entity
// ids this generator mints are byte-identical to the ones pinned in
// examples/weave-mini/answer-key.json. Zero API key, fully deterministic.
// ---------------------------------------------------------------------------

interface FixtureDoc {
  file: string;
  documentId: string;
  sectionPath: string;
}
type FixtureProposal = Omit<CandidateProposal, "citedChunkId">;
interface FixtureResponses {
  docs: FixtureDoc[];
  proposals: Record<string, FixtureProposal[]>;
}

class FixtureProvider implements LlmProvider {
  constructor(
    private readonly documentId: string,
    private readonly proposalsByDoc: Record<string, FixtureProposal[]>
  ) {}
  async propose(chunkId: string): Promise<CandidateProposal[]> {
    const batch = this.proposalsByDoc[this.documentId] ?? [];
    return batch.map((p) => ({ ...p, citedChunkId: chunkId }));
  }
}

/** A requirement candidate resolved from the ingest run, entity id computed
 *  the SAME way autoCluster mints it (entityIdFor("requirement", fields.text)
 *  — requirement proposals have no "name" field, so their mention/entity
 *  surface form is the full requirement statement text; see
 *  packages/candidates/src/mentions/index.ts#surfaceFormsFromProposal). */
interface RequirementCandidate {
  reqId: string; // e.g. "REQ-001"
  text: string;
  entityId: string;
}

/** A need candidate — needs DO carry a "name" field, so their surface form
 *  (and entity id) is the short human name, not the full text. */
interface NeedCandidate {
  needId: string; // e.g. "N-001"
  name: string;
  entityId: string;
}

interface IngestResult {
  mentions: MentionRecord[];
  requirements: RequirementCandidate[];
  needs: NeedCandidate[];
  /** documentId -> the single chunk id that document parsed+chunked to
   *  (every weave-mini doc is small enough to be exactly one chunk). */
  chunkIdByDoc: Map<string, string>;
  totalDroppedUnverbatimMentions: number;
}

async function ingestCorpus(): Promise<IngestResult> {
  const fixtures = JSON.parse(
    await readFile(FIXTURES_PATH, "utf8")
  ) as FixtureResponses;

  const mentions: MentionRecord[] = [];
  const requirements: RequirementCandidate[] = [];
  const needs: NeedCandidate[] = [];
  const chunkIdByDoc = new Map<string, string>();
  let totalDroppedUnverbatimMentions = 0;

  for (const d of fixtures.docs) {
    const filePath = path.join(CORPUS_DIR, d.file);
    const raw = fs.readFileSync(filePath);
    const docSha256 = createHash("sha256").update(raw).digest("hex");
    const parsed = await parseDocument(filePath);
    const provider = new FixtureProvider(d.documentId, fixtures.proposals);

    const result = await runIngestPipeline({
      text: parsed.text,
      context: {
        documentHash: docSha256,
        sectionId: "sec-root",
        sectionPath: d.sectionPath,
        pageStart: 0,
        pageEnd: Math.max(parsed.pages.length - 1, 0),
        documentId: d.documentId,
      },
      provider,
      chunkOptions: { chunkSize: 20000, chunkOverlap: 0 },
    });

    console.log(
      `    ${d.file}: chunks=${result.totalChunks} candidates=${result.candidates.length} ` +
        `mentions=${result.mentions.length} droppedUnverbatimMentions=${result.droppedUnverbatimMentions}`
    );
    totalDroppedUnverbatimMentions += result.droppedUnverbatimMentions;
    mentions.push(...result.mentions);

    if (result.candidates.length > 0) {
      chunkIdByDoc.set(d.documentId, result.candidates[0]!.citation.chunkId);
    }

    for (const c of result.candidates) {
      if (c.kind === "requirement") {
        const reqId = typeof c.fields["id"] === "string" ? (c.fields["id"] as string) : null;
        const text = typeof c.fields["text"] === "string" ? (c.fields["text"] as string) : null;
        if (reqId && text) {
          requirements.push({ reqId, text, entityId: entityIdFor("requirement", text) });
        }
      } else if (c.kind === "need") {
        const needId = typeof c.fields["id"] === "string" ? (c.fields["id"] as string) : null;
        const name = typeof c.fields["name"] === "string" ? (c.fields["name"] as string) : null;
        if (needId && name) {
          needs.push({ needId, name, entityId: entityIdFor("need", name) });
        }
      }
    }
  }

  return { mentions, requirements, needs, chunkIdByDoc, totalDroppedUnverbatimMentions };
}

// ---------------------------------------------------------------------------
// Step 2 — model construction
// ---------------------------------------------------------------------------

function findComponent(entities: EntityRecord[], name: string): EntityRecord {
  const e = entities.find((x) => x.kind === "component" && x.canonicalName === name);
  if (!e) throw new Error(`FATAL: component entity not found for "${name}"`);
  return e;
}
function findFunction(entities: EntityRecord[], name: string): EntityRecord {
  const e = entities.find((x) => x.kind === "function" && x.canonicalName === name);
  if (!e) throw new Error(`FATAL: function entity not found for "${name}"`);
  return e;
}
function findMode(entities: EntityRecord[], name: string): EntityRecord {
  const e = entities.find((x) => x.kind === "mode" && x.canonicalName === name);
  if (!e) throw new Error(`FATAL: mode entity not found for "${name}"`);
  return e;
}
function findRequirement(reqs: RequirementCandidate[], reqId: string): RequirementCandidate {
  const r = reqs.find((x) => x.reqId === reqId);
  if (!r) throw new Error(`FATAL: requirement "${reqId}" not found in ingested corpus`);
  return r;
}
function findNeed(needs: NeedCandidate[], needId: string): NeedCandidate {
  const n = needs.find((x) => x.needId === needId);
  if (!n) throw new Error(`FATAL: need "${needId}" not found in ingested corpus`);
  return n;
}

function mkUsage(
  id: string,
  type: string,
  name: string,
  ownerId: string | null,
  provenanceSourceId?: string
): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name,
    shortName: null,
    qualifiedName: null,
    ownerId,
    ownedElementIds: [],
    raw: provenanceSourceId ? { provenanceSourceId } : {},
  };
}

// Synthetic container element ids — stable, deterministic, prefixed to avoid
// any clash with the content-addressed entity ids.
const REQUIREMENTS_CONTAINER_ID = "wm::requirements-container";
const COMPONENTS_CONTAINER_ID = "wm::components-container";
const FUNCTIONS_CONTAINER_ID = "wm::functions-container";
const MODES_CONTAINER_ID = "wm::modes-container";
const VERIFY_CASE_ID = "wm::verify-req001";

export interface BuildModelResult {
  elements: SysmlElement[];
  relationships: SysmlRelationship[];
  chainCandidate: TypedChainCandidate;
  acceptedRelations: AcceptedRelation[];
  cooccurrenceCount: number;
  suggestedMergeCount: number;
}

function buildModel(
  entities: EntityRecord[],
  mentions: MentionRecord[],
  requirements: RequirementCandidate[],
  needs: NeedCandidate[],
  chunkIdByDoc: Map<string, string>
): BuildModelResult {
  // ---- resolve the corpus entities the trace threads reference -----------
  const chc = findComponent(entities, "Cargo Handling Controller");
  const chcAcronym = findComponent(entities, "CHC");
  const posSensor = findComponent(entities, "Position Sensor Array");
  const conveyorMotor = findComponent(entities, "Conveyor Drive Motor");
  const loadCell = findComponent(entities, "Load Cell Assembly");
  const interlockComp = findComponent(entities, "Interlock");
  const boomActuator = findComponent(entities, "Boom Actuator");
  const faultLogger = findComponent(entities, "Fault Logger");
  const diagInterface = findComponent(entities, "Diagnostic Interface");

  const detectCargo = findFunction(entities, "Detect Cargo Presence");
  const computeLoad = findFunction(entities, "Compute Load Distribution");
  const monitorSpeed = findFunction(entities, "Monitor Conveyor Speed");
  const validateLoad = findFunction(entities, "Validate Load Capacity");
  const logFault = findFunction(entities, "Log Fault Event");
  const transmitFault = findFunction(entities, "Transmit Fault Summary");

  const interlockMode = findMode(entities, "Interlock");
  const standby = findMode(entities, "Standby");

  const reqIds = [
    "REQ-001", "REQ-002", "REQ-003", "REQ-004", "REQ-005",
    "REQ-006", "REQ-007", "REQ-008", "REQ-009", "REQ-010",
  ];
  const reqs = reqIds.map((id) => findRequirement(requirements, id));
  const need001 = findNeed(needs, "N-001");

  // ---- exercise the W1/W2 cross-doc enumerators (LOG ONLY) ----------------
  // suggestMerges / enumerateCooccurrence emit PROPOSALS. No-auto-approve:
  // a proposal is never materialized into the model here — only logged as
  // evidence the enumerators actually ran over this corpus.
  const merges = suggestMerges(entities);
  const cooc = enumerateCooccurrence(entities, mentions, {
    families: ["allocation", "modeMembership"],
  });
  console.log(
    `    suggested entity merges (band 2, PROPOSAL only): ${merges.length} ` +
      `(e.g. ${merges.map((m) => `${m.id} [${m.reason}]`).join(", ") || "none"})`
  );
  console.log(
    `    cross-document co-occurrence candidates (PROPOSAL only): ${cooc.candidates.length}`
  );

  // ---- the containment-fed allocation chain --------------------------------
  // Two hand-authored, already-ACCEPTED relations (the corpus-backed facts
  // this eval pins in answer-key.json's `chain` block):
  //   r1: allocation   Detect Cargo Presence -> Position Sensor Array
  //   r2: containment  Position Sensor Array -> Cargo Handling Controller
  // composed via COMPOSITION_TABLE's `allocation ∘ containment -> allocation`
  // row into: Detect Cargo Presence -> Cargo Handling Controller (allocation).
  const doc1Chunk = chunkIdByDoc.get("weave-mini-overview");
  const doc2Chunk = chunkIdByDoc.get("weave-mini-subsystem");
  if (!doc1Chunk || !doc2Chunk) {
    throw new Error("FATAL: expected exactly one chunk each for weave-mini-overview/subsystem");
  }
  const acceptedRelations: AcceptedRelation[] = [
    {
      id: "wm-accepted-r1",
      family: "allocation",
      sourceId: detectCargo.entityId,
      targetId: posSensor.entityId,
      status: "accepted",
      evidenceChunkIds: [doc1Chunk],
    },
    {
      id: "wm-accepted-r2",
      family: "containment",
      sourceId: posSensor.entityId,
      targetId: chc.entityId,
      status: "accepted",
      evidenceChunkIds: [doc2Chunk],
    },
  ];
  const chainEnum = enumerateChains(acceptedRelations);
  const gated = applyChainTypeGate(chainEnum.candidates);
  if (gated.accepted.length !== 1 || gated.rejected.length !== 0) {
    throw new Error(
      `FATAL: expected exactly 1 accepted, 0 rejected chain candidates; got ` +
        `${gated.accepted.length} accepted, ${gated.rejected.length} rejected`
    );
  }
  const chainCandidate = gated.accepted[0]!;
  const expectedChainId = chainStableId(
    "allocation",
    "containment",
    detectCargo.entityId,
    posSensor.entityId,
    chc.entityId
  );
  if (chainCandidate.id !== expectedChainId) {
    throw new Error(
      `FATAL: chain id mismatch — got ${chainCandidate.id}, expected ${expectedChainId} ` +
        `(recompute independently against examples/weave-mini/answer-key.json's pinned chain.id)`
    );
  }

  // ---- elements -------------------------------------------------------------
  const elements: SysmlElement[] = [];

  // Requirements + need (Package container). name = the short corpus id
  // ("REQ-001", "N-001") — concise, unique, used directly in trace statements;
  // the full statement text stays in the corpus (not re-embedded in SysML).
  for (const r of reqs) {
    elements.push(mkUsage(r.entityId, "RequirementUsage", r.reqId, REQUIREMENTS_CONTAINER_ID, r.reqId));
  }
  elements.push(
    mkUsage(need001.entityId, "RequirementUsage", need001.needId, REQUIREMENTS_CONTAINER_ID, need001.needId)
  );
  elements.push({
    id: REQUIREMENTS_CONTAINER_ID,
    elementId: REQUIREMENTS_CONTAINER_ID,
    type: "Package",
    name: "Cargo Handling Requirements",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [...reqs.map((r) => r.entityId), need001.entityId],
    raw: {},
  });

  // Components (PartUsage). Position Sensor Array nests INSIDE Cargo Handling
  // Controller (ownerId) — the structural rendering of the containment
  // relation r2 (containment is structural nesting, not a flat trace
  // statement — see packages/candidates/src/inference/types.ts's
  // `RelationFamily` doc comment on "containment").
  //
  // "CHC" is kept as a SEPARATE, unmerged component usage — it is the
  // acronym-alias entity `suggestMerges` proposes to merge with "Cargo
  // Handling Controller" (band 2), but no-auto-approve means the merge is
  // NEVER auto-applied; the model reflects the current (pre-approval) state
  // honestly, with both entities visible.
  const topComponents = [
    chc, chcAcronym, conveyorMotor, loadCell, interlockComp, boomActuator, faultLogger, diagInterface,
  ];
  for (const c of topComponents) {
    elements.push(
      mkUsage(
        c.entityId,
        "PartUsage",
        c.canonicalName,
        COMPONENTS_CONTAINER_ID,
        c.entityId === chcAcronym.entityId ? "unmerged-acronym-alias-pending-review" : undefined
      )
    );
  }
  elements.push(
    mkUsage(posSensor.entityId, "PartUsage", posSensor.canonicalName, chc.entityId, "wm-accepted-r2")
  );
  elements.push({
    id: COMPONENTS_CONTAINER_ID,
    elementId: COMPONENTS_CONTAINER_ID,
    type: "PartUsage",
    name: "Cargo Handling Subsystem",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: topComponents.map((c) => c.entityId),
    raw: {},
  });
  // Cargo Handling Controller owns Position Sensor Array (containment r2).
  const chcElement = elements.find((e) => e.id === chc.entityId)!;
  chcElement.ownedElementIds = [posSensor.entityId];

  // Functions (ActionUsage).
  const functions = [detectCargo, computeLoad, monitorSpeed, validateLoad, logFault, transmitFault];
  for (const f of functions) {
    elements.push(mkUsage(f.entityId, "ActionUsage", f.canonicalName, FUNCTIONS_CONTAINER_ID));
  }
  elements.push({
    id: FUNCTIONS_CONTAINER_ID,
    elementId: FUNCTIONS_CONTAINER_ID,
    type: "ActionUsage",
    name: "Cargo Handling Operations",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: functions.map((f) => f.entityId),
    raw: {},
  });

  // Modes (StateUsage nested under a StateDefinition — the grammar-proven
  // pattern; see packages/sysml/src/__tests__/sysml-serializer-aspects.test.ts
  // TC3, which nests StateUsage under a StateDefinition owner). "Interlock"
  // the MODE is presented as "Interlock Mode" to disambiguate it from
  // "Interlock" the COMPONENT — the corpus TRAP (same surface form, different
  // kindHint; answer-key.json's `trap`). They are two DISTINCT entities with
  // two DISTINCT ids that correctly did NOT auto-cluster or merge-propose
  // together; only the PRESENTATION name is disambiguated here to avoid a
  // duplicate top-level SysML name in the same flat reference namespace.
  elements.push(mkUsage(interlockMode.entityId, "StateUsage", "Interlock Mode", MODES_CONTAINER_ID));
  elements.push(mkUsage(standby.entityId, "StateUsage", "Standby", MODES_CONTAINER_ID));
  elements.push({
    id: MODES_CONTAINER_ID,
    elementId: MODES_CONTAINER_ID,
    type: "StateDefinition",
    name: "Cargo Handling Modes",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [interlockMode.entityId, standby.entityId],
    raw: {},
  });

  // Verification case (root-level def; nested `objective { verify REQ-001; }`
  // — the ONLY legal `verify` placement per R3 / the cheatsheet).
  elements.push({
    id: VERIFY_CASE_ID,
    elementId: VERIFY_CASE_ID,
    type: "VerificationCaseDefinition",
    name: "Verify_REQ_001",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
  });

  // ---- relationships ----------------------------------------------------
  const relationships: SysmlRelationship[] = [
    {
      id: "wm-rel-satisfy-req001",
      type: "SatisfyRequirementUsage",
      sourceIds: [detectCargo.entityId],
      targetIds: [findRequirement(reqs, "REQ-001").entityId],
      raw: {},
    },
    {
      id: "wm-rel-derive-req001-n001",
      type: "DeriveRequirementUsage",
      sourceIds: [findRequirement(reqs, "REQ-001").entityId],
      targetIds: [need001.entityId],
      raw: {},
    },
    // r1 — the accepted, corpus-backed allocation constituent of the chain.
    {
      id: "wm-accepted-r1",
      type: "AllocationUsage",
      sourceIds: [detectCargo.entityId],
      targetIds: [posSensor.entityId],
      raw: { provenanceSourceId: "wm-accepted-r1" },
    },
    // composed chain result — allocation ∘ containment -> allocation.
    {
      id: chainCandidate.id,
      type: "AllocationUsage",
      sourceIds: [chainCandidate.sourceId],
      targetIds: [chainCandidate.targetId],
      raw: { provenanceSourceId: chainCandidate.id },
    },
    // verify (handled structurally by the serializer — see R3/cheatsheet).
    {
      id: "wm-rel-verify-req001",
      type: "VerifyRequirementUsage",
      sourceIds: [VERIFY_CASE_ID],
      targetIds: [findRequirement(reqs, "REQ-001").entityId],
      raw: {},
    },
  ];

  return {
    elements,
    relationships,
    chainCandidate,
    acceptedRelations,
    cooccurrenceCount: cooc.candidates.length,
    suggestedMergeCount: merges.length,
  };
}

// ---------------------------------------------------------------------------
// Gate 1 — audit() input filtering
//
// Mirrors examples/angars/pipeline/build-model.ts's `auditableRelationships`
// EXACTLY (same rationale, same exclusion): a `verify` edge's source is a
// VerificationCaseDefinition, a legitimate grammar-valid Definition operand
// for the structural `objective { verify ...; }` form the serializer emits —
// R4-def-operand flags ANY Definition trace operand, so this ONE relationship
// type is excluded from what gets audited, same as ANGARS's clean control.
// ---------------------------------------------------------------------------

const VERIFY_TRACE_TYPE = "VerifyRequirementUsage";
function auditableRelationships(rels: SysmlRelationship[]): SysmlRelationship[] {
  return rels.filter((r) => r.type !== VERIFY_TRACE_TYPE);
}

// ---------------------------------------------------------------------------
// Orchestration — ingest -> build -> serialize -> Gate 1 -> Gate 2
// ---------------------------------------------------------------------------

export interface GenerateResult {
  elements: SysmlElement[];
  relationships: SysmlRelationship[];
  sysmlText: string;
  auditResult: AuditResult;
  chainCandidate: TypedChainCandidate;
  acceptedRelations: AcceptedRelation[];
}

/**
 * Run the full generator: ingest -> build -> serialize -> write both copies
 * -> Gate 1 (audit) -> Gate 2 (grammar). Throws on any gate failure — the
 * single source of truth both `main()` and the assertion script call, so
 * neither can silently drift from the other.
 */
export async function generate(): Promise<GenerateResult> {
  console.log("Step 1: Ingesting weave-mini corpus (fixture provider, zero API key, deterministic)...");
  const { mentions, requirements, needs, chunkIdByDoc, totalDroppedUnverbatimMentions } =
    await ingestCorpus();
  console.log(
    `  total mentions=${mentions.length} totalDroppedUnverbatimMentions=${totalDroppedUnverbatimMentions}\n`
  );

  console.log("Step 2: auto-clustering mentions into entities (W1, deterministic band-1 only)...");
  const entities = autoCluster(mentions);
  console.log(`  ${entities.length} entities\n`);

  console.log("Step 3: building the model (elements + satisfy/derive/containment-fed allocation chain)...");
  const built = buildModel(entities, mentions, requirements, needs, chunkIdByDoc);
  console.log(`  ${built.elements.length} elements, ${built.relationships.length} relationships`);
  console.log(`  containment-fed allocation chain: ${built.chainCandidate.id}`);
  console.log(
    `    r1 (accepted, allocation):   ${built.acceptedRelations[0]!.sourceId} -> ${built.acceptedRelations[0]!.targetId}`
  );
  console.log(
    `    r2 (accepted, containment):  ${built.acceptedRelations[1]!.sourceId} -> ${built.acceptedRelations[1]!.targetId} (rendered as structural nesting)`
  );
  console.log(
    `    composed (chain result):     ${built.chainCandidate.sourceId} -> ${built.chainCandidate.targetId} (${built.chainCandidate.relationFamily})\n`
  );

  console.log("Step 4: serializing to SysML v2...");
  const sysmlText = serializeToSysml(built.elements, built.relationships, undefined, {
    emitElementIds: true,
  });
  fs.mkdirSync(OUT_DIR, { recursive: true });
  fs.writeFileSync(OUTPUT_SYSML_GITIGNORED, sysmlText, "utf8");
  fs.writeFileSync(OUTPUT_SYSML_COMMITTED, sysmlText, "utf8");
  console.log(
    `  written to ${path.relative(REPO_ROOT, OUTPUT_SYSML_COMMITTED)} ` +
      `(${sysmlText.length} chars, ${sysmlText.split("\n").length} lines)\n`
  );

  console.log("Step 5: Gate 1 -- @sysml-bridge/gates audit() (R4-def-operand + relational + entity rules)...");
  const mentionIds = new Set(mentions.map((m) => m.mentionId));
  const auditResult: AuditResult = audit(
    built.elements,
    auditableRelationships(built.relationships),
    // weave-mini has no ANGARS-style hand-extracted extracted.json corpus —
    // it is a prose corpus ingested via the LLM candidate pipeline, not a
    // structured extraction. Passing null degrades ONLY the corpus-gated
    // provenance-existence/fidelity checks to a single GATE03-corpus-
    // unavailable WARNING (never an error, see packages/gates/src/index.ts);
    // relationalFindings (incl. R4-def-operand), coverageMatrix, and the
    // entity rule pack below all still run at full strength.
    null,
    { entities, mentionIds }
  );
  const errorFindings = auditResult.findings.filter((f) => f.severity === "error");
  const warningFindings = auditResult.findings.filter((f) => f.severity === "warning");
  console.log(
    `  findings: ${auditResult.findings.length} total (${errorFindings.length} error, ${warningFindings.length} warning)`
  );
  for (const f of auditResult.findings) {
    console.log(`    [${f.severity}] ${f.ruleId} -- ${f.elementId} -- ${f.message}`);
  }
  fs.writeFileSync(
    AUDIT_JSON,
    JSON.stringify({ findings: auditResult.findings, generatedBy: "weave-mini:build-weave-model" }, null, 2)
  );

  if (errorFindings.length > 0) {
    throw new Error(
      `Gate 1 (audit) FAILED for the weave-mini model: ${errorFindings.length} error finding(s). ` +
        `See findings above / ${path.relative(REPO_ROOT, AUDIT_JSON)}.`
    );
  }
  console.log(`  Gate 1: PASS (0 errors)\n`);

  console.log("Step 6: Gate 2 -- SysML v2 grammar validator (tools/sysml-validator)...");
  runGrammarGate(OUTPUT_SYSML_COMMITTED);
  console.log(`  Gate 2: PASS (0 errors)\n`);

  return {
    elements: built.elements,
    relationships: built.relationships,
    sysmlText,
    auditResult,
    chainCandidate: built.chainCandidate,
    acceptedRelations: built.acceptedRelations,
  };
}

/**
 * Invoke the local SysML v2 grammar validator on `sysmlPath`.
 *
 * exit 0 -> prints PASS and returns.
 * exit 1 -> prints validator output and FAILs generation (throws).
 * exit 2 -> venv missing: prints setup instructions and HARD-FAILs (exit 2).
 *
 * Identical contract to examples/angars/pipeline/build-model.ts's
 * runGrammarGate — same validator, same gate discipline.
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
    process.exit(2);
  }

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

async function main(): Promise<void> {
  console.log("=== weave-mini WEAVER model generator ===\n");
  await generate();
  console.log("=== weave-model.sysml generation complete ===");
}

// Run the full generator ONLY when this module is the entry point (mirrors
// examples/angars/pipeline/build-model.ts) -- importing `generate` for reuse
// (e.g. the assertion script) must NOT auto-run the whole pipeline.
const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
const isEntryPoint = invokedPath !== "" && fileURLToPath(import.meta.url) === invokedPath;

if (isEntryPoint) {
  main().catch((err) => {
    console.error("Fatal:", err);
    process.exit(1);
  });
}
