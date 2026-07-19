/**
 * relevance-filter.ts — Deterministic relevance filter stage (post-generation,
 * pre-type-gate). Bounds candidate enumeration by corpus signals so generation
 * is never all-pairs (spec §2: "bounded by graph locality, not all-pairs";
 * Risk #1 popularity bias, Risk #4 queue volume).
 *
 * SIGNALS USED (documented per conductor instruction):
 *
 * allocation — a (leaf function, component) pair survives iff at least one of:
 *   (a) name-token overlap: normalized (lowercase, split on non-alphanumeric),
 *       stopword-stripped, tokens of length ≥4; ≥1 shared token between the
 *       function name and the component name.
 *   (b) flow co-occurrence: the function's L2 parent (natural key extracted from
 *       the owner label, e.g. "F1: Manage Refueling Requests" → "F1") appears as
 *       an endpoint label in functional-scope N2 triples; the EXACT normalized
 *       flow labels of those triples are intersected with the exact normalized
 *       flow labels of component-scope N2 triples touching the component itself.
 *       Non-empty intersection → signal.
 *       NOTE: subsystem-aggregated flow matching was prototyped and rejected as
 *       too permissive on the ANGARS corpus (926/1836 survived vs 356/1836 with
 *       component-own flows); token-level (vs exact-label) flow matching was also
 *       rejected (1221/1836). Signal (c) (satisfy-chain co-occurrence) from the
 *       conductor's list is NOT implemented — (a)+(b) meet the minimum bar.
 *
 * controlJoin — sibling-bounded pairs are kept EXCEPT pairs already connected by
 *   an approved prose succession (kind="succession", fields.fromAction/toAction
 *   matching the pair's function names or natural keys, case-insensitive) —
 *   those are already corpus-stated and must not be re-proposed
 *   (reason: rejected_unbounded:succession_already_stated).
 *
 * modeMembership / flowTyping — pass-through (already bounded by the small
 *   approved-mode / prose-interface counts).
 *
 * PER-FAMILY CAP — after filtering, each family is capped (INFER_FAMILY_CAP env,
 * default 150). Excess candidates are kept highest-signal-first (signal strength
 * = matched token count + matched flow count, descending; deterministic
 * tie-break on candidate stable id) and the rest are recorded as rejected_capped.
 */

import type { InferredComposedIR } from "@sysml-bridge/model";
import type { RawCandidate } from "./candidate-generator.js";
import type { RelationFamily, RelevanceRejectedCandidate, CappedCandidate } from "./types.js";

export type { RelevanceRejectedCandidate, CappedCandidate } from "./types.js";

// ── Normalization helpers ────────────────────────────────────────────────────

/** Generic/structural words that carry no allocation signal. */
const STOPWORDS = new Set([
  "the", "and", "for", "with", "from", "data", "system", "module",
  "unit", "control", "subsystem", "management",
]);

const MIN_TOKEN_LEN = 4;

/** Lowercase, split on non-alphanumeric, drop stopwords and short tokens. */
export function nameTokens(s: string): Set<string> {
  const out = new Set<string>();
  for (const t of s.toLowerCase().split(/[^a-z0-9]+/)) {
    if (t.length >= MIN_TOKEN_LEN && !STOPWORDS.has(t)) out.add(t);
  }
  return out;
}

/** Exact-match normalization for flow labels: lowercase, collapse separators. */
export function normalizeFlowLabel(s: string): string {
  return s.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean).join(" ");
}

/** Extract the L2 parent natural key from an owner label, e.g. "F1: Manage…" → "F1". */
export function extractL2Key(owner: string): string | null {
  const m = owner.match(/^([A-Za-z]+\d+(?:\.\d+)*)/);
  return m ? m[1]! : null;
}

// ── Signal scoring ───────────────────────────────────────────────────────────

export interface AllocationSignalScore {
  signalA: boolean;
  signalB: boolean;
  /** Tokens shared between function name and component name (signal a). */
  matchedTokens: string[];
  /** Normalized flow labels shared via the L2-parent bridge (signal b). */
  matchedFlows: string[];
  /** Combined strength: matchedTokens.length + matchedFlows.length. */
  strength: number;
}

interface AllocationSignalIndex {
  fnTokensById: Map<string, Set<string>>;
  fnL2FlowsById: Map<string, Set<string>>;
  compTokensById: Map<string, Set<string>>;
  compFlowsById: Map<string, Set<string>>;
}

function buildAllocationSignalIndex(ir: InferredComposedIR): AllocationSignalIndex {
  const corpus = ir.extracted;
  const functions = corpus.functions ?? [];
  const components = corpus.components ?? [];
  const n2 = corpus.n2Interfaces ?? [];

  // L2 label (e.g. "F1") → exact normalized flows on functional-scope triples
  const l2Flows = new Map<string, Set<string>>();
  for (const t of n2) {
    if (t.scope !== "functional") continue;
    const flow = normalizeFlowLabel(t.flow);
    for (const label of [t.sourceLabel, t.targetLabel]) {
      if (!l2Flows.has(label)) l2Flows.set(label, new Set());
      l2Flows.get(label)!.add(flow);
    }
  }

  // component id → exact normalized flows on component-scope triples touching it
  const compFlowsById = new Map<string, Set<string>>();
  for (const t of n2) {
    if (t.scope !== "component") continue;
    const flow = normalizeFlowLabel(t.flow);
    for (const id of [t.sourceId, t.targetId]) {
      if (!compFlowsById.has(id)) compFlowsById.set(id, new Set());
      compFlowsById.get(id)!.add(flow);
    }
  }

  const fnTokensById = new Map<string, Set<string>>();
  const fnL2FlowsById = new Map<string, Set<string>>();
  for (const fn of functions) {
    fnTokensById.set(fn.id, nameTokens(fn.name));
    const key = extractL2Key(fn.owner);
    fnL2FlowsById.set(fn.id, (key !== null && l2Flows.get(key)) || new Set());
  }

  const compTokensById = new Map<string, Set<string>>();
  for (const c of components) {
    compTokensById.set(c.id, nameTokens(c.name));
  }

  return { fnTokensById, fnL2FlowsById, compTokensById, compFlowsById };
}

function scoreWithIndex(
  index: AllocationSignalIndex,
  sourceId: string,
  targetId: string
): AllocationSignalScore {
  const fnToks = index.fnTokensById.get(sourceId) ?? new Set<string>();
  const compToks = index.compTokensById.get(targetId) ?? new Set<string>();
  const matchedTokens = [...fnToks].filter((t) => compToks.has(t)).sort();

  const fnFlows = index.fnL2FlowsById.get(sourceId) ?? new Set<string>();
  const compFlows = index.compFlowsById.get(targetId) ?? new Set<string>();
  const matchedFlows = [...fnFlows].filter((f) => compFlows.has(f)).sort();

  return {
    signalA: matchedTokens.length > 0,
    signalB: matchedFlows.length > 0,
    matchedTokens,
    matchedFlows,
    strength: matchedTokens.length + matchedFlows.length,
  };
}

/**
 * Score the allocation signals for a single (function, component) pair.
 * Convenience wrapper (tests, audits); the batch filter builds the index once.
 */
export function scoreAllocationSignals(
  sourceId: string,
  targetId: string,
  ir: InferredComposedIR
): AllocationSignalScore {
  return scoreWithIndex(buildAllocationSignalIndex(ir), sourceId, targetId);
}

// ── controlJoin: succession-already-stated detection ─────────────────────────

/**
 * Build the set of "already stated" (sourceId, targetId) pairs from approved
 * prose successions. fromAction/toAction may reference a function by name or
 * by natural key; matching is case-insensitive.
 */
function buildStatedSuccessionPairs(ir: InferredComposedIR): Set<string> {
  const functions = ir.extracted.functions ?? [];

  // name (lower) / naturalKey (lower) → function id
  const lookup = new Map<string, string>();
  for (const fn of functions) {
    lookup.set(fn.name.toLowerCase(), fn.id);
    lookup.set(fn.naturalKey.toLowerCase(), fn.id);
  }

  const stated = new Set<string>();
  for (const entry of ir.proseEntries) {
    if (entry.kind !== "succession") continue;
    if (entry.status !== "approved") continue;
    const from = entry.fields["fromAction"];
    const to = entry.fields["toAction"];
    if (typeof from !== "string" || typeof to !== "string") continue;
    const fromId = lookup.get(from.toLowerCase());
    const toId = lookup.get(to.toLowerCase());
    if (fromId && toId) stated.add(`${fromId}→${toId}`);
  }
  return stated;
}

// ── Filter output types ──────────────────────────────────────────────────────

export interface RelevanceFilterResult {
  /** Candidates that survive the filter AND the per-family cap. */
  kept: RawCandidate[];
  /** Candidates rejected by the relevance signals (reason-coded, logged). */
  rejected: RelevanceRejectedCandidate[];
  /** Candidates that survived signals but exceeded the per-family cap. */
  capped: CappedCandidate[];
}

export interface RelevanceFilterOptions {
  /** Per-family cap; default from INFER_FAMILY_CAP env, else 150. */
  familyCap?: number;
}

export const DEFAULT_FAMILY_CAP = 150;

export function resolveFamilyCap(explicit?: number): number {
  if (explicit !== undefined) return explicit;
  const env = process.env["INFER_FAMILY_CAP"];
  if (env !== undefined) {
    const n = Number.parseInt(env, 10);
    if (Number.isFinite(n) && n > 0) return n;
  }
  return DEFAULT_FAMILY_CAP;
}

// ── Main filter ──────────────────────────────────────────────────────────────

/**
 * Apply the relevance filter + per-family cap to a batch of raw candidates.
 * Candidates may span multiple families; the cap applies per family.
 */
export function applyRelevanceFilter(
  candidates: RawCandidate[],
  ir: InferredComposedIR,
  options: RelevanceFilterOptions = {}
): RelevanceFilterResult {
  const familyCap = resolveFamilyCap(options.familyCap);
  const allocationIndex = buildAllocationSignalIndex(ir);
  const statedPairs = buildStatedSuccessionPairs(ir);

  const rejected: RelevanceRejectedCandidate[] = [];
  const capped: CappedCandidate[] = [];

  // survivors per family with their signal strength (for cap ordering)
  const survivorsByFamily = new Map<RelationFamily, Array<{ cand: RawCandidate; strength: number }>>();
  const push = (cand: RawCandidate, strength: number) => {
    if (!survivorsByFamily.has(cand.family)) survivorsByFamily.set(cand.family, []);
    survivorsByFamily.get(cand.family)!.push({ cand, strength });
  };

  for (const cand of candidates) {
    switch (cand.family) {
      case "allocation": {
        const score = scoreWithIndex(allocationIndex, cand.sourceId, cand.targetId);
        if (!score.signalA && !score.signalB) {
          rejected.push({
            id: cand.stableId,
            relationFamily: cand.family,
            sourceId: cand.sourceId,
            targetId: cand.targetId,
            stage: "rejected_unbounded",
            reasonCode: "rejected_unbounded:no_signal",
            reason:
              "no corpus signal links this pair: no name-token overlap (signal a) and no L2-parent functional-flow ∩ component-flow co-occurrence (signal b)",
          });
        } else {
          push(cand, score.strength);
        }
        break;
      }
      case "controlJoin": {
        if (statedPairs.has(`${cand.sourceId}→${cand.targetId}`)) {
          rejected.push({
            id: cand.stableId,
            relationFamily: cand.family,
            sourceId: cand.sourceId,
            targetId: cand.targetId,
            stage: "rejected_unbounded",
            reasonCode: "rejected_unbounded:succession_already_stated",
            reason:
              "pair is already connected by an approved prose succession — corpus-stated flow must not be re-proposed as inference",
          });
        } else {
          push(cand, 0);
        }
        break;
      }
      // modeMembership / flowTyping: pass through (already bounded)
      default:
        push(cand, 0);
        break;
    }
  }

  // ── Per-family cap: keep highest-signal first, deterministic tie-break ─────
  const kept: RawCandidate[] = [];
  for (const [, survivors] of survivorsByFamily) {
    survivors.sort((a, b) => {
      if (b.strength !== a.strength) return b.strength - a.strength;
      return a.cand.stableId < b.cand.stableId ? -1 : a.cand.stableId > b.cand.stableId ? 1 : 0;
    });
    for (let i = 0; i < survivors.length; i++) {
      const { cand } = survivors[i]!;
      if (i < familyCap) {
        kept.push(cand);
      } else {
        capped.push({
          id: cand.stableId,
          relationFamily: cand.family,
          sourceId: cand.sourceId,
          targetId: cand.targetId,
          stage: "rejected_capped",
          reasonCode: "rejected_capped",
          reason: `per-family cap ${familyCap} exceeded (rank ${i + 1}); kept highest-signal first`,
        });
      }
    }
  }

  return { kept, rejected, capped };
}
