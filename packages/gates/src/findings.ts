/**
 * Canonical Finding type vocabulary for Gate 1 audit (GATE-01 shape).
 *
 * Rule-ID registry (implemented in later plans):
 *   R4-def-operand                      error   — trace relationship operand is a Definition, not a Usage
 *   GATE02-dangling-endpoint            error   — relationship source/target id not found in model
 *   GATE02-id-duplicate                 error   — two elements share the same id
 *   GATE02-orphan                       warning — leaf design element has no satisfy/allocate/derive edge
 *   GATE02-unsatisfied                  warning — system requirement has no satisfy trace
 *   GATE02-unverified                   warning — system requirement has no verify trace
 *   GATE02-unbacktraced                 warning — system requirement has no derive trace
 *   GATE02-uncovered-need               warning — stakeholder need not covered by any system requirement
 *   GATE03-missing-provenance           warning — element has no provenanceSourceId
 *   GATE03-unresolvable-provenance      error   — provenanceSourceId not in corpus resolution set
 *   GATE03-model-asserted               info    — element uses "model-asserted" provenance (allocate convention)
 *   GATE03-corpus-unavailable           warning — extracted.json could not be loaded; provenance checks skipped
 *   PROSE-suspect-source                warning — prose entry cites a doc whose hash no longer matches the
 *                                                 ingest manifest; entry still composes but should be re-reviewed
 *   PROSE-unverbatim-quote              error   — approved prose entry whose citation.quote does NOT occur
 *                                                 verbatim in its cited chunk (hallucinated/drifted quote)
 *   PROSE-unverbatim-quote-unavailable  warning — approved prose entries present but the ingest chunk store
 *                                                 was unavailable, so quotes were not re-verified (degrade path)
 *   INFER-suspect-premise               warning — inferred entry whose ANY premise id resolves to a suspect/
 *                                                 superseded entry, or is missing from the composed IR;
 *                                                 entry still composes but should be re-reviewed
 *   INFER-unpremised                    error   — inferred-layer entry with no resolvable premises
 *                                                 (defense-in-depth; the pipeline should drop these pre-approval)
 */

// No runtime imports — pure type + const file, same as sysml-elements.ts

export type Severity = "error" | "warning" | "info";

export interface Finding {
  elementId: string;
  ruleId: string;
  message: string;
  severity: Severity;
  suggestedFix: string;
}

export interface FidelityRow {
  corpusId: string;
  corpusName: string;
  kind: string;
}

export interface NearMatch extends FidelityRow {
  modelElementId: string;
  modelName: string;
  similarity: number;
  band: "confident" | "review";
}

export interface MatrixRow {
  reqId: string;
  reqName: string | null;
  satisfied: boolean;
  verified: boolean;
  derived: boolean;
}

export interface AuditResult {
  findings: Finding[];
  fidelity: {
    drops: FidelityRow[];
    fabrications: FidelityRow[];
    nearMatches: NearMatch[];
    /**
     * F8 fourth bucket: count of model elements whose provenanceSourceId resolves
     * to an inferred-layer id (approvedInferredIds). Separate from drops/fabrications/
     * nearMatches — NEVER netted against them.
     */
    inferred: number;
  };
  matrix: MatrixRow[];
}
