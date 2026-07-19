/**
 * prose-verbatim.ts — PROSE-unverbatim-quote (C6 audit re-check).
 *
 * The ingest pipeline drops a proposal whose quote does not verbatim-resolve
 * into its cited chunk (C6). This gate is the AUDIT counterpart: it re-checks
 * ALREADY-APPROVED prose entries so a quote that drifted (or was approved before
 * the pipeline gate existed) is caught after the fact — every approved entry's
 * quote must still verbatim-resolve into the text of its cited chunk.
 *
 * Corpus-availability discipline (mirrors GATE03-corpus-unavailable): the check
 * needs the ingest chunk store (chunkId → chunk text). When the store is ABSENT,
 * the gate emits ONE PROSE-unverbatim-quote-unavailable warning rather than
 * vacuously passing. When the store is present but a specific approved entry's
 * chunkId is not in it, that entry is unverifiable → a per-entry warning (never a
 * false error). Only when the chunk text IS available and the quote does NOT
 * occur in it does the gate emit the PROSE-unverbatim-quote ERROR.
 *
 * Uses the SAME matcher as the pipeline (@sysml-bridge/model `quoteOccursInChunk`)
 * so emit-time and audit-time enforcement never drift.
 */

import type { ProseApprovedEntry } from "@sysml-bridge/model";
import { quoteOccursInChunk } from "@sysml-bridge/model";
import type { Finding } from "./findings.js";

/**
 * Re-check approved prose entries' quotes against the ingest chunk store.
 *
 * @param proseEntries The active prose entries from the composed IR.
 * @param chunkStore   chunkId → chunk text, or undefined when unavailable.
 */
export function proseVerbatimFindings(
  proseEntries: readonly ProseApprovedEntry[],
  chunkStore: ReadonlyMap<string, string> | undefined
): Finding[] {
  const findings: Finding[] = [];

  // Only 'approved' entries are re-checked: suspect entries are already flagged
  // by PROSE-suspect-source, and superseded entries are excluded from the IR.
  const approved = proseEntries.filter((e) => e.status === "approved");
  if (approved.length === 0) return findings; // nothing to verify — genuine no-op

  // Chunk store absent → degrade (never a vacuous pass over real approved entries).
  if (chunkStore === undefined) {
    findings.push({
      elementId: "prose",
      ruleId: "PROSE-unverbatim-quote-unavailable",
      severity: "warning",
      message:
        `${approved.length} approved prose entr${approved.length === 1 ? "y" : "ies"} ` +
        `could not be verbatim-checked — the ingest chunk store (chunkId → text) was not ` +
        `provided to the audit, so citation quotes were not re-verified against their chunks.`,
      suggestedFix:
        "Attach the prose ingest chunk store (ProseComposedIR.chunkStore) before auditing so " +
        "PROSE-unverbatim-quote can re-check each approved entry's quote against its cited chunk.",
    });
    return findings;
  }

  for (const entry of approved) {
    const chunkText = chunkStore.get(entry.citation.chunkId);
    if (chunkText === undefined) {
      // Store present but this entry's chunk is missing → unverifiable (not a false error).
      findings.push({
        elementId: entry.id,
        ruleId: "PROSE-unverbatim-quote-unavailable",
        severity: "warning",
        message:
          `Prose entry '${entry.id}' (${entry.kind}) cites chunk '${entry.citation.chunkId}' ` +
          `which is absent from the provided chunk store — its quote could not be re-verified.`,
        suggestedFix:
          "Re-ingest the source document so the chunk store contains the cited chunk, or " +
          "re-approve the extraction against the current chunking.",
      });
      continue;
    }

    if (!quoteOccursInChunk(entry.citation.quote, chunkText)) {
      findings.push({
        elementId: entry.id,
        ruleId: "PROSE-unverbatim-quote",
        severity: "error",
        message:
          `Prose entry '${entry.id}' (${entry.kind}) has a citation.quote that does NOT occur ` +
          `verbatim in its cited chunk '${entry.citation.chunkId}' (doc '${entry.citation.docId}'). ` +
          `The approved quote is not supported by the source text — a hallucinated or drifted quote.`,
        suggestedFix:
          "Re-review the extraction: correct citation.quote to a span that appears in the cited " +
          "chunk, or reject the entry. The ingest pipeline drops such proposals (C6); an approved " +
          "entry that fails here predates the gate or the source document changed.",
      });
    }
  }

  return findings;
}
