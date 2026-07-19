/**
 * contract.ts — the shared candidate/disposition contract for this package.
 *
 * INVARIANT: a candidate NEVER enters composeIR without an explicit disposition
 * record. There is NO auto-approve path — anywhere. Every candidate produced by
 * `./prose` (PDF ingestion) or `./inference` (F8 extrapolation) is inert until a
 * human (or an explicit approval helper call, itself gated behind review) writes
 * an approval or rejection record for it. `composeIR` / `composeProseTwoLayer` in
 * `@sysml-bridge/model` read ONLY disposition records — never raw candidates —
 * when deciding what enters the composed IR. See
 * `packages/candidates/src/__tests__/no-auto-approve.test.ts` (Task 3) for the
 * ratchet that keeps this true as the codebase grows.
 *
 * The types and helpers below are owned by `@sysml-bridge/model` (the IR
 * package everything composes against); this module re-exports them so callers
 * within `@sysml-bridge/candidates` have one place to import the contract from.
 */

export type { CandidateEntry } from "@sysml-bridge/model";
export type { InferenceCandidate } from "@sysml-bridge/model";

export {
  appendApproval,
  recordRejection,
  isApproved,
  isRejected,
  appendInferredApproval,
  recordInferredRejection,
  isInferredApproved,
  isInferredRejected,
} from "@sysml-bridge/model";
