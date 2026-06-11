/**
 * prose-approved.ts — append-only approved prose layer for the two-layer IR.
 *
 * ProseApprovedEntry: a zod-validated, append-only record representing one
 * human-approved prose extraction from an ingested source document.
 *
 * composeIR: deterministic merge of an extracted.json corpus + an optional
 * prose-approved.json layer (+ optional manifest for hash verification).
 *   - No prose layer   → backward-compat; proseEntries=[], approvedProseIds={}
 *   - Supersede chains → latest non-superseded wins; old entries RETAINED in file
 *   - Hash mismatch    → entry composes with status:'suspect'
 *
 * The ProseComposedIR type is the new canonical input for the audit pipeline
 * wherever corpus is currently consumed as `Extracted`. The `extracted` field
 * contains the unchanged Extracted corpus; `proseEntries` contains the active
 * (non-superseded) prose entries; `approvedProseIds` is the set of ids that
 * may resolve GATE03-unresolvable-provenance.
 */

import { z } from "zod";
import { promises as fs } from "node:fs";
import { stableId } from "./stable-id.js";
import { ExtractedSchema, type Extracted } from "./schema.js";

export { stableId }; // re-export for consumer convenience

// ---------------------------------------------------------------------------
// ProseApprovedEntry schema
// ---------------------------------------------------------------------------

const CitationSchema = z.object({
  docId: z.string(),
  docSha256: z.string(),
  chunkId: z.string(),
  sectionPath: z.string(),
  quote: z.string().max(300),
});

export const ProseApprovedEntrySchema = z.object({
  /** Stable hash of the natural key — use stableId("prose", naturalKey) */
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
  /** Arbitrary key/value fields extracted from the prose */
  fields: z.record(z.unknown()),
  citation: CitationSchema,
  approvedBy: z.string(),
  approvedAt: z.string(), // ISO datetime
  /** The candidate extraction id this was approved from */
  candidateId: z.string(),
  status: z.enum(["approved", "superseded", "suspect"]),
  /** Id of the entry this supersedes (set on the NEW entry, not the old one) */
  supersedes: z.string().optional(),
});

export type ProseApprovedEntry = z.infer<typeof ProseApprovedEntrySchema>;

// ---------------------------------------------------------------------------
// ProseApprovedFile schema (the on-disk shape)
// ---------------------------------------------------------------------------

const ProseApprovedFileSchema = z.object({
  entries: z.array(ProseApprovedEntrySchema),
});

// ---------------------------------------------------------------------------
// Ingest manifest schema (for hash verification — C9)
// ---------------------------------------------------------------------------

const IngestManifestDocSchema = z.object({
  docId: z.string(),
  docSha256: z.string(),
  ingestedAt: z.string().optional(),
  chunks: z.array(z.object({ chunkId: z.string() }).passthrough()).optional(),
});

const IngestManifestSchema = z.object({
  documents: z.array(IngestManifestDocSchema),
});

// ---------------------------------------------------------------------------
// ProseComposedIR — the output of composeIR
// ---------------------------------------------------------------------------

export interface ProseComposedIR {
  /** The unchanged, validated Extracted corpus */
  extracted: Extracted;
  /**
   * Active prose entries (superseded entries are filtered out).
   * Entries with status:'suspect' are included here — they still compose.
   */
  proseEntries: ProseApprovedEntry[];
  /**
   * Set of prose entry ids whose status is 'approved' (not suspect, not
   * superseded). These are the ids that GATE03 provenance checks accept.
   */
  approvedProseIds: Set<string>;
}

// ---------------------------------------------------------------------------
// composeIR — deterministic merge
// ---------------------------------------------------------------------------

/**
 * Compose the two-layer IR: extracted corpus + optional approved prose layer.
 *
 * @param extractedPath      Path to extracted.json (required)
 * @param proseApprovedPath  Path to prose-approved.json (optional)
 * @param manifestPath       Path to prose-ingest-manifest.json for hash
 *                           verification (optional; no-op if absent)
 *
 * Backward-compat guarantee: composeIR(extractedPath) with no prose layer
 * returns { extracted, proseEntries: [], approvedProseIds: Set() }
 * — structurally equivalent to today's loadCorpus() call.
 *
 * Supersede resolution: if entry A has status='superseded', it is excluded
 * from proseEntries in the output, but the FILE is never modified (append-only).
 *
 * Hash verification: when manifestPath is provided, each entry whose
 * citation.docId appears in the manifest is checked against the manifest's
 * docSha256. On mismatch, the entry composes with status:'suspect' (overrides
 * the stored status for the returned object only — file unchanged).
 */
export async function composeIR(
  extractedPath: string,
  proseApprovedPath?: string,
  manifestPath?: string
): Promise<ProseComposedIR> {
  // ── Load + validate extracted corpus ─────────────────────────────────────
  const rawExtracted = await fs.readFile(extractedPath, "utf8");
  const extracted = ExtractedSchema.parse(JSON.parse(rawExtracted));

  // ── Early return when no prose layer ────────────────────────────────────
  if (!proseApprovedPath) {
    return { extracted, proseEntries: [], approvedProseIds: new Set() };
  }

  // ── Load + validate prose-approved.json ─────────────────────────────────
  const rawProse = await fs.readFile(proseApprovedPath, "utf8");
  const proseFile = ProseApprovedFileSchema.parse(JSON.parse(rawProse));

  // ── Load ingest manifest (optional) for hash verification ───────────────
  const docHashMap = new Map<string, string>(); // docId → trustedSha256
  if (manifestPath) {
    try {
      const rawManifest = await fs.readFile(manifestPath, "utf8");
      const manifest = IngestManifestSchema.parse(JSON.parse(rawManifest));
      for (const doc of manifest.documents) {
        docHashMap.set(doc.docId, doc.docSha256);
      }
    } catch {
      // Manifest load failure is non-fatal; hash checks are skipped
    }
  }

  // ── Resolve supersede chains: build set of superseded ids ───────────────
  // An entry is superseded when another entry's `supersedes` field points at it.
  const supersededIds = new Set<string>();
  for (const entry of proseFile.entries) {
    if (entry.supersedes) {
      supersededIds.add(entry.supersedes);
    }
  }

  // ── Build active entries (excluding superseded) with hash verification ───
  const proseEntries: ProseApprovedEntry[] = [];
  const approvedProseIds = new Set<string>();

  for (const entry of proseFile.entries) {
    // Skip superseded entries — they are retained in the file but not composed
    if (supersededIds.has(entry.id)) {
      continue;
    }

    // Hash verification: if manifest covers this docId, check the hash
    let effectiveStatus = entry.status;
    const trustedHash = docHashMap.get(entry.citation.docId);
    if (trustedHash !== undefined && entry.citation.docSha256 !== trustedHash) {
      // Mismatch → override status to 'suspect' for the composed output
      // (file is NOT modified — this is a runtime-only classification)
      effectiveStatus = "suspect";
    }

    const composedEntry: ProseApprovedEntry = {
      ...entry,
      status: effectiveStatus,
    };

    proseEntries.push(composedEntry);

    // Only 'approved' entries enter the Gate-1 resolution set
    if (effectiveStatus === "approved") {
      approvedProseIds.add(entry.id);
    }
  }

  return { extracted, proseEntries, approvedProseIds };
}
