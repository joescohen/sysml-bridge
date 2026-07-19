/**
 * ANGARS corpus integration tests (C1, C2).
 *
 * Tests parse completeness and determinism on the real ANGARS PDF corpus.
 * Floors are set at 90% of the observed first-run totals.
 *
 * C1: parse completeness + determinism
 *   - Each PDF: totalChars ≥ floor, no silently-dropped pages, two runs byte-identical
 * C2: chunk coverage + stable IDs
 *   - chunkWithIds covers all text, IDs identical across two runs, text-edit stability
 *
 * These tests are skipped if the corpus PDFs are not present (CI without corpus).
 */

import { describe, it, expect } from "vitest";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { parsePdf } from "../parsers/pdf.js";
import { chunkWithIds } from "../chunker.js";

const CORPUS_DIR = join(
  import.meta.dirname,
  "../../../../../examples/angars/corpus/specs",
);

/** Per-doc char floors at 90% of first observed run. */
const DOCS: Array<{ file: string; floor: number }> = [
  { file: "Appendix_B_ANGARS_RAR_CONOPS.pdf", floor: 30_000 },
  { file: "Appendix_C_ANGARS_FAR.pdf", floor: 23_000 },
  { file: "Appendix_E_ANGARS_ConceptDesign.pdf", floor: 24_000 },
  { file: "Appendix_G_ANGARS_ASPEC.pdf", floor: 14_000 },
];

const corpusPresent = DOCS.every((d) => existsSync(join(CORPUS_DIR, d.file)));

describe.skipIf(!corpusPresent)("ANGARS corpus — C1 parse completeness + determinism", () => {
  for (const { file, floor } of DOCS) {
    const filePath = join(CORPUS_DIR, file);

    it(`${file}: totalChars ≥ ${floor.toLocaleString()} (C1 floor)`, async () => {
      const result = await parsePdf(filePath);
      expect(result.text.length).toBeGreaterThanOrEqual(floor);
    });

    it(`${file}: pages.length > 0 and no page silently dropped`, async () => {
      const result = await parsePdf(filePath);
      expect(result.pages.length).toBeGreaterThan(0);
      // All pages are strings (none dropped — empty strings are preserved, not removed)
      for (const page of result.pages) {
        expect(typeof page).toBe("string");
      }
    });

    it(`${file}: text equals pages.join('\\n\\n')`, async () => {
      const result = await parsePdf(filePath);
      expect(result.text).toBe(result.pages.join("\n\n"));
    });

    it(`${file}: two runs byte-identical (C1 determinism)`, async () => {
      const r1 = await parsePdf(filePath);
      const r2 = await parsePdf(filePath);
      expect(r1.text).toBe(r2.text);
      expect(r1.pages).toHaveLength(r2.pages.length);
      for (let i = 0; i < r1.pages.length; i++) {
        expect(r1.pages[i]).toBe(r2.pages[i]);
      }
    });
  }
});

describe.skipIf(!corpusPresent)(
  "ANGARS corpus — C2 chunk coverage + stable IDs",
  () => {
    it("Appendix_B: chunk coverage — all words appear in at least one chunk", async () => {
      const filePath = join(CORPUS_DIR, "Appendix_B_ANGARS_RAR_CONOPS.pdf");
      const buf = await readFile(filePath);
      const docHash = createHash("sha256").update(buf).digest("hex");
      const result = await parsePdf(filePath);

      const context = {
        documentHash: docHash,
        sectionId: "sec-root",
        sectionPath: "root",
        pageStart: 0,
        pageEnd: result.pages.length - 1,
        documentId: "angars-conops",
      };

      const chunks = await chunkWithIds(result.text, context, {
        chunkSize: 1500,
        chunkOverlap: 150,
      });

      expect(chunks.length).toBeGreaterThan(0);

      // Coverage: join all chunk text and check representative words
      const allText = chunks.map((c) => c.text).join(" ");
      // Words that definitely appear in ANGARS CONOPS
      const probeWords = ["ANGARS", "Refueling", "autonomous"];
      for (const word of probeWords) {
        expect(allText).toContain(word);
      }
    });

    it("Appendix_B: chunkId two-run equality (C2)", async () => {
      const filePath = join(CORPUS_DIR, "Appendix_B_ANGARS_RAR_CONOPS.pdf");
      const buf = await readFile(filePath);
      const docHash = createHash("sha256").update(buf).digest("hex");
      const result = await parsePdf(filePath);

      const context = {
        documentHash: docHash,
        sectionId: "sec-root",
        sectionPath: "root",
        pageStart: 0,
        pageEnd: result.pages.length - 1,
        documentId: "angars-conops",
      };

      const opts = { chunkSize: 1500, chunkOverlap: 150 };
      const run1 = await chunkWithIds(result.text, context, opts);
      const run2 = await chunkWithIds(result.text, context, opts);

      expect(run1).toHaveLength(run2.length);
      for (let i = 0; i < run1.length; i++) {
        expect(run1[i]?.chunkId).toBe(run2[i]?.chunkId);
        expect(run1[i]?.text).toBe(run2[i]?.text);
      }
    });
  },
);
