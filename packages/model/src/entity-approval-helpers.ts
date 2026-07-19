/**
 * entity-approval-helpers.ts — Human-approval gate for entity-merge proposals (W1).
 *
 * The W1 entity resolver produces THREE bands (see the corpus-weaver design §3):
 *   1. deterministic auto-cluster — no judgment, no approval needed;
 *   2. suggested merges (acronym / token-overlap / debate) — a PROPOSAL only;
 *   3. human disposition — approve records the merge, reject records the pair.
 * Bands 2/3 are fuzzy/LLM-driven, so per the no-auto-approve invariant they may
 * never write a merge without an explicit human decision. These helpers execute
 * that decision — they perform NO merge judgement themselves.
 *
 * appendEntityMerge: validate a merge candidate, stamp approval fields, append to
 *   entity-approved.json (append-only; never overwrites). The disposition id is
 *   CONTENT-ADDRESSED from the unordered entity-id pair (`entityMergePairKey`),
 *   so approving the same merge twice is idempotent-by-id and the record is
 *   independent of approvedBy / timestamp — mirroring appendApproval's stableId.
 *
 * recordEntityRejection: record a merge's CONTENT-ADDRESSED PAIR KEY in
 *   entity-rejections.json so the SAME suggestion is never re-proposed (mirrors
 *   the inferred-rejection triple pattern — the pair, not a mint id, is the
 *   identity). Idempotent.
 *
 * isEntityMergeApproved / isEntityMergeRejected: skip predicates for the
 *   re-proposal filter.
 *
 * NO AUTO-APPROVAL PATH EXISTS. Called ONLY from the review-ui human gate (or a
 * /mbse-approve-equivalent skill) after an explicit approve/reject click.
 */

import { z } from "zod";
import { promises as fs } from "node:fs";
import { stableId } from "./stable-id.js";

// ---------------------------------------------------------------------------
// Content-addressed pair key — the shared identity of a merge of two entities
// ---------------------------------------------------------------------------

/**
 * The content-addressed identity of a merge between two entities: a stableId over
 * the UNORDERED entity-id pair. merge(A,B) === merge(B,A). This same key is the
 * approved entry's id AND the rejection record's key, so a rejected pair is never
 * re-proposed and an approved pair is idempotent-by-id — independent of which
 * side proposed it, the canonical name chosen, or the timestamp.
 */
export function entityMergePairKey(entityIdA: string, entityIdB: string): string {
  const [a, b] = [entityIdA, entityIdB].sort();
  return stableId("entity-merge", `${a}:${b}`);
}

// ---------------------------------------------------------------------------
// EntityMergeCandidate — shape of a pending merge proposal from the entities module
// ---------------------------------------------------------------------------

// Mirrors the candidates-side `MentionKind` union (the model package cannot
// import from candidates). Keep in sync: adding a MentionKind means adding it here.
const MERGE_KINDS = [
  "component",
  "function",
  "requirement",
  "need",
  "mode",
  "interface",
  "flow",
  "verification",
  "unknown",
] as const;

const MergeEvidenceSchema = z.object({
  aQuotes: z.array(z.string()),
  bQuotes: z.array(z.string()),
});

export const EntityMergeCandidateSchema = z.object({
  /** Content-addressed pair key (== entityMergePairKey(entityIdA, entityIdB)). */
  id: z.string(),
  /** The two entities proposed for merge (order-insensitive identity). */
  entityIdA: z.string(),
  entityIdB: z.string(),
  /** Coarse kind of the merged entity (may be human-reclassified at approval). */
  kind: z.enum(MERGE_KINDS),
  /**
   * Canonical name for the merged entity. Default = the most-frequent surface
   * form; human-editable at approval time (§9 decision — a text input suffices).
   */
  canonicalName: z.string(),
  /** All merged surface forms across both entities. */
  aliases: z.array(z.string()),
  /** Full provenance — every mentionId reachable from either entity. */
  mentionIds: z.array(z.string()),
  /** Which suggester produced this proposal. */
  reason: z.enum(["acronym", "token-overlap", "debate"]),
  /** Evidence quotes from both sides (audit context for the human). */
  evidence: MergeEvidenceSchema.optional(),
  /** Proposal confidence at generation time. */
  confidence: z.number(),
});

export type EntityMergeCandidate = z.infer<typeof EntityMergeCandidateSchema>;

// ---------------------------------------------------------------------------
// EntityMergeApprovedEntry — on-disk approved-merge record
// ---------------------------------------------------------------------------

export const EntityMergeApprovedEntrySchema = z.object({
  /** Content-addressed disposition id == entityMergePairKey(entityIdA, entityIdB). */
  id: z.string(),
  entityIdA: z.string(),
  entityIdB: z.string(),
  /** Kind of the merged entity — MAY differ from the candidate kind if the human
   *  reclassified it at approval time (§9 decision: reclassification is admitted). */
  kind: z.enum(MERGE_KINDS),
  /** Human-chosen (or defaulted) canonical name for the merged entity. */
  canonicalName: z.string(),
  aliases: z.array(z.string()),
  mentionIds: z.array(z.string()),
  reason: z.enum(["acronym", "token-overlap", "debate"]),
  confidence: z.number(),
  /** The candidate id this disposition executed (== id — the pair key). */
  candidateId: z.string(),
  /** PROV wasAttributedTo — the human approver. */
  approvedBy: z.string(),
  /** ISO datetime of approval. */
  approvedAt: z.string(),
  status: z.literal("approved"),
});

export type EntityMergeApprovedEntry = z.infer<typeof EntityMergeApprovedEntrySchema>;

const EntityApprovedFileSchema = z.object({
  entries: z.array(EntityMergeApprovedEntrySchema),
});

const EntityRejectionsFileSchema = z.object({
  /** Content-addressed pair keys of rejected merges — never re-proposed. */
  rejectedPairKeys: z.array(z.string()),
});

// ---------------------------------------------------------------------------
// appendEntityMerge
// ---------------------------------------------------------------------------

/**
 * Validate a merge candidate, build an EntityMergeApprovedEntry, and append it to
 * entity-approved.json (creating the file if absent). Append-only.
 *
 * The entry id is DERIVED from the unordered entity-id pair via
 * `entityMergePairKey` — content-addressed, deterministic, independent of
 * approvedBy / approvedAt / canonicalName. An optional `canonicalName` /
 * `kind` override lets the human edit the canonical name or reclassify the kind
 * at approval time (§9) without changing the merge's identity.
 *
 * @param candidate    The merge candidate to approve
 * @param approvedBy   Human user identity (typically git config user.name)
 * @param approvedPath Path to entity-approved.json (created if absent)
 * @param _rejectionsPath Path to entity-rejections.json (reserved; not read here)
 * @param overrides    Optional human edits: canonicalName and/or kind
 */
export async function appendEntityMerge(
  candidate: EntityMergeCandidate,
  approvedBy: string,
  approvedPath: string,
  _rejectionsPath: string,
  overrides?: { canonicalName?: string; kind?: EntityMergeApprovedEntry["kind"] }
): Promise<EntityMergeApprovedEntry> {
  const parsed = EntityMergeCandidateSchema.parse(candidate);

  let existingEntries: EntityMergeApprovedEntry[] = [];
  try {
    const raw = await fs.readFile(approvedPath, "utf8");
    const file = EntityApprovedFileSchema.parse(JSON.parse(raw));
    existingEntries = file.entries;
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
      const isParseError = err instanceof SyntaxError || err instanceof z.ZodError;
      if (isParseError) throw err;
    }
    existingEntries = [];
  }

  // Content-addressed disposition id — the unordered pair, never the candidate's
  // reported id (recomputed here so a forged candidate id cannot mislabel a merge).
  const entryId = entityMergePairKey(parsed.entityIdA, parsed.entityIdB);

  const entry: EntityMergeApprovedEntry = {
    id: entryId,
    entityIdA: parsed.entityIdA,
    entityIdB: parsed.entityIdB,
    kind: overrides?.kind ?? parsed.kind,
    canonicalName: overrides?.canonicalName ?? parsed.canonicalName,
    aliases: parsed.aliases,
    mentionIds: parsed.mentionIds,
    reason: parsed.reason,
    confidence: parsed.confidence,
    candidateId: entryId,
    approvedBy,
    approvedAt: new Date().toISOString(),
    status: "approved",
  };

  EntityMergeApprovedEntrySchema.parse(entry);

  const newEntries = [...existingEntries, entry];
  await fs.writeFile(
    approvedPath,
    JSON.stringify({ entries: newEntries }, null, 2) + "\n",
    "utf8"
  );

  return entry;
}

// ---------------------------------------------------------------------------
// recordEntityRejection
// ---------------------------------------------------------------------------

/**
 * Record a merge's content-addressed PAIR KEY in entity-rejections.json so the
 * same suggestion is never re-proposed. Idempotent — recording the same pair key
 * twice is safe.
 *
 * @param pairKey        The content-addressed pair key (entityMergePairKey(...))
 * @param rejectionsPath Path to entity-rejections.json (created if absent)
 */
export async function recordEntityRejection(
  pairKey: string,
  rejectionsPath: string
): Promise<void> {
  let existingKeys: string[] = [];
  try {
    const raw = await fs.readFile(rejectionsPath, "utf8");
    const file = EntityRejectionsFileSchema.parse(JSON.parse(raw));
    existingKeys = file.rejectedPairKeys;
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
      const isParseError = err instanceof SyntaxError || err instanceof z.ZodError;
      if (isParseError) throw err;
    }
    existingKeys = [];
  }

  if (!existingKeys.includes(pairKey)) {
    existingKeys = [...existingKeys, pairKey];
  }

  await fs.writeFile(
    rejectionsPath,
    JSON.stringify({ rejectedPairKeys: existingKeys }, null, 2) + "\n",
    "utf8"
  );
}

// ---------------------------------------------------------------------------
// Skip predicates
// ---------------------------------------------------------------------------

/** True if this merge (by content-addressed pair key) has already been approved. */
export async function isEntityMergeApproved(
  pairKey: string,
  approvedPath: string
): Promise<boolean> {
  try {
    const raw = await fs.readFile(approvedPath, "utf8");
    const file = EntityApprovedFileSchema.parse(JSON.parse(raw));
    return file.entries.some((e) => e.id === pairKey);
  } catch {
    return false;
  }
}

/** True if this merge (by content-addressed pair key) has been rejected. */
export async function isEntityMergeRejected(
  pairKey: string,
  rejectionsPath: string
): Promise<boolean> {
  try {
    const raw = await fs.readFile(rejectionsPath, "utf8");
    const file = EntityRejectionsFileSchema.parse(JSON.parse(raw));
    return file.rejectedPairKeys.includes(pairKey);
  } catch {
    return false;
  }
}
