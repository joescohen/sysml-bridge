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
 *   audit(elements, relationships, corpus: Extracted | ProseComposedIR | InferredComposedIR | null): AuditResult
 *   — synchronous, not async (keeps it unit-testable without fs)
 *   — export names: relationalFindings, provenanceFindings (per plan 05-02/05-03)
 *
 * Re-exports of the full public surface so consumers import only from
 * "../audit/index.js":
 *   - audit, AuditResult, Finding, FidelityRow, NearMatch, MatrixRow (types)
 *   - loadCorpusCached, buildResolutionSet, buildResolutionSetFromComposed,
 *     buildResolutionSetFromInferred, clearCorpusCache (corpus helpers)
 */

import type { SysmlElement, SysmlRelationship } from "@sysml-bridge/model";
import type { Extracted, ProseComposedIR, InferredComposedIR } from "@sysml-bridge/model";
import type { AuditResult, Finding } from "./findings.js";
import { relationalFindings } from "./relational.js";
import { provenanceFindings } from "./provenance.js";
import { proseVerbatimFindings } from "./prose-verbatim.js";
import { entityFindings, type EntityRecordLike } from "./entities.js";
import { fidelityReport } from "./fidelity.js";
import { coverageMatrix } from "./matrix.js";
import {
  buildResolutionSet,
  buildResolutionSetFromComposed,
  buildResolutionSetFromInferred,
} from "./corpus.js";

// ---------------------------------------------------------------------------
// Type guards
// ---------------------------------------------------------------------------

/**
 * Type guard: is the corpus argument an InferredComposedIR (three-layer)?
 * Must be checked BEFORE isProseComposedIR since InferredComposedIR extends ProseComposedIR.
 */
function isInferredComposedIR(
  corpus: Extracted | ProseComposedIR | InferredComposedIR | null
): corpus is InferredComposedIR {
  return (
    corpus !== null &&
    typeof corpus === "object" &&
    "extracted" in corpus &&
    "proseEntries" in corpus &&
    "approvedProseIds" in corpus &&
    "inferredEntries" in corpus &&
    "approvedInferredIds" in corpus
  );
}

/**
 * Type guard: is the corpus argument a ProseComposedIR (two-layer)?
 * isInferredComposedIR must be checked first.
 */
function isProseComposedIR(
  corpus: Extracted | ProseComposedIR | InferredComposedIR | null
): corpus is ProseComposedIR {
  return (
    corpus !== null &&
    typeof corpus === "object" &&
    "extracted" in corpus &&
    "proseEntries" in corpus &&
    "approvedProseIds" in corpus
  );
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/**
 * Assemble a complete AuditResult from model snapshots.
 *
 * @param elements      All SysML elements (from store.queryElements())
 * @param relationships All relationships (from store.queryRelationships())
 * @param corpus        One of:
 *                        - InferredComposedIR: three-layer IR (corpus + prose + inferred)
 *                        - ProseComposedIR: two-layer IR with prose entries
 *                        - Extracted: plain corpus (backward-compat)
 *                        - null: corpus unavailable; GATE03-corpus-unavailable warning emitted
 *
 * When an InferredComposedIR is provided:
 *   - approved inferred ids resolve GATE03 (inferred-id Gate-1 extension)
 *   - INFER-suspect-premise warnings for suspect inferred entries
 *   - INFER-unpremised errors for entries with empty premises (defense-in-depth)
 *   - fidelity.inferred counts model elements with inferred-layer provenance
 *
 * When a ProseComposedIR is provided (two-layer):
 *   - approved prose ids resolve GATE03 (prose-id Gate-1 extension)
 *   - PROSE-suspect-source warnings for suspect prose entries
 */
/**
 * Optional W1 entity-store inputs to the audit. Absent → the ENT-* rules are
 * skipped entirely (a model with no resolved entities). Present with
 * `mentionIds` undefined → ENT-dangling-mention-ref degrades to a warning rather
 * than a vacuous pass.
 */
export interface AuditEntityInput {
  /** The resolved entity records (entities.json). */
  entities?: EntityRecordLike[];
  /** The set of every mentionId in the mention store, or undefined if unavailable. */
  mentionIds?: ReadonlySet<string>;
}

export function audit(
  elements: SysmlElement[],
  relationships: SysmlRelationship[],
  corpus: Extracted | ProseComposedIR | InferredComposedIR | null,
  entityInput?: AuditEntityInput
): AuditResult {
  // ── Always-on rules ──────────────────────────────────────────────────────
  const findings: Finding[] = relationalFindings(elements, relationships);
  const matrix = coverageMatrix(elements, relationships);

  // ── W1 entity rules (only when an entity store is supplied) ──────────────
  if (entityInput?.entities !== undefined) {
    findings.push(...entityFindings(entityInput.entities, entityInput.mentionIds));
  }

  // ── Corpus-gated rules ───────────────────────────────────────────────────
  let fidelity: AuditResult["fidelity"];

  if (corpus !== null) {
    let extracted: Extracted;
    let resolutionSet: ReturnType<typeof buildResolutionSet>;

    if (isInferredComposedIR(corpus)) {
      // ── Three-layer path: inferred + prose + corpus ──────────────────────
      extracted = corpus.extracted;
      resolutionSet = buildResolutionSetFromInferred(corpus);

      // PROSE-suspect-source (C9): warn on suspect prose entries
      for (const entry of corpus.proseEntries) {
        if (entry.status === "suspect") {
          findings.push({
            elementId: entry.id,
            ruleId: "PROSE-suspect-source",
            severity: "warning",
            message:
              `Prose entry '${entry.id}' (${entry.kind}) has status:'suspect' — ` +
              `citation.docSha256 no longer matches the ingest manifest for doc '${entry.citation.docId}'. ` +
              `Re-review required.`,
            suggestedFix:
              "Re-ingest the source document and re-approve the prose extraction, or " +
              "update citation.docSha256 to match the current document hash.",
          });
        }
      }

      // INFER-suspect-premise (A6): warn on suspect inferred entries
      for (const entry of corpus.inferredEntries) {
        if (entry.status === "suspect") {
          findings.push({
            elementId: entry.id,
            ruleId: "INFER-suspect-premise",
            severity: "warning",
            message:
              `Inferred entry '${entry.id}' (${entry.relationFamily}) has status:'suspect' — ` +
              `one or more premises (${entry.premises.join(", ")}) resolve to a suspect/superseded ` +
              `entry or are missing from the composed IR. Re-review required.`,
            suggestedFix:
              "Re-run the inference pipeline after the premise entries are re-approved, " +
              "or supersede this entry and re-propose.",
          });
        }
      }

      // PROSE-unverbatim-quote (C6): re-check approved prose entries' quotes
      // against the ingest chunk store (degrades to a warning when unavailable).
      findings.push(...proseVerbatimFindings(corpus.proseEntries, corpus.chunkStore));

      // INFER-unpremised (defense-in-depth): error on zero-premise entries
      for (const entry of corpus.inferredEntries) {
        if (entry.premises.length === 0) {
          findings.push({
            elementId: entry.id,
            ruleId: "INFER-unpremised",
            severity: "error",
            message:
              `Inferred entry '${entry.id}' (${entry.relationFamily}) has no premises — ` +
              `every inferred link must cite at least one composed-IR premise.`,
            suggestedFix:
              "Re-run the inference pipeline; proposals without resolvable premises should have " +
              "been dropped before reaching the approval queue.",
          });
        }
      }

    } else if (isProseComposedIR(corpus)) {
      // ── Two-layer path: prose + corpus ───────────────────────────────────
      extracted = corpus.extracted;
      resolutionSet = buildResolutionSetFromComposed(corpus);

      // PROSE-suspect-source (C9): warn on suspect entries
      for (const entry of corpus.proseEntries) {
        if (entry.status === "suspect") {
          findings.push({
            elementId: entry.id,
            ruleId: "PROSE-suspect-source",
            severity: "warning",
            message:
              `Prose entry '${entry.id}' (${entry.kind}) has status:'suspect' — ` +
              `citation.docSha256 no longer matches the ingest manifest for doc '${entry.citation.docId}'. ` +
              `Re-review required.`,
            suggestedFix:
              "Re-ingest the source document and re-approve the prose extraction, or " +
              "update citation.docSha256 to match the current document hash.",
          });
        }
      }

      // PROSE-unverbatim-quote (C6): re-check approved prose entries' quotes
      // against the ingest chunk store (degrades to a warning when unavailable).
      findings.push(...proseVerbatimFindings(corpus.proseEntries, corpus.chunkStore));
    } else {
      // ── One-layer path: plain Extracted corpus (backward-compat) ─────────
      extracted = corpus;
      resolutionSet = buildResolutionSet(extracted);
    }

    findings.push(...provenanceFindings(elements, resolutionSet));

    // Compute three-bucket fidelity report
    const { drops, fabrications, nearMatches } = fidelityReport(
      elements,
      extracted,
      resolutionSet
    );

    // F8 fourth bucket: count model elements with inferred-layer provenance
    // Separate from other buckets — NEVER netted against drops/fabrications/nearMatches
    let inferred = 0;
    if (isInferredComposedIR(corpus)) {
      for (const el of elements) {
        const prov = el.raw.provenanceSourceId;
        if (typeof prov === "string" && corpus.approvedInferredIds.has(prov)) {
          inferred++;
        }
      }
    }

    fidelity = { drops, fabrications, nearMatches, inferred };
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
    fidelity = { drops: [], fabrications: [], nearMatches: [], inferred: 0 };
  }

  return { findings, fidelity, matrix };
}

// ---------------------------------------------------------------------------
// Re-exports — full public surface for consumers
// ---------------------------------------------------------------------------

export type { AuditResult, Finding, FidelityRow, NearMatch, MatrixRow } from "./findings.js";
export {
  loadCorpusCached,
  buildResolutionSet,
  buildResolutionSetFromComposed,
  buildResolutionSetFromInferred,
  clearCorpusCache,
} from "./corpus.js";

// ── GATE-05 structural check surface (consumed by mcp-server edit tools) ──
export { structuralCheck, checkBatch, resolveGateCorpus } from "./structural.js";
export type { Candidate } from "./structural.js";

// ── Traceability type sets + report writer (consumed by validate_model tool) ──
export { TRACE_TYPES, FORWARD_TYPES, VERIFY_TYPES, BACKWARD_TYPES } from "./relational.js";
export { writeReports } from "./report.js";

// ── Gate-1 rule pack (consumed directly by prose-gate/inferred-gate tests) ──
export { provenanceFindings } from "./provenance.js";
export { proseVerbatimFindings } from "./prose-verbatim.js";
export { entityFindings } from "./entities.js";
export type { EntityRecordLike } from "./entities.js";
