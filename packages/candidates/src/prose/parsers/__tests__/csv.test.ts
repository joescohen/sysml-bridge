/**
 * Tests for parsers/csv.ts — CSV parsing via the `xlsx` (SheetJS) reader.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseCsv } from "../csv.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");
const sampleCsv = join(fixturesDir, "sample.csv");

describe("parseCsv — unit (sample fixture)", () => {
  it("returns RawParseResult with non-empty text", async () => {
    const result = await parseCsv(sampleCsv);
    expect(result.text.length).toBeGreaterThan(0);
  });

  it("returns exactly one page (CSV is a single sheet)", async () => {
    const result = await parseCsv(sampleCsv);
    expect(result.pages).toHaveLength(1);
  });

  it("text equals pages joined with double newline", async () => {
    const result = await parseCsv(sampleCsv);
    expect(result.text).toBe(result.pages.join("\n\n"));
  });

  it("captures the header row in metadata", async () => {
    const result = await parseCsv(sampleCsv);
    expect(result.metadata.headerRow).toContain("ID");
    expect(result.metadata.headerRow).toContain("Statement");
    expect(result.metadata.headerRow).toContain("Priority");
  });

  it("preserves all data rows in the extracted text", async () => {
    const result = await parseCsv(sampleCsv);
    expect(result.text).toContain("REQ-001");
    expect(result.text).toContain("REQ-002");
    expect(result.text).toContain("REQ-003");
  });

  it("produces one heading titled after the file's basename", async () => {
    const result = await parseCsv(sampleCsv);
    const headings = result.metadata.headings as Array<{ title: string; pageIndex: number }>;
    expect(headings).toHaveLength(1);
    expect(headings[0]?.title).toBe("sample.csv");
    expect(headings[0]?.pageIndex).toBe(0);
  });

  it("two runs produce byte-identical text (determinism)", async () => {
    const r1 = await parseCsv(sampleCsv);
    const r2 = await parseCsv(sampleCsv);
    expect(r1.text).toBe(r2.text);
  });

  it("returns empty result for an empty file", async () => {
    const os = await import("node:os");
    const fs = await import("node:fs/promises");
    const tmp = join(os.tmpdir(), `empty-${Date.now()}.csv`);
    await fs.writeFile(tmp, "");
    try {
      const result = await parseCsv(tmp);
      expect(result.text).toBe("");
      expect(result.pages).toEqual([]);
    } finally {
      await fs.unlink(tmp);
    }
  });
});

describe("parseCsv — error path", () => {
  it("throws descriptive error for non-existent file", async () => {
    await expect(parseCsv("/nonexistent/does-not-exist.csv")).rejects.toThrow();
  });

  it("error message references the file path or parse failure", async () => {
    await expect(parseCsv("/nonexistent/does-not-exist.csv")).rejects.toThrow(
      /CSV|parse|ENOENT|nonexistent/i,
    );
  });
});
