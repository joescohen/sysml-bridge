/**
 * @sysml-bridge/candidates/retrieval — lexical BM25 evidence retrieval.
 *
 * Dependency-free, in-memory BM25 over chunk texts, used to give the inference
 * engine's link adjudication real corpus evidence. LEXICAL ONLY — no embeddings,
 * no vector DB. Lives outside the prose/ ingest path so the C5 no-retrieval
 * ingest invariant is untouched.
 */

export { Bm25Index, tokenize } from "./bm25.js";
export type { RetrievalChunk, ScoredChunk, Bm25Options } from "./bm25.js";
