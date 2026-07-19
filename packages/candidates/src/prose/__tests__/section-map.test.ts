/**
 * Tests for section-map.ts — section extraction from PDF pages.
 *
 * TDD: written BEFORE implementation.
 */

import { describe, it, expect } from "vitest";
import { extractSectionMapFromPages, extractSectionMap, generateSectionId } from "../section-map.js";

describe("generateSectionId", () => {
  it("returns a deterministic string", () => {
    const id1 = generateSectionId("doc-1", "Introduction", 1);
    const id2 = generateSectionId("doc-1", "Introduction", 1);
    expect(id1).toBe(id2);
  });

  it("is prefixed with sec-", () => {
    const id = generateSectionId("doc-1", "Introduction", 1);
    expect(id).toMatch(/^sec-/);
  });

  it("returns 20 characters total (sec- prefix + 16 hex chars)", () => {
    const id = generateSectionId("doc-1", "Introduction", 1);
    expect(id).toHaveLength(20);
  });

  it("different titles produce different IDs", () => {
    const id1 = generateSectionId("doc-1", "Introduction", 1);
    const id2 = generateSectionId("doc-1", "Background", 1);
    expect(id1).not.toBe(id2);
  });

  it("different document IDs produce different IDs", () => {
    const id1 = generateSectionId("doc-1", "Introduction", 1);
    const id2 = generateSectionId("doc-2", "Introduction", 1);
    expect(id1).not.toBe(id2);
  });

  it("normalizes title whitespace before hashing", () => {
    const id1 = generateSectionId("doc-1", "  Introduction  ", 1);
    const id2 = generateSectionId("doc-1", "Introduction", 1);
    expect(id1).toBe(id2);
  });
});

describe("extractSectionMapFromPages", () => {
  it("returns empty sections for empty pages array", () => {
    const result = extractSectionMapFromPages([], "doc-empty");
    expect(result.sections).toHaveLength(0);
    expect(result.format).toBe("pdf");
  });

  it("returns empty sections for pages with no headings", () => {
    const result = extractSectionMapFromPages(
      ["just body text here\nno headings whatsoever"],
      "doc-no-h",
    );
    expect(result.sections).toHaveLength(0);
  });

  it("detects numbered section headings (e.g., '1. INTRODUCTION')", () => {
    const result = extractSectionMapFromPages(["1. INTRODUCTION\nBody text here."], "doc-pdf");
    expect(result.sections.length).toBeGreaterThan(0);
    expect(result.sections[0]?.title).toContain("INTRODUCTION");
  });

  it("detects '3.2.1' style numbered sections", () => {
    const result = extractSectionMapFromPages(
      ["3.2.1 Functional Requirements\nBody text."],
      "doc-pdf",
    );
    expect(result.sections.length).toBeGreaterThan(0);
    expect(result.sections[0]?.title).toContain("Functional Requirements");
  });

  it("detects ALL CAPS short lines as headings", () => {
    const result = extractSectionMapFromPages(
      ["EXECUTIVE SUMMARY\nThis section covers..."],
      "doc-pdf",
    );
    expect(result.sections.length).toBeGreaterThan(0);
  });

  it("sets page_start from page index", () => {
    const result = extractSectionMapFromPages(
      ["page 0 body text only", "1. INTRODUCTION\nBody text on page 1"],
      "doc-pdf",
    );
    const intro = result.sections.find((s) => s.title.includes("INTRODUCTION"));
    expect(intro?.pageStart).toBe(1);
  });

  it("assigns deterministic IDs (same pages twice = same IDs)", () => {
    const pages = ["1. INTRODUCTION\nBody text.", "2. BACKGROUND\nMore text."];
    const r1 = extractSectionMapFromPages(pages, "doc-1");
    const r2 = extractSectionMapFromPages(pages, "doc-1");
    expect(r1.sections[0]?.id).toBe(r2.sections[0]?.id);
  });

  it("returns format pdf", () => {
    const result = extractSectionMapFromPages(["1. INTRO\nText."], "doc-pdf");
    expect(result.format).toBe("pdf");
  });

  it("sets totalPages to the number of pages", () => {
    const pages = ["page 1", "page 2", "page 3"];
    const result = extractSectionMapFromPages(pages, "doc-3p");
    expect(result.totalPages).toBe(3);
  });
});

describe("extractSectionMap — heading-driven (non-PDF formats)", () => {
  it("builds a tree directly from an exact heading list, no text heuristics", () => {
    const headings = [
      { title: "Introduction", level: 1, pageIndex: 0 },
      { title: "Background", level: 2, pageIndex: 1 },
      { title: "Requirements", level: 1, pageIndex: 2 },
    ];
    const result = extractSectionMap(headings, "doc-md", "md", 3);
    expect(result.format).toBe("md");
    expect(result.totalPages).toBe(3);
    expect(result.sections.map((s) => s.title)).toEqual(["Introduction", "Requirements"]);
    expect(result.sections[0]?.children.map((c) => c.title)).toEqual(["Background"]);
  });

  it("supports the xlsx/csv/docx/txt format tags", () => {
    for (const format of ["xlsx", "csv", "docx", "txt"] as const) {
      const result = extractSectionMap([{ title: "Sheet1", level: 1, pageIndex: 0 }], "doc", format);
      expect(result.format).toBe(format);
    }
  });

  it("returns empty sections for an empty heading list", () => {
    const result = extractSectionMap([], "doc-empty", "txt");
    expect(result.sections).toHaveLength(0);
  });

  it("assigns deterministic IDs (same headings twice = same IDs)", () => {
    const headings = [{ title: "Intro", level: 1, pageIndex: 0 }];
    const r1 = extractSectionMap(headings, "doc-1", "md");
    const r2 = extractSectionMap(headings, "doc-1", "md");
    expect(r1.sections[0]?.id).toBe(r2.sections[0]?.id);
  });

  it("omits totalPages when not provided", () => {
    const result = extractSectionMap([{ title: "A", level: 1, pageIndex: 0 }], "doc", "md");
    expect(result.totalPages).toBeUndefined();
  });
});
