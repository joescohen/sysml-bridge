/**
 * inferred-approved.ts — F8 inference layer: append-only approved inferred entries.
 *
 * InferredApprovedEntry: a zod-validated, append-only record representing one
 * human-approved inferred relation link reasoned over the composed IR graph.
 *
 * Third layer extension to composeIR: spreadsheet ∪ prose-approved ∪ inferred-approved.
 *   - No inferred layer  → backward-compat; inferredEntries=[], approvedInferredIds={}
 *   - Supersede chains  → same convention as prose layer (latest non-superseded wins)
 *   - Suspect premise propagation (A6): at composition, an inferred entry whose ANY
 *     premise id (a) doesn't resolve in the composed IR, or (b) resolves to a prose/inferred
 *     entry that is suspect or superseded → composes with status:'suspect'.
 *
 * The InferredComposedIR type extends ProseComposedIR with the inferred third layer.
 * composeIR is re-exported from here with the extended signature.
 */

import { z } from "zod";
import { promises as fs } from "node:fs";
import {
  composeIR as composeProse,
  type ProseComposedIR,
  type ProseApprovedEntry,
} from "./prose-approved.js";
import { ExtractedSchema, type Extracted } from "./schema.js";

// ---------------------------------------------------------------------------
// InferredApprovedEntry schema
// ---------------------------------------------------------------------------

export const InferredApprovedEntrySchema = z.object({
  /** Stable hash of (relationFamily, sourceId, targetId) */
  id: z.string(),
  relationFamily: z.enum(["allocation", "modeMembership", "flowTyping", "controlJoin"]),
  /** Composed-IR id of the source element */
  sourceId: z.string(),
  /** Composed-IR id of the target element */
  targetId: z.string(),
  /** Composed-IR ids of corpus/prose entries this link was reasoned from — at least 1 */
  premises: z.array(z.string()).min(1),
  /** Audit-only rationale — NEVER exposed in tool results or exported models */
  rationale: z.string(),
  /** Proposal confidence at approval time */
  confidence: z.number(),
  /** Debate outcome — audit-only; advocate/challenger prose excluded per DEBAT-04 */
  debate: z
    .object({
      verdict: z.enum(["confirmed", "uncertain"]),
      advocate: z.number(),
      challenger: z.number(),
    })
    .optional(),
  /** PROV wasAssociatedWith — the inference run that produced this proposal */
  inferenceRunId: z.string(),
  /** PROV wasAttributedTo — the human approver */
  approvedBy: z.string(),
  /** ISO datetime of approval */
  approvedAt: z.string(),
  status: z.enum(["approved", "superseded", "suspect"]),
  /** Id of the entry this supersedes (set on the NEW entry) */
  supersedes: z.string().optional(),
});

export type InferredApprovedEntry = z.infer<typeof InferredApprovedEntrySchema>;

// ---------------------------------------------------------------------------
// InferredApprovedFile schema (on-disk shape)
// ---------------------------------------------------------------------------

const InferredApprovedFileSchema = z.object({
  entries: z.array(InferredApprovedEntrySchema),
});

// ---------------------------------------------------------------------------
// InferredComposedIR — the output of composeIR with all three layers
// ---------------------------------------------------------------------------

export interface InferredComposedIR extends ProseComposedIR {
  /**
   * Active inferred entries (superseded entries are filtered out).
   * Entries with status:'suspect' (premise drift) are included — they still compose.
   */
  inferredEntries: InferredApprovedEntry[];
  /**
   * Set of inferred entry ids whose status is 'approved' after premise propagation.
   * These are the ids that GATE03 provenance checks accept for inferred-layer elements.
   * Suspect and superseded ids are NOT in this set.
   */
  approvedInferredIds: Set<string>;
}

// ---------------------------------------------------------------------------
// composeIR — three-layer deterministic merge
// ---------------------------------------------------------------------------

/**
 * Compose the three-layer IR: extracted corpus + optional prose layer + optional inferred layer.
 *
 * This is a drop-in extension of the two-layer composeIR. The first three params are
 * identical; the fourth adds the inferred layer.
 *
 * @param extractedPath          Path to extracted.json (required)
 * @param proseApprovedPath      Path to prose-approved.json (optional)
 * @param manifestPath           Path to prose-ingest-manifest.json (optional)
 * @param inferredApprovedPath   Path to inferred-approved.json (optional)
 *
 * Backward-compat guarantee (A9): composeIR(extractedPath) and
 * composeIR(extractedPath, undefined, undefined, undefined) both return an
 * InferredComposedIR with inferredEntries=[] and approvedInferredIds=Set().
 * The extracted and proseEntries/approvedProseIds fields are structurally identical
 * to the two-layer composeIR output.
 *
 * Suspect premise propagation (A6): at composition, an inferred entry is composed
 * as status:'suspect' if ANY of its premise ids:
 *   (a) is not resolvable in the composed IR (not in corpus entity ids, not in
 *       approved/suspect prose ids), OR
 *   (b) resolves to a prose entry that is suspect or superseded
 * Superseded inferred entries are excluded from the output (same as prose layer).
 */
export async function composeIR(
  extractedPath: string,
  proseApprovedPath?: string,
  manifestPath?: string,
  inferredApprovedPath?: string
): Promise<InferredComposedIR> {
  // ── Two-layer compose (prose + corpus) ──────────────────────────────────
  const proseComposed = await composeProse(extractedPath, proseApprovedPath, manifestPath);

  // Early return when no inferred layer
  if (!inferredApprovedPath) {
    return {
      ...proseComposed,
      inferredEntries: [],
      approvedInferredIds: new Set(),
    };
  }

  // ── Load + validate inferred-approved.json ───────────────────────────────
  const rawInferred = await fs.readFile(inferredApprovedPath, "utf8");
  const inferredFile = InferredApprovedFileSchema.parse(JSON.parse(rawInferred));

  // ── Build premise validity map ───────────────────────────────────────────
  // A premise id is "valid approved" if it:
  //   (a) is a corpus entity id (in corpus resolution ids), OR
  //   (b) is a prose entry id with status:'approved' (in approvedProseIds)
  // A premise id is "suspect" if it:
  //   (a) is a prose entry id with status:'suspect', OR
  //   (b) is a superseded prose entry id (not in proseEntries but was in the file)
  // A premise id is "missing" if it is not resolvable at all.

  // Build corpus entity id set
  const corpusEntityIds = new Set<string>();
  const corpus = proseComposed.extracted;
  const entityKinds = [
    "needs",
    "requirements",
    "functions",
    "components",
    "subsystems",
    "kpps",
    "behaviorDecomp",
    "n2Interfaces",
  ] as const;
  for (const k of entityKinds) {
    for (const e of ((corpus as Record<string, unknown>)[k] as Array<{ id?: string }>) ?? []) {
      if (e.id) corpusEntityIds.add(e.id);
    }
  }

  // Build a map: prose entry id → effective status in the composed output
  // (proseEntries already has superseded entries filtered out; we need the full picture
  // of what's suspect/superseded to assess premise quality)
  const proseStatusMap = new Map<string, "approved" | "suspect" | "superseded">();
  for (const entry of proseComposed.proseEntries) {
    // These are non-superseded entries with their effective status
    proseStatusMap.set(entry.id, entry.status);
  }

  // ── Resolve supersede chains for inferred entries ────────────────────────
  const inferredSupersededIds = new Set<string>();
  for (const entry of inferredFile.entries) {
    if (entry.supersedes) {
      inferredSupersededIds.add(entry.supersedes);
    }
  }

  // ── Build composed inferred entries with premise propagation ─────────────
  const inferredEntries: InferredApprovedEntry[] = [];
  const approvedInferredIds = new Set<string>();

  for (const entry of inferredFile.entries) {
    // Skip superseded entries — retained in file but not composed
    if (inferredSupersededIds.has(entry.id)) {
      continue;
    }

    // Premise propagation: check all premise ids
    let effectiveStatus = entry.status;

    if (effectiveStatus !== "superseded") {
      // Check each premise for suspicion
      for (const premiseId of entry.premises) {
        // Case 1: premise is a corpus entity id → valid
        if (corpusEntityIds.has(premiseId)) {
          continue;
        }
        // Case 2: premise is a prose entry — check its effective status
        const proseStatus = proseStatusMap.get(premiseId);
        if (proseStatus !== undefined) {
          if (proseStatus === "suspect" || proseStatus === "superseded") {
            effectiveStatus = "suspect";
            break;
          }
          // proseStatus === 'approved' → valid premise, continue
          continue;
        }
        // Case 3: premise not resolvable in composed IR at all → suspect
        effectiveStatus = "suspect";
        break;
      }
    }

    const composedEntry: InferredApprovedEntry = {
      ...entry,
      status: effectiveStatus,
    };

    inferredEntries.push(composedEntry);

    // Only 'approved' entries (after premise propagation) enter the Gate-1 resolution set
    if (effectiveStatus === "approved") {
      approvedInferredIds.add(entry.id);
    }
  }

  return {
    ...proseComposed,
    inferredEntries,
    approvedInferredIds,
  };
}

// Re-export prose types for consumer convenience
export type { ProseComposedIR, ProseApprovedEntry } from "./prose-approved.js";
export { stableId } from "./stable-id.js";
