/**
 * entities — cross-document entity resolution (W1), the hub the whole weaver
 * clusters on. Three bands, one human gate (spec §3):
 *
 *   1. ./cluster.ts   — DETERMINISTIC auto-cluster (no approval): normSurface +
 *                       kindHint exact match → one canonical EntityRecord.
 *   2. ./suggest.ts   — SUGGESTED merges (proposals): acronym/expansion +
 *                       token-overlap (Jaccard over nameTokens).
 *   3. ./adjudicate.ts— mid-band LLM adjudication reusing the advocate/challenger
 *                       debate seam VERBATIM (mock-testable, provider-injected).
 *
 * ./store.ts persists the resolved entities as entities.json (self-describing
 * envelope, throw-on-malformed) — sibling of chunks.json / mentions.json.
 *
 * Bands 2/3 are fuzzy/LLM and therefore emit only PROPOSALS (EntityMergeCandidate,
 * from @sysml-bridge/model) into the existing review queue; only a human
 * disposition makes a merge real (no-auto-approve, spec §2). Lives OUTSIDE
 * prose/ and scripts/ (like chunk-store) so the C5 no-retrieval grep-test stays
 * green.
 */

export { autoCluster, entityIdFor, type EntityRecord } from "./cluster.js";
export {
  suggestMerges,
  acronymMatch,
  isAcronymExpansion,
  tokenOverlap,
  TOKEN_OVERLAP_MIN,
  ACRONYM_CONFIDENCE,
  type SuggestMergesOptions,
} from "./suggest.js";
export { adjudicateEntityMerge, type MergeEvidence } from "./adjudicate.js";
export {
  ENTITY_STORE_SCHEMA,
  serializeEntityStore,
  parseEntityStore,
  writeEntityStoreFile,
  loadEntityStoreFile,
  type EntityStoreFile,
} from "./store.js";
