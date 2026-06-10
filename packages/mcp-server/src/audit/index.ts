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
import type { Extracted } from "@sysml-bridge/ir";
import type { AuditResult, Finding } from "./findings.js";
import { relationalFindings } from "./relational.js";
import { provenanceFindings } from "./provenance.js";
import { fidelityReport } from "./fidelity.js";
import { coverageMatrix } from "./matrix.js";
import { buildResolutionSet } from "./corpus.js";

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/**
 * Assemble a complete AuditResult from model snapshots.
 *
 * @param elements      All SysML elements (from store.queryElements())
 * @param relationships All relationships (from store.queryRelationships())
 * @param corpus        Loaded + validated corpus, or null if unavailable.
 *                      When null: GATE03-corpus-unavailable warning is emitted;
 *                      provenance existence checks and fidelity are skipped.
 */
export function audit(
  elements: SysmlElement[],
  relationships: SysmlRelationship[],
  corpus: Extracted | null
): AuditResult {
  // ── Always-on rules ──────────────────────────────────────────────────────
  const findings: Finding[] = relationalFindings(elements, relationships);
  const matrix = coverageMatrix(elements, relationships);

  // ── Corpus-gated rules ───────────────────────────────────────────────────
  let fidelity: AuditResult["fidelity"];

  if (corpus !== null) {
    const resolutionSet = buildResolutionSet(corpus);
    findings.push(...provenanceFindings(elements, resolutionSet));
    fidelity = fidelityReport(elements, corpus, resolutionSet);
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
export { loadCorpusCached, buildResolutionSet, clearCorpusCache } from "./corpus.js";
