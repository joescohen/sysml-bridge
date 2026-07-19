/**
 * suggest.ts — Band 2 of entity resolution: SUGGESTED merges (proposals only).
 *
 * These heuristics require JUDGMENT, so per the no-auto-approve invariant (spec
 * §2) they NEVER merge — each emits an `EntityMergeCandidate` that enters the
 * existing human review queue as the `entity-merge` candidate kind. Two
 * dependency-free suggesters:
 *
 *   - ACRONYM / expansion: an initial-letter match — "FCM" ↔ "Flight Control
 *     Module". Deterministic, exact.
 *   - TOKEN-OVERLAP: Jaccard over `nameTokens` (reused from the relevance filter
 *     — lowercase, split, min-len-4, stopword-stripped) ≥ `TOKEN_OVERLAP_MIN`.
 *
 * Both are PURE and DETERMINISTIC over the entity set: pairs are enumerated in a
 * stable (i<j) order; proposals are content-addressed by the unordered pair key
 * (`entityMergePairKey`) and de-duplicated. A `skipPairKeys` set (already
 * approved OR rejected merges) filters re-proposals — a rejected pair is never
 * re-asked. The mid-band LLM adjudication is a SEPARATE band (./adjudicate.ts),
 * never invoked here (this stays synchronous + provider-free for determinism).
 */

import { entityMergePairKey, type EntityMergeCandidate } from "@sysml-bridge/model";
import { nameTokens } from "../inference/relevance-filter.js";
import type { EntityRecord } from "./cluster.js";

/** Jaccard token-overlap at or above this → a token-overlap merge proposal. */
export const TOKEN_OVERLAP_MIN = 0.5;

/** Confidence stamped on an acronym proposal (mid-band — needs human judgment). */
export const ACRONYM_CONFIDENCE = 0.6;

export interface SuggestMergesOptions {
  /** Content-addressed pair keys already approved OR rejected — never re-proposed. */
  skipPairKeys?: ReadonlySet<string>;
  /**
   * Optional mentionId → quote map. When present, evidence quotes are drawn from
   * each entity's mentions; otherwise evidence falls back to the alias forms.
   */
  mentionQuotes?: ReadonlyMap<string, string>;
  /** Max evidence quotes per side (default 3). */
  maxEvidence?: number;
}

/**
 * Enumerate suggested merges over an entity set. Deterministic and pure.
 * Two entities are only ever proposed for merge when their kinds are COMPATIBLE
 * (equal, or one is "unknown"): an acronym and its expansion are the same thing,
 * not two different-kinded things sharing letters.
 */
export function suggestMerges(
  entities: readonly EntityRecord[],
  options: SuggestMergesOptions = {}
): EntityMergeCandidate[] {
  const skip = options.skipPairKeys ?? new Set<string>();
  const maxEvidence = options.maxEvidence ?? 3;
  const out: EntityMergeCandidate[] = [];
  const emitted = new Set<string>();

  for (let i = 0; i < entities.length; i++) {
    for (let j = i + 1; j < entities.length; j++) {
      const a = entities[i]!;
      const b = entities[j]!;
      if (!kindsCompatible(a.kind, b.kind)) continue;

      const pairKey = entityMergePairKey(a.entityId, b.entityId);
      if (skip.has(pairKey) || emitted.has(pairKey)) continue;

      const acronym = acronymMatch(a, b);
      const jaccard = tokenOverlap(a, b);
      const isTokenOverlap = jaccard >= TOKEN_OVERLAP_MIN;
      if (!acronym && !isTokenOverlap) continue;

      // Acronym is the stronger, exact signal; prefer it as the reason.
      const reason: EntityMergeCandidate["reason"] = acronym ? "acronym" : "token-overlap";
      const confidence = acronym ? ACRONYM_CONFIDENCE : jaccard;

      emitted.add(pairKey);
      out.push(
        buildProposal(a, b, pairKey, reason, confidence, options.mentionQuotes, maxEvidence)
      );
    }
  }

  return out;
}

// ── Kind compatibility ───────────────────────────────────────────────────────

function kindsCompatible(a: EntityRecord["kind"], b: EntityRecord["kind"]): boolean {
  return a === b || a === "unknown" || b === "unknown";
}

// ── Acronym / expansion suggester ────────────────────────────────────────────

/** Any alias-pair (across the two entities) that reads as acronym ↔ expansion. */
export function acronymMatch(a: EntityRecord, b: EntityRecord): boolean {
  for (const sa of a.aliases) {
    for (const sb of b.aliases) {
      if (isAcronymExpansion(sa, sb) || isAcronymExpansion(sb, sa)) return true;
    }
  }
  return false;
}

/** True iff `short` is the initial-letter acronym of the multi-word `long`. */
export function isAcronymExpansion(short: string, long: string): boolean {
  const words = long.trim().split(/\s+/).filter((w) => /^[A-Za-z]/.test(w));
  if (words.length < 2) return false; // an acronym expands a MULTI-word name
  const initials = words.map((w) => w[0]!.toLowerCase()).join("");
  const compact = short.toLowerCase().replace(/[^a-z0-9]/g, "");
  return compact.length >= 2 && compact === initials;
}

// ── Token-overlap suggester ──────────────────────────────────────────────────

/** Max Jaccard token-overlap over the entities' alias sets. */
export function tokenOverlap(a: EntityRecord, b: EntityRecord): number {
  const ta = unionTokens(a.aliases);
  const tb = unionTokens(b.aliases);
  return jaccard(ta, tb);
}

function unionTokens(aliases: readonly string[]): Set<string> {
  const out = new Set<string>();
  for (const s of aliases) for (const t of nameTokens(s)) out.add(t);
  return out;
}

function jaccard(a: Set<string>, b: Set<string>): number {
  if (a.size === 0 && b.size === 0) return 0;
  let inter = 0;
  for (const t of a) if (b.has(t)) inter++;
  const union = a.size + b.size - inter;
  return union === 0 ? 0 : inter / union;
}

// ── Proposal builder ─────────────────────────────────────────────────────────

function buildProposal(
  a: EntityRecord,
  b: EntityRecord,
  pairKey: string,
  reason: EntityMergeCandidate["reason"],
  confidence: number,
  mentionQuotes: ReadonlyMap<string, string> | undefined,
  maxEvidence: number
): EntityMergeCandidate {
  // Default canonical name = the more-attested entity's canonical (more mentions);
  // tie → the lower-sorted entityId's, for determinism. Human-editable at approval.
  const aWins =
    a.mentionIds.length > b.mentionIds.length ||
    (a.mentionIds.length === b.mentionIds.length && a.entityId <= b.entityId);
  const canonicalName = aWins ? a.canonicalName : b.canonicalName;
  const kind = a.kind === "unknown" ? b.kind : a.kind;

  return {
    id: pairKey,
    entityIdA: a.entityId,
    entityIdB: b.entityId,
    kind,
    canonicalName,
    aliases: dedupe([...a.aliases, ...b.aliases]),
    mentionIds: dedupe([...a.mentionIds, ...b.mentionIds]),
    reason,
    evidence: {
      aQuotes: evidenceFor(a, mentionQuotes, maxEvidence),
      bQuotes: evidenceFor(b, mentionQuotes, maxEvidence),
    },
    confidence,
  };
}

function evidenceFor(
  e: EntityRecord,
  mentionQuotes: ReadonlyMap<string, string> | undefined,
  max: number
): string[] {
  if (mentionQuotes) {
    const quotes: string[] = [];
    for (const id of e.mentionIds) {
      const q = mentionQuotes.get(id);
      if (q) quotes.push(q);
      if (quotes.length >= max) break;
    }
    if (quotes.length > 0) return quotes;
  }
  return e.aliases.slice(0, max);
}

function dedupe(xs: readonly string[]): string[] {
  return [...new Set(xs)];
}
