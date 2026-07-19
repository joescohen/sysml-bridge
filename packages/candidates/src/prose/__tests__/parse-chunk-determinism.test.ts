/**
 * parse -> chunk determinism for the new prose-ingest formats (DOCX, XLSX,
 * CSV, MD, TXT).
 *
 * Requirement: the chunker's chunk-ID scheme (chunker.ts) is untouched by
 * this work — same file in, same chunk IDs out. This test proves that
 * end-to-end through parseDocument (format dispatch) -> chunkWithIds, for
 * every new format, across two independent parse+chunk runs.
 */

import { describe, it, expect } from "vitest";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseDocument } from "../parsers/dispatch.js";
import { chunkWithIds } from "../chunker.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "../../../fixtures");

const CASES = [
  { file: "sample.md", documentId: "doc-md" },
  { file: "sample.docx", documentId: "doc-docx" },
  { file: "sample.xlsx", documentId: "doc-xlsx" },
  { file: "sample.csv", documentId: "doc-csv" },
  { file: "sample.txt", documentId: "doc-txt" },
];

describe("parse -> chunk determinism (new formats)", () => {
  for (const { file, documentId } of CASES) {
    it(`${file}: same file in -> same chunk IDs out, across two independent parse+chunk runs`, async () => {
      const filePath = join(fixturesDir, file);
      const raw = await readFile(filePath);
      const docSha256 = createHash("sha256").update(raw).digest("hex");

      async function runOnce() {
        const parsed = await parseDocument(filePath);
        const context = {
          documentHash: docSha256,
          sectionId: "sec-root",
          sectionPath: "root",
          pageStart: 0,
          pageEnd: Math.max(parsed.pages.length - 1, 0),
          documentId,
        };
        return chunkWithIds(parsed.text, context, { chunkSize: 400, chunkOverlap: 40 });
      }

      const run1 = await runOnce();
      const run2 = await runOnce();

      expect(run1.length).toBeGreaterThan(0);
      expect(run1).toHaveLength(run2.length);
      for (let i = 0; i < run1.length; i++) {
        expect(run1[i]?.chunkId).toBe(run2[i]?.chunkId);
        expect(run1[i]?.text).toBe(run2[i]?.text);
      }
      // Chunk IDs are content-addressed by (documentHash, sectionPath,
      // chunkIndex) — NOT chunk text — per chunker.ts's documented scheme.
      // Re-derive one ID independently to prove the scheme wasn't touched.
      const expectedFirstId = createHash("sha256")
        .update(JSON.stringify({ doc: docSha256, sec: "root", idx: 0 }))
        .digest("hex")
        .slice(0, 32);
      expect(run1[0]?.chunkId).toBe(expectedFirstId);
    });
  }
});
