/**
 * Tests for parsers/md.ts — Markdown parsing (same RawParseResult contract as
 * pdf.ts) + heading-driven section boundaries.
 */

import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseMd } from "../md.js";
import { extractSectionMap } from "../../section-map.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../../fixtures");
const sampleMd = join(fixturesDir, "sample.md");

describe("parseMd — unit (sample fixture)", () => {
  it("returns RawParseResult with non-empty text", async () => {
    const result = await parseMd(sampleMd);
    expect(typeof result.text).toBe("string");
    expect(result.text.length).toBeGreaterThan(0);
  });

  it("returns pages array with length > 0", async () => {
    const result = await parseMd(sampleMd);
    expect(Array.isArray(result.pages)).toBe(true);
    expect(result.pages.length).toBeGreaterThan(0);
  });

  it("text equals pages joined with double newline", async () => {
    const result = await parseMd(sampleMd);
    expect(result.text).toBe(result.pages.join("\n\n"));
  });

  it("two runs produce byte-identical text (determinism)", async () => {
    const r1 = await parseMd(sampleMd);
    const r2 = await parseMd(sampleMd);
    expect(r1.text).toBe(r2.text);
    expect(r1.pages).toEqual(r2.pages);
  });

  it("metadata.parser is 'md'", async () => {
    const result = await parseMd(sampleMd);
    expect(result.metadata.parser).toBe("md");
  });

  it("detects every ATX heading with correct title and level", async () => {
    const result = await parseMd(sampleMd);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    expect(headings.map((h) => h.title)).toEqual([
      "ANGARS Refueling System — Sample Spec",
      "Introduction",
      "Background",
      "Prior Art",
      "Requirements",
    ]);
    expect(headings.map((h) => h.level)).toEqual([1, 2, 2, 3, 2]);
  });

  it("headings feed extractSectionMap into a nested tree (Prior Art under Background)", async () => {
    const result = await parseMd(sampleMd);
    const headings = result.metadata.headings as Array<{
      title: string;
      level: number;
      pageIndex: number;
    }>;
    const sectionMap = extractSectionMap(headings, "doc-md", "md", result.pages.length);
    expect(sectionMap.format).toBe("md");

    const top = sectionMap.sections.find((s) => s.title === "ANGARS Refueling System — Sample Spec");
    expect(top).toBeDefined();
    const background = top?.children.find((s) => s.title === "Background");
    expect(background).toBeDefined();
    expect(background?.children.map((c) => c.title)).toContain("Prior Art");
  });

  it("returns empty result for an empty file", async () => {
    const os = await import("node:os");
    const fs = await import("node:fs/promises");
    const tmp = join(os.tmpdir(), `empty-${Date.now()}.md`);
    await fs.writeFile(tmp, "");
    try {
      const result = await parseMd(tmp);
      expect(result.text).toBe("");
      expect(result.pages).toEqual([]);
    } finally {
      await fs.unlink(tmp);
    }
  });

  it("a file with no headings becomes a single unheaded page", async () => {
    const os = await import("node:os");
    const fs = await import("node:fs/promises");
    const tmp = join(os.tmpdir(), `no-headings-${Date.now()}.md`);
    await fs.writeFile(tmp, "Just body text.\nNo headings whatsoever.\n");
    try {
      const result = await parseMd(tmp);
      expect(result.pages).toHaveLength(1);
      expect(result.metadata.headings).toEqual([]);
    } finally {
      await fs.unlink(tmp);
    }
  });
});

describe("parseMd — error path", () => {
  it("throws descriptive error for non-existent file", async () => {
    await expect(parseMd("/nonexistent/does-not-exist.md")).rejects.toThrow();
  });

  it("error message references the file path or parse failure", async () => {
    await expect(parseMd("/nonexistent/does-not-exist.md")).rejects.toThrow(
      /Markdown|parse|ENOENT|nonexistent/i,
    );
  });
});
