/**
 * PPTX (PowerPoint presentation) document parser.
 *
 * PPTX is a ZIP container of XML parts (OOXML/PresentationML). Mirrors
 * docx.ts's approach exactly:
 *   - `fflate`'s `unzipSync` + `strFromU8` to unzip the container — same
 *     rationale as docx.ts: hand-rolling ZIP parsing risks subtle corruption
 *     bugs, and fflate is already a repo dependency for exactly this.
 *   - A small regex-based reader over each slide part — DrawingML text runs
 *     (`<a:t>`) inside paragraphs (`<a:p>`) inside shapes (`<p:sp>`) are
 *     regular enough that a full XML DOM parser isn't needed for text +
 *     title extraction.
 *
 * One "page" per slide (`ppt/slides/slideN.xml`), extracted in slide NUMBER
 * order — not zip-entry / lexical order, so "slide2.xml" sorts before
 * "slide10.xml" the way PowerPoint itself orders slides. A slide's title
 * placeholder (`<p:ph type="title"/>` or `type="ctrTitle"/>`) is used as its
 * heading text when present — the same "read the format's own structure,
 * don't infer it from text shape" principle as docx's heading-style
 * detection — falling back to "Slide N" when a slide has no title
 * placeholder (or the placeholder is empty), so every slide still yields a
 * heading/section boundary the way XLSX yields one per sheet.
 *
 * Slide notes (`ppt/notesSlides/`) are intentionally NOT extracted: scoped
 * out to keep this parser a direct mirror of docx.ts (slide body text only),
 * matching the task's guidance that notes support is optional.
 *
 * No CUI/banner detection — out of scope by design.
 */

import { readFile } from "node:fs/promises";
import { unzipSync, strFromU8 } from "fflate";
import type { RawParseResult, ParsedHeading } from "./types.js";

const SLIDE_PATH_RE = /^ppt\/slides\/slide(\d+)\.xml$/;

// Matches, in document order, a paragraph block within a slide's XML.
const PARAGRAPH_RE = /<a:p(?:\s[^>]*)?>[\s\S]*?<\/a:p>/g;
// A shape block — the container that may carry a placeholder-type marker
// (`<p:ph type="title"/>`) identifying it as the slide's title.
const SHAPE_RE = /<p:sp(?:\s[^>]*)?>[\s\S]*?<\/p:sp>/g;
const TITLE_PLACEHOLDER_RE = /<p:ph\s[^>]*\btype="(?:title|ctrTitle)"/;
// Matches, in document order: a text run's content, or an inline break
// marker (sits as a sibling of <a:t>, not inside it, same as docx's <w:br/>).
const RUN_TOKEN_RE = /<a:t(?:\s[^>]*)?>([\s\S]*?)<\/a:t>|<a:br\s*\/>/g;

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
    } else {
      parts.push("\n");
    }
  }
  return parts.join("").trim();
}

/** All non-empty paragraph texts within a slide (or shape) XML fragment, in document order. */
function paragraphTexts(xmlFragment: string): string[] {
  const paragraphs = xmlFragment.match(PARAGRAPH_RE) ?? [];
  return paragraphs.map(paragraphText).filter((t) => t.length > 0);
}

/** The slide's title-placeholder text, if it has one and it's non-empty. */
function slideTitle(slideXml: string): string | null {
  const shapes = slideXml.match(SHAPE_RE) ?? [];
  for (const shape of shapes) {
    if (TITLE_PLACEHOLDER_RE.test(shape)) {
      const title = paragraphTexts(shape).join(" ").trim();
      if (title.length > 0) return title;
    }
  }
  return null;
}

/** Slide zip entries sorted by slide NUMBER (slide2 before slide10), not lexical path order. */
function sortedSlideEntries(
  entries: Record<string, Uint8Array>,
): Array<{ slideNumber: number; path: string }> {
  const slides: Array<{ slideNumber: number; path: string }> = [];
  for (const path of Object.keys(entries)) {
    const match = SLIDE_PATH_RE.exec(path);
    if (match) {
      slides.push({ slideNumber: Number.parseInt(match[1] ?? "0", 10), path });
    }
  }
  slides.sort((a, b) => a.slideNumber - b.slideNumber);
  return slides;
}

export async function parsePptx(filePath: string): Promise<RawParseResult> {
  let buffer: Buffer;
  try {
    buffer = await readFile(filePath);
  } catch (err) {
    throw new Error(
      `PPTX parse failed for "${filePath}": ${err instanceof Error ? err.message : String(err)}`,
      { cause: err },
    );
  }

  let entries: Record<string, Uint8Array>;
  try {
    entries = unzipSync(new Uint8Array(buffer));
  } catch (err) {
    throw new Error(
      `PPTX parse failed for "${filePath}": not a valid .pptx (zip) container (${err instanceof Error ? err.message : String(err)})`,
      { cause: err },
    );
  }

  const slideEntries = sortedSlideEntries(entries);
  if (slideEntries.length === 0) {
    throw new Error(
      `PPTX parse failed for "${filePath}": no ppt/slides/slideN.xml parts found (not a valid PowerPoint presentation)`,
    );
  }

  const pages: string[] = [];
  const headings: ParsedHeading[] = [];

  for (const { slideNumber, path } of slideEntries) {
    const slideXml = strFromU8(entries[path] as Uint8Array);
    const pageText = paragraphTexts(slideXml).join("\n").trim();

    const pageIndex = pages.length;
    pages.push(pageText);

    const title = slideTitle(slideXml) ?? `Slide ${slideNumber}`;
    headings.push({ title, level: 1, pageIndex });
  }

  return {
    text: pages.join("\n\n"),
    pages,
    metadata: {
      parser: "pptx-native-zip",
      slideCount: slideEntries.length,
      headingCount: headings.length,
      headings,
      totalPages: pages.length,
    },
  };
}
