/**
 * @sysml-bridge/candidates/inference — F8 inference / extrapolation engine.
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

export { runInferenceEngine, hashComposedIR, estimateCostUsd, validatePremises, boundedPool, resolveInferConcurrency } from "./engine.js";
export type { EngineOptions, EngineResult, PoolFailure, PoolResult, EntityStoreInput } from "./engine.js";

// ── W2: cross-document candidate enumeration ─────────────────────────────────
export { enumerateCooccurrence, cooccurrenceStableId } from "./cooccurrence.js";
export type {
  CooccurrenceCandidate,
  EnumerateCooccurrenceOptions,
  EnumerateCooccurrenceResult,
} from "./cooccurrence.js";
export { enumerateChains, projectApprovedInferredToRelations } from "./chains.js";
export type { AcceptedRelation, EnumerateChainsResult } from "./chains.js";
export {
  COMPOSITION_TABLE,
  COMPOSITION_KEYS,
  compositionKey,
  composeChain,
  isLegalComposition,
  applyChainTypeGate,
  chainStableId,
} from "./composition-table.js";
export type {
  CompositionEntry,
  RawChainCandidate,
  TypedChainCandidate,
  RejectedChainCandidate,
  ChainTypeGateResult,
} from "./composition-table.js";

export type { InferenceProvider } from "./inference-provider.js";
export {
  AnthropicInferenceProvider,
  buildProposeUserMessage,
  renderOfferedFacts,
  renderRetrievedEvidence,
  parseProposeResponse,
  PREMISE_ID_INSTRUCTION,
} from "./inference-provider.js";

export { repairPremises, normalizeLabel } from "./premise-repair.js";
export type { RepairResult } from "./premise-repair.js";

export {
  buildElementMap,
  applyTypeGate,
  checkTypeGate,
  buildEntityElementMap,
} from "./type-gate.js";

export { buildCrossDocContextBundle } from "./neighborhood.js";
export type { CrossDocContextInput } from "./neighborhood.js";

export {
  generateCandidates,
  inferenceStableId,
  buildSkipSet,
} from "./candidate-generator.js";
export type { RawCandidate, GenerationResult } from "./candidate-generator.js";

export {
  applyRelevanceFilter,
  scoreAllocationSignals,
  resolveFamilyCap,
  nameTokens,
  normalizeFlowLabel,
  extractL2Key,
  DEFAULT_FAMILY_CAP,
} from "./relevance-filter.js";
export type {
  RelevanceFilterResult,
  RelevanceFilterOptions,
  AllocationSignalScore,
} from "./relevance-filter.js";

export { computeDebateVerdict, runDebate, DEBATE_ADVOCATE_CONFIRM, DEBATE_CHALLENGER_REJECT, DEBATE_CHALLENGER_MAX_CONFIRM } from "./debate.js";

export { buildContextBundle, serializeNeighborhood, collectOfferedFacts, retrieveEvidence } from "./neighborhood.js";

export type {
  RelationFamily,
  TypedCandidate,
  RejectedCandidate,
  RelevanceRejectedCandidate,
  CappedCandidate,
  DroppedUnpremisedCandidate,
  ProposalOutput,
  ProposeResult,
  OfferedFact,
  AutoRejectedRecord,
  QueuedRecord,
  DebateRecord,
  DebateResult,
  DebateVerdict,
  ContextBundle,
  RetrievedChunk,
  CandidateRecord,
  RunStats,
  BandLabel,
} from "./types.js";
export { classifyBand, CONF_FLOOR, CONF_DEBATE_MAX, ProposalOutputSchema } from "./types.js";
