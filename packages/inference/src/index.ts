/**
 * @sysml-bridge/inference — F8 inference / extrapolation engine.
 *
 * Public API:
 *   - runInferenceEngine: main orchestrator (generate → type gate → propose → debate → output)
 *   - InferenceProvider: injectable provider interface
 *   - AnthropicInferenceProvider: real Anthropic implementation
 *   - Types: RelationFamily, TypedCandidate, RejectedCandidate, etc.
 *   - buildElementMap, applyTypeGate, checkTypeGate: type gate utilities
 *   - generateCandidates, inferenceStableId: candidate generation
 *   - computeDebateVerdict, runDebate: debate stage
 *   - buildContextBundle, serializeNeighborhood: context building
 */

export { runInferenceEngine, hashComposedIR, estimateCostUsd, validatePremises } from "./engine.js";
export type { EngineOptions, EngineResult } from "./engine.js";

export type { InferenceProvider } from "./inference-provider.js";
export { AnthropicInferenceProvider } from "./inference-provider.js";

export {
  buildElementMap,
  applyTypeGate,
  checkTypeGate,
} from "./type-gate.js";

export {
  generateCandidates,
  inferenceStableId,
  buildSkipSet,
} from "./candidate-generator.js";
export type { RawCandidate, GenerationResult } from "./candidate-generator.js";

export { computeDebateVerdict, runDebate, DEBATE_ADVOCATE_CONFIRM, DEBATE_CHALLENGER_REJECT, DEBATE_CHALLENGER_MAX_CONFIRM } from "./debate.js";

export { buildContextBundle, serializeNeighborhood } from "./neighborhood.js";

export type {
  RelationFamily,
  TypedCandidate,
  RejectedCandidate,
  DroppedUnpremisedCandidate,
  ProposalOutput,
  AutoRejectedRecord,
  QueuedRecord,
  DebateRecord,
  DebateResult,
  DebateVerdict,
  ContextBundle,
  CandidateRecord,
  RunStats,
  BandLabel,
} from "./types.js";
export { classifyBand, CONF_FLOOR, CONF_DEBATE_MAX, ProposalOutputSchema } from "./types.js";
