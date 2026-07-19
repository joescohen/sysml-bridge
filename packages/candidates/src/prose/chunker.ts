// Ported from se-process-platform/packages/engine/src/corpus/chunker.ts @ b39b071

/**
 * Content-addressed text chunking for the corpus ingestion pipeline.
 *
 * Chunk IDs are stable across re-ingestion:
 *   ID = first 32 chars of SHA-256 hex of JSON.stringify({doc, sec, idx})
 *   where sec is the NORMALIZED sectionPath (trim, collapse whitespace, lowercase).
 *
 * CRITICAL: chunk text is NOT included in the hash. This ensures IDs survive
 * minor document edits without invalidating all downstream citations.
 *
 * Splitter: simple char-window splitter (no @langchain/textsplitters dependency).
 * Separators tried in order: \n\n, \n, ". ", " ". Falls back to hard slice at
 * chunkSize when no separator fits.
 */

import { createHash } from "node:crypto";

// ── Chunk ID Generation ────────────────────────────────────────────────────────

/** Input for chunk ID generation. Does NOT include chunk text (by design). */
export interface ChunkIdInput {
  /** SHA-256 hex hash of the raw document bytes. */
  documentHash: string;
  /** Section path string (e.g., "3.2.1" or "Introduction/Background"). */
  sectionPath: string;
  /** Zero-based chunk index within the section. */
  chunkIndex: number;
}

/**
 * Generate a deterministic, content-addressed chunk ID.
 *
 * ID = first 32 chars of SHA-256 hex of JSON.stringify({doc, sec, idx}).
 * sectionPath is normalized (trim, collapse whitespace, lowercase) before hashing.
 * Chunk text is EXCLUDED from the hash.
 */
export function generateChunkId(input: ChunkIdInput): string {
  const normalizedSectionPath = input.sectionPath.trim().replace(/\s+/g, " ").toLowerCase();
  const canonical = JSON.stringify({
    doc: input.documentHash,
    sec: normalizedSectionPath,
    idx: input.chunkIndex,
  });
  return createHash("sha256").update(canonical, "utf8").digest("hex").slice(0, 32);
}

// ── Chunk Type ────────────────────────────────────────────────────────────────

/** A content-addressed text chunk with deterministic ID. */
export interface Chunk {
  chunkId: string;
  documentId: string;
  sectionId: string;
  pageStart: number;
  pageEnd: number;
  text: string;
  chunkIndex: number;
}

// ── Context ───────────────────────────────────────────────────────────────────

/** Context for chunk ID generation and Chunk object construction. */
export interface ChunkContext {
  /** SHA-256 hex of raw document bytes. */
  documentHash: string;
  /** Section ID from SectionMap (stable section identifier). */
  sectionId: string;
  /** Human-readable section path for hash input. */
  sectionPath: string;
  /** Page number where section starts. */
  pageStart: number;
  /** Page number where section ends. */
  pageEnd: number;
  /** Document identifier. */
  documentId: string;
}

// ── Options ───────────────────────────────────────────────────────────────────

export interface ChunkTextOptions {
  /** Target chunk size in characters. Default: 1500. */
  chunkSize?: number;
  /** Overlap between adjacent chunks in characters. Default: 150. */
  chunkOverlap?: number;
}

// ── Default params ────────────────────────────────────────────────────────────

const DEFAULT_CHUNK_SIZE = 1500;
const DEFAULT_CHUNK_OVERLAP = 150;
const DEFAULT_SEPARATORS = ["\n\n", "\n", ". ", " "];

// ── Simple char-window splitter ───────────────────────────────────────────────

/**
 * Split text into overlapping chunks using a simple char-window approach.
 * Tries separators in order: \n\n → \n → ". " → " " → hard slice.
 * No external dependency (replaces @langchain/textsplitters).
 */
export async function chunkText(
  text: string,
  opts?: ChunkTextOptions,
): Promise<string[]> {
  const chunkSize = opts?.chunkSize ?? DEFAULT_CHUNK_SIZE;
  const chunkOverlap = opts?.chunkOverlap ?? DEFAULT_CHUNK_OVERLAP;

  if (text.trim().length === 0) return [];

  // If entire text fits in one chunk, return it directly
  if (text.length <= chunkSize) return [text];

  const chunks: string[] = [];
  let start = 0;

  while (start < text.length) {
    const end = Math.min(start + chunkSize, text.length);

    // If this is the last slice, just take it
    if (end === text.length) {
      const slice = text.slice(start).trim();
      if (slice.length > 0) chunks.push(slice);
      break;
    }

    // Try to find a good split boundary near `end`, walking backwards
    let splitAt = end;
    let foundSep = false;

    for (const sep of DEFAULT_SEPARATORS) {
      // Search backwards from end for the separator
      const searchFrom = Math.max(start, end - Math.floor(chunkSize / 2));
      const idx = text.lastIndexOf(sep, end);
      if (idx >= searchFrom && idx > start) {
        splitAt = idx + sep.length;
        foundSep = true;
        break;
      }
    }

    if (!foundSep) {
      splitAt = end;
    }

    const chunk = text.slice(start, splitAt).trim();
    if (chunk.length > 0) chunks.push(chunk);

    // Next chunk starts with overlap
    start = Math.max(start + 1, splitAt - chunkOverlap);
  }

  return chunks.filter((c) => c.length > 0);
}

// ── Full pipeline ─────────────────────────────────────────────────────────────

/**
 * Split text into content-addressed Chunk objects with deterministic IDs.
 *
 * Two runs with identical inputs → identical Chunk[] (same IDs, same text).
 * Empty text → [].
 */
export async function chunkWithIds(
  text: string,
  context: ChunkContext,
  opts?: ChunkTextOptions,
): Promise<Chunk[]> {
  if (text.trim().length === 0) return [];

  const rawChunks = await chunkText(text, opts);

  return rawChunks.map((rawText, index) => {
    const chunkId = generateChunkId({
      documentHash: context.documentHash,
      sectionPath: context.sectionPath,
      chunkIndex: index,
    });

    return {
      chunkId,
      documentId: context.documentId,
      sectionId: context.sectionId,
      pageStart: context.pageStart,
      pageEnd: context.pageEnd,
      text: rawText,
      chunkIndex: index,
    };
  });
}
