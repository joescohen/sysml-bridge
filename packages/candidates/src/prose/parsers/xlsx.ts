/**
 * XLSX (Excel workbook) document parser.
 *
 * Uses the `xlsx` (SheetJS) package — already a repo dependency (root
 * devDependency, used by the ANGARS extractor in examples/angars/pipeline/).
 * Reused here rather than adding a second spreadsheet library.
 *
 * One "page" per sheet: the page text is the sheet name followed by its
 * CSV-serialized rows (header row included), so both the section title
 * (sheet name) and the header-row columns are visible for citation context.
 *
 * No CUI/banner detection — out of scope by design.
 */

import { readFile } from "node:fs/promises";
import * as XLSX from "xlsx";
import type { RawParseResult, ParsedHeading } from "./types.js";

export async function parseXlsx(filePath: string): Promise<RawParseResult> {
  let buffer: Buffer;
  try {
    buffer = await readFile(filePath);
  } catch (err) {
    throw new Error(
      `XLSX parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  let workbook: XLSX.WorkBook;
  try {
    workbook = XLSX.read(buffer, { type: "buffer" });
  } catch (err) {
    throw new Error(
      `XLSX parse failed for "${filePath}": not a valid workbook (${err instanceof Error ? err.message : String(err)})`,
      { cause: err },
    );
  }

  const sheetNames = workbook.SheetNames ?? [];
  const pages: string[] = [];
  const headings: ParsedHeading[] = [];
  const headerRows: Record<string, string> = {};

  for (let i = 0; i < sheetNames.length; i++) {
    const name = sheetNames[i] ?? `Sheet${i + 1}`;
    const sheet = workbook.Sheets[name];
    const csvBody = sheet ? XLSX.utils.sheet_to_csv(sheet, { blankrows: false }).trim() : "";
    const headerRow = csvBody.split("\n")[0] ?? "";
    headerRows[name] = headerRow;

    const pageIndex = pages.length;
    pages.push(`Sheet: ${name}\n${csvBody}`.trim());
    headings.push({ title: name, level: 1, pageIndex });
  }

  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      parser: "xlsx",
      sheetCount: sheetNames.length,
      sheetNames,
      headerRows,
      headings,
      totalPages: pages.length,
    },
  };
}
