/**
 * weave — the gap-driven pass loop (W3).
 *
 * One pass = audit → queue → propose → (human reviews) → recompose → re-audit →
 * record (spec §5). This barrel exposes the three building blocks; the I/O
 * orchestration (load model/corpus/stores, run `audit()`, write the review
 * queue + pass record) lives in `scripts/weave.ts`, OUTSIDE any package src, so
 * `packages/candidates` keeps no runtime dependency on `packages/gates` and the
 * no-auto-approve source-scan ratchet (which walks only `packages/<pkg>/src`)
 * never sees weave's orchestration.
 *
 *   - gap-queue.ts   — the finding→query table (DATA) + `planQueries` (pure).
 *   - pass-runner.ts — bounded, targeted inference; attributes candidates to gaps.
 *   - pass-record.ts — the `pass-record@1` envelope, audit summary, convergence gate.
 *
 * NOTHING here writes a disposition or imports an approval writer — weave only
 * PROPOSES (spec §2, no-auto-approve).
 */

export {
  GAP_QUERY_TABLE,
  MAPPED_FINDING_RULE_IDS,
  QUERY_FAMILY_TO_RELATION_FAMILY,
  planQueries,
  queryFamiliesToRelationFamilies,
  type WeaveFinding,
  type QueryFamily,
  type WeaveQuery,
  type UnmappedFinding,
  type QueuePlan,
  type GapContext,
} from "./gap-queue.js";

export {
  scopeEntitiesToQuery,
  runTargetedInference,
  type TargetedInferenceInput,
  type TargetedInferenceResult,
} from "./pass-runner.js";

export {
  PASS_RECORD_SCHEMA,
  summarizeAudit,
  computeWarningsDelta,
  evaluateConvergence,
  passFileName,
  serializePassRecord,
  parsePassRecord,
  writePassRecordFile,
  loadPassRecordFile,
  type RuleCounts,
  type AuditSummary,
  type ProposedCandidateSummary,
  type DispositionSummary,
  type WarningsDeltaEntry,
  type PassRecord,
  type PassRecordFile,
  type ConvergenceVerdict,
} from "./pass-record.js";
