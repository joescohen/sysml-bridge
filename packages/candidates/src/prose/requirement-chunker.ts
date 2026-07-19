// Ported from se-process-platform/packages/engine/src/corpus/requirement-chunker.ts @ b39b071

/**
 * Structure-aware requirement chunking (C3, C5).
 *
 * Detects numbered requirement items in parsed document text and emits one
 * RequirementChunk per item with its section-hierarchy context prefix prepended.
 *
 * Detection patterns (union — first match wins per line):
 *   1. Alphanumeric ID pattern:  /^[A-Z][\w-]+-[A-Z]?-?\d+[:.]/
 *      Matches: SYS-REQ-042:, CONOPS-SCN-003., CORP-05:
 *   2. Section-numbered shall:   /^\d+\.\d[\d.]*\s+(?:The|This|An? )\w/
 *      Matches: 3.2.1 The system shall..., 4.1 An operator...
 *   3. Explicit Req label:       /^Req(?:uirement)?\s+\d+[:.]/i
 *      Matches: Requirement 17:, Req 3:
 *
 * PURE function — no retrieval, no embedding, no I/O (C5).
 */

import { createHash } from "node:crypto";
import { generateChunkId } from "./chunker.js";
import type { ChunkContext } from "./chunker.js";

// ── Context type ──────────────────────────────────────────────────────────────

export interface RequirementChunkerContext extends ChunkContext {}

// ── Result type ───────────────────────────────────────────────────────────────

export interface RequirementChunk {
  chunkId: string;
  documentId: string;
  sectionId: string;
  pageStart: number;
  pageEnd: number;
  text: string;
  chunkIndex: number;
  tracesTo: string[];
  verifiedBy: string[];
}

// ── Detection regexes ─────────────────────────────────────────────────────────

const REQUIREMENT_PATTERNS: RegExp[] = [
  // Pattern 1: Alphanumeric ID with hyphen separators
  /^[A-Z][\w-]+-[A-Z]?-?\d+[:.]/,
  // Pattern 2: Section-numbered with leading determiner
  /^\d+\.\d[\d.]*\s+(?:The|This|An?)\s+\w/,
  // Pattern 3: Explicit Req/Requirement label
  /^Req(?:uirement)?\s+\d+[:.]/i,
];

function isRequirementStart(line: string): boolean {
  const trimmed = line.trim();
  return REQUIREMENT_PATTERNS.some((pattern) => pattern.test(trimmed));
}

// ── Trace extraction ──────────────────────────────────────────────────────────

function extractTracesTo(text: string): string[] {
  const match = text.match(/[Tt]races?\s+to:\s*([^\n]+)/);
  if (!match) return [];
  return match[1]!
    .split(/[,;]\s*/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function extractVerifiedBy(text: string): string[] {
  const match = text.match(/[Vv]erified?\s+by:\s*([^\n]+)/);
  if (!match) return [];
  return match[1]!
    .split(/[,;]\s*/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

// ── Main function ─────────────────────────────────────────────────────────────

/**
 * Detect requirement items in text and emit one RequirementChunk per item.
 *
 * Algorithm:
 * 1. Split text into lines. Scan each line against the three detection regexes.
 * 2. When a match is found, close the previous item buffer and start a new one.
 * 3. Non-matching lines are appended to the current item buffer.
 * 4. After the last line, close the final item buffer.
 * 5. For each item: extract tracesTo and verifiedBy from item text.
 * 6. Prepend section context: "[Section: {sectionContext}] ".
 * 7. Generate chunkId and construct RequirementChunk.
 *
 * PURE: no embedding, no retrieval, no I/O (C5).
 */
export async function detectAndChunkRequirements(
  text: string,
  sectionContext: string,
  context: RequirementChunkerContext,
): Promise<RequirementChunk[]> {
  const lines = text.split("\n");

  const itemBuffers: string[][] = [];
  let currentBuffer: string[] | null = null;

  for (const line of lines) {
    if (isRequirementStart(line)) {
      if (currentBuffer !== null && currentBuffer.length > 0) {
        itemBuffers.push(currentBuffer);
      }
      currentBuffer = [line];
    } else if (currentBuffer !== null) {
      currentBuffer.push(line);
    }
  }

  if (currentBuffer !== null && currentBuffer.length > 0) {
    itemBuffers.push(currentBuffer);
  }

  if (itemBuffers.length === 0) return [];

  const chunks: RequirementChunk[] = [];

  for (let itemIndex = 0; itemIndex < itemBuffers.length; itemIndex++) {
    const buffer = itemBuffers[itemIndex]!;
    const rawText = buffer.join("\n").trim();

    const tracesTo = extractTracesTo(rawText);
    const verifiedBy = extractVerifiedBy(rawText);

    const chunkText = `[Section: ${sectionContext}] ${rawText}`;

    const chunkId = generateChunkId({
      documentHash: context.documentHash,
      sectionPath: context.sectionPath + ":req-item",
      chunkIndex: itemIndex,
    });

    chunks.push({
      chunkId,
      documentId: context.documentId,
      sectionId: context.sectionId,
      pageStart: context.pageStart,
      pageEnd: context.pageEnd,
      text: chunkText,
      chunkIndex: itemIndex,
      tracesTo,
      verifiedBy,
    });
  }

  return chunks;
}
