/**
 * Tests for parsers/xlsx.ts — XLSX parsing via the `xlsx` (SheetJS) reader,
 * one page per sheet, sheet names as section headings.
 *
 * fixtures/sample.xlsx has 2 sheets: "Requirements" (3 rows incl. header),
 * "Needs" (2 rows incl. header) — see fixtures/gen-fixtures.ts.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseXlsx } from "../xlsx.js";
import { extractSectionMap } from "../../section-map.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");
const sampleXlsx = join(fixturesDir, "sample.xlsx");
const malformedXlsx = join(fixturesDir, "malformed.xlsx");

describe("parseXlsx — unit (sample fixture, 2 sheets)", () => {
  it("returns RawParseResult with non-empty text", async () => {
    const result = await parseXlsx(sampleXlsx);
    expect(result.text.length).toBeGreaterThan(0);
  });

  it("returns one page per sheet", async () => {
    const result = await parseXlsx(sampleXlsx);
    expect(result.pages).toHaveLength(2);
    expect(result.metadata.sheetCount).toBe(2);
    expect(result.metadata.sheetNames).toEqual(["Requirements", "Needs"]);
  });

  it("text equals pages joined with double newline", async () => {
    const result = await parseXlsx(sampleXlsx);
    expect(result.text).toBe(result.pages.join("\n\n"));
  });

  it("each page text starts with the sheet name and contains its header row", async () => {
    const result = await parseXlsx(sampleXlsx);
    expect(result.pages[0]).toContain("Sheet: Requirements");
    expect(result.pages[0]).toContain("Statement");
    expect(result.pages[1]).toContain("Sheet: Needs");
    expect(result.pages[1]).toContain("Description");
  });

  it("preserves row data in the extracted text", async () => {
    const result = await parseXlsx(sampleXlsx);
    expect(result.text).toContain("REQ-001");
    expect(result.text).toContain("Autonomous Refueling");
  });

  it("produces one heading per sheet, titled by sheet name", async () => {
    const result = await parseXlsx(sampleXlsx);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    expect(headings).toEqual([
      { title: "Requirements", level: 1, pageIndex: 0 },
      { title: "Needs", level: 1, pageIndex: 1 },
    ]);
  });

  it("headings feed extractSectionMap into two top-level sections", async () => {
    const result = await parseXlsx(sampleXlsx);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    const sectionMap = extractSectionMap(headings, "doc-xlsx", "xlsx", result.pages.length);
    expect(sectionMap.format).toBe("xlsx");
    expect(sectionMap.sections.map((s) => s.title)).toEqual(["Requirements", "Needs"]);
  });

  it("two runs produce byte-identical text (determinism)", async () => {
    const r1 = await parseXlsx(sampleXlsx);
    const r2 = await parseXlsx(sampleXlsx);
    expect(r1.text).toBe(r2.text);
  });
});

describe("parseXlsx — error path", () => {
  it("throws descriptive error for non-existent file", async () => {
    await expect(parseXlsx("/nonexistent/does-not-exist.xlsx")).rejects.toThrow();
  });

  it("error message references the file path or parse failure", async () => {
    await expect(parseXlsx("/nonexistent/does-not-exist.xlsx")).rejects.toThrow(
      /XLSX|parse|ENOENT|nonexistent/i,
    );
  });

  it("throws cleanly (not a crash) for a malformed workbook", async () => {
    await expect(parseXlsx(malformedXlsx)).rejects.toThrow(/XLSX/i);
  });
});
