/**
 * Tests for parsers/docx.ts — DOCX parsing via fflate (zip) + a lightweight
 * WordprocessingML text/heading-style reader.
 *
 * fixtures/sample.docx has one Heading1 ("Introduction"), one nested Heading2
 * ("Background") and a second Heading1 ("Requirements") — see
 * fixtures/gen-fixtures.ts.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseDocx } from "../docx.js";
import { extractSectionMap } from "../../section-map.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");
const sampleDocx = join(fixturesDir, "sample.docx");
const malformedDocx = join(fixturesDir, "malformed.docx");

describe("parseDocx — unit (sample fixture)", () => {
  it("returns RawParseResult with non-empty text", async () => {
    const result = await parseDocx(sampleDocx);
    expect(result.text.length).toBeGreaterThan(0);
  });

  it("returns pages array with length > 0", async () => {
    const result = await parseDocx(sampleDocx);
    expect(result.pages.length).toBeGreaterThan(0);
  });

  it("text equals pages joined with double newline", async () => {
    const result = await parseDocx(sampleDocx);
    expect(result.text).toBe(result.pages.join("\n\n"));
  });

  it("extracts the plain-text paragraph content (entity-decoded)", async () => {
    const result = await parseDocx(sampleDocx);
    expect(result.text).toContain("Sample fixture for the DOCX parser test suite.");
    expect(result.text).toContain("REQ-001: The system shall detect coupling faults");
  });

  it("detects Heading1/Heading2 styles as section boundaries in document order", async () => {
    const result = await parseDocx(sampleDocx);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    expect(headings.map((h) => h.title)).toEqual(["Introduction", "Background", "Requirements"]);
    expect(headings.map((h) => h.level)).toEqual([1, 2, 1]);
  });

  it("headings feed extractSectionMap into a nested tree (Background under Introduction)", async () => {
    const result = await parseDocx(sampleDocx);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    const sectionMap = extractSectionMap(headings, "doc-docx", "docx", result.pages.length);
    expect(sectionMap.format).toBe("docx");
    const intro = sectionMap.sections.find((s) => s.title === "Introduction");
    expect(intro?.children.map((c) => c.title)).toEqual(["Background"]);
    expect(sectionMap.sections.map((s) => s.title)).toContain("Requirements");
  });

  it("metadata.parser identifies the native-zip reader", async () => {
    const result = await parseDocx(sampleDocx);
    expect(result.metadata.parser).toBe("docx-native-zip");
  });

  it("two runs produce byte-identical text (determinism)", async () => {
    const r1 = await parseDocx(sampleDocx);
    const r2 = await parseDocx(sampleDocx);
    expect(r1.text).toBe(r2.text);
    expect(r1.pages).toEqual(r2.pages);
  });
});

describe("parseDocx — error path", () => {
  it("throws descriptive error for non-existent file", async () => {
    await expect(parseDocx("/nonexistent/does-not-exist.docx")).rejects.toThrow();
  });

  it("error message references the file path or parse failure", async () => {
    await expect(parseDocx("/nonexistent/does-not-exist.docx")).rejects.toThrow(
      /DOCX|parse|ENOENT|nonexistent/i,
    );
  });

  it("throws cleanly (not a crash) for a malformed (non-zip) .docx", async () => {
    await expect(parseDocx(malformedDocx)).rejects.toThrow(/DOCX/i);
  });
});
