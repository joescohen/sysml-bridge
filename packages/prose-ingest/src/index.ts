/**
 * @sysml-bridge/prose-ingest — PDF parsing, text chunking, requirement detection.
 *
 * Ported from se-process-platform/packages/engine/src/corpus/ @ b39b071.
 * Pure prose ingestion pipeline: no embedding, no retrieval (C5).
 */

export { parsePdf } from "./parsers/pdf.js";
export type { RawParseResult } from "./parsers/pdf.js";

export { generateChunkId, chunkText, chunkWithIds } from "./chunker.js";
export type { ChunkIdInput, ChunkContext, ChunkTextOptions, Chunk } from "./chunker.js";

export { detectAndChunkRequirements } from "./requirement-chunker.js";
export type { RequirementChunkerContext, RequirementChunk } from "./requirement-chunker.js";

export { generateSectionId, extractSectionMapFromPages } from "./section-map.js";
export type { SectionNode, SectionMap } from "./section-map.js";
