/**
 * inferred-approval-helpers.ts — Human-approval gate for the inference pipeline.
 *
 * appendInferredApproval: validate a candidate, stamp approval fields, append to
 *   inferred-approved.json (creating the file if absent). Append-only; never
 *   overwrites existing entries.
 *
 * recordInferredRejection: record a candidate id in inferred-rejections.json.
 *   Idempotent — recording the same id twice is safe.
 *
 * isInferredApproved: predicate — returns true if candidateId matches an entry id
 *   in inferred-approved.json (used by the re-ingest skip logic).
 * isInferredRejected: predicate — returns true if candidateId appears in
 *   inferred-rejections.json.
 *
 * NO AUTO-APPROVAL PATH EXISTS. These helpers are called ONLY from the mbse-infer
 * skill after an explicit human approve/reject decision via AskUserQuestion. The
 * helpers themselves perform no approval decision — they execute the human's choice.
 *
 * Mirrors the prose-layer approval-helpers.ts discipline.
 */

import { z } from "zod";
import { promises as fs } from "node:fs";
import {
  InferredApprovedEntrySchema,
  type InferredApprovedEntry,
} from "./inferred-approved.js";

// ---------------------------------------------------------------------------
// InferenceCandidate — shape of a pending inference proposal
// ---------------------------------------------------------------------------

export const InferenceCandidateSchema = z.object({
  /** Candidate / proposal id (assigned at inference time) */
  id: z.string(),
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
  /** Composed-IR id of the source element */
  sourceId: z.string(),
  /** Composed-IR id of the target element */
  targetId: z.string(),
  /** Composed-IR ids of premises — at least 1 */
  premises: z.array(z.string()).min(1),
  /** Audit-only rationale — stored, NEVER exported or surfaced to users */
  rationale: z.string(),
  /** Proposal confidence at generation time */
  confidence: z.number(),
  /** Debate outcome — optional, audit-only */
  debate: z
    .object({
      verdict: z.enum(["confirmed", "uncertain"]),
      advocate: z.number(),
      challenger: z.number(),
    })
    .optional(),
  /** PROV wasAssociatedWith — the inference run that produced this proposal */
  inferenceRunId: z.string(),
  /** Id of an already-approved entry this candidate would supersede (optional) */
  supersedes: z.string().optional(),
});

export type InferenceCandidate = z.infer<typeof InferenceCandidateSchema>;

// ---------------------------------------------------------------------------
// On-disk file schemas
// ---------------------------------------------------------------------------

const InferredApprovedFileSchema = z.object({
  entries: z.array(InferredApprovedEntrySchema),
});

const InferredRejectionsFileSchema = z.object({
  rejectedIds: z.array(z.string()),
});

// ---------------------------------------------------------------------------
// appendInferredApproval
// ---------------------------------------------------------------------------

/**
 * Validate a candidate, build an InferredApprovedEntry, and append it to
 * inferred-approved.json (creating the file if it does not exist).
 *
 * The entry id is taken directly from the candidate id (unlike prose-layer
 * stableId derivation — inference ids are already stable from the pipeline).
 *
 * @param candidate      The inference candidate to approve
 * @param approvedBy     Human user identity (typically git config user.name)
 * @param approvedPath   Path to inferred-approved.json (created if absent)
 * @param _rejectionsPath Path to inferred-rejections.json (reserved; not read here)
 * @returns              The newly created InferredApprovedEntry (also appended to file)
 */
export async function appendInferredApproval(
  candidate: InferenceCandidate,
  approvedBy: string,
  approvedPath: string,
  _rejectionsPath: string
): Promise<InferredApprovedEntry> {
  // Validate candidate input shape
  const parsed = InferenceCandidateSchema.parse(candidate);

  // Load existing file or start fresh
  let existingEntries: InferredApprovedEntry[] = [];
  try {
    const raw = await fs.readFile(approvedPath, "utf8");
    const file = InferredApprovedFileSchema.parse(JSON.parse(raw));
    existingEntries = file.entries;
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
      const isParseError =
        err instanceof SyntaxError || err instanceof z.ZodError;
      if (isParseError) throw err;
    }
    existingEntries = [];
  }

  // Build the approved entry
  const entry: InferredApprovedEntry = {
    id: parsed.id,
    relationFamily: parsed.relationFamily,
    sourceId: parsed.sourceId,
    targetId: parsed.targetId,
    premises: parsed.premises,
    rationale: parsed.rationale,
    confidence: parsed.confidence,
    inferenceRunId: parsed.inferenceRunId,
    approvedBy,
    approvedAt: new Date().toISOString(),
    status: "approved",
    ...(parsed.debate !== undefined ? { debate: parsed.debate } : {}),
    ...(parsed.supersedes !== undefined ? { supersedes: parsed.supersedes } : {}),
  };

  // Validate the full entry before writing
  InferredApprovedEntrySchema.parse(entry);

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
// recordInferredRejection
// ---------------------------------------------------------------------------

/**
 * Record a candidate id in inferred-rejections.json so the inference pipeline
 * skips it on future runs. Idempotent — recording the same id twice is safe.
 *
 * @param candidateId    The candidate id to mark as rejected
 * @param rejectionsPath Path to inferred-rejections.json (created if absent)
 */
export async function recordInferredRejection(
  candidateId: string,
  rejectionsPath: string
): Promise<void> {
  let existingIds: string[] = [];
  try {
    const raw = await fs.readFile(rejectionsPath, "utf8");
    const file = InferredRejectionsFileSchema.parse(JSON.parse(raw));
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
// isInferredApproved — skip predicate
// ---------------------------------------------------------------------------

/**
 * Returns true if the given candidateId has been approved (i.e. it appears as
 * the entry id in inferred-approved.json). Used by the inference pipeline to
 * skip candidates that already have an approval record.
 *
 * @param candidateId  Candidate id to check
 * @param approvedPath Path to inferred-approved.json
 */
export async function isInferredApproved(
  candidateId: string,
  approvedPath: string
): Promise<boolean> {
  try {
    const raw = await fs.readFile(approvedPath, "utf8");
    const file = InferredApprovedFileSchema.parse(JSON.parse(raw));
    return file.entries.some((e) => e.id === candidateId);
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// isInferredRejected — skip predicate
// ---------------------------------------------------------------------------

/**
 * Returns true if the given candidateId has been explicitly rejected (i.e. it
 * appears in inferred-rejections.json). Used by the inference pipeline to skip
 * candidates that were previously rejected by the human reviewer.
 *
 * @param candidateId    Candidate id to check
 * @param rejectionsPath Path to inferred-rejections.json
 */
export async function isInferredRejected(
  candidateId: string,
  rejectionsPath: string
): Promise<boolean> {
  try {
    const raw = await fs.readFile(rejectionsPath, "utf8");
    const file = InferredRejectionsFileSchema.parse(JSON.parse(raw));
    return file.rejectedIds.includes(candidateId);
  } catch {
    return false;
  }
}
