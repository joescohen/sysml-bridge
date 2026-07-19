#!/usr/bin/env tsx
/**
 * gen-fixtures.ts — (re)generates the binary parser fixtures.
 *
 * Run manually with `tsx fixtures/gen-fixtures.ts` from packages/candidates/
 * (needs the package's own `xlsx` and `fflate` dependencies, so run
 * `pnpm install` first). Not part of `pnpm build` / `pnpm test` — the
 * generated files are committed binaries, regenerated only when the fixture
 * content needs to change.
 *
 * Text fixtures (sample.md, sample.txt, sample-paginated.txt, sample.csv)
 * are committed directly as plain text and are NOT produced by this script.
 * malformed.xlsx/.docx/.pptx need real "looks like a zip but isn't" bytes to
 * exercise the corrupt-container path, so they're generated here too.
 */

import { writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { zipSync, strToU8 } from "fflate";
import * as XLSX from "xlsx";

const here = dirname(fileURLToPath(import.meta.url));

// ── sample.docx ──────────────────────────────────────────────────────────────

const CONTENT_TYPES = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>`;

const RELS = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>`;

const DOCUMENT = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Sample fixture for the DOCX parser test suite.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Introduction</w:t></w:r></w:p>
    <w:p><w:r><w:t>The system shall provide autonomous aerial refueling operations under all specified environmental conditions.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Background</w:t></w:r></w:p>
    <w:p><w:r><w:t>Prior systems required manual intervention at each step of the refueling sequence.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Requirements</w:t></w:r></w:p>
    <w:p><w:r><w:t>REQ-001: The system shall detect coupling faults within 2 seconds.</w:t></w:r></w:p>
  </w:body>
</w:document>`;

const docxBytes = zipSync(
  {
    "[Content_Types].xml": strToU8(CONTENT_TYPES),
    "_rels/.rels": strToU8(RELS),
    "word/document.xml": strToU8(DOCUMENT),
  },
  { level: 6 },
);
writeFileSync(join(here, "sample.docx"), docxBytes);
console.log(`wrote sample.docx (${docxBytes.length} bytes)`);

// ── sample.pptx (10 slides — exercises numeric slide ordering) ─────────────
// slideN.xml zip-entry names sort LEXICALLY as "slide1", "slide10", "slide2",
// … — the parser must sort by numeric slide number instead, so this fixture
// deliberately goes past 9 slides to make a lexical-sort bug fail loudly:
// if pptx.ts used string/zip-entry order, "Slide 10" content would land
// right after "Slide 1" instead of last.

const PPTX_CONTENT_TYPES = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
</Types>`;

const PPTX_RELS = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>`;

const PPTX_PRESENTATION = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>`;

/** A slide with a title placeholder + one body text run. */
function slideXml(title: string, body: string): string {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:nvSpPr>
          <p:nvPr><p:ph type="title"/></p:nvPr>
        </p:nvSpPr>
        <p:txBody>
          <a:p><a:r><a:t>${title}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
      <p:sp>
        <p:nvSpPr>
          <p:nvPr><p:ph idx="1"/></p:nvPr>
        </p:nvSpPr>
        <p:txBody>
          <a:p><a:r><a:t>${body}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>`;
}

const SLIDE_CONTENT: Record<number, [title: string, body: string]> = {
  1: [
    "Overview",
    "The system shall provide autonomous aerial refueling operations under all specified environmental conditions.",
  ],
  2: ["Requirements", "REQ-001: The system shall detect coupling faults within 2 seconds."],
  3: ["Background", "Prior systems required manual intervention at each step of the refueling sequence."],
  4: ["Slide Four", "Filler content for slide four."],
  5: ["Slide Five", "Filler content for slide five."],
  6: ["Slide Six", "Filler content for slide six."],
  7: ["Slide Seven", "Filler content for slide seven."],
  8: ["Slide Eight", "Filler content for slide eight."],
  9: ["Slide Nine", "Filler content for slide nine."],
  10: ["Closing", "Tenth slide content — must sort after slide 2, not lexically before it."],
};

const pptxEntries: Record<string, Uint8Array> = {
  "[Content_Types].xml": strToU8(PPTX_CONTENT_TYPES),
  "_rels/.rels": strToU8(PPTX_RELS),
  "ppt/presentation.xml": strToU8(PPTX_PRESENTATION),
};
for (const [n, [title, body]] of Object.entries(SLIDE_CONTENT)) {
  pptxEntries[`ppt/slides/slide${n}.xml`] = strToU8(slideXml(title, body));
}

const pptxBytes = zipSync(pptxEntries, { level: 6 });
writeFileSync(join(here, "sample.pptx"), pptxBytes);
console.log(`wrote sample.pptx (${pptxBytes.length} bytes, 10 slides)`);

// ── sample.xlsx (2 sheets) ──────────────────────────────────────────────────

const wb = XLSX.utils.book_new();

const reqSheet = XLSX.utils.aoa_to_sheet([
  ["ID", "Statement", "Priority"],
  ["REQ-001", "The system shall detect coupling faults within 2 seconds.", "High"],
  ["REQ-002", "The system shall log all fault events with a UTC timestamp.", "Medium"],
]);
XLSX.utils.book_append_sheet(wb, reqSheet, "Requirements");

const needsSheet = XLSX.utils.aoa_to_sheet([
  ["ID", "Name", "Description"],
  ["N-001", "Autonomous Refueling", "Provide autonomous aerial refueling capability."],
]);
XLSX.utils.book_append_sheet(wb, needsSheet, "Needs");

const xlsxBuffer = XLSX.write(wb, { type: "buffer", bookType: "xlsx" }) as Buffer;
writeFileSync(join(here, "sample.xlsx"), xlsxBuffer);
console.log(`wrote sample.xlsx (${xlsxBuffer.length} bytes, 2 sheets: Requirements, Needs)`);

// ── malformed.xlsx / malformed.docx ─────────────────────────────────────────
// Real ZIP signature (PK\x03\x04) followed by garbage — enough to make both
// `xlsx`'s workbook reader and fflate's unzipSync attempt real zip parsing
// and fail on it, rather than falling back to a "treat as CSV/text" path.

const corruptZipBytes = Buffer.concat([
  Buffer.from([0x50, 0x4b, 0x03, 0x04]), // "PK\x03\x04" local file header signature
  Buffer.from(
    "this local file header is truncated and the lengths below are garbage, not a real docx or xlsx".repeat(
      4,
    ),
    "latin1",
  ),
]);
writeFileSync(join(here, "malformed.xlsx"), corruptZipBytes);
writeFileSync(join(here, "malformed.docx"), corruptZipBytes);
writeFileSync(join(here, "malformed.pptx"), corruptZipBytes);
console.log("wrote malformed.xlsx, malformed.docx, malformed.pptx");
