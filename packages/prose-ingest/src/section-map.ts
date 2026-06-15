// Ported from se-process-platform/packages/engine/src/corpus/section-map.ts @ b39b071

/**
 * Section-map extraction for document heading hierarchies.
 *
 * Extracts a structured heading tree from PDF page text arrays.
 * Section IDs are deterministic: same inputs → same ID.
 * IDs are SHA-256 of (documentId, normalizedTitle, level, parentId).
 */

import { createHash } from "node:crypto";

// ── Section node type ──────────────────────────────────────────────────────────

export interface SectionNode {
  id: string;
  title: string;
  level: number;
  pageStart: number;
  pageEnd?: number;
  children: SectionNode[];
}

// ── Section map type ──────────────────────────────────────────────────────────

export interface SectionMap {
  documentId: string;
  format: "pdf" | "docx";
  sections: SectionNode[];
  totalPages?: number;
}

// ── Section ID Generation ─────────────────────────────────────────────────────

/**
 * Generate a deterministic section ID.
 * ID = 'sec-' + first 16 chars of SHA-256 hex of canonical JSON.
 * Title is normalized (trim, collapse whitespace, lowercase) before hashing.
 */
export function generateSectionId(
  documentId: string,
  title: string,
  level: number,
  parentId?: string,
): string {
  const normalizedTitle = title.trim().replace(/\s+/g, " ").toLowerCase();
  const canonical = JSON.stringify({
    doc: documentId,
    title: normalizedTitle,
    level,
    parent: parentId ?? null,
  });
  const hex = createHash("sha256").update(canonical, "utf8").digest("hex");
  return `sec-${hex.slice(0, 16)}`;
}

// ── Flat heading representation ────────────────────────────────────────────────

interface FlatHeading {
  title: string;
  level: number;
  pageStart: number;
}

// ── PDF heading detection ──────────────────────────────────────────────────────

const NUMBERED_SECTION_RE = /^(\d+(?:\.\d+)*)\.?\s+(.+)$/;
const ALL_CAPS_RE = /^[A-Z][A-Z\s\-\/&:,()]{2,78}[A-Z]$/;

function levelFromPrefix(prefix: string): number {
  return prefix.split(".").length;
}

function extractPageHeadings(pageText: string, pageIndex: number): FlatHeading[] {
  const headings: FlatHeading[] = [];
  const lines = pageText.split("\n");

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (line.length === 0) continue;

    // Heuristic 1: Numbered section
    const numberedMatch = NUMBERED_SECTION_RE.exec(line);
    if (numberedMatch !== null) {
      const prefix = numberedMatch[1] ?? "";
      const titlePart = (numberedMatch[2] ?? "").trim();
      if (titlePart.length > 0) {
        const title = `${prefix} ${titlePart}`;
        headings.push({ title, level: Math.min(levelFromPrefix(prefix), 6), pageStart: pageIndex });
        continue;
      }
    }

    // Heuristic 2: ALL CAPS line
    if (
      line.length >= 4 &&
      line.length < 80 &&
      !line.endsWith(".") &&
      ALL_CAPS_RE.test(line) &&
      !/^\d+$/.test(line)
    ) {
      headings.push({ title: line, level: 1, pageStart: pageIndex });
    }
  }

  return headings;
}

// ── Tree Builder ──────────────────────────────────────────────────────────────

function buildTree(headings: FlatHeading[], documentId: string): SectionNode[] {
  const topLevel: SectionNode[] = [];
  const stack: Array<{ node: SectionNode; level: number }> = [];

  for (const heading of headings) {
    while (stack.length > 0 && (stack[stack.length - 1]?.level ?? 0) >= heading.level) {
      stack.pop();
    }

    const parentId =
      stack.length > 0 ? (stack[stack.length - 1]?.node.id ?? undefined) : undefined;
    const id = generateSectionId(documentId, heading.title, heading.level, parentId);

    const node: SectionNode = {
      id,
      title: heading.title,
      level: heading.level,
      pageStart: heading.pageStart,
      children: [],
    };

    if (stack.length === 0) {
      topLevel.push(node);
    } else {
      stack[stack.length - 1]?.node.children.push(node);
    }

    stack.push({ node, level: heading.level });
  }

  return topLevel;
}

// ── Public API ─────────────────────────────────────────────────────────────────

/**
 * Extract a section map from a per-page text array (PDF path).
 *
 * Uses text heuristics for heading detection:
 * 1. Numbered section patterns (e.g., "3.2.1 Functional Requirements")
 * 2. ALL CAPS short lines (e.g., "EXECUTIVE SUMMARY")
 */
export function extractSectionMapFromPages(pages: string[], documentId: string): SectionMap {
  const flatHeadings: FlatHeading[] = [];

  for (let pageIndex = 0; pageIndex < pages.length; pageIndex++) {
    const pageText = pages[pageIndex] ?? "";
    const pageHeadings = extractPageHeadings(pageText, pageIndex);
    flatHeadings.push(...pageHeadings);
  }

  const sections = buildTree(flatHeadings, documentId);

  return {
    documentId,
    format: "pdf",
    sections,
    totalPages: pages.length,
  };
}
