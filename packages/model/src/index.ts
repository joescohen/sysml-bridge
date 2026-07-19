export { ExtractedSchema, SCHEMA_VERSION } from "./schema.js";
export type { Extracted } from "./schema.js";
export { stableId } from "./stable-id.js";
export { normalizeForVerbatim, quoteOccursInChunk, normSurface } from "./verbatim.js";
export { ProseApprovedEntrySchema, composeIR as composeProseTwoLayer } from "./prose-approved.js";
export type { ProseApprovedEntry, ProseComposedIR } from "./prose-approved.js";
export {
  InferredApprovedEntrySchema,
  composeIR,
  FAMILY_TO_RELATIONSHIP_TYPE,
  isTraceRelationFamily,
  projectInferredTraceRelationships,
} from "./inferred-approved.js";
export type {
  InferredApprovedEntry,
  InferredComposedIR,
  TraceRelationFamily,
} from "./inferred-approved.js";
export {
  appendApproval,
  recordRejection,
  isApproved,
  isRejected,
  CandidateEntrySchema,
} from "./approval-helpers.js";
export type { CandidateEntry } from "./approval-helpers.js";
export {
  appendInferredApproval,
  recordInferredRejection,
  isInferredApproved,
  isInferredRejected,
  InferenceCandidateSchema,
} from "./inferred-approval-helpers.js";
export type { InferenceCandidate } from "./inferred-approval-helpers.js";
export {
  appendEntityMerge,
  recordEntityRejection,
  isEntityMergeApproved,
  isEntityMergeRejected,
  entityMergePairKey,
  EntityMergeCandidateSchema,
  EntityMergeApprovedEntrySchema,
} from "./entity-approval-helpers.js";
export type {
  EntityMergeCandidate,
  EntityMergeApprovedEntry,
} from "./entity-approval-helpers.js";
export { parseNeeds, parseActivityId, stripIdPrefix } from "./extract/parsers.js";
export { assertCount } from "./extract/counts.js";
export { extractN2Triples, assertSpotCheck, N2_ROW_IS_SOURCE } from "./extract/n2.js";
export type { N2RawTriple } from "./extract/n2.js";
export {
  WORKBOOKS,
  N2_SHEETS,
  ANGARS_SS_HEADERS,
  SUBSYSTEM_SHEET_MAP,
} from "./extract/workbook-config.js";
export type { WorkbookConfig, N2SheetConfig } from "./extract/workbook-config.js";

// --- store (merged from the former mcp-server store half) ---
export * from "./store/store.js";
export * from "./store/file-store.js";
export * from "./store/types.js";
export * from "./store/sysml-v2-api-store.js";
