/**
 * adjudicate.ts — Band 3 of entity resolution: mid-band LLM adjudication.
 *
 * For a merge proposal in the mid-confidence band, we reuse the EXISTING
 * advocate/challenger debate VERBATIM (spec §3): the advocate argues the two
 * entities are the SAME, the challenger argues they are DISTINCT, and the
 * verdict is decided by the SAME deterministic thresholds as inference debate
 * (`computeDebateVerdict` in ../inference/debate.ts). No new adjudication
 * machinery, no new verdict math — the entity-merge question is simply framed
 * into the `ProposalOutput` / `ContextBundle` the debate seam already consumes,
 * and `runDebate` is called unchanged.
 *
 * Reusing the `InferenceProvider` seam keeps this mock-testable with zero API
 * key (the mock returns fixed advocate/challenger scores; the verdict is a pure
 * function of those). Like every fuzzy/LLM band, the OUTPUT is only ever a
 * PROPOSAL — the verdict raises or lowers a suggestion's standing for the human,
 * it NEVER approves a merge.
 */

import { runDebate } from "../inference/debate.js";
import type { InferenceProvider } from "../inference/inference-provider.js";
import type { ContextBundle, DebateResult, ProposalOutput, RelationFamily } from "../inference/types.js";
import type { EntityRecord } from "./cluster.js";

/**
 * Debate machinery reuses a `RelationFamily` only to pick a per-family prompt;
 * entity-merge adjudication carries its framing in the proposal + context, so we
 * pass a fixed nominal family. (The advocate/challenger seam ignores the merge
 * framing beyond the neighborhoods/quotes we hand it — the deterministic verdict
 * is what we consume.)
 */
const MERGE_DEBATE_FAMILY: RelationFamily = "allocation";

export interface MergeEvidence {
  aQuotes: string[];
  bQuotes: string[];
}

/**
 * Adjudicate whether two entities should merge, reusing the advocate/challenger
 * debate. Returns the raw `DebateResult` (verdict + scores + audit-only prose).
 * A "confirmed" verdict strengthens the merge proposal; "auto_rejected" /
 * "uncertain" weaken it — but the result is advisory to the human gate, never a
 * disposition.
 */
export async function adjudicateEntityMerge(
  provider: InferenceProvider,
  a: EntityRecord,
  b: EntityRecord,
  evidence: MergeEvidence = { aQuotes: [], bQuotes: [] }
): Promise<DebateResult> {
  const proposal: ProposalOutput = {
    sourceId: a.entityId,
    targetId: b.entityId,
    relationFamily: MERGE_DEBATE_FAMILY,
    // Premises = the two entity ids under debate (≥1 required by the schema).
    premises: [a.entityId, b.entityId],
    rationale: `same-entity? "${a.canonicalName}" (${a.kind}) vs "${b.canonicalName}" (${b.kind})`,
    confidence: 0.5,
  };

  const context: ContextBundle = {
    sourceNeighborhood: describeSide(a, evidence.aQuotes),
    targetNeighborhood: describeSide(b, evidence.bQuotes),
    corpusQuotes: [...evidence.aQuotes, ...evidence.bQuotes],
    offeredFacts: [
      { id: a.entityId, kind: a.kind, name: a.canonicalName, detail: a.aliases.join("; ") },
      { id: b.entityId, kind: b.kind, name: b.canonicalName, detail: b.aliases.join("; ") },
    ],
  };

  return runDebate(provider, MERGE_DEBATE_FAMILY, proposal, context);
}

function describeSide(e: EntityRecord, quotes: readonly string[]): string {
  const aliasLine = `${e.canonicalName} (${e.kind}); aliases: ${e.aliases.join(", ")}`;
  return quotes.length > 0 ? `${aliasLine}\n${quotes.join("\n")}` : aliasLine;
}
