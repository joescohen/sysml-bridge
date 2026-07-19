/**
 * Tests for parsers/pptx.ts — PPTX parsing via fflate (zip) + a lightweight
 * PresentationML/DrawingML text/title-placeholder reader.
 *
 * fixtures/sample.pptx has 10 slides — slide1 ("Overview"), slide2
 * ("Requirements"), slide3 ("Background"), slide4-9 (filler), slide10
 * ("Closing") — see fixtures/gen-fixtures.ts. Slide numbering intentionally
 * goes past 9 so a lexical (zip-entry/string) sort bug would be caught:
 * lexically, "slide10.xml" sorts right after "slide1.xml", ahead of
 * "slide2.xml" — the parser must sort by numeric slide number instead.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parsePptx } from "../pptx.js";
import { extractSectionMap } from "../../section-map.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");
const samplePptx = join(fixturesDir, "sample.pptx");
const malformedPptx = join(fixturesDir, "malformed.pptx");

describe("parsePptx — unit (sample fixture, 10 slides)", () => {
  it("returns RawParseResult with non-empty text", async () => {
    const result = await parsePptx(samplePptx);
    expect(result.text.length).toBeGreaterThan(0);
  });

  it("returns one page per slide", async () => {
    const result = await parsePptx(samplePptx);
    expect(result.pages).toHaveLength(10);
    expect(result.metadata.slideCount).toBe(10);
  });

  it("text equals pages joined with double newline", async () => {
    const result = await parsePptx(samplePptx);
    expect(result.text).toBe(result.pages.join("\n\n"));
  });

  it("extracts the plain-text slide content", async () => {
    const result = await parsePptx(samplePptx);
    expect(result.text).toContain(
      "The system shall provide autonomous aerial refueling operations",
    );
    expect(result.text).toContain("REQ-001: The system shall detect coupling faults");
  });

  it("extracts slides in NUMERIC slide order, not lexical order", async () => {
    const result = await parsePptx(samplePptx);
    // Lexically, "slide10" < "slide2" — if the parser sorted by zip-entry
    // string instead of slide number, "Closing" (slide10) would appear at
    // pages[1], right after "Overview" (slide1), not at the end.
    expect(result.pages[0]).toContain("Overview");
    expect(result.pages[1]).toContain("Requirements");
    expect(result.pages[9]).toContain("Closing");
    expect(result.pages[9]).toContain("Tenth slide content");
    // Sanity: slide10's content never lands ahead of slide2's in the joined text.
    expect(result.text.indexOf("Tenth slide content")).toBeGreaterThan(
      result.text.indexOf("REQ-001"),
    );
  });

  it("detects each slide's title placeholder as a level-1 heading, in slide order", async () => {
    const result = await parsePptx(samplePptx);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    expect(headings.map((h) => h.title)).toEqual([
      "Overview",
      "Requirements",
      "Background",
      "Slide Four",
      "Slide Five",
      "Slide Six",
      "Slide Seven",
      "Slide Eight",
      "Slide Nine",
      "Closing",
    ]);
    expect(headings.every((h) => h.level === 1)).toBe(true);
    expect(headings.map((h) => h.pageIndex)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });

  it("headings feed extractSectionMap into ten top-level sections", async () => {
    const result = await parsePptx(samplePptx);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    const sectionMap = extractSectionMap(headings, "doc-pptx", "pptx", result.pages.length);
    expect(sectionMap.format).toBe("pptx");
    expect(sectionMap.sections.map((s) => s.title)).toEqual(headings.map((h) => h.title));
  });

  it("metadata.parser identifies the native-zip reader", async () => {
    const result = await parsePptx(samplePptx);
    expect(result.metadata.parser).toBe("pptx-native-zip");
  });

  it("two runs produce byte-identical text (determinism)", async () => {
    const r1 = await parsePptx(samplePptx);
    const r2 = await parsePptx(samplePptx);
    expect(r1.text).toBe(r2.text);
    expect(r1.pages).toEqual(r2.pages);
    expect(r1.metadata.headings).toEqual(r2.metadata.headings);
  });
});

describe("parsePptx — error path", () => {
  it("throws descriptive error for non-existent file", async () => {
    await expect(parsePptx("/nonexistent/does-not-exist.pptx")).rejects.toThrow();
  });

  it("error message references the file path or parse failure", async () => {
    await expect(parsePptx("/nonexistent/does-not-exist.pptx")).rejects.toThrow(
      /PPTX|parse|ENOENT|nonexistent/i,
    );
  });

  it("throws cleanly (not a crash) for a malformed (non-zip) .pptx", async () => {
    await expect(parsePptx(malformedPptx)).rejects.toThrow(/PPTX/i);
  });
});
