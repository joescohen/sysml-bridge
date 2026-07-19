/**
 * CSV document parser.
 *
 * Uses the `xlsx` (SheetJS) package rather than a hand-rolled splitter —
 * SheetJS auto-detects and correctly re-serializes CSV (quoted fields,
 * embedded commas/newlines) through the same `read`/`sheet_to_csv` path used
 * for XLSX, so quoting edge cases don't need a second implementation.
 *
 * A CSV file is a single "sheet" — one page, titled by the file's basename,
 * with the header row (first line) recorded separately in metadata for
 * citation context.
 *
 * No CUI/banner detection — out of scope by design.
 */

import { readFile } from "node:fs/promises";
import { basename } from "node:path";
import * as XLSX from "xlsx";
import type { RawParseResult, ParsedHeading } from "./types.js";

export async function parseCsv(filePath: string): Promise<RawParseResult> {
  let buffer: Buffer;
  try {
    buffer = await readFile(filePath);
  } catch (err) {
    throw new Error(
      `CSV parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  let workbook: XLSX.WorkBook;
  try {
    workbook = XLSX.read(buffer, { type: "buffer", raw: true });
  } catch (err) {
    throw new Error(
      `CSV parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  const sheetName = workbook.SheetNames?.[0];
  const sheet = sheetName ? workbook.Sheets[sheetName] : undefined;
  const csvBody = sheet ? XLSX.utils.sheet_to_csv(sheet, { blankrows: false }).trim() : "";

  if (csvBody.length === 0) {
    return {
      text: "",
      pages: [],
      metadata: { parser: "csv", headerRow: "", headings: [], totalPages: 0 },
    };
  }

  const title = basename(filePath);
  const headerRow = csvBody.split("\n")[0] ?? "";
  const headings: ParsedHeading[] = [{ title, level: 1, pageIndex: 0 }];

  return {
    text: csvBody,
    pages: [csvBody],
    metadata: {
      parser: "csv",
      headerRow,
      headings,
      totalPages: 1,
    },
  };
}
