/**
 * audit() orchestrator — Gate 1 production audit entry point.
 *
 * Pure, synchronous, no I/O. Assembles AuditResult from the rule sub-modules:
 *   - relationalFindings   (always computed)
 *   - provenanceFindings   (only when corpus is non-null)
 *   - coverageMatrix       (always computed)
 *   - fidelityReport       (only when corpus is non-null)
 *
 * When corpus is null the orchestrator emits a single GATE03-corpus-unavailable
 * warning finding and returns empty fidelity buckets. Relational findings and
 * the coverage matrix are still produced.
 *
 * AUTHORITATIVE-SIGNATURE:
 *   audit(elements, relationships, corpus: Extracted | null): AuditResult
 *   — synchronous, not async (keeps it unit-testable without fs)
 *   — export names: relationalFindings, provenanceFindings (per plan 05-02/05-03)
 *
 * Re-exports of the full public surface so consumers import only from
 * "../audit/index.js":
 *   - audit, AuditResult, Finding, FidelityRow, NearMatch, MatrixRow (types)
 *   - loadCorpusCached, buildResolutionSet, clearCorpusCache (corpus helpers)
 */

import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";
import type { Extracted, ProseComposedIR } from "@sysml-bridge/ir";
import type { AuditResult, Finding } from "./findings.js";
import { relationalFindings } from "./relational.js";
import { provenanceFindings } from "./provenance.js";
import { fidelityReport } from "./fidelity.js";
import { coverageMatrix } from "./matrix.js";
import { buildResolutionSet, buildResolutionSetFromComposed } from "./corpus.js";

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/**
 * Type guard: is the corpus argument a ProseComposedIR?
 */
function isProseComposedIR(
  corpus: Extracted | ProseComposedIR | null
): corpus is ProseComposedIR {
  return (
    corpus !== null &&
    typeof corpus === "object" &&
    "extracted" in corpus &&
    "proseEntries" in corpus &&
    "approvedProseIds" in corpus
  );
}

/**
 * Assemble a complete AuditResult from model snapshots.
 *
 * @param elements      All SysML elements (from store.queryElements())
 * @param relationships All relationships (from store.queryRelationships())
 * @param corpus        One of:
 *                        - ProseComposedIR: two-layer IR with prose entries
 *                        - Extracted: plain corpus (backward-compat)
 *                        - null: corpus unavailable; GATE03-corpus-unavailable warning emitted
 *
 * When a ProseComposedIR is provided, approved prose ids are added to the
 * GATE03 resolution set (Gate-1 prose-id extension), and PROSE-suspect-source
 * warnings are emitted for any suspect entries (C9).
 */
export function audit(
  elements: SysmlElement[],
  relationships: SysmlRelationship[],
  corpus: Extracted | ProseComposedIR | null
): AuditResult {
  // ── Always-on rules ──────────────────────────────────────────────────────
  const findings: Finding[] = relationalFindings(elements, relationships);
  const matrix = coverageMatrix(elements, relationships);

  // ── Corpus-gated rules ───────────────────────────────────────────────────
  let fidelity: AuditResult["fidelity"];

  if (corpus !== null) {
    // Resolve which Extracted corpus and which resolution set to use
    let extracted: Extracted;
    let resolutionSet: ReturnType<typeof buildResolutionSet>;

    if (isProseComposedIR(corpus)) {
      extracted = corpus.extracted;
      resolutionSet = buildResolutionSetFromComposed(corpus);

      // ── PROSE-suspect-source (C9): warn on suspect entries ────────────
      for (const entry of corpus.proseEntries) {
        if (entry.status === "suspect") {
          findings.push({
            elementId: entry.id,
            ruleId: "PROSE-suspect-source",
            severity: "warning",
            message: `Prose entry '${entry.id}' (${entry.kind}) has status:'suspect' — ` +
              `citation.docSha256 no longer matches the ingest manifest for doc '${entry.citation.docId}'. ` +
              `Re-review required.`,
            suggestedFix:
              "Re-ingest the source document and re-approve the prose extraction, or " +
              "update citation.docSha256 to match the current document hash.",
          });
        }
      }
    } else {
      // Plain Extracted corpus — backward-compat path
      extracted = corpus;
      resolutionSet = buildResolutionSet(extracted);
    }

    findings.push(...provenanceFindings(elements, resolutionSet));
    fidelity = fidelityReport(elements, extracted, resolutionSet);
  } else {
    // Corpus unavailable — emit degradation finding and return empty fidelity
    findings.push({
      elementId: "model",
      ruleId: "GATE03-corpus-unavailable",
      severity: "warning",
      message:
        "extracted.json could not be loaded — provenance existence and fidelity checks skipped",
      suggestedFix:
        "Set SYSML_BRIDGE_CORPUS_PATH or pass corpus_path to validate_model",
    });
    fidelity = { drops: [], fabrications: [], nearMatches: [] };
  }

  return { findings, fidelity, matrix };
}

// ---------------------------------------------------------------------------
// Re-exports — full public surface for consumers
// ---------------------------------------------------------------------------

export type { AuditResult, Finding, FidelityRow, NearMatch, MatrixRow } from "./findings.js";
export { loadCorpusCached, buildResolutionSet, buildResolutionSetFromComposed, clearCorpusCache } from "./corpus.js";
