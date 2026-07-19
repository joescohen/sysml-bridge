/**
 * pass-runner.ts — the bounded, targeted inference leg of a gap-driven pass (W3).
 *
 * Given the queries planned by `gap-queue.ts`, this runs a SINGLE bounded
 * inference pass (spec §5 step 2) over the cross-document entity store and
 * attributes the resulting queued candidates back to the gaps they close.
 *
 * Boundedness + no silent caps (spec §2): the budget cap is forwarded to the
 * engine and LOGGED; the per-query scoping is LOGGED (how many entities each
 * query retrieved). Nothing is capped silently.
 *
 * Targeting: each query retrieves a scope of entities by lexical overlap
 * (`nameTokens` Jaccard, the SAME tokenizer the relevance filter uses) between
 * the gap element's name+text and each entity's canonical name / aliases. A
 * queued candidate "targets" a gap iff either of its endpoints is in that gap's
 * retrieved entity scope. Candidates that target no gap are NOT reported as
 * pass proposals (the pass is targeted, not a full re-inference).
 *
 * This module NEVER writes dispositions and imports NO approval writer — it only
 * produces proposals for the normal review queue (the caller persists them). The
 * no-auto-approve ratchet therefore stays green.
 */

import { runInferenceEngine, type EntityStoreInput, type EngineResult } from "../inference/engine.js";
import type { InferenceProvider } from "../inference/inference-provider.js";
import type { RelationFamily, QueuedRecord } from "../inference/types.js";
import type { AcceptedRelation } from "../inference/chains.js";
import { nameTokens } from "../inference/relevance-filter.js";
import type { EntityRecord } from "../entities/cluster.js";
import type { MentionRecord } from "../mentions/index.js";
import type { InferredComposedIR } from "@sysml-bridge/model";

import type { WeaveQuery } from "./gap-queue.js";
import type { ProposedCandidateSummary } from "./pass-record.js";

/** Inputs to a targeted inference pass. */
export interface TargetedInferenceInput {
  /** Composed IR the engine enumerates structural candidates over. */
  ir: InferredComposedIR;
  /** Injected provider (mock in tests / no-key runs, Anthropic when keyed). */
  provider: InferenceProvider;
  /** The queries planned from the completeness findings. */
  queries: readonly WeaveQuery[];
  /** Canonical entities (entities.json). */
  entities: readonly EntityRecord[];
  /** Mentions (mentions.json) — the co-occurrence signal. */
  mentions: readonly MentionRecord[];
  /** Accepted relations (corpus-backed + human-approved) — NEVER pending. */
  acceptedRelations?: readonly AcceptedRelation[];
  /** Relation families to enumerate co-occurrence spokes for (default all four). */
  families?: readonly RelationFamily[];
  /** Shared-chunk threshold for co-occurrence (default 1). */
  minCooccur?: number;
  /** Budget cap in USD — forwarded to the engine and LOGGED. */
  budgetUsd?: number;
  /** Dry run — generate + type gate only, no LLM spend. */
  dryRun?: boolean;
  /** Log sink (defaults to stderr). */
  log?: (msg: string) => void;
}

export interface TargetedInferenceResult {
  irHash: string;
  engineResult: EngineResult;
  /** Queued candidates that target ≥1 gap, attributed to those gaps. */
  proposedCandidates: ProposedCandidateSummary[];
  /** gapElementId → the entity ids that gap's query retrieved (audit trail). */
  scopeByGap: Record<string, string[]>;
}

/**
 * Retrieve the entity scope for a query: every entity whose canonical name or
 * an alias shares ≥1 significant token with the query text. Deterministic —
 * entities are scanned in input order. Pure.
 */
export function scopeEntitiesToQuery(
  query: WeaveQuery,
  entities: readonly EntityRecord[],
): string[] {
  const queryTokens = nameTokens(query.bm25Query);
  if (queryTokens.size === 0) return [];
  const scoped: string[] = [];
  for (const entity of entities) {
    const surfaces = [entity.canonicalName, ...entity.aliases];
    const hit = surfaces.some((s) => {
      for (const t of nameTokens(s)) {
        if (queryTokens.has(t)) return true;
      }
      return false;
    });
    if (hit) scoped.push(entity.entityId);
  }
  return scoped;
}

/**
 * Run one bounded, targeted inference pass. Enumerates cross-document candidates
 * over the entity store (behind the engine's `entityStore` seam), then attributes
 * queued candidates to the gaps whose entity scope contains one of their
 * endpoints.
 */
export async function runTargetedInference(
  input: TargetedInferenceInput,
): Promise<TargetedInferenceResult> {
  const log = input.log ?? ((msg: string) => process.stderr.write(msg + "\n"));

  // ── Per-query entity scopes (LOGGED — no silent scoping) ────────────────────
  const scopeByGap: Record<string, string[]> = {};
  const entityToGaps = new Map<string, Set<string>>();
  for (const query of input.queries) {
    const scoped = scopeEntitiesToQuery(query, input.entities);
    scopeByGap[query.gapElementId] = scoped;
    for (const entityId of scoped) {
      (entityToGaps.get(entityId) ?? entityToGaps.set(entityId, new Set()).get(entityId)!).add(
        query.gapElementId,
      );
    }
    log(
      `[weave] query ${query.findingRuleId}(${query.gapElementId}) family=${query.family} ` +
        `retrieved ${scoped.length} entit${scoped.length === 1 ? "y" : "ies"}`,
    );
  }

  // ── Bounded inference (budget LOGGED — no silent cap) ───────────────────────
  log(
    `[weave] bounded targeted inference: ${input.queries.length} quer${
      input.queries.length === 1 ? "y" : "ies"
    }, budget cap = ${input.budgetUsd === undefined ? "none" : `$${input.budgetUsd}`}` +
      `${input.dryRun ? " (dry-run: no LLM spend)" : ""}`,
  );

  const entityStore: EntityStoreInput = {
    entities: input.entities,
    mentions: input.mentions,
    acceptedRelations: input.acceptedRelations,
    families: input.families,
    minCooccur: input.minCooccur,
  };

  const engineResult = await runInferenceEngine(input.ir, input.provider, {
    entityStore,
    budgetUsd: input.budgetUsd,
    dryRun: input.dryRun,
    log,
  });

  // ── Attribute queued candidates to the gaps they target ─────────────────────
  const proposedCandidates: ProposedCandidateSummary[] = [];
  for (const record of engineResult.records) {
    if (record.stage !== "queued") continue;
    const q = record as QueuedRecord;
    const gaps = new Set<string>();
    for (const gapSet of [entityToGaps.get(q.sourceId), entityToGaps.get(q.targetId)]) {
      if (gapSet) for (const g of gapSet) gaps.add(g);
    }
    if (gaps.size === 0) continue; // not targeted at any gap — not a pass proposal
    proposedCandidates.push({
      id: q.id,
      relationFamily: q.relationFamily,
      sourceId: q.sourceId,
      targetId: q.targetId,
      targetsGapElementIds: [...gaps].sort(),
    });
  }

  log(
    `[weave] ${proposedCandidates.length} candidate${
      proposedCandidates.length === 1 ? "" : "s"
    } target a gap (of ${engineResult.records.filter((r) => r.stage === "queued").length} queued)`,
  );

  return { irHash: engineResult.irHash, engineResult, proposedCandidates, scopeByGap };
}
