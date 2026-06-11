/**
 * types.ts — shared types for the F8 inference engine.
 *
 * Candidate records flow through the stage machine:
 *   raw → typed (type gate) → proposal | dropped_unpremised
 *      → auto_rejected | debate | queued
 *      → (debate) confirmed | auto_rejected | uncertain
 *
 * All stage-annotated records are written to inference-candidates.json
 * (gitignored; corpus-derived). Idempotent ids keyed on stableId(family+source+target).
 */

import { z } from "zod";

export type RelationFamily = "allocation" | "modeMembership" | "flowTyping" | "controlJoin";

// ── Typed candidate (passed type gate, pre-LLM) ─────────────────────────────

export interface TypedCandidate {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "typed";
}

// ── Rejected by type gate ────────────────────────────────────────────────────

export interface RejectedCandidate {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "rejected_type";
  reasonCode: string; // e.g. "rejected_type:allocation.source_not_leaf_function"
  reason: string;     // human-readable explanation
}

// ── Rejected by relevance filter (unbounded) / per-family cap ───────────────

export interface RelevanceRejectedCandidate {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "rejected_unbounded";
  reasonCode: string; // rejected_unbounded:<signal-miss>
  reason: string;
}

export interface CappedCandidate {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "rejected_capped";
  reasonCode: "rejected_capped";
  reason: string;
}

// ── Dropped (unpremised) ────────────────────────────────────────────────────

export interface DroppedUnpremisedCandidate {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "dropped_unpremised";
  unresolvablePremises: string[]; // the premise ids that didn't resolve
}

// ── Proposal from LLM (zod schema for structured output) ────────────────────

export const ProposalOutputSchema = z.object({
  sourceId: z.string(),
  targetId: z.string(),
  relationFamily: z.enum(["allocation", "modeMembership", "flowTyping", "controlJoin"]),
  /** Composed-IR ids this proposal is reasoned from — at least 1 */
  premises: z.array(z.string()).min(1),
  /** Audit-only: reasoning for the link (NEVER surfaced in tool results / exports) */
  rationale: z.string(),
  /** 0.0–1.0 model confidence */
  confidence: z.number().min(0).max(1),
});

export type ProposalOutput = z.infer<typeof ProposalOutputSchema>;

// ── Confidence bands (A3) ───────────────────────────────────────────────────

export const CONF_FLOOR = 0.40;   // below → auto_rejected
export const CONF_DEBATE_MAX = 0.70; // [FLOOR, DEBATE_MAX) → debate stage
// ≥ DEBATE_MAX → queued directly

export type BandLabel = "auto_rejected" | "debate" | "queued";

export function classifyBand(confidence: number): BandLabel {
  if (confidence < CONF_FLOOR) return "auto_rejected";
  if (confidence < CONF_DEBATE_MAX) return "debate";
  return "queued";
}

// ── Staged records (written to candidates file) ──────────────────────────────

export interface AutoRejectedRecord {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "auto_rejected";
  confidence: number;
  premises: string[];
  rationale: string; // audit-only
}

export interface QueuedRecord {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "queued";
  confidence: number;
  premises: string[];
  rationale: string; // audit-only
  debateVerdict?: "confirmed" | "uncertain"; // set if went through debate
  debateAdvocate?: number;
  debateChallenger?: number;
  debateUncertain?: boolean; // true if debate was uncertain (still queued)
}

export interface DebateRecord {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  stage: "debate";
  confidence: number;
  premises: string[];
  rationale: string; // audit-only
}

// ── Debate verdict ──────────────────────────────────────────────────────────

export type DebateVerdict = "confirmed" | "auto_rejected" | "uncertain";

export interface DebateResult {
  verdict: DebateVerdict;
  advocate: number;
  challenger: number;
  /** Audit-only prose — NEVER surface in tool results / exports */
  advocateSummary: string;
  challengerSummary: string;
}

// ── Inference run context (for provider) ────────────────────────────────────

export interface ContextBundle {
  /** Serialized 1-hop neighborhood of the source element */
  sourceNeighborhood: string;
  /** Serialized 1-hop neighborhood of the target element */
  targetNeighborhood: string;
  /** Relevant corpus/prose quotes for source + target */
  corpusQuotes: string[];
}

// ── Union of all stage-annotated candidate records ──────────────────────────

export type CandidateRecord =
  | TypedCandidate
  | RejectedCandidate
  | RelevanceRejectedCandidate
  | CappedCandidate
  | DroppedUnpremisedCandidate
  | AutoRejectedRecord
  | DebateRecord
  | QueuedRecord;

// ── Run stats (for logging and dry-run output) ───────────────────────────────

export interface RunStats {
  family: RelationFamily;
  generated: number;
  /** Rejected by the relevance filter (no corpus signal / already stated). */
  rejectedUnbounded: number;
  /** Rejected by the per-family cap (INFER_FAMILY_CAP, default 150). */
  rejectedCapped: number;
  rejectedType: number;
  proposed: number;
  droppedUnpremised: number;
  autoRejected: number;
  debate: number;
  queued: number;
}
