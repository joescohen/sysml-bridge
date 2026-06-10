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
  };
  matrix: MatrixRow[];
}
