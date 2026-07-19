/**
 * Format-dispatch entry point for prose-ingest parsing.
 *
 * Callers/scripts should go through `parseDocument` rather than importing a
 * specific parser directly — it picks the right parser by file extension and
 * gives a clear, listed error for anything unsupported, so a typo or a new
 * corpus format shows up as one readable error instead of a silent
 * mis-parse or a scattered set of ad-hoc extension checks.
 */

import { extname } from "node:path";
import { parsePdf } from "./pdf.js";
import { parseDocx } from "./docx.js";
import { parsePptx } from "./pptx.js";
import { parseXlsx } from "./xlsx.js";
import { parseCsv } from "./csv.js";
import { parseMd } from "./md.js";
import { parseTxt } from "./txt.js";
import type { RawParseResult } from "./types.js";

export type SupportedFormat = "pdf" | "docx" | "pptx" | "xlsx" | "csv" | "md" | "txt";

/** File extension (lowercase, with leading dot) -> parser format. */
const EXTENSION_TO_FORMAT: Record<string, SupportedFormat> = {
  ".pdf": "pdf",
  ".docx": "docx",
  ".pptx": "pptx",
  ".xlsx": "xlsx",
  ".csv": "csv",
  ".md": "md",
  ".markdown": "md",
  ".txt": "txt",
};

const PARSERS: Record<SupportedFormat, (filePath: string) => Promise<RawParseResult>> = {
  pdf: parsePdf,
  docx: parseDocx,
  pptx: parsePptx,
  xlsx: parseXlsx,
  csv: parseCsv,
  md: parseMd,
  txt: parseTxt,
};

/**
 * Determine the parser format for a file path by its extension.
 * Throws a clear, actionable error for unsupported extensions.
 */
export function detectFormat(filePath: string): SupportedFormat {
  const ext = extname(filePath).toLowerCase();
  const format = EXTENSION_TO_FORMAT[ext];
  if (!format) {
    const supported = [...new Set(Object.values(EXTENSION_TO_FORMAT))].sort().join(", ");
    const extensions = Object.keys(EXTENSION_TO_FORMAT).sort().join(", ");
    throw new Error(
      `Unsupported prose-ingest file extension "${ext || "(none)"}" for "${filePath}". ` +
        `Supported formats: ${supported} (extensions: ${extensions}).`,
    );
  }
  return format;
}

/**
 * Parse any supported document by dispatching on file extension.
 * Same RawParseResult shape ({ text, pages, metadata }) regardless of format.
 */
export async function parseDocument(filePath: string): Promise<RawParseResult> {
  const format = detectFormat(filePath);
  return PARSERS[format](filePath);
}
