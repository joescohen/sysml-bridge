/**
 * Tests for parsers/dispatch.ts — format-dispatch entry point.
 *
 * Verifies extension -> format routing, that parseDocument routes to the
 * right parser (checked via metadata.parser), and that unsupported
 * extensions get a clear, listed error instead of a silent mis-parse.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { detectFormat, parseDocument } from "../dispatch.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");

describe("detectFormat", () => {
  it.each([
    ["/a/b/report.pdf", "pdf"],
    ["/a/b/report.PDF", "pdf"],
    ["/a/b/spec.docx", "docx"],
    ["/a/b/deck.pptx", "pptx"],
    ["/a/b/data.xlsx", "xlsx"],
    ["/a/b/table.csv", "csv"],
    ["/a/b/notes.md", "md"],
    ["/a/b/notes.markdown", "md"],
    ["/a/b/log.txt", "txt"],
  ])("%s -> %s", (filePath, expected) => {
    expect(detectFormat(filePath)).toBe(expected);
  });

  it("throws a clear error for an unsupported extension", () => {
    expect(() => detectFormat("/a/b/image.png")).toThrow(/Unsupported prose-ingest file extension/);
  });

  it("error message lists the supported formats", () => {
    expect(() => detectFormat("/a/b/image.png")).toThrow(/pdf.*docx.*xlsx|docx.*csv.*md/);
  });

  it("throws for a file with no extension", () => {
    expect(() => detectFormat("/a/b/README")).toThrow(/Unsupported/);
  });
});

describe("parseDocument — routes to the correct parser per extension", () => {
  it("routes .md to the markdown parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.md"));
    expect(result.metadata.parser).toBe("md");
  });

  it("routes .txt to the txt parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.txt"));
    expect(result.metadata.parser).toBe("txt");
  });

  it("routes .csv to the csv parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.csv"));
    expect(result.metadata.parser).toBe("csv");
  });

  it("routes .xlsx to the xlsx parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.xlsx"));
    expect(result.metadata.parser).toBe("xlsx");
  });

  it("routes .docx to the docx parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.docx"));
    expect(result.metadata.parser).toBe("docx-native-zip");
  });

  it("routes .pptx to the pptx parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.pptx"));
    expect(result.metadata.parser).toBe("pptx-native-zip");
  });

  it("routes .pdf to the pdf parser", async () => {
    const result = await parseDocument(join(fixturesDir, "sample.pdf"));
    expect(["unpdf", "mupdf"]).toContain(result.metadata.parser);
  });

  it("rejects an unsupported format before touching the filesystem", async () => {
    await expect(parseDocument("/a/b/image.png")).rejects.toThrow(/Unsupported/);
  });
});
