/**
 * approval-helpers.ts — Human-approval gate for the prose-ingest pipeline.
 *
 * appendApproval: validate + append a candidate to prose-approved.json.
 *   - Append-only: never overwrites existing entries.
 *   - Assigns stableId from candidate citation (content-addressed, deterministic).
 *   - Sets status:'approved', approvedAt (ISO), approvedBy, candidateId.
 *   - Propagates candidate.supersedes to the new entry if present.
 *
 * recordRejection: record a candidate id in prose-rejections.json.
 *   - Idempotent: recording the same id twice leaves a single entry.
 *   - Append-only: existing rejection ids are never removed.
 *
 * isApproved: predicate — returns true if candidateId appears in prose-approved.json
 *   as a candidateId (used by the re-ingest skip logic).
 * isRejected: predicate — returns true if candidateId appears in prose-rejections.json.
 *
 * NO AUTO-APPROVAL PATH EXISTS. These helpers are called ONLY from the mbse-ingest
 * skill after an explicit human approve/reject decision via AskUserQuestion. The
 * helpers themselves perform no approval decision — they execute the human's choice.
 */

import { z } from "zod";
import { promises as fs } from "node:fs";
import { stableId } from "./stable-id.js";
import { ProseApprovedEntrySchema, type ProseApprovedEntry } from "./prose-approved.js";

// ---------------------------------------------------------------------------
// CandidateEntry — shape of a pending extraction from prose-candidates.json
// ---------------------------------------------------------------------------

const CitationSchema = z.object({
  docId: z.string(),
  docSha256: z.string(),
  chunkId: z.string(),
  sectionPath: z.string(),
  quote: z.string().max(300),
});

export const CandidateEntrySchema = z.object({
  /** Unique identifier for this candidate (assigned at extraction time) */
  id: z.string(),
  kind: z.enum([
    "requirement",
    "need",
    "mode",
    "modeTransition",
    "interface",
    "component",
    "function",
  ]),
  fields: z.record(z.unknown()),
  citation: CitationSchema,
  /** Id of an already-approved entry this candidate would supersede (optional) */
  supersedes: z.string().optional(),
});

export type CandidateEntry = z.infer<typeof CandidateEntrySchema>;

// ---------------------------------------------------------------------------
// ProseApprovedFile — on-disk shape
// ---------------------------------------------------------------------------

const ProseApprovedFileSchema = z.object({
  entries: z.array(ProseApprovedEntrySchema),
});

// ---------------------------------------------------------------------------
// ProseRejectionsFile — on-disk shape
// ---------------------------------------------------------------------------

const ProseRejectionsFileSchema = z.object({
  rejectedIds: z.array(z.string()),
});

// ---------------------------------------------------------------------------
// appendApproval
// ---------------------------------------------------------------------------

/**
 * Validate a candidate, build a ProseApprovedEntry, and append it to
 * prose-approved.json (creating the file if it does not exist).
 *
 * The stable id is derived from: stableId("prose", `${citation.docId}:${citation.chunkId}:${citation.quote}`)
 * — this is deterministic across runs and independent of approvedBy/timestamp.
 *
 * @param candidate    The candidate extraction to approve
 * @param approvedBy   Human user identity (typically git config user.name)
 * @param approvedPath Path to prose-approved.json (will be created if absent)
 * @param _rejectionsPath Path to prose-rejections.json (reserved for future guard; not read here)
 * @returns            The newly created ProseApprovedEntry (also appended to file)
 */
export async function appendApproval(
  candidate: CandidateEntry,
  approvedBy: string,
  approvedPath: string,
  _rejectionsPath: string
): Promise<ProseApprovedEntry> {
  // Validate the candidate input shape
  const parsed = CandidateEntrySchema.parse(candidate);

  // Load existing file or start fresh
  let existingEntries: ProseApprovedEntry[] = [];
  try {
    const raw = await fs.readFile(approvedPath, "utf8");
    const file = ProseApprovedFileSchema.parse(JSON.parse(raw));
    existingEntries = file.entries;
  } catch (err: unknown) {
    // File does not exist or is malformed — start fresh
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
      // Rethrow if it is a real parse error (not just missing file)
      const isParseError =
        err instanceof SyntaxError ||
        (err instanceof z.ZodError);
      if (isParseError) throw err;
    }
    existingEntries = [];
  }

  // Derive deterministic stable id from citation content + kind. The `kind` is
  // part of the natural key because a single sentence can legitimately ground two
  // distinct entries of different kinds (e.g. a `mode` and a `modeTransition` that
  // both cite the sentence defining a substage). Without `kind`, those collide on
  // a single id; including it keeps each entry uniquely addressable while staying
  // deterministic (same kind + same quote → same id).
  const naturalKey = `${parsed.kind}:${parsed.citation.docId}:${parsed.citation.chunkId}:${parsed.citation.quote}`;
  const entryId = stableId("prose", naturalKey);

  // Build the approved entry
  const entry: ProseApprovedEntry = {
    id: entryId,
    kind: parsed.kind,
    fields: parsed.fields,
    citation: parsed.citation,
    approvedBy,
    approvedAt: new Date().toISOString(),
    candidateId: parsed.id,
    status: "approved",
    ...(parsed.supersedes !== undefined ? { supersedes: parsed.supersedes } : {}),
  };

  // Validate the full entry before writing
  ProseApprovedEntrySchema.parse(entry);

  // Append and write (append-only)
  const newEntries = [...existingEntries, entry];
  await fs.writeFile(
    approvedPath,
    JSON.stringify({ entries: newEntries }, null, 2) + "\n",
    "utf8"
  );

  return entry;
}

// ---------------------------------------------------------------------------
// recordRejection
// ---------------------------------------------------------------------------

/**
 * Record a candidate id in prose-rejections.json so the re-ingest pipeline
 * skips it on future runs. Idempotent — recording the same id twice is safe.
 *
 * @param candidateId    The candidate id to mark as rejected
 * @param rejectionsPath Path to prose-rejections.json (created if absent)
 */
export async function recordRejection(
  candidateId: string,
  rejectionsPath: string
): Promise<void> {
  let existingIds: string[] = [];
  try {
    const raw = await fs.readFile(rejectionsPath, "utf8");
    const file = ProseRejectionsFileSchema.parse(JSON.parse(raw));
    existingIds = file.rejectedIds;
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
      const isParseError = err instanceof SyntaxError || err instanceof z.ZodError;
      if (isParseError) throw err;
    }
    existingIds = [];
  }

  // Idempotent: only add if not already present
  if (!existingIds.includes(candidateId)) {
    existingIds = [...existingIds, candidateId];
  }

  await fs.writeFile(
    rejectionsPath,
    JSON.stringify({ rejectedIds: existingIds }, null, 2) + "\n",
    "utf8"
  );
}

// ---------------------------------------------------------------------------
// isApproved — re-ingest skip predicate
// ---------------------------------------------------------------------------

/**
 * Returns true if the given candidateId has already been approved (i.e. it
 * appears as candidateId in prose-approved.json). Used by the re-ingest
 * pipeline to skip candidates that already have an approval record.
 *
 * @param candidateId  Candidate id to check
 * @param approvedPath Path to prose-approved.json
 */
export async function isApproved(
  candidateId: string,
  approvedPath: string
): Promise<boolean> {
  try {
    const raw = await fs.readFile(approvedPath, "utf8");
    const file = ProseApprovedFileSchema.parse(JSON.parse(raw));
    return file.entries.some((e) => e.candidateId === candidateId);
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// isRejected — re-ingest skip predicate
// ---------------------------------------------------------------------------

/**
 * Returns true if the given candidateId has been explicitly rejected (i.e. it
 * appears in prose-rejections.json). Used by the re-ingest pipeline to skip
 * candidates that were previously rejected by the human reviewer.
 *
 * @param candidateId    Candidate id to check
 * @param rejectionsPath Path to prose-rejections.json
 */
export async function isRejected(
  candidateId: string,
  rejectionsPath: string
): Promise<boolean> {
  try {
    const raw = await fs.readFile(rejectionsPath, "utf8");
    const file = ProseRejectionsFileSchema.parse(JSON.parse(raw));
    return file.rejectedIds.includes(candidateId);
  } catch {
    return false;
  }
}
