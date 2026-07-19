/**
 * Markdown document parser.
 *
 * No dependency — ATX headings (`#` .. `######`) are the section-boundary
 * signal, matching the CommonMark form used throughout engineering corpora.
 * Setext headings (`===`/`---` underlines) are intentionally NOT supported —
 * ATX is the dominant convention in the corpora this ingests, and adding a
 * second heading grammar would double the edge cases for no real gain here.
 *
 * A "page" is synthesized per heading: any text before the first heading is
 * page 0 (untitled preamble, omitted from `metadata.headings`); each
 * subsequent heading starts a new page. This mirrors how the PDF path scans
 * per-page text for section boundaries, but the boundaries here are exact
 * (from markdown syntax) rather than heuristic.
 *
 * No CUI/banner detection — out of scope by design.
 */

import { readFile } from "node:fs/promises";
import type { RawParseResult, ParsedHeading } from "./types.js";

const ATX_HEADING_RE = /^(#{1,6})\s+(.+?)\s*#*\s*$/;

interface Segment {
  lines: string[];
  heading?: { title: string; level: number };
}

function segmentByHeadings(raw: string): { segments: Segment[] } {
  const lines = raw.split(/\r\n|\r|\n/);
  const segments: Segment[] = [{ lines: [] }];

  for (const line of lines) {
    const match = ATX_HEADING_RE.exec(line.trim());
    if (match) {
      const level = (match[1] ?? "#").length;
      const title = (match[2] ?? "").trim();
      if (title.length > 0) {
        segments.push({ lines: [line], heading: { title, level } });
        continue;
      }
    }
    segments[segments.length - 1]?.lines.push(line);
  }

  return { segments };
}

/**
 * Parse a Markdown file into pages segmented by heading, with an exact
 * heading list (title/level/pageIndex) in metadata.
 *
 * @param filePath - Absolute or relative path to the .md file.
 */
export async function parseMd(filePath: string): Promise<RawParseResult> {
  let raw: string;
  try {
    raw = await readFile(filePath, "utf8");
  } catch (err) {
    throw new Error(
      `Markdown parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  if (raw.trim().length === 0) {
    return { text: "", pages: [], metadata: { parser: "md", headingCount: 0, headings: [], totalPages: 0 } };
  }

  const { segments } = segmentByHeadings(raw);

  const pages: string[] = [];
  const headings: ParsedHeading[] = [];

  for (const segment of segments) {
    const text = segment.lines.join("\n").trim();
    if (text.length === 0 && segment.heading === undefined) continue; // drop empty preamble
    const pageIndex = pages.length;
    pages.push(text);
    if (segment.heading) {
      headings.push({ title: segment.heading.title, level: segment.heading.level, pageIndex });
    }
  }

  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      parser: "md",
      headingCount: headings.length,
      headings,
      totalPages: pages.length,
    },
  };
}
