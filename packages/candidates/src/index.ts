/**
 * @sysml-bridge/candidates — human-gated LLM candidate layers under one contract.
 *
 * Barrels:
 *   - ./contract.js — the shared candidate/disposition contract (no-auto-approve invariant)
 *   - ./telemetry.js — RunCounters: counted + logged LLM parse-failure sites
 *   - ./prose/index.js — PDF parsing, text chunking, requirement detection (former prose-ingest)
 *   - ./inference/index.js — F8 inference / extrapolation engine (former inference)
 *   - ./retrieval/index.js — dependency-free BM25 lexical evidence retrieval
 *   - ./chunk-store/index.js — persist/reload the ingest chunk store (chunks.json)
 *   - ./mentions/index.js — mention derivation + persist/reload the mention store (mentions.json)
 *   - ./entities/index.js — entity resolution (auto-cluster + merge proposals) + entity store (entities.json)
 *   - ./weave/index.js — gap-driven pass loop (finding→query table, pass runner, pass records)
 */

export * from "./contract.js";
export * from "./telemetry.js";
export * from "./prose/index.js";
export * from "./inference/index.js";
export * from "./retrieval/index.js";
export * from "./chunk-store/index.js";
export * from "./mentions/index.js";
export * from "./entities/index.js";
export * from "./weave/index.js";
