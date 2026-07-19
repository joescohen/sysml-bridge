/**
 * engine.ts — F8 inference engine orchestrator.
 *
 * Stage machine:
 *   generate → relevance filter (+ per-family cap) → type gate
 *     → LLM propose → premise check → band route → debate → output
 *
 * Features:
 *   - Sentinel keyed on composed-IR hash: skip if hash unchanged (INFER_FORCE=1 overrides)
 *   - Pre-flight cost estimate logged before first LLM call
 *   - INFER_BUDGET_USD cap: aborts before spend if estimated cost > budget
 *   - Failure isolation: errored pair → uncertain, continues
 *   - emittedUnpremised count = 0 guaranteed (A2)
 *   - Bounded concurrency: PROPOSE + DEBATE stages run with up to INFER_CONCURRENCY
 *     concurrent tasks (default 8, clamp 1..16). Output ordering is deterministic
 *     (stable by original accepted-order), regardless of completion order.
 */

import { createHash } from "node:crypto";
import type { InferredComposedIR } from "@sysml-bridge/model";
import type { RunCountersSnapshot } from "../telemetry.js";
import type { InferenceProvider } from "./inference-provider.js";
import type {
  RelationFamily,
  TypedCandidate,
  RejectedCandidate,
  DroppedUnpremisedCandidate,
  AutoRejectedRecord,
  QueuedRecord,
  DebateRecord,
  CandidateRecord,
  RunStats,
} from "./types.js";
import { classifyBand, zeroFamilyCounts, formatFamilyCounts, ALL_RELATION_FAMILIES } from "./types.js";
import { buildElementMap, applyTypeGate } from "./type-gate.js";
import { generateCandidates, buildSkipSet, inferenceStableId } from "./candidate-generator.js";
import { applyRelevanceFilter, resolveFamilyCap } from "./relevance-filter.js";
import { buildContextBundle, buildCrossDocContextBundle } from "./neighborhood.js";
import { repairPremises } from "./premise-repair.js";
import { runDebate } from "./debate.js";
import { enumerateCooccurrence } from "./cooccurrence.js";
import { enumerateChains, type AcceptedRelation } from "./chains.js";
import { applyChainTypeGate } from "./composition-table.js";
import { Bm25Index } from "../retrieval/bm25.js";
import type { RetrievalChunk } from "../retrieval/bm25.js";
import type { ContextBundle } from "./types.js";
import type { EntityRecord } from "../entities/cluster.js";
import type { MentionRecord } from "../mentions/index.js";

// ── Bounded concurrency helper ────────────────────────────────────────────────

/**
 * Resolve INFER_CONCURRENCY from the environment.
 * Default: 8. Clamped to [1, 16].
 */
export function resolveInferConcurrency(override?: number): number {
  if (override !== undefined) return Math.max(1, Math.min(16, override));
  const env = process.env["INFER_CONCURRENCY"];
  if (env) {
    const n = parseInt(env, 10);
    if (Number.isFinite(n)) return Math.max(1, Math.min(16, n));
  }
  return 8;
}

/** A single task failure captured by boundedPool (index + stringified error). */
export interface PoolFailure {
  /** Original index of the failed task in the input array. */
  index: number;
  /** The task's rejection, stringified (Error.message or String(err)). */
  error: string;
}

/** boundedPool result: per-slot results (undefined = that slot's task threw) + the failure list. */
export interface PoolResult<T> {
  /** One entry per input task, in original order. A failed slot is `undefined`. */
  results: Array<T | undefined>;
  /** One entry per task that threw; other tasks are unaffected. */
  failures: PoolFailure[];
}

/**
 * Run `tasks[i]` → `T` for every index with bounded concurrency.
 * Returns results in the original task order regardless of completion order.
 *
 * Failure isolation (the §6 fix): a task that throws NO LONGER rejects the whole
 * pool. Each task runs inside a per-task try/catch — a rejection is recorded into
 * `failures` as `{index, error}`, that slot is left `undefined`, and the remaining
 * tasks continue. The pool always resolves; callers inspect `failures` to detect
 * partial degradation. (Previously one rejection rejected Promise.all and aborted
 * the entire run, losing every other task's work.)
 *
 * Implementation: a simple index-counter worker pool — no extra dependencies.
 *   - Workers pull the next pending index atomically (JS is single-threaded,
 *     so incrementing a shared counter is safe).
 *   - Each task result is stored back into its original slot; a thrown task
 *     leaves its slot `undefined` and appends to `failures`.
 */
export async function boundedPool<T>(
  tasks: ReadonlyArray<() => Promise<T>>,
  concurrency: number
): Promise<PoolResult<T>> {
  const results = new Array<T | undefined>(tasks.length);
  const failures: PoolFailure[] = [];
  let nextIndex = 0;

  async function worker(): Promise<void> {
    while (true) {
      const i = nextIndex++;
      if (i >= tasks.length) return;
      try {
        results[i] = await tasks[i]!();
      } catch (err: unknown) {
        // Failure isolation: record + continue; do NOT reject the pool.
        results[i] = undefined;
        failures.push({ index: i, error: err instanceof Error ? err.message : String(err) });
      }
    }
  }

  const limit = Math.min(concurrency, tasks.length);
  if (limit <= 0) return { results, failures };
  const workers = Array.from({ length: limit }, worker);
  await Promise.all(workers);
  return { results, failures };
}

// ── Sentinel ─────────────────────────────────────────────────────────────────

/**
 * Compute a hash over the composed IR to use as a sentinel.
 * If the hash matches a previous run and INFER_FORCE is not set, we can skip.
 */
export function hashComposedIR(ir: InferredComposedIR): string {
  const corpus = ir.extracted;
  const input = JSON.stringify({
    functions: corpus.functions?.length ?? 0,
    components: corpus.components?.length ?? 0,
    n2Interfaces: corpus.n2Interfaces?.length ?? 0,
    subsystems: corpus.subsystems?.length ?? 0,
    proseEntries: ir.proseEntries.length,
    inferredEntries: ir.inferredEntries.length,
  });
  return createHash("sha256").update(input, "utf8").digest("hex").slice(0, 16);
}

// ── Cost estimation (rough) ───────────────────────────────────────────────────

// Haiku pricing (approximate, may drift): $0.25/MTok input, $1.25/MTok output
const HAIKU_INPUT_COST_PER_TOKEN = 0.25 / 1_000_000;
const HAIKU_OUTPUT_COST_PER_TOKEN = 1.25 / 1_000_000;
// Rough token estimates per call
const TOKENS_PROPOSE_IN = 600;
const TOKENS_PROPOSE_OUT = 200;
const TOKENS_DEBATE_IN = 400;
const TOKENS_DEBATE_OUT = 150;

export function estimateCostUsd(
  typedCandidateCount: number,
  debateFraction: number = 0.3
): number {
  const proposeCost =
    typedCandidateCount * (TOKENS_PROPOSE_IN * HAIKU_INPUT_COST_PER_TOKEN + TOKENS_PROPOSE_OUT * HAIKU_OUTPUT_COST_PER_TOKEN);
  const debateCount = Math.ceil(typedCandidateCount * debateFraction);
  // debate = 2 calls (advocate + challenger)
  const debateCost =
    debateCount * 2 * (TOKENS_DEBATE_IN * HAIKU_INPUT_COST_PER_TOKEN + TOKENS_DEBATE_OUT * HAIKU_OUTPUT_COST_PER_TOKEN);
  return proposeCost + debateCost;
}

// ── Premise validation ────────────────────────────────────────────────────────

/**
 * Validate that all premise ids resolve in the composed IR.
 * Returns { valid: true } or { valid: false, unresolvable: string[] }.
 *
 * `extraResolvableIds` lets the caller admit ids that are real corpus references
 * but live outside the composed IR — specifically BM25-retrieved chunk ids from
 * the chunk store. A retrieved chunk offered as evidence is a legitimate,
 * resolvable premise, so citing it does NOT drop the candidate as unpremised.
 */
export function validatePremises(
  premises: string[],
  ir: InferredComposedIR,
  extraResolvableIds?: ReadonlySet<string>
): { valid: true } | { valid: false; unresolvable: string[] } {
  // Build a flat set of all resolvable ids
  const resolvable = new Set<string>();
  const corpus = ir.extracted;
  const entityKinds = [
    "needs", "requirements", "functions", "components",
    "subsystems", "kpps", "behaviorDecomp", "n2Interfaces",
  ] as const;
  for (const k of entityKinds) {
    for (const e of ((corpus as Record<string, unknown>)[k] as Array<{ id?: string }>) ?? []) {
      if (e.id) resolvable.add(e.id);
    }
  }
  for (const entry of ir.proseEntries) {
    resolvable.add(entry.id);
  }
  for (const entry of ir.inferredEntries) {
    resolvable.add(entry.id);
  }
  if (extraResolvableIds) {
    for (const id of extraResolvableIds) resolvable.add(id);
  }

  const unresolvable = premises.filter((p) => !resolvable.has(p));
  if (unresolvable.length === 0) return { valid: true };
  return { valid: false, unresolvable };
}

// ── Engine options ────────────────────────────────────────────────────────────

export interface EngineOptions {
  /** Skip if the composed-IR hash matches this value (sentinel). Pass undefined to always run. */
  sentinelHash?: string;
  /** Dry run: generate + type gate only; skip LLM calls. */
  dryRun?: boolean;
  /** Budget cap in USD. If estimated cost exceeds this, abort before any LLM spend. */
  budgetUsd?: number;
  /** Per-family candidate cap. Default: INFER_FAMILY_CAP env, else 150. */
  familyCap?: number;
  /** Verbose logging function (defaults to console.error). */
  log?: (msg: string) => void;
  /**
   * Max concurrent LLM calls for propose and debate stages.
   * Default: INFER_CONCURRENCY env, else 8. Clamped to [1, 16].
   */
  concurrency?: number;
  /**
   * Optional lexical chunk store for BM25 evidence retrieval. When provided, an
   * in-memory BM25 index is built once and queried per candidate so the propose
   * context includes retrieved corpus passages (as citable, resolvable premises).
   * When omitted, context assembly falls back to exact-id quotes only.
   */
  chunkStore?: readonly RetrievalChunk[];
  /**
   * Optional cross-document entity store (W2). When provided, the engine ALSO
   * enumerates co-occurrence spokes and 2-hop chains over canonical entities and
   * runs them through the SAME downstream pipeline (type gate → premise contract
   * → debate → queue). Mirrors `chunkStore` threading: built once, its premise
   * ids are admitted as resolvable, and per-candidate context is entity-aware.
   *
   * ABSENT entityStore → this whole path is skipped → EXACTLY today's behavior
   * (the structural candidate flow is byte-identical). No production caller yet;
   * a W3 weave loop supplies this to enable cross-document enumeration.
   */
  entityStore?: EntityStoreInput;
}

/**
 * Cross-document enumeration input for the engine. `entities` + `mentions` drive
 * co-occurrence; `acceptedRelations` drives chains (only accepted relations are
 * composed — pending proposals are never chained). `families` declares which
 * relation families to enumerate co-occurrence spokes for (default: all four
 * pipeline families); `minCooccur` is the shared-chunk threshold (default 1).
 */
export interface EntityStoreInput {
  entities: readonly EntityRecord[];
  mentions: readonly MentionRecord[];
  acceptedRelations?: readonly AcceptedRelation[];
  families?: readonly RelationFamily[];
  minCooccur?: number;
}

export interface EngineResult {
  records: CandidateRecord[];
  stats: RunStats[];
  irHash: string;
  skippedSentinel: boolean;
  estimatedCostUsd: number;
  emittedUnpremised: number; // MUST be 0
  droppedUnpremised: number;
  /**
   * Per-candidate tasks whose worker threw an UNEXPECTED error (not a handled
   * propose/debate fallback — those are counted in RunStats). Isolated by
   * boundedPool: a throw here no longer aborts the run; the slot is skipped and
   * recorded so a partially-degraded run is visible rather than silent. `[]` on a
   * clean run.
   */
  taskFailures: PoolFailure[];
  /**
   * LLM parse/schema-failure counts from the provider's debate passes
   * (advocate/challenge). The 0.5 default still applies — it is now counted and
   * logged. Undefined if the provider does not expose a `counters` getter.
   */
  parseFailures?: RunCountersSnapshot;
}

// ── Main engine function ──────────────────────────────────────────────────────

/**
 * Run the full F8 inference engine over the composed IR.
 *
 * If dryRun=true, generates candidates and applies the type gate but makes no LLM calls.
 * Returns typed candidate counts and rejection counts.
 */
export async function runInferenceEngine(
  ir: InferredComposedIR,
  provider: InferenceProvider,
  options: EngineOptions = {}
): Promise<EngineResult> {
  const log = options.log ?? ((msg: string) => process.stderr.write(msg + "\n"));

  // ── Sentinel check ─────────────────────────────────────────────────────────
  const irHash = hashComposedIR(ir);
  const forceRun = process.env["INFER_FORCE"] === "1";

  if (options.sentinelHash === irHash && !forceRun && !options.dryRun) {
    log(`[inference] Sentinel match (hash=${irHash}) — skipping run. Set INFER_FORCE=1 to override.`);
    return {
      records: [],
      stats: [],
      irHash,
      skippedSentinel: true,
      estimatedCostUsd: 0,
      emittedUnpremised: 0,
      droppedUnpremised: 0,
      taskFailures: [],
    };
  }

  // ── BM25 evidence index (optional) ─────────────────────────────────────────
  // Built once per run over the supplied chunk store; queried per candidate in
  // buildContextBundle. Its chunk ids are admitted as resolvable premises.
  const retrievalIndex =
    options.chunkStore && options.chunkStore.length > 0
      ? new Bm25Index(options.chunkStore)
      : undefined;
  const retrievedChunkIds = retrievalIndex?.chunkIds();
  if (retrievalIndex) {
    log(`[inference] BM25 evidence index: ${retrievalIndex.size} chunk(s)`);
  }

  // ── Candidate generation ───────────────────────────────────────────────────
  const skipSet = buildSkipSet(ir);
  const { candidates, countsByFamily } = generateCandidates(ir, skipSet);

  log(`[inference] Generated candidates: ${formatFamilyCounts(countsByFamily)} (total=${candidates.length})`);

  // ── Relevance filter + per-family cap (pre-type-gate, deterministic) ───────
  const familyCap = resolveFamilyCap(options.familyCap);
  const {
    kept: relevantCandidates,
    rejected: relevanceRejected,
    capped: capRejected,
  } = applyRelevanceFilter(candidates, ir, { familyCap });

  const unboundedByFamily = zeroFamilyCounts();
  for (const r of relevanceRejected) unboundedByFamily[r.relationFamily]++;
  const cappedByFamily = zeroFamilyCounts();
  for (const r of capRejected) cappedByFamily[r.relationFamily]++;

  log(`[inference] Relevance filter: ${relevantCandidates.length} kept, ${relevanceRejected.length} rejected_unbounded, ${capRejected.length} rejected_capped (cap=${familyCap}/family)`);
  log(`[inference] rejected_unbounded by family: ${formatFamilyCounts(unboundedByFamily)}`);
  log(`[inference] rejected_capped by family: ${formatFamilyCounts(cappedByFamily)}`);

  // ── Type gate ──────────────────────────────────────────────────────────────
  const elementMap = buildElementMap(ir);
  const { accepted, rejected } = applyTypeGate(relevantCandidates, elementMap);

  const rejectedByFamily = zeroFamilyCounts();
  for (const r of rejected) rejectedByFamily[r.relationFamily]++;

  log(`[inference] Type gate: ${accepted.length} accepted, ${rejected.length} rejected`);
  log(`[inference] Type gate rejections by family: ${formatFamilyCounts(rejectedByFamily)}`);

  // ── Dry run exit ───────────────────────────────────────────────────────────
  if (options.dryRun) {
    const stats: RunStats[] = ALL_RELATION_FAMILIES.map((family) => ({
      family,
      generated: countsByFamily[family],
      rejectedUnbounded: unboundedByFamily[family],
      rejectedCapped: cappedByFamily[family],
      rejectedType: rejectedByFamily[family],
      proposed: 0,
      proposalDeclined: 0,
      proposalParseError: 0,
      premiseRepaired: 0,
      droppedUnpremised: 0,
      autoRejected: 0,
      debate: 0,
      queued: 0,
    }));

    return {
      records: [...accepted, ...relevanceRejected, ...capRejected, ...rejected],
      stats,
      irHash,
      skippedSentinel: false,
      estimatedCostUsd: 0,
      emittedUnpremised: 0,
      droppedUnpremised: 0,
      taskFailures: [],
    };
  }

  // ── Cross-document enumeration (W2) — only when an entity store is supplied ─
  // Absent entityStore: crossDocAccepted stays [], crossDocContexts empty, and
  // resolvableIds === retrievedChunkIds — so the structural flow below is
  // BYTE-IDENTICAL to today's behavior. The demo supplies no entityStore.
  const crossDocContexts = new Map<string, ContextBundle>();
  const crossDocPremiseIds = new Set<string>();
  const crossDocAccepted: TypedCandidate[] = [];

  if (options.entityStore) {
    const es = options.entityStore;
    // Default enumeration families: the four structural families, the two
    // ENUMERABLE trace families (satisfy, derive), plus `containment` (the
    // structural composition spoke whose approval feeds the chain enumerator).
    // `verify` is intentionally omitted — no proposer enumerates it (see the
    // verify case in type-gate.ts).
    const families =
      es.families ??
      ([
        "allocation",
        "modeMembership",
        "flowTyping",
        "controlJoin",
        "satisfy",
        "derive",
        "containment",
      ] as RelationFamily[]);
    const entityNames = new Map(es.entities.map((e) => [e.entityId, e.canonicalName]));

    // Co-occurrence spokes (per-family caps logged inside).
    const { candidates: cooc } = enumerateCooccurrence(es.entities, es.mentions, {
      families,
      ...(es.minCooccur !== undefined ? { minCooccur: es.minCooccur } : {}),
      ...(options.familyCap !== undefined ? { familyCap: options.familyCap } : {}),
      log,
    });

    // 2-hop chains over ACCEPTED relations; illegal compositions rejected here.
    const rawChains = enumerateChains(es.acceptedRelations ?? []);
    const { accepted: chainAccepted, rejected: chainRejected } = applyChainTypeGate(rawChains.candidates);

    log(
      `[inference:crossdoc] cooccurrence=${cooc.length}, chains_accepted=${chainAccepted.length}, chains_rejected_composition=${chainRejected.length}, pending_relations_skipped=${rawChains.pendingSkipped}`,
    );

    const register = (
      id: string,
      family: RelationFamily,
      sourceId: string,
      targetId: string,
      premiseIds: readonly string[],
      ctx: ContextBundle,
    ) => {
      crossDocAccepted.push({ id, relationFamily: family, sourceId, targetId, stage: "typed" });
      for (const p of premiseIds) crossDocPremiseIds.add(p);
      // Endpoints themselves are resolvable premises (entity ids).
      crossDocPremiseIds.add(sourceId);
      crossDocPremiseIds.add(targetId);
      crossDocContexts.set(id, ctx);
    };

    for (const c of cooc) {
      register(
        c.id,
        c.relationFamily,
        c.sourceId,
        c.targetId,
        c.premiseIds,
        buildCrossDocContextBundle(
          { sourceId: c.sourceId, targetId: c.targetId, relationFamily: c.relationFamily, premiseIds: c.premiseIds, origin: "cooccurrence" },
          entityNames,
        ),
      );
    }
    for (const c of chainAccepted) {
      register(
        c.id,
        c.relationFamily,
        c.sourceId,
        c.targetId,
        c.premiseIds,
        buildCrossDocContextBundle(
          { sourceId: c.sourceId, targetId: c.targetId, relationFamily: c.relationFamily, premiseIds: c.premiseIds, origin: "chain" },
          entityNames,
        ),
      );
    }
  }

  // The full pipeline candidate set: structural (today's) + cross-doc (W2).
  // When entityStore is absent, this === `accepted` (same order, same contents).
  const pipelineAccepted: TypedCandidate[] = [...accepted, ...crossDocAccepted];

  // Resolvable-premise set: BM25 chunk ids ∪ cross-doc premise ids. When no
  // cross-doc premises exist, this is exactly `retrievedChunkIds` (unchanged).
  const resolvableIds =
    crossDocPremiseIds.size > 0
      ? new Set<string>([...(retrievedChunkIds ?? []), ...crossDocPremiseIds])
      : retrievedChunkIds;

  // ── Pre-flight cost estimate ───────────────────────────────────────────────
  const estimatedCostUsd = estimateCostUsd(pipelineAccepted.length);
  log(`[inference] Pre-flight estimate: ~${pipelineAccepted.length} propose calls, est. $${estimatedCostUsd.toFixed(4)} USD`);

  if (options.budgetUsd !== undefined && estimatedCostUsd > options.budgetUsd) {
    const msg = `[inference] ABORT: estimated cost $${estimatedCostUsd.toFixed(4)} exceeds budget $${options.budgetUsd.toFixed(4)}. Set INFER_BUDGET_USD or INFER_FORCE=1.`;
    log(msg);
    throw new Error(msg);
  }

  // ── Proposal pass + band routing + debate ────────────────────────────────
  const allRecords: CandidateRecord[] = [...relevanceRejected, ...capRejected, ...rejected];
  const statMap = new Map<RelationFamily, RunStats>();
  for (const family of ALL_RELATION_FAMILIES) {
    statMap.set(family, {
      family,
      generated: countsByFamily[family],
      rejectedUnbounded: unboundedByFamily[family],
      rejectedCapped: cappedByFamily[family],
      rejectedType: rejectedByFamily[family],
      proposed: 0,
      proposalDeclined: 0,
      proposalParseError: 0,
      premiseRepaired: 0,
      droppedUnpremised: 0,
      autoRejected: 0,
      debate: 0,
      queued: 0,
    });
  }

  let emittedUnpremised = 0;
  let droppedUnpremised = 0;

  const concurrency = resolveInferConcurrency(options.concurrency);
  log(`[inference] Running propose+debate with concurrency=${concurrency}`);

  /**
   * Per-candidate task: runs propose → premise check → band route → debate.
   * Returns an optional CandidateRecord (null = skipped/error with no record).
   * All counter mutations are deferred to the accumulation pass below to keep
   * JS's single-threaded event loop as the sole synchroniser (no explicit locks needed).
   */
  type CandidateOutcome = {
    record: CandidateRecord | null;
    /** Deltas accumulated per-family for statMap */
    statDelta: {
      family: RelationFamily;
      proposed: number;
      proposalDeclined: number;
      proposalParseError: number;
      premiseRepaired: number;
      droppedUnpremised: number;
      autoRejected: number;
      debate: number;
      queued: number;
    };
    droppedUnpremised: number;
  };

  const tasks: Array<() => Promise<CandidateOutcome>> = pipelineAccepted.map((candidate) => async () => {
    const delta = {
      family: candidate.relationFamily,
      proposed: 0,
      proposalDeclined: 0,
      proposalParseError: 0,
      premiseRepaired: 0,
      droppedUnpremised: 0,
      autoRejected: 0,
      debate: 0,
      queued: 0,
    };

    // Cross-doc candidates carry a precomputed entity-aware context (their
    // endpoints are entity ids the IR-centric bundle can't resolve). Structural
    // candidates fall through to today's IR neighborhood bundle unchanged.
    const context =
      crossDocContexts.get(candidate.id) ??
      buildContextBundle(candidate.sourceId, candidate.targetId, ir, retrievalIndex);

    // ── Propose ────────────────────────────────────────────────────────────
    let proposeResult: import("./types.js").ProposeResult;
    try {
      proposeResult = await provider.propose(
        candidate.relationFamily,
        candidate.sourceId,
        candidate.targetId,
        context
      );
    } catch (err: unknown) {
      // Failure isolation: transport/SDK error — counted as parse_error so
      // declined-vs-failure is always distinguishable in the stats.
      log(`[inference] propose error for ${candidate.id}: ${err instanceof Error ? err.message : String(err)} — counted proposal_parse_error`);
      delta.proposalParseError++;
      return { record: null, statDelta: delta, droppedUnpremised: 0 };
    }

    if (proposeResult.kind === "declined") {
      // Provider explicitly declined to propose — counted, no record
      delta.proposalDeclined++;
      return { record: null, statDelta: delta, droppedUnpremised: 0 };
    }

    if (proposeResult.kind === "parse_error") {
      // Response failed JSON/schema parsing — counted + logged with detail
      log(`[inference] proposal_parse_error: ${candidate.id} (${proposeResult.detail})`);
      delta.proposalParseError++;
      return { record: null, statDelta: delta, droppedUnpremised: 0 };
    }

    let proposal = proposeResult.proposal;
    delta.proposed++;

    // ── Deterministic name→id premise repair (bounded to the offered bundle) ──
    const repair = repairPremises(proposal.premises, context.offeredFacts);
    if (repair.repairedCount > 0) {
      log(`[inference] premise_repaired: ${candidate.id} (${repair.repairedCount} name-citation${repair.repairedCount === 1 ? "" : "s"} → offered-fact id)`);
      delta.premiseRepaired += repair.repairedCount;
      proposal = { ...proposal, premises: repair.premises };
    }

    // ── Premise validation (A2: emittedUnpremised MUST stay 0) ────────────
    const premiseCheck = validatePremises(proposal.premises, ir, resolvableIds);
    if (!premiseCheck.valid) {
      const dropped: DroppedUnpremisedCandidate = {
        id: candidate.id,
        relationFamily: candidate.relationFamily,
        sourceId: candidate.sourceId,
        targetId: candidate.targetId,
        stage: "dropped_unpremised",
        unresolvablePremises: premiseCheck.unresolvable,
      };
      log(`[inference] dropped_unpremised: ${candidate.id} (unresolvable: ${premiseCheck.unresolvable.join(", ")})`);
      delta.droppedUnpremised++;
      return { record: dropped, statDelta: delta, droppedUnpremised: 1 };
    }

    // ── Band routing (A3) ───────────────────────────────────────────────
    const band = classifyBand(proposal.confidence);

    if (band === "auto_rejected") {
      const record: AutoRejectedRecord = {
        id: candidate.id,
        relationFamily: candidate.relationFamily,
        sourceId: candidate.sourceId,
        targetId: candidate.targetId,
        stage: "auto_rejected",
        confidence: proposal.confidence,
        premises: proposal.premises,
        rationale: proposal.rationale, // audit-only
      };
      log(`[inference] auto_rejected: ${candidate.id} (conf=${proposal.confidence.toFixed(3)})`);
      delta.autoRejected++;
      return { record, statDelta: delta, droppedUnpremised: 0 };
    }

    if (band === "debate") {
      // Run adversarial debate for mid-confidence band.
      // Within a single proposal, advocate→challenger stays sequential
      // (challenger needs the advocate summary). Parallelism is ACROSS proposals.
      delta.debate++;
      let debateResult: import("./types.js").DebateResult;

      try {
        debateResult = await runDebate(provider, candidate.relationFamily, proposal, context);
      } catch (err: unknown) {
        // Failure isolation: uncertain, continue
        debateResult = {
          verdict: "uncertain",
          advocate: 0.5,
          challenger: 0.5,
          advocateSummary: `debate error: ${err instanceof Error ? err.message : String(err)}`,
          challengerSummary: "debate error",
        };
      }

      log(`[inference] debate: ${candidate.id} verdict=${debateResult.verdict} (adv=${debateResult.advocate.toFixed(2)}, chl=${debateResult.challenger.toFixed(2)})`);

      if (debateResult.verdict === "auto_rejected") {
        const record: AutoRejectedRecord = {
          id: candidate.id,
          relationFamily: candidate.relationFamily,
          sourceId: candidate.sourceId,
          targetId: candidate.targetId,
          stage: "auto_rejected",
          confidence: proposal.confidence,
          premises: proposal.premises,
          rationale: proposal.rationale, // audit-only
        };
        delta.autoRejected++;
        return { record, statDelta: delta, droppedUnpremised: 0 };
      }

      // confirmed or uncertain → queued with debate annotation
      const record: QueuedRecord = {
        id: candidate.id,
        relationFamily: candidate.relationFamily,
        sourceId: candidate.sourceId,
        targetId: candidate.targetId,
        stage: "queued",
        confidence: proposal.confidence,
        premises: proposal.premises,
        rationale: proposal.rationale, // audit-only
        debateVerdict: debateResult.verdict,
        debateAdvocate: debateResult.advocate,
        debateChallenger: debateResult.challenger,
        // Only set debateUncertain=true when the verdict is actually uncertain
        ...(debateResult.verdict === "uncertain" ? { debateUncertain: true } : {}),
      };
      delta.queued++;
      return { record, statDelta: delta, droppedUnpremised: 0 };
    }

    // band === "queued" (high confidence, no debate)
    const record: QueuedRecord = {
      id: candidate.id,
      relationFamily: candidate.relationFamily,
      sourceId: candidate.sourceId,
      targetId: candidate.targetId,
      stage: "queued",
      confidence: proposal.confidence,
      premises: proposal.premises,
      rationale: proposal.rationale, // audit-only
    };
    delta.queued++;
    return { record, statDelta: delta, droppedUnpremised: 0 };
  });

  // Run all tasks with bounded concurrency; results come back in original accepted order.
  // Failure isolation: an unexpected throw in a candidate task (outside the handled
  // propose/debate try/catch — e.g. context building) is captured per-task and does
  // NOT abort the run. Failed slots are `undefined` in `results` and recorded in
  // `taskFailures`; the accumulation below skips them.
  const { results: outcomes, failures: taskFailures } = await boundedPool(tasks, concurrency);

  if (taskFailures.length > 0) {
    log(`[inference] taskFailures: ${taskFailures.length} candidate task(s) threw unexpectedly and were skipped — indices [${taskFailures.map((f) => f.index).join(", ")}]`);
  }

  // ── Accumulate results in original order (deterministic output) ───────────
  for (const outcome of outcomes) {
    if (outcome === undefined) continue; // slot's task threw; recorded in taskFailures
    if (outcome.record !== null) {
      allRecords.push(outcome.record);
    }
    droppedUnpremised += outcome.droppedUnpremised;
    const s = statMap.get(outcome.statDelta.family)!;
    s.proposed += outcome.statDelta.proposed;
    s.proposalDeclined += outcome.statDelta.proposalDeclined;
    s.proposalParseError += outcome.statDelta.proposalParseError;
    s.premiseRepaired += outcome.statDelta.premiseRepaired;
    s.droppedUnpremised += outcome.statDelta.droppedUnpremised;
    s.autoRejected += outcome.statDelta.autoRejected;
    s.debate += outcome.statDelta.debate;
    s.queued += outcome.statDelta.queued;
  }

  // Per-family null-proposal split (declined vs parse error) + repair summary
  for (const s of statMap.values()) {
    if (s.proposalDeclined + s.proposalParseError + s.premiseRepaired > 0) {
      log(`[inference] ${s.family}: proposal_declined=${s.proposalDeclined}, proposal_parse_error=${s.proposalParseError}, premise_repaired=${s.premiseRepaired}`);
    }
  }

  // ── Invariant check (defensive): emittedUnpremised MUST be 0 ─────────────
  // (We already drop them; this is a defense-in-depth assertion)
  const queuedRecords = allRecords.filter((r): r is QueuedRecord => r.stage === "queued");
  for (const r of queuedRecords) {
    const check = validatePremises(r.premises, ir, resolvableIds);
    if (!check.valid) {
      emittedUnpremised++;
      log(`[inference] BUG: queued record ${r.id} has unresolvable premises: ${check.unresolvable.join(", ")}`);
    }
  }

  // Surface the provider's LLM parse-failure counts (debate advocate/challenge
  // 0.5-default sites) in the run result, if the provider exposes them.
  const parseFailures = provider.counters?.snapshot();

  return {
    records: allRecords,
    stats: Array.from(statMap.values()),
    irHash,
    skippedSentinel: false,
    estimatedCostUsd,
    emittedUnpremised,
    droppedUnpremised,
    taskFailures,
    ...(parseFailures ? { parseFailures } : {}),
  };
}
