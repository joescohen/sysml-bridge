/**
 * Plain-text document parser.
 *
 * No dependency. TXT has no native section syntax, so pages are synthesized
 * with a two-tier fallback:
 *   1. Form-feed page breaks (\f, 0x0C) — some exported/legacy plaintext
 *      corpora carry real page boundaries this way. If present, each
 *      form-feed-delimited segment is one page ("Page N").
 *   2. Paragraph fallback — blank-line-delimited paragraphs, each treated as
 *      one page ("Paragraph N"), so citations stay human-navigable even
 *      without any real page/section markers.
 *   3. Whole-document fallback — a single-paragraph file becomes one
 *      unheaded page (mirrors the PDF single-page case; no synthetic
 *      heading is fabricated for a document with no internal structure).
 *
 * No CUI/banner detection — out of scope by design.
 */

import { readFile } from "node:fs/promises";
import type { RawParseResult, ParsedHeading } from "./types.js";

const FORM_FEED = "\f";

export async function parseTxt(filePath: string): Promise<RawParseResult> {
  let raw: string;
  try {
    raw = await readFile(filePath, "utf8");
  } catch (err) {
    throw new Error(
      `TXT parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  if (raw.trim().length === 0) {
    return {
      text: "",
      pages: [],
      metadata: { parser: "txt", pageBreakSource: "empty", headings: [], totalPages: 0 },
    };
  }

  let pages: string[];
  let pageBreakSource: "form-feed" | "paragraph" | "whole-document";

  if (raw.includes(FORM_FEED)) {
    pages = raw
      .split(FORM_FEED)
      .map((p) => p.trim())
      .filter((p) => p.length > 0);
    pageBreakSource = "form-feed";
  } else {
    const paragraphs = raw
      .split(/\n{2,}/)
      .map((p) => p.trim())
      .filter((p) => p.length > 0);

    if (paragraphs.length > 1) {
      pages = paragraphs;
      pageBreakSource = "paragraph";
    } else {
      pages = [raw.trim()];
      pageBreakSource = "whole-document";
    }
  }

  const headings: ParsedHeading[] =
    pageBreakSource === "form-feed"
      ? pages.map((_, i) => ({ title: `Page ${i + 1}`, level: 1, pageIndex: i }))
      : pageBreakSource === "paragraph"
        ? pages.map((_, i) => ({ title: `Paragraph ${i + 1}`, level: 1, pageIndex: i }))
        : [];

  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      parser: "txt",
      pageBreakSource,
      headings,
      totalPages: pages.length,
    },
  };
}
