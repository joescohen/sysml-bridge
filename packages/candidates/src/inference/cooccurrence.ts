/**
 * cooccurrence.ts — cross-document co-occurrence ("spokes") enumeration
 * (W2, spec §4).
 *
 * For each declared relation family, enumerate (entityA, entityB) pairs whose
 * MENTIONS co-occur — either in ≥ `minCooccur` shared chunks, or within the same
 * `sectionPath` prefix. Endpoints are canonical entity ids (never raw names); the
 * co-occurrence signal is the mentions' chunk + sectionPath citations.
 *
 * Typed by the EXISTING type gate (`checkTypeGate`) via an entity-aware element
 * map (`buildEntityElementMap`) — the gate rules are unchanged. Per-family caps
 * via `resolveFamilyCap`, with dropped-count LOGGING (no silent caps). The
 * co-occurring chunk ids become the candidate's citable premises (resolvable via
 * the engine's `extraResolvableIds`, exactly like BM25 chunks).
 *
 * PURE + deterministic: entities iterate in input order; pairs iterate by
 * (i<j) index; families in declared order; each pair contributes forward then
 * reverse direction. Same inputs → byte-identical candidate sequence (asserted).
 */

import { createHash } from "node:crypto";
import type { EntityRecord } from "../entities/cluster.js";
import type { MentionRecord } from "../mentions/index.js";
import type { RelationFamily } from "./types.js";
import { checkTypeGate, buildEntityElementMap } from "./type-gate.js";
import { resolveFamilyCap } from "./relevance-filter.js";

/** Deterministic stable id for a co-occurrence candidate. */
export function cooccurrenceStableId(
  family: RelationFamily,
  sourceId: string,
  targetId: string,
): string {
  const input = `cooccur:${family}:${sourceId}:${targetId}`;
  const hex = createHash("sha256").update(input, "utf8").digest("hex");
  return `cooccur-${hex.slice(0, 16)}`;
}

/** A co-occurrence candidate that passed the entity-aware type gate. */
export interface CooccurrenceCandidate {
  id: string;
  relationFamily: RelationFamily;
  /** Canonical entity id of the source endpoint. */
  sourceId: string;
  /** Canonical entity id of the target endpoint. */
  targetId: string;
  /** Co-occurring chunk ids — the citable, resolvable premises for the pair. */
  premiseIds: string[];
  /** How the pair co-occurs: shared chunk(s) and/or shared section prefix. */
  cooccurKind: "chunk" | "section" | "chunk+section";
  stage: "typed_cooccurrence";
}

export interface EnumerateCooccurrenceOptions {
  /** Relation families to enumerate spokes for (declared by the caller). */
  families: readonly RelationFamily[];
  /** Minimum shared-chunk count for chunk co-occurrence. Default 1. */
  minCooccur?: number;
  /** Per-family cap (default from INFER_FAMILY_CAP env, else 150). */
  familyCap?: number;
  /** Logger for dropped-count reporting. Default: stderr. */
  log?: (msg: string) => void;
}

export interface EnumerateCooccurrenceResult {
  candidates: CooccurrenceCandidate[];
  /** Per-family count of candidates dropped by the cap (never silent). */
  droppedByFamily: Record<string, number>;
}

// ── Section-path prefix helper ────────────────────────────────────────────────

/** Split a sectionPath into ordered segments (delimiters: `>`, `/`). */
function sectionSegments(path: string): string[] {
  return path
    .split(/\s*>\s*|\/+/)
    .map((s) => s.trim().toLowerCase())
    .filter((s) => s.length > 0);
}

/** True iff `a`'s segments are a (non-empty) prefix of `b`'s, or vice versa. */
function segmentsSharePrefix(a: string[], b: string[]): boolean {
  if (a.length === 0 || b.length === 0) return false;
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

// ── Per-entity mention footprint ──────────────────────────────────────────────

interface EntityFootprint {
  entity: EntityRecord;
  /** Chunk ids this entity is mentioned in (first-seen order). */
  chunkIds: string[];
  chunkSet: Set<string>;
  /** Distinct section paths this entity is mentioned in. */
  sectionPaths: string[];
}

function buildFootprints(
  entities: readonly EntityRecord[],
  mentions: readonly MentionRecord[],
): EntityFootprint[] {
  const byMentionId = new Map<string, MentionRecord>();
  for (const m of mentions) byMentionId.set(m.mentionId, m);

  return entities.map((entity) => {
    const chunkIds: string[] = [];
    const chunkSet = new Set<string>();
    const sectionPaths: string[] = [];
    const sectionSet = new Set<string>();
    for (const mid of entity.mentionIds) {
      const m = byMentionId.get(mid);
      if (m === undefined) continue; // mention not in store — skip (gate covers dangling)
      const cid = m.citation.chunkId;
      if (!chunkSet.has(cid)) {
        chunkSet.add(cid);
        chunkIds.push(cid);
      }
      const sp = m.citation.sectionPath;
      if (!sectionSet.has(sp)) {
        sectionSet.add(sp);
        sectionPaths.push(sp);
      }
    }
    return { entity, chunkIds, chunkSet, sectionPaths };
  });
}

/**
 * Compute the co-occurrence signal for an ordered entity pair. Returns the
 * shared chunk ids (sorted, deterministic), whether a section-prefix match
 * exists, and the derived premise ids. `null` when the pair does not co-occur.
 */
function cooccurSignal(
  a: EntityFootprint,
  b: EntityFootprint,
  minCooccur: number,
): { premiseIds: string[]; kind: CooccurrenceCandidate["cooccurKind"] } | null {
  // Shared chunks (chunk co-occurrence).
  const shared: string[] = [];
  for (const cid of a.chunkIds) {
    if (b.chunkSet.has(cid)) shared.push(cid);
  }
  const chunkCooccur = shared.length >= minCooccur;

  // Section-prefix co-occurrence.
  const aSegs = a.sectionPaths.map(sectionSegments);
  const bSegs = b.sectionPaths.map(sectionSegments);
  let sectionCooccur = false;
  outer: for (const as of aSegs) {
    for (const bs of bSegs) {
      if (segmentsSharePrefix(as, bs)) {
        sectionCooccur = true;
        break outer;
      }
    }
  }

  if (!chunkCooccur && !sectionCooccur) return null;

  // Premises: the shared chunks (chunk co-occurrence). When co-occurrence is
  // ONLY via section prefix, fall back to the union of both entities' chunk ids
  // (deterministic: a's order then b's new ids) so the pair still carries
  // resolvable evidence premises.
  let premiseIds: string[];
  if (shared.length > 0) {
    premiseIds = [...shared].sort();
  } else {
    const union: string[] = [...a.chunkIds];
    const seen = new Set(a.chunkIds);
    for (const cid of b.chunkIds) {
      if (!seen.has(cid)) {
        seen.add(cid);
        union.push(cid);
      }
    }
    premiseIds = union;
  }

  const kind: CooccurrenceCandidate["cooccurKind"] =
    chunkCooccur && sectionCooccur ? "chunk+section" : chunkCooccur ? "chunk" : "section";

  return { premiseIds, kind };
}

/**
 * Enumerate co-occurrence spokes. Deterministic and pure.
 */
export function enumerateCooccurrence(
  entities: readonly EntityRecord[],
  mentions: readonly MentionRecord[],
  options: EnumerateCooccurrenceOptions,
): EnumerateCooccurrenceResult {
  const log = options.log ?? ((msg: string) => process.stderr.write(msg + "\n"));
  const minCooccur = options.minCooccur ?? 1;
  const familyCap = resolveFamilyCap(options.familyCap);
  const elementMap = buildEntityElementMap(entities);
  const footprints = buildFootprints(entities, mentions);

  // Survivors per family, in deterministic enumeration order (pre-cap).
  const survivorsByFamily = new Map<RelationFamily, CooccurrenceCandidate[]>();
  for (const f of options.families) survivorsByFamily.set(f, []);

  for (let i = 0; i < footprints.length; i++) {
    for (let j = i + 1; j < footprints.length; j++) {
      const a = footprints[i]!;
      const b = footprints[j]!;
      const signal = cooccurSignal(a, b, minCooccur);
      if (signal === null) continue;

      // Each co-occurring pair contributes both directions per family; the type
      // gate keeps only the well-typed direction(s).
      for (const family of options.families) {
        for (const [src, tgt] of [
          [a.entity.entityId, b.entity.entityId],
          [b.entity.entityId, a.entity.entityId],
        ] as const) {
          const gate = checkTypeGate(family, src, tgt, elementMap);
          if (!gate.pass) continue;
          survivorsByFamily.get(family)!.push({
            id: cooccurrenceStableId(family, src, tgt),
            relationFamily: family,
            sourceId: src,
            targetId: tgt,
            premiseIds: signal.premiseIds,
            cooccurKind: signal.kind,
            stage: "typed_cooccurrence",
          });
        }
      }
    }
  }

  // Per-family cap with dropped-count LOGGING (no silent caps).
  const candidates: CooccurrenceCandidate[] = [];
  const droppedByFamily: Record<string, number> = {};
  for (const family of options.families) {
    const survivors = survivorsByFamily.get(family)!;
    if (survivors.length > familyCap) {
      const dropped = survivors.length - familyCap;
      droppedByFamily[family] = dropped;
      log(
        `[cooccurrence] cap: family=${family} kept=${familyCap} dropped=${dropped} (cap=${familyCap}/family, INFER_FAMILY_CAP)`,
      );
      candidates.push(...survivors.slice(0, familyCap));
    } else {
      droppedByFamily[family] = 0;
      candidates.push(...survivors);
    }
  }

  return { candidates, droppedByFamily };
}
