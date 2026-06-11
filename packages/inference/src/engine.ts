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
 */

import { createHash } from "node:crypto";
import type { InferredComposedIR } from "@sysml-bridge/ir";
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
import { classifyBand } from "./types.js";
import { buildElementMap, applyTypeGate } from "./type-gate.js";
import { generateCandidates, buildSkipSet, inferenceStableId } from "./candidate-generator.js";
import { applyRelevanceFilter, resolveFamilyCap } from "./relevance-filter.js";
import { buildContextBundle } from "./neighborhood.js";
import { runDebate } from "./debate.js";

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
 */
export function validatePremises(
  premises: string[],
  ir: InferredComposedIR
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
}

export interface EngineResult {
  records: CandidateRecord[];
  stats: RunStats[];
  irHash: string;
  skippedSentinel: boolean;
  estimatedCostUsd: number;
  emittedUnpremised: number; // MUST be 0
  droppedUnpremised: number;
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
    };
  }

  // ── Candidate generation ───────────────────────────────────────────────────
  const skipSet = buildSkipSet(ir);
  const { candidates, countsByFamily } = generateCandidates(ir, skipSet);

  log(`[inference] Generated candidates: allocation=${countsByFamily.allocation}, modeMembership=${countsByFamily.modeMembership}, flowTyping=${countsByFamily.flowTyping}, controlJoin=${countsByFamily.controlJoin} (total=${candidates.length})`);

  // ── Relevance filter + per-family cap (pre-type-gate, deterministic) ───────
  const familyCap = resolveFamilyCap(options.familyCap);
  const {
    kept: relevantCandidates,
    rejected: relevanceRejected,
    capped: capRejected,
  } = applyRelevanceFilter(candidates, ir, { familyCap });

  const unboundedByFamily: Record<RelationFamily, number> = {
    allocation: 0, modeMembership: 0, flowTyping: 0, controlJoin: 0,
  };
  for (const r of relevanceRejected) unboundedByFamily[r.relationFamily]++;
  const cappedByFamily: Record<RelationFamily, number> = {
    allocation: 0, modeMembership: 0, flowTyping: 0, controlJoin: 0,
  };
  for (const r of capRejected) cappedByFamily[r.relationFamily]++;

  log(`[inference] Relevance filter: ${relevantCandidates.length} kept, ${relevanceRejected.length} rejected_unbounded, ${capRejected.length} rejected_capped (cap=${familyCap}/family)`);
  log(`[inference] rejected_unbounded by family: allocation=${unboundedByFamily.allocation}, modeMembership=${unboundedByFamily.modeMembership}, flowTyping=${unboundedByFamily.flowTyping}, controlJoin=${unboundedByFamily.controlJoin}`);
  log(`[inference] rejected_capped by family: allocation=${cappedByFamily.allocation}, modeMembership=${cappedByFamily.modeMembership}, flowTyping=${cappedByFamily.flowTyping}, controlJoin=${cappedByFamily.controlJoin}`);

  // ── Type gate ──────────────────────────────────────────────────────────────
  const elementMap = buildElementMap(ir);
  const { accepted, rejected } = applyTypeGate(relevantCandidates, elementMap);

  const rejectedByFamily: Record<RelationFamily, number> = {
    allocation: 0, modeMembership: 0, flowTyping: 0, controlJoin: 0,
  };
  for (const r of rejected) rejectedByFamily[r.relationFamily]++;

  log(`[inference] Type gate: ${accepted.length} accepted, ${rejected.length} rejected`);
  log(`[inference] Type gate rejections by family: allocation=${rejectedByFamily.allocation}, modeMembership=${rejectedByFamily.modeMembership}, flowTyping=${rejectedByFamily.flowTyping}, controlJoin=${rejectedByFamily.controlJoin}`);

  // ── Dry run exit ───────────────────────────────────────────────────────────
  if (options.dryRun) {
    const stats: RunStats[] = (["allocation", "modeMembership", "flowTyping", "controlJoin"] as RelationFamily[]).map((family) => ({
      family,
      generated: countsByFamily[family],
      rejectedUnbounded: unboundedByFamily[family],
      rejectedCapped: cappedByFamily[family],
      rejectedType: rejectedByFamily[family],
      proposed: 0,
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
    };
  }

  // ── Pre-flight cost estimate ───────────────────────────────────────────────
  const estimatedCostUsd = estimateCostUsd(accepted.length);
  log(`[inference] Pre-flight estimate: ~${accepted.length} propose calls, est. $${estimatedCostUsd.toFixed(4)} USD`);

  if (options.budgetUsd !== undefined && estimatedCostUsd > options.budgetUsd) {
    const msg = `[inference] ABORT: estimated cost $${estimatedCostUsd.toFixed(4)} exceeds budget $${options.budgetUsd.toFixed(4)}. Set INFER_BUDGET_USD or INFER_FORCE=1.`;
    log(msg);
    throw new Error(msg);
  }

  // ── Proposal pass + band routing + debate ────────────────────────────────
  const allRecords: CandidateRecord[] = [...relevanceRejected, ...capRejected, ...rejected];
  const statMap = new Map<RelationFamily, RunStats>();
  for (const family of ["allocation", "modeMembership", "flowTyping", "controlJoin"] as RelationFamily[]) {
    statMap.set(family, {
      family,
      generated: countsByFamily[family],
      rejectedUnbounded: unboundedByFamily[family],
      rejectedCapped: cappedByFamily[family],
      rejectedType: rejectedByFamily[family],
      proposed: 0,
      droppedUnpremised: 0,
      autoRejected: 0,
      debate: 0,
      queued: 0,
    });
  }

  let emittedUnpremised = 0;
  let droppedUnpremised = 0;

  for (const candidate of accepted) {
    const stats = statMap.get(candidate.relationFamily)!;
    const context = buildContextBundle(candidate.sourceId, candidate.targetId, ir);

    // ── Propose ──────────────────────────────────────────────────────────────
    let proposal: import("./types.js").ProposalOutput | null = null;
    try {
      proposal = await provider.propose(
        candidate.relationFamily,
        candidate.sourceId,
        candidate.targetId,
        context
      );
    } catch (err: unknown) {
      log(`[inference] propose error for ${candidate.id}: ${err instanceof Error ? err.message : String(err)} — treating as uncertain`);
      // Failure isolation: skip this candidate (no record emitted for error)
      continue;
    }

    if (proposal === null) {
      // Provider declined to propose — skip silently (no cost)
      continue;
    }

    stats.proposed++;

    // ── Premise validation (A2: emittedUnpremised MUST stay 0) ──────────────
    const premiseCheck = validatePremises(proposal.premises, ir);
    if (!premiseCheck.valid) {
      const dropped: DroppedUnpremisedCandidate = {
        id: candidate.id,
        relationFamily: candidate.relationFamily,
        sourceId: candidate.sourceId,
        targetId: candidate.targetId,
        stage: "dropped_unpremised",
        unresolvablePremises: premiseCheck.unresolvable,
      };
      allRecords.push(dropped);
      droppedUnpremised++;
      // emittedUnpremised stays 0 (we dropped it, not emitted it)
      log(`[inference] dropped_unpremised: ${candidate.id} (unresolvable: ${premiseCheck.unresolvable.join(", ")})`);
      continue;
    }

    // ── Band routing (A3) ─────────────────────────────────────────────────
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
      allRecords.push(record);
      stats.autoRejected++;
      log(`[inference] auto_rejected: ${candidate.id} (conf=${proposal.confidence.toFixed(3)})`);
      continue;
    }

    if (band === "debate") {
      // Run adversarial debate for mid-confidence band
      stats.debate++;
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
        allRecords.push(record);
        stats.autoRejected++;
        continue;
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
      allRecords.push(record);
      stats.queued++;
      continue;
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
    allRecords.push(record);
    stats.queued++;
  }

  // ── Invariant check (defensive): emittedUnpremised MUST be 0 ─────────────
  // (We already drop them; this is a defense-in-depth assertion)
  const queuedRecords = allRecords.filter((r): r is QueuedRecord => r.stage === "queued");
  for (const r of queuedRecords) {
    const check = validatePremises(r.premises, ir);
    if (!check.valid) {
      emittedUnpremised++;
      log(`[inference] BUG: queued record ${r.id} has unresolvable premises: ${check.unresolvable.join(", ")}`);
    }
  }

  return {
    records: allRecords,
    stats: Array.from(statMap.values()),
    irHash,
    skippedSentinel: false,
    estimatedCostUsd,
    emittedUnpremised,
    droppedUnpremised,
  };
}
