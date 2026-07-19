/**
 * DOCX (Word document) document parser.
 *
 * DOCX is a ZIP container of XML parts (OOXML/WordprocessingML). Rather than
 * pulling in `mammoth` (which bundles style-mapping and HTML-rendering
 * machinery this ingest layer doesn't need — no CUI/banner detection, no
 * rich formatting, just text + heading structure), this parser uses:
 *   - `fflate` (zero-dependency, ~8KB) to unzip the container — the ZIP
 *     binary format (local/central-directory headers, DEFLATE) is fiddly
 *     enough that hand-rolling it risks subtle corruption bugs, and fflate
 *     is a single, small, widely-used dependency for exactly this.
 *   - A small regex-based reader for word/document.xml — WordprocessingML
 *     paragraphs (`<w:p>`) and text runs (`<w:t>`) are regular enough that a
 *     full XML DOM parser isn't needed for text + heading-style extraction.
 *
 * Headings are detected via paragraph style (`<w:pStyle w:val="HeadingN"/>`)
 * — the same signal Word itself uses for its Navigation Pane / TOC, so this
 * matches the file's actual authored structure rather than guessing from
 * text shape. A "page" is synthesized per heading paragraph, mirroring md.ts.
 *
 * No CUI/banner detection — out of scope by design.
 */

import { readFile } from "node:fs/promises";
import { unzipSync, strFromU8 } from "fflate";
import type { RawParseResult, ParsedHeading } from "./types.js";

const DOCUMENT_XML_PATH = "word/document.xml";

const PARAGRAPH_RE = /<w:p[ >][\s\S]*?<\/w:p>/g;
const PSTYLE_RE = /<w:pStyle\s+w:val="([^"]*)"/;
const HEADING_STYLE_RE = /^Heading(\d)$/i;
// Matches, in document order: a text run's content, or an inline tab/break
// marker (these sit as siblings of <w:t>, not inside it, so a single
// alternation keeps everything in the right order).
const RUN_TOKEN_RE = /<w:t(?:\s[^>]*)?>([\s\S]*?)<\/w:t>|<w:tab\s*\/>|<w:br\s*\/>/g;

const XML_ENTITIES: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&apos;": "'",
};

function decodeXmlEntities(s: string): string {
  return s.replace(/&(amp|lt|gt|quot|apos);/g, (m) => XML_ENTITIES[m] ?? m);
}

function paragraphText(paragraphXml: string): string {
  const parts: string[] = [];
  let match: RegExpExecArray | null;
  RUN_TOKEN_RE.lastIndex = 0;
  while ((match = RUN_TOKEN_RE.exec(paragraphXml)) !== null) {
    if (match[1] !== undefined) {
      parts.push(decodeXmlEntities(match[1]));
    } else if (match[0].startsWith("<w:tab")) {
      parts.push("\t");
    } else {
      parts.push("\n");
    }
  }
  return parts.join("").trim();
}

function paragraphHeadingLevel(paragraphXml: string): number | null {
  const styleMatch = PSTYLE_RE.exec(paragraphXml);
  if (!styleMatch) return null;
  const styleVal = styleMatch[1] ?? "";
  const headingMatch = HEADING_STYLE_RE.exec(styleVal);
  if (!headingMatch) return null;
  const level = Number.parseInt(headingMatch[1] ?? "1", 10);
  return Number.isFinite(level) && level > 0 ? level : null;
}

export async function parseDocx(filePath: string): Promise<RawParseResult> {
  let buffer: Buffer;
  try {
    buffer = await readFile(filePath);
  } catch (err) {
    throw new Error(
      `DOCX parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  let entries: Record<string, Uint8Array>;
  try {
    entries = unzipSync(new Uint8Array(buffer));
  } catch (err) {
    throw new Error(
      `DOCX parse failed for "${filePath}": not a valid .docx (zip) container (${err instanceof Error ? err.message : String(err)})`,
      { cause: err },
    );
  }

  const documentXmlBytes = entries[DOCUMENT_XML_PATH];
  if (!documentXmlBytes) {
    throw new Error(
      `DOCX parse failed for "${filePath}": missing ${DOCUMENT_XML_PATH} (not a valid Word document)`,
    );
  }

  const documentXml = strFromU8(documentXmlBytes);
  const paragraphs = documentXml.match(PARAGRAPH_RE) ?? [];

  const pages: string[] = [];
  const headings: ParsedHeading[] = [];
  let currentLines: string[] = [];

  const flushSegment = () => {
    const text = currentLines.join("\n").trim();
    if (text.length > 0) pages.push(text);
    currentLines = [];
  };

  for (const paragraphXml of paragraphs) {
    const text = paragraphText(paragraphXml);
    const headingLevel = paragraphHeadingLevel(paragraphXml);

    if (headingLevel !== null && text.length > 0) {
      flushSegment();
      const pageIndex = pages.length; // index this new segment will occupy
      currentLines.push(text);
      headings.push({ title: text, level: headingLevel, pageIndex });
      continue;
    }

    if (text.length > 0) currentLines.push(text);
  }
  flushSegment();

  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      parser: "docx-native-zip",
      paragraphCount: paragraphs.length,
      headingCount: headings.length,
      headings,
      totalPages: pages.length,
    },
  };
}
