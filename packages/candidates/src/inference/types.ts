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
import type { ScoredChunk } from "../retrieval/bm25.js";

export type RelationFamily =
  | "allocation"
  | "modeMembership"
  | "flowTyping"
  | "controlJoin"
  // Requirement-trace families (W-relations). These are the cross-document trace
  // links the gap-driven weave loop proposes to close GATE02 completeness
  // warnings. `satisfy` and `derive` are enumerated over the entity store;
  // `verify` is a defined family (typed + mapped) but is NOT enumerated by a
  // proposer — see type-gate.ts / cooccurrence sourcing notes and the report.
  | "satisfy"
  | "derive"
  | "verify"
  // Structural containment (composition): source = parent component (or
  // subsystem), target = child component. Enumerated over the entity store like
  // satisfy/derive; co-occurrence is symmetric so BOTH directions are proposed
  // and the HUMAN GATE resolves which is the real parent→child (no auto-approve,
  // never guess direction). Unlike the flat-trace families, containment is a
  // STRUCTURAL membership (nested part usage) — it does NOT map to a flat SysML
  // trace relationship (see FAMILY_TO_RELATIONSHIP_TYPE), so it is intentionally
  // absent there. Its purpose here is to become an AcceptedRelation the chain
  // enumerator composes (`allocation ∘ containment → allocation`).
  | "containment";

/** Every relation family, in canonical declaration order. */
export const ALL_RELATION_FAMILIES: readonly RelationFamily[] = [
  "allocation",
  "modeMembership",
  "flowTyping",
  "controlJoin",
  "satisfy",
  "derive",
  "verify",
  "containment",
];

/** A per-family counter record initialised to zero for every family. */
export function zeroFamilyCounts(): Record<RelationFamily, number> {
  return {
    allocation: 0,
    modeMembership: 0,
    flowTyping: 0,
    controlJoin: 0,
    satisfy: 0,
    derive: 0,
    verify: 0,
    containment: 0,
  };
}

/** Render a per-family counter record as `family=count, ...` in canonical order. */
export function formatFamilyCounts(counts: Record<RelationFamily, number>): string {
  return ALL_RELATION_FAMILIES.map((f) => `${f}=${counts[f]}`).join(", ");
}

/** A retrieved corpus passage surfaced into the context bundle as evidence.
 *  Carries its chunkId + sectionPath so a premise citing it stays resolvable. */
export type RetrievedChunk = ScoredChunk;

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
  relationFamily: z.enum([
    "allocation",
    "modeMembership",
    "flowTyping",
    "controlJoin",
    "satisfy",
    "derive",
    "verify",
    "containment",
  ]),
  /** Composed-IR ids this proposal is reasoned from — at least 1 */
  premises: z.array(z.string()).min(1),
  /** Audit-only: reasoning for the link (NEVER surfaced in tool results / exports) */
  rationale: z.string(),
  /** 0.0–1.0 model confidence */
  confidence: z.number().min(0).max(1),
});

export type ProposalOutput = z.infer<typeof ProposalOutputSchema>;

// ── Propose result (distinguishes declined vs parse failure) ─────────────────

/**
 * Discriminated result of a propose() call:
 *   - "proposal"    — the model proposed a link (zod-validated)
 *   - "declined"    — the model explicitly returned no proposal ({"proposal": null})
 *   - "parse_error" — the response could not be parsed/validated (JSON or schema)
 */
export type ProposeResult =
  | { kind: "proposal"; proposal: ProposalOutput }
  | { kind: "declined" }
  | { kind: "parse_error"; detail: string };

// ── Offered fact (the premise id contract) ────────────────────────────────────

/**
 * One fact offered to the LLM in the propose context. Rendered in the prompt as:
 *   [id: <id>] <kind> "<name>" — <detail>
 * Premises MUST cite these ids; the deterministic name→id repair only matches
 * against facts that were offered (name + aliases), never the whole IR.
 */
export interface OfferedFact {
  id: string;
  kind: string;
  name: string;
  detail: string;
  /** Additional matchable labels for repair (e.g. naturalKey, "key: name"). */
  aliases?: string[];
}

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
  /**
   * The facts offered for premise citation — source, target, and their 1-hop
   * neighbors, each with its composed-IR id. The propose prompt renders these
   * as `[id: …]` lines; the premise repair only matches within this list.
   *
   * When a chunk store is supplied to the engine, BM25-retrieved chunks are
   * appended here as `kind: "chunk"` facts (id = chunkId) so the LLM can cite
   * them as premises that resolve.
   */
  offeredFacts: OfferedFact[];
  /**
   * BM25-retrieved corpus passages discussing the source/target concepts,
   * merged in as clearly-labeled evidence. Empty/omitted when no chunk store
   * was provided (falls back to exact-id quotes only).
   */
  retrievedEvidence?: RetrievedChunk[];
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
  /** Provider explicitly returned no proposal ({"proposal": null}). */
  proposalDeclined: number;
  /** Provider response failed JSON/schema parsing (or threw). */
  proposalParseError: number;
  /** Premises mechanically repaired from offered-fact name → composed-IR id. */
  premiseRepaired: number;
  droppedUnpremised: number;
  autoRejected: number;
  debate: number;
  queued: number;
}
