// Ported from se-process-platform/packages/engine/src/corpus/parsers/pdf.ts @ b39b071

/**
 * PDF document parser.
 *
 * Primary: unpdf (pdfjs-dist wrapper) — extracts per-page text array via
 * getDocumentProxy + extractText. Returns pages joined with double newlines.
 *
 * Fallback: mupdf — used when unpdf throws. Loads the PDF via
 * Document.openDocument, iterates pages, extracts text from structured
 * JSON block/line model.
 */

import { readFile } from "node:fs/promises";
import { extractText, getDocumentProxy } from "unpdf";
import type { RawParseResult } from "./types.js";

// ── mupdf structured text types ───────────────────────────────────────────────

interface MupdfTextLine {
  text?: string;
  spans?: Array<{ text?: string }>;
}

interface MupdfTextBlock {
  type: "text" | string;
  lines?: MupdfTextLine[];
}

interface MupdfStructuredText {
  blocks: MupdfTextBlock[];
}

// ── Result type ───────────────────────────────────────────────────────────────

// RawParseResult now lives in ./types.js — every parser (docx, xlsx, csv, md,
// txt) shares this exact shape. Re-exported here for backward compatibility
// (index.ts and existing tests import it from "./pdf.js").
export type { RawParseResult };

// ── unpdf primary path ────────────────────────────────────────────────────────

async function parsePdfUnpdf(buffer: Uint8Array): Promise<RawParseResult> {
  const pdf = await getDocumentProxy(buffer);
  const { totalPages, text: pageTexts } = await extractText(pdf, { mergePages: false });
  const pages = pageTexts as string[];
  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      totalPages,
      parser: "unpdf",
    },
  };
}

// ── mupdf fallback path ───────────────────────────────────────────────────────

async function parsePdfMupdf(buffer: Buffer): Promise<RawParseResult> {
  const mupdf = await import("mupdf");
  const doc = mupdf.Document.openDocument(buffer, "application/pdf");
  const pageCount = doc.countPages();

  const pages: string[] = [];
  for (let i = 0; i < pageCount; i++) {
    const page = doc.loadPage(i);
    const structuredText = page.toStructuredText("preserve-whitespace");
    const data = JSON.parse(structuredText.asJSON()) as MupdfStructuredText;

    const lines: string[] = [];
    for (const block of data.blocks) {
      if (block.type === "text" && block.lines) {
        for (const line of block.lines) {
          if (line.text) {
            lines.push(line.text);
          } else if (line.spans) {
            const spanText = line.spans.map((s) => s.text ?? "").join("");
            if (spanText) lines.push(spanText);
          }
        }
      }
    }

    pages.push(lines.join("\n"));
  }

  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      totalPages: pageCount,
      parser: "mupdf",
    },
  };
}

// ── Public API ─────────────────────────────────────────────────────────────────

/**
 * Parse a PDF file, extracting per-page text.
 *
 * Attempts unpdf first; falls back to mupdf on failure.
 * If both fail, throws with a combined error message.
 *
 * @param filePath - Absolute or relative path to the .pdf file.
 * @returns RawParseResult with full text, per-page array, and metadata.
 */
export async function parsePdf(filePath: string): Promise<RawParseResult> {
  let buffer: Buffer;
  try {
    buffer = await readFile(filePath);
  } catch (err) {
    throw new Error(
      `PDF parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  // Primary: unpdf
  try {
    return await parsePdfUnpdf(new Uint8Array(buffer));
  } catch (primaryErr) {
    // Fallback: mupdf
    try {
      return await parsePdfMupdf(buffer);
    } catch (fallbackErr) {
      throw new Error(
        `PDF parse failed for "${filePath}" (both unpdf and mupdf failed): ` +
          `unpdf: ${primaryErr instanceof Error ? primaryErr.message : String(primaryErr)}; ` +
          `mupdf: ${fallbackErr instanceof Error ? fallbackErr.message : String(fallbackErr)}`,
        { cause: primaryErr },
      );
    }
  }
}
