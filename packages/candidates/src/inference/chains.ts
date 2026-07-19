/**
 * chains.ts — 2-hop chain enumeration over ALREADY-ACCEPTED relations (W2, spec §4).
 *
 * A chain composes two accepted relations that share a middle entity:
 *
 *     A --left--> B    ∘    B --right--> C    ⇒    A --result--> C
 *
 * HARD constraints (spec §2 / §4):
 *   - 2-hop ONLY. No 3+-hop transitive closure.
 *   - Constituents MUST be already-accepted relations — corpus-backed or
 *     human-approved inferred. A relation whose `status` is `"pending"` (an
 *     unapproved proposal) is NEVER composed: feeding only pending relations
 *     yields ZERO chain candidates.
 *   - The composed family is decided by the explicit `COMPOSITION_TABLE`
 *     (see composition-table.ts) via the chain type gate. This module ENUMERATES
 *     every 2-hop path (regardless of table legality) so the type gate can
 *     reject an illegal composition explicitly — the enumerator never silently
 *     drops a composition.
 *   - Premises of the composed candidate = the two constituent relation ids plus
 *     their evidence chunk ids. No new evidence is fabricated.
 *
 * PURE + deterministic: relations are processed in input order; the emitted
 * chain order is (r1-index, r2-index); same input → byte-identical output.
 */

import type { InferredApprovedEntry } from "@sysml-bridge/model";
import { chainStableId, type RawChainCandidate } from "./composition-table.js";

/**
 * An accepted (or pending) relation between two canonical entities — the input
 * substrate for chain composition. `status` gates composition: only `"accepted"`
 * relations are ever composed.
 */
export interface AcceptedRelation {
  /** Content-addressed relation id — becomes a citable premise of the chain. */
  id: string;
  /** Constituent family name (may be a corpus family outside the 4 pipeline families). */
  family: string;
  /** Source canonical entity id. */
  sourceId: string;
  /** Target canonical entity id. */
  targetId: string;
  /**
   * `"accepted"` = corpus-backed or human-approved (composable).
   * `"pending"`  = an unapproved proposal — NEVER composed.
   */
  status: "accepted" | "pending";
  /** Evidence chunk ids backing this relation — folded into the chain's premises. */
  evidenceChunkIds?: readonly string[];
}

export interface EnumerateChainsResult {
  candidates: RawChainCandidate[];
  /** Count of input relations skipped because they were pending (not accepted). */
  pendingSkipped: number;
}

/**
 * Enumerate 2-hop chain candidates from accepted relations.
 *
 * For each accepted relation r1 (A→B) and each accepted relation r2 (B→C) whose
 * source equals r1's target (shared middle B), emit one raw chain candidate
 * A→C tagged with (r1.family, r2.family). Self-chains (A === C) are excluded.
 *
 * Determinism: r1 iterates in input order; for each r1, r2 iterates in input
 * order — so the candidate sequence is a pure function of the input ordering.
 */
export function enumerateChains(
  relations: readonly AcceptedRelation[],
): EnumerateChainsResult {
  const accepted: AcceptedRelation[] = [];
  let pendingSkipped = 0;
  for (const r of relations) {
    if (r.status === "accepted") accepted.push(r);
    else pendingSkipped++;
  }

  const candidates: RawChainCandidate[] = [];
  const seen = new Set<string>();

  for (const r1 of accepted) {
    for (const r2 of accepted) {
      if (r1.targetId !== r2.sourceId) continue; // must share the middle entity B
      if (r1.sourceId === r2.targetId) continue; // no self-chain A→…→A
      const stableId = chainStableId(
        r1.family,
        r2.family,
        r1.sourceId,
        r1.targetId,
        r2.targetId,
      );
      if (seen.has(stableId)) continue; // dedup identical composed path
      seen.add(stableId);

      const premiseIds = [
        r1.id,
        r2.id,
        ...(r1.evidenceChunkIds ?? []),
        ...(r2.evidenceChunkIds ?? []),
      ];

      candidates.push({
        stableId,
        leftFamily: r1.family,
        rightFamily: r2.family,
        sourceId: r1.sourceId,
        middleId: r1.targetId,
        targetId: r2.targetId,
        premiseIds,
      });
    }
  }

  return { candidates, pendingSkipped };
}

/**
 * Project HUMAN-APPROVED inferred entries into `AcceptedRelation`s the chain
 * enumerator can compose (weaver-containment brief §B).
 *
 * This is the wiring that makes an approved inferred CONTAINMENT (or allocation,
 * etc.) become composable substrate in the NEXT weave pass: a human approves a
 * containment proposal → it lands in `inferred-approved.json` with status
 * `"approved"` → this projection turns it into an `AcceptedRelation`
 * (`family:"containment"`, `status:"accepted"`) → the chain enumerator composes
 * `allocation ∘ containment → allocation`. Mirrors `projectInferredTraceRelationships`
 * (model) which projects the SAME entries to flat SysML trace relationships — this
 * is its chain-substrate counterpart.
 *
 * Invariants preserved:
 *   - NEVER composes a pending proposal: only entries whose on-disk `status` is
 *     `"approved"` project. `"suspect"`/`"superseded"` are excluded (a superseded
 *     entry — one named in another entry's `supersedes`, or itself marked
 *     `superseded` — is never emitted).
 *   - Determinism: entries iterate in input order; the relation `id` is the entry
 *     id (content-addressed upstream); `evidenceChunkIds` carry the entry premises
 *     through so the composed chain's premises stay resolvable.
 *   - The family is carried through UNCHANGED (a plain `string`), so any approved
 *     family can be a chain constituent; the chain type gate (COMPOSITION_TABLE)
 *     still decides which compositions are legal.
 */
export function projectApprovedInferredToRelations(
  entries: readonly InferredApprovedEntry[],
): AcceptedRelation[] {
  // Ids superseded by a later entry are excluded (mirror composeIR's rule).
  const supersededIds = new Set<string>();
  for (const e of entries) {
    if (e.supersedes) supersededIds.add(e.supersedes);
  }

  const out: AcceptedRelation[] = [];
  for (const entry of entries) {
    if (entry.status !== "approved") continue; // never compose suspect/superseded
    if (supersededIds.has(entry.id)) continue; // excluded by a later supersede
    out.push({
      id: entry.id,
      family: entry.relationFamily,
      sourceId: entry.sourceId,
      targetId: entry.targetId,
      status: "accepted",
      evidenceChunkIds: [...entry.premises],
    });
  }
  return out;
}
