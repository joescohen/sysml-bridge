/**
 * candidates.ts — load, normalize, and disposition-merge the demo candidate
 * files for the review UI. NO WRITES happen here (read-only); the approval /
 * rejection writers live in server.ts so the no-auto-approve ratchet has a
 * single allowlisted write surface.
 *
 * The demo candidate files are REAL pipeline artifacts from the ANGARS corpus,
 * not clean candidate lists. Their on-disk shapes:
 *
 *   prose-candidates.json    → { ...meta, candidates: CandidateEntry-ish[] }
 *   inference-candidates.json → { ...meta, irHash, records: StageRecord[] }
 *
 * Neither is a bare array and neither is 1:1 with the zod candidate schemas:
 *   - prose: 4 of 323 entries use kinds ("succession", "parallel") outside
 *     CandidateEntrySchema's enum — those are dropped (not reviewable here).
 *   - inference: records are pipeline-STAGE rows; only "queued" / "auto_rejected"
 *     rows carry candidate content (premises + confidence + rationale). Even
 *     those lack `inferenceRunId` and use FLAT debate fields
 *     (debateVerdict/debateAdvocate/debateChallenger) rather than the nested
 *     `debate` object the schema wants. We adapt each: synthesize
 *     `inferenceRunId` from the file's `irHash`, fold the flat debate fields
 *     into a `debate` object. Stage rows without candidate content are dropped.
 *
 * The result is that every candidate this module surfaces validates against the
 * SAME zod schema the /mbse-approve helpers validate — so an approve/reject
 * click writes a byte-compatible disposition record.
 */

import { promises as fs } from "node:fs";
import * as path from "node:path";
import {
  CandidateEntrySchema,
  InferenceCandidateSchema,
  EntityMergeCandidateSchema,
  type CandidateEntry,
  type InferenceCandidate,
  type EntityMergeCandidate,
} from "@sysml-bridge/model";

export type Layer = "prose" | "inference" | "entity";
export type Disposition = "pending" | "approved" | "rejected";

/** A candidate ready to surface in the UI, tagged with its layer + status. */
export interface ReviewItem {
  layer: Layer;
  /** The candidate id used as the disposition key (candidate.id / candidateId). */
  candidateId: string;
  /** Human-facing kind (prose kind, inference relationFamily, or entity-merge reason). */
  kind: string;
  /** Human-facing one-line label. */
  name: string;
  /** Current disposition, merged from the dispositions dir. */
  status: Disposition;
  /** The schema-valid candidate object (fed verbatim to the write helpers). */
  candidate: CandidateEntry | InferenceCandidate | EntityMergeCandidate;
}

// ---------------------------------------------------------------------------
// Disposition filenames (canonical — the same names the helpers write)
// ---------------------------------------------------------------------------

export const DISPOSITION_FILES = {
  proseApproved: "prose-approved.json",
  proseRejections: "prose-rejections.json",
  inferredApproved: "inferred-approved.json",
  inferredRejections: "inferred-rejections.json",
  entityApproved: "entity-approved.json",
  entityRejections: "entity-rejections.json",
} as const;

// ---------------------------------------------------------------------------
// Prose candidate loading + normalization
// ---------------------------------------------------------------------------

/** Load prose-candidates.json and return only entries valid per CandidateEntrySchema. */
export async function loadProseCandidates(
  candidatesDir: string
): Promise<CandidateEntry[]> {
  const raw = await readJson(path.join(candidatesDir, "prose-candidates.json"));
  const arr: unknown[] = Array.isArray(raw)
    ? raw
    : Array.isArray((raw as { candidates?: unknown }).candidates)
      ? ((raw as { candidates: unknown[] }).candidates)
      : [];
  const out: CandidateEntry[] = [];
  for (const c of arr) {
    const parsed = CandidateEntrySchema.safeParse(c);
    if (parsed.success) out.push(parsed.data);
    // kinds outside the enum (succession/parallel) are silently dropped — they
    // are not reviewable through the prose-approved schema.
  }
  return out;
}

// ---------------------------------------------------------------------------
// Inference candidate loading + adaptation
// ---------------------------------------------------------------------------

interface RawInferenceRecord {
  id: string;
  relationFamily?: string;
  sourceId?: string;
  targetId?: string;
  premises?: string[];
  rationale?: string;
  confidence?: number;
  debateVerdict?: "confirmed" | "uncertain";
  debateAdvocate?: number;
  debateChallenger?: number;
}

/**
 * Load inference-candidates.json, keep records that carry candidate content,
 * and adapt each into a schema-valid InferenceCandidate.
 */
export async function loadInferenceCandidates(
  candidatesDir: string
): Promise<InferenceCandidate[]> {
  const raw = (await readJson(
    path.join(candidatesDir, "inference-candidates.json")
  )) as { irHash?: string; records?: RawInferenceRecord[] } | RawInferenceRecord[];

  const records: RawInferenceRecord[] = Array.isArray(raw)
    ? raw
    : Array.isArray(raw.records)
      ? raw.records
      : [];
  const irHash = Array.isArray(raw) ? "unknown" : (raw.irHash ?? "unknown");
  const inferenceRunId = `angars-inference-${irHash}`;

  const out: InferenceCandidate[] = [];
  for (const r of records) {
    // Only records with candidate content are reviewable.
    if (!r.premises || r.confidence === undefined || !r.rationale) continue;
    const adapted: Record<string, unknown> = {
      id: r.id,
      relationFamily: r.relationFamily,
      sourceId: r.sourceId,
      targetId: r.targetId,
      premises: r.premises,
      rationale: r.rationale,
      confidence: r.confidence,
      inferenceRunId,
    };
    if (r.debateVerdict) {
      adapted.debate = {
        verdict: r.debateVerdict,
        advocate: r.debateAdvocate ?? 0,
        challenger: r.debateChallenger ?? 0,
      };
    }
    const parsed = InferenceCandidateSchema.safeParse(adapted);
    if (parsed.success) out.push(parsed.data);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Entity-merge candidate loading
// ---------------------------------------------------------------------------

/**
 * Load entity-candidates.json and return only entries valid per
 * EntityMergeCandidateSchema. Missing file → [] (entity resolution is optional
 * for a given project; the queue is simply empty). On-disk shape:
 *   { ...meta, proposals: EntityMergeCandidate[] }  (or a bare array)
 */
export async function loadEntityCandidates(
  candidatesDir: string
): Promise<EntityMergeCandidate[]> {
  const raw = await readJsonOrNull(path.join(candidatesDir, "entity-candidates.json"));
  if (raw === null) return [];
  const arr: unknown[] = Array.isArray(raw)
    ? raw
    : Array.isArray((raw as { proposals?: unknown }).proposals)
      ? ((raw as { proposals: unknown[] }).proposals)
      : [];
  const out: EntityMergeCandidate[] = [];
  for (const c of arr) {
    const parsed = EntityMergeCandidateSchema.safeParse(c);
    if (parsed.success) out.push(parsed.data);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Disposition reading
// ---------------------------------------------------------------------------

/** ids approved (by candidateId for prose, by entry id for inference). */
async function readApprovedCandidateIds(
  dispositionsDir: string,
  file: string,
  key: "candidateId" | "id"
): Promise<Set<string>> {
  const raw = await readJsonOrNull(path.join(dispositionsDir, file));
  const entries: Array<Record<string, unknown>> =
    raw && Array.isArray((raw as { entries?: unknown }).entries)
      ? ((raw as { entries: Array<Record<string, unknown>> }).entries)
      : [];
  const ids = new Set<string>();
  for (const e of entries) {
    const v = e[key];
    if (typeof v === "string") ids.add(v);
  }
  return ids;
}

/** ids rejected — from `rejectedIds` (prose/inference) or `rejectedPairKeys` (entity). */
async function readRejectedIds(
  dispositionsDir: string,
  file: string,
  field: "rejectedIds" | "rejectedPairKeys" = "rejectedIds"
): Promise<Set<string>> {
  const raw = await readJsonOrNull(path.join(dispositionsDir, file));
  const arr: unknown =
    raw && Array.isArray((raw as Record<string, unknown>)[field])
      ? ((raw as Record<string, unknown[]>)[field])
      : [];
  const ids = new Set<string>();
  for (const v of arr as unknown[]) if (typeof v === "string") ids.add(v);
  return ids;
}

// ---------------------------------------------------------------------------
// State assembly — candidates ∪ dispositions
// ---------------------------------------------------------------------------

export interface ReviewState {
  prose: ReviewItem[];
  inference: ReviewItem[];
  entity: ReviewItem[];
  counts: {
    prose: Record<Disposition, number>;
    inference: Record<Disposition, number>;
    entity: Record<Disposition, number>;
  };
}

export async function buildState(
  candidatesDir: string,
  dispositionsDir: string
): Promise<ReviewState> {
  const [proseCands, infCands, entityCands] = await Promise.all([
    loadProseCandidates(candidatesDir),
    loadInferenceCandidates(candidatesDir),
    loadEntityCandidates(candidatesDir),
  ]);

  const [
    proseApproved,
    proseRejected,
    infApproved,
    infRejected,
    entityApproved,
    entityRejected,
  ] = await Promise.all([
    readApprovedCandidateIds(dispositionsDir, DISPOSITION_FILES.proseApproved, "candidateId"),
    readRejectedIds(dispositionsDir, DISPOSITION_FILES.proseRejections),
    readApprovedCandidateIds(dispositionsDir, DISPOSITION_FILES.inferredApproved, "id"),
    readRejectedIds(dispositionsDir, DISPOSITION_FILES.inferredRejections),
    readApprovedCandidateIds(dispositionsDir, DISPOSITION_FILES.entityApproved, "candidateId"),
    readRejectedIds(dispositionsDir, DISPOSITION_FILES.entityRejections, "rejectedPairKeys"),
  ]);

  const prose: ReviewItem[] = proseCands.map((c) => ({
    layer: "prose" as const,
    candidateId: c.id,
    kind: c.kind,
    name: proseName(c),
    status: statusFor(c.id, proseApproved, proseRejected),
    candidate: c,
  }));

  const inference: ReviewItem[] = infCands.map((c) => ({
    layer: "inference" as const,
    candidateId: c.id,
    kind: c.relationFamily,
    name: inferenceName(c),
    status: statusFor(c.id, infApproved, infRejected),
    candidate: c,
  }));

  const entity: ReviewItem[] = entityCands.map((c) => ({
    layer: "entity" as const,
    candidateId: c.id,
    kind: `entity-merge (${c.reason})`,
    name: entityName(c),
    status: statusFor(c.id, entityApproved, entityRejected),
    candidate: c,
  }));

  return {
    prose,
    inference,
    entity,
    counts: {
      prose: tally(prose),
      inference: tally(inference),
      entity: tally(entity),
    },
  };
}

function statusFor(
  id: string,
  approved: Set<string>,
  rejected: Set<string>
): Disposition {
  if (approved.has(id)) return "approved";
  if (rejected.has(id)) return "rejected";
  return "pending";
}

function tally(items: ReviewItem[]): Record<Disposition, number> {
  const t: Record<Disposition, number> = { pending: 0, approved: 0, rejected: 0 };
  for (const i of items) t[i.status]++;
  return t;
}

// ---------------------------------------------------------------------------
// Naming — one-line labels for the list
// ---------------------------------------------------------------------------

function proseName(c: CandidateEntry): string {
  const f = c.fields as Record<string, unknown>;
  const s =
    (typeof f.name === "string" && f.name) ||
    (typeof f.text === "string" && f.text) ||
    (typeof f.statement === "string" && f.statement) ||
    c.id;
  return String(s);
}

function inferenceName(c: InferenceCandidate): string {
  return `${c.relationFamily}: ${c.sourceId} → ${c.targetId}`;
}

function entityName(c: EntityMergeCandidate): string {
  return `merge → "${c.canonicalName}" (${c.aliases.join(" / ")})`;
}

// ---------------------------------------------------------------------------
// Candidate lookup — for the write endpoints
// ---------------------------------------------------------------------------

export async function findProseCandidate(
  candidatesDir: string,
  candidateId: string
): Promise<CandidateEntry | undefined> {
  const cands = await loadProseCandidates(candidatesDir);
  return cands.find((c) => c.id === candidateId);
}

export async function findInferenceCandidate(
  candidatesDir: string,
  candidateId: string
): Promise<InferenceCandidate | undefined> {
  const cands = await loadInferenceCandidates(candidatesDir);
  return cands.find((c) => c.id === candidateId);
}

export async function findEntityCandidate(
  candidatesDir: string,
  candidateId: string
): Promise<EntityMergeCandidate | undefined> {
  const cands = await loadEntityCandidates(candidatesDir);
  return cands.find((c) => c.id === candidateId);
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

async function readJson(file: string): Promise<unknown> {
  const raw = await fs.readFile(file, "utf8");
  return JSON.parse(raw);
}

async function readJsonOrNull(file: string): Promise<unknown | null> {
  try {
    return await readJson(file);
  } catch {
    return null;
  }
}
