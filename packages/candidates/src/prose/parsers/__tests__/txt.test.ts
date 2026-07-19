/**
 * Tests for parsers/txt.ts — plain-text parsing: form-feed page-break path,
 * paragraph-fallback path, and whole-document fallback.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseTxt } from "../txt.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");
const sampleTxt = join(fixturesDir, "sample.txt");
const paginatedTxt = join(fixturesDir, "sample-paginated.txt");

describe("parseTxt — paragraph fallback (sample.txt, no form feeds)", () => {
  it("returns RawParseResult with non-empty text", async () => {
    const result = await parseTxt(sampleTxt);
    expect(result.text.length).toBeGreaterThan(0);
  });

  it("text equals pages joined with double newline", async () => {
    const result = await parseTxt(sampleTxt);
    expect(result.text).toBe(result.pages.join("\n\n"));
  });

  it("splits into multiple pages, one per blank-line-delimited paragraph", async () => {
    const result = await parseTxt(sampleTxt);
    expect(result.pages.length).toBeGreaterThan(1);
    expect(result.metadata.pageBreakSource).toBe("paragraph");
  });

  it("synthesizes 'Paragraph N' headings for citation purposes", async () => {
    const result = await parseTxt(sampleTxt);
    const headings = result.metadata.headings as Array<{ title: string; pageIndex: number }>;
    expect(headings.length).toBe(result.pages.length);
    expect(headings[0]?.title).toBe("Paragraph 1");
    expect(headings[headings.length - 1]?.title).toBe(`Paragraph ${headings.length}`);
  });

  it("two runs produce byte-identical text (determinism)", async () => {
    const r1 = await parseTxt(sampleTxt);
    const r2 = await parseTxt(sampleTxt);
    expect(r1.text).toBe(r2.text);
    expect(r1.pages).toEqual(r2.pages);
  });
});

describe("parseTxt — form-feed page breaks (sample-paginated.txt)", () => {
  it("splits into exactly 3 pages at form-feed boundaries", async () => {
    const result = await parseTxt(paginatedTxt);
    expect(result.pages).toHaveLength(3);
    expect(result.metadata.pageBreakSource).toBe("form-feed");
  });

  it("synthesizes 'Page N' headings", async () => {
    const result = await parseTxt(paginatedTxt);
    const headings = result.metadata.headings as Array<{ title: string }>;
    expect(headings.map((h) => h.title)).toEqual(["Page 1", "Page 2", "Page 3"]);
  });

  it("page content matches the text between form feeds", async () => {
    const result = await parseTxt(paginatedTxt);
    expect(result.pages[0]).toContain("system overview and scope");
    expect(result.pages[1]).toContain("detailed requirements listing");
    expect(result.pages[2]).toContain("verification and validation matrix");
  });
});

describe("parseTxt — whole-document fallback (single paragraph, no breaks)", () => {
  it("a single-paragraph file becomes one page with no synthetic headings", async () => {
    const os = await import("node:os");
    const fs = await import("node:fs/promises");
    const tmp = join(os.tmpdir(), `single-para-${Date.now()}.txt`);
    await fs.writeFile(tmp, "Just one paragraph, no blank lines, no form feeds.");
    try {
      const result = await parseTxt(tmp);
      expect(result.pages).toHaveLength(1);
      expect(result.metadata.pageBreakSource).toBe("whole-document");
      expect(result.metadata.headings).toEqual([]);
    } finally {
      await fs.unlink(tmp);
    }
  });

  it("returns empty result for an empty file", async () => {
    const os = await import("node:os");
    const fs = await import("node:fs/promises");
    const tmp = join(os.tmpdir(), `empty-${Date.now()}.txt`);
    await fs.writeFile(tmp, "");
    try {
      const result = await parseTxt(tmp);
      expect(result.text).toBe("");
      expect(result.pages).toEqual([]);
    } finally {
      await fs.unlink(tmp);
    }
  });
});

describe("parseTxt — error path", () => {
  it("throws descriptive error for non-existent file", async () => {
    await expect(parseTxt("/nonexistent/does-not-exist.txt")).rejects.toThrow();
  });

  it("error message references the file path or parse failure", async () => {
    await expect(parseTxt("/nonexistent/does-not-exist.txt")).rejects.toThrow(
      /TXT|parse|ENOENT|nonexistent/i,
    );
  });
});
