/**
 * debate.ts — Adversarial debate stage for mid-confidence band proposals.
 *
 * Implements SEPAL's advocate/challenger pattern (adopted per spec §2 + §7):
 *   1. Advocate: argue FOR the link (sees premises + neighborhoods)
 *   2. Challenger: argue AGAINST (sees the advocate's summary)
 *   3. Deterministic verdict:
 *      - advocate ≥ 0.7 AND challenger < 0.5  → "confirmed"   (→ queued stage)
 *      - challenger ≥ 0.7                      → "auto_rejected"
 *      - else                                  → "uncertain"   (→ queued, marked)
 *
 * Failure isolation: an errored pair → "uncertain", loop continues.
 * Advocate/challenger prose is stored AUDIT-ONLY — never returned in tool results
 * or placed in exports (DEBAT-04 discipline).
 */

import type { InferenceProvider } from "./inference-provider.js";
import type { ProposalOutput, ContextBundle, DebateResult, DebateVerdict } from "./types.js";
import type { RelationFamily } from "./types.js";

// ── Debate thresholds (SEPAL-derived, deterministic) ─────────────────────────

export const DEBATE_ADVOCATE_CONFIRM = 0.7;   // advocate ≥ this → eligible for confirm
export const DEBATE_CHALLENGER_REJECT = 0.7;  // challenger ≥ this → auto_rejected
export const DEBATE_CHALLENGER_MAX_CONFIRM = 0.5; // challenger MUST be < this to confirm

/**
 * Determine the debate verdict deterministically from advocate and challenger scores.
 *
 * Logic (A4):
 *   - advocate ≥ 0.7 AND challenger < 0.5  → "confirmed"
 *   - challenger ≥ 0.7                      → "auto_rejected"
 *   - else                                  → "uncertain"
 */
export function computeDebateVerdict(advocate: number, challenger: number): DebateVerdict {
  if (challenger >= DEBATE_CHALLENGER_REJECT) return "auto_rejected";
  if (advocate >= DEBATE_ADVOCATE_CONFIRM && challenger < DEBATE_CHALLENGER_MAX_CONFIRM) {
    return "confirmed";
  }
  return "uncertain";
}

/**
 * Run the full debate for a single mid-confidence proposal.
 *
 * Failure isolation: any error in the advocate or challenger pass → returns an
 * "uncertain" result with the error noted in the audit summary.
 *
 * @param provider   The InferenceProvider to use for advocate/challenger
 * @param family     The relation family of the proposal
 * @param proposal   The proposal to debate
 * @param context    Context bundle (1-hop neighborhoods + corpus quotes)
 * @returns          DebateResult (verdict + scores + audit-only prose)
 */
export async function runDebate(
  provider: InferenceProvider,
  family: RelationFamily,
  proposal: ProposalOutput,
  context: ContextBundle
): Promise<DebateResult> {
  let advocateScore = 0.5;
  let advocateSummary = "advocate pass skipped (error)";
  let challengerScore = 0.5;
  let challengerSummary = "challenger pass skipped (error)";

  // ── Advocate pass ──────────────────────────────────────────────────────────
  try {
    const advocateResult = await provider.advocate(family, proposal, context);
    advocateScore = advocateResult.score;
    advocateSummary = advocateResult.summary;
  } catch (err: unknown) {
    // Failure isolation: mark uncertain, continue
    advocateSummary = `advocate error: ${err instanceof Error ? err.message : String(err)}`;
  }

  // ── Challenger pass ────────────────────────────────────────────────────────
  try {
    const challengerResult = await provider.challenge(
      family,
      proposal,
      advocateSummary,
      context
    );
    challengerScore = challengerResult.score;
    challengerSummary = challengerResult.summary;
  } catch (err: unknown) {
    challengerSummary = `challenger error: ${err instanceof Error ? err.message : String(err)}`;
  }

  // ── Deterministic verdict ─────────────────────────────────────────────────
  const verdict = computeDebateVerdict(advocateScore, challengerScore);

  return {
    verdict,
    advocate: advocateScore,
    challenger: challengerScore,
    // Audit-only prose — caller MUST NOT surface these in tool results or exports
    advocateSummary,
    challengerSummary,
  };
}
