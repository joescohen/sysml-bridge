#!/usr/bin/env tsx
/**
 * gen-weave-mini-corpus.ts — (re)generates the binary corpus files for
 * examples/weave-mini/ (W4, proof-of-recall eval).
 *
 * Run manually with `tsx fixtures/gen-weave-mini-corpus.ts` from
 * packages/candidates/ (needs the package's own `xlsx` and `fflate`
 * dependencies — same technique as gen-fixtures.ts). Not part of `pnpm
 * build` / `pnpm test` — the generated .docx/.xlsx are committed binaries,
 * regenerated only when the weave-mini corpus content needs to change.
 *
 * `system-overview.md` (the third corpus document) is plain text and is
 * committed directly, not produced by this script.
 */

import { writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { zipSync, strToU8 } from "fflate";
import * as XLSX from "xlsx";

const here = dirname(fileURLToPath(import.meta.url));
const outDir = join(here, "../../../examples/weave-mini/corpus");

// ── subsystem-spec.docx ──────────────────────────────────────────────────────

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

// Paragraph text is authored to be byte-identical, sentence-for-sentence,
// with the quotes in fixture-responses.ts — the mention-derivation citation
// gate (C4/C6) verbatim-checks every quote against this exact text.
const DOCUMENT = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Cargo Handling Controller Subsystem</w:t></w:r></w:p>
    <w:p><w:r><w:t>The CHC executes the top-level cargo handling control loop and mediates all safety interlocks.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Components</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Interlock isolates power to the conveyor drive within 500 milliseconds of a detected obstruction.</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Boom Actuator raises and lowers the loading boom during pallet transfer.</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Fault Logger records fault events raised by any cargo handling component.</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Position Sensor Array reports pallet position to the CHC over the internal data bus.</w:t></w:r></w:p>
    <w:p><w:r><w:t>The system may enter the Standby mode when no cargo handling activity has been commanded for 10 minutes.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Functions</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Validate Load Capacity function confirms that the measured pallet weight is within the rated conveyor capacity.</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Log Fault Event function appends a fault record to the fault logger whenever a component reports an error.</w:t></w:r></w:p>
    <w:p><w:r><w:t>The Transmit Fault Summary function assembles the fault log and Diagnostic Interface status into a per-flight report.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Requirements</w:t></w:r></w:p>
    <w:p><w:r><w:t>REQ-004: The Interlock shall isolate power to the conveyor drive within 500 milliseconds of a detected obstruction.</w:t></w:r></w:p>
    <w:p><w:r><w:t>REQ-005: The Boom Actuator shall complete a full raise cycle within 3 seconds.</w:t></w:r></w:p>
    <w:p><w:r><w:t>REQ-006: The Fault Logger shall retain the most recent 500 fault records.</w:t></w:r></w:p>
    <w:p><w:r><w:t>REQ-007: The CHC shall command the Boom Actuator only while the system is outside the Interlock mode.</w:t></w:r></w:p>
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
writeFileSync(join(outDir, "subsystem-spec.docx"), docxBytes);
console.log(`wrote subsystem-spec.docx (${docxBytes.length} bytes)`);

// ── requirements-matrix.xlsx (2 sheets) ──────────────────────────────────────

const wb = XLSX.utils.book_new();

const reqSheet = XLSX.utils.aoa_to_sheet([
  ["ID", "Statement", "Priority"],
  ["REQ-008", "The Diagnostic Interface shall report component health status every 5 seconds.", "Medium"],
  ["REQ-009", "The Cargo Handling Controller shall log all mode transitions with a UTC timestamp.", "High"],
  [
    "REQ-010",
    "The Fault Logger shall transmit a summary report to the Cargo Handling Controller once per flight leg.",
    "Medium",
  ],
]);
XLSX.utils.book_append_sheet(wb, reqSheet, "Requirements");

const needsSheet = XLSX.utils.aoa_to_sheet([
  ["ID", "Name", "Description"],
  ["N-001", "Autonomous Cargo Handling", "Provide autonomous cargo loading and unloading without manual intervention."],
]);
XLSX.utils.book_append_sheet(wb, needsSheet, "Needs");

const xlsxBuffer = XLSX.write(wb, { type: "buffer", bookType: "xlsx" }) as Buffer;
writeFileSync(join(outDir, "requirements-matrix.xlsx"), xlsxBuffer);
console.log(`wrote requirements-matrix.xlsx (${xlsxBuffer.length} bytes, 2 sheets: Requirements, Needs)`);
