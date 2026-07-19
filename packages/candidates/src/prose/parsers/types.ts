/**
 * Shared types for prose-ingest parsers (pdf, docx, pptx, xlsx, csv, md, txt).
 *
 * Every parser in this directory returns exactly this shape so that
 * chunker.ts / section-map.ts and the ingest pipeline work unchanged
 * regardless of source format.
 */

// ── Result type ───────────────────────────────────────────────────────────────

export interface RawParseResult {
  /** Full extracted text from the document (pages joined with \n\n). */
  text: string;
  /**
   * Per-"page" text array. For paginated formats (PDF) these are real pages.
   * For non-paginated formats, a parser synthesizes logical pages — a segment
   * per heading (MD/DOCX), a segment per sheet (XLSX), a segment per slide
   * (PPTX), or a page/paragraph fallback (TXT) — so citations stay
   * human-navigable.
   */
  pages: string[];
  /** Parser-specific metadata. */
  metadata: Record<string, unknown>;
}

// ── Heading type ──────────────────────────────────────────────────────────────

/**
 * One heading/section boundary as detected DIRECTLY from a format's native
 * structure (markdown `#`, DOCX heading styles, XLSX sheet names, PPTX slide
 * titles, TXT page/paragraph fallback) — not inferred by text heuristics the
 * way the PDF path infers headings from raw page text. Non-PDF parsers place
 * these on `metadata.headings` for `section-map.ts#extractSectionMap`.
 */
export interface ParsedHeading {
  title: string;
  level: number;
  /** Index into the RawParseResult.pages array where this heading starts. */
  pageIndex: number;
}
