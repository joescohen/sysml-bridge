/**
 * @sysml-bridge/candidates/prose — PDF parsing, text chunking, requirement detection.
 *
 * Ported from se-process-platform/packages/engine/src/corpus/ @ b39b071.
 * Pure prose ingestion pipeline: no embedding, no retrieval (C5).
 */

export { parsePdf } from "./parsers/pdf.js";
export { parseDocx } from "./parsers/docx.js";
export { parseXlsx } from "./parsers/xlsx.js";
export { parseCsv } from "./parsers/csv.js";
export { parseMd } from "./parsers/md.js";
export { parseTxt } from "./parsers/txt.js";
export { parseDocument, detectFormat } from "./parsers/dispatch.js";
export type { SupportedFormat } from "./parsers/dispatch.js";
export type { RawParseResult, ParsedHeading } from "./parsers/types.js";

export { generateChunkId, chunkText, chunkWithIds } from "./chunker.js";
export type { ChunkIdInput, ChunkContext, ChunkTextOptions, Chunk } from "./chunker.js";

export { detectAndChunkRequirements } from "./requirement-chunker.js";
export type { RequirementChunkerContext, RequirementChunk } from "./requirement-chunker.js";

export {
  generateSectionId,
  extractSectionMapFromPages,
  extractSectionMap,
} from "./section-map.js";
export type { SectionNode, SectionMap, HeadingInput } from "./section-map.js";
