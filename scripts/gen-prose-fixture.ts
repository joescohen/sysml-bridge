/**
 * gen-prose-fixture.ts — Generate prose-approved-fixture.json for G-D/G-E tests.
 *
 * Runs the parser + requirement-chunker over Appendix_G_ANGARS_ASPEC.pdf
 * (the shortest ANGARS doc, ~14k chars) to derive REAL chunk IDs, then builds
 * a small approved-set fixture with 3 requirement-kind entries whose citation
 * chunkIds are genuine content-addressed hashes from that PDF.
 *
 * Output: packages/ir/src/__tests__/fixtures/prose-approved-fixture.json
 * (synthetic field text — no corpus quote text committed; only the chunkId hashes)
 *
 * Usage: pnpm tsx scripts/gen-prose-fixture.ts
 */

import { readFile, mkdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { parsePdf } from "../packages/prose-ingest/src/parsers/pdf.js";
import { detectAndChunkRequirements } from "../packages/prose-ingest/src/requirement-chunker.js";
import { stableId } from "../packages/ir/src/stable-id.js";
import type { ProseApprovedEntry } from "../packages/ir/src/prose-approved.js";

const REPO_ROOT = resolve(__dirname, "..");
const PDF_PATH = join(REPO_ROOT, "examples/angars/corpus/specs/Appendix_G_ANGARS_ASPEC.pdf");
const OUT_DIR = join(REPO_ROOT, "packages/ir/src/__tests__/fixtures");
const OUT_PATH = join(OUT_DIR, "prose-approved-fixture.json");

const DOC_ID = "angars-aspec-appendix-g";

async function main() {
  console.log(`Parsing: ${PDF_PATH}`);
  const buf = await readFile(PDF_PATH);
  const docHash = createHash("sha256").update(buf).digest("hex");

  console.log(`docHash: ${docHash}`);

  const parsed = await parsePdf(PDF_PATH);
  console.log(`Pages: ${parsed.pages.length}, chars: ${parsed.text.length}`);

  // Run requirement chunker over the full text as one section
  const sectionPath = "ANGARS ASPEC";
  const context = {
    documentHash: docHash,
    sectionId: "sec-aspec-root",
    sectionPath,
    pageStart: 0,
    pageEnd: parsed.pages.length - 1,
    documentId: DOC_ID,
  };

  const reqChunks = await detectAndChunkRequirements(
    parsed.text,
    sectionPath,
    context
  );

  console.log(`Requirement chunks detected: ${reqChunks.length}`);

  if (reqChunks.length < 3) {
    // If fewer than 3 req chunks, fall back to plain chunks
    const { chunkWithIds } = await import("../packages/prose-ingest/src/chunker.js");
    const plainChunks = await chunkWithIds(parsed.text, context, {
      chunkSize: 1500,
      chunkOverlap: 150,
    });
    console.log(`Fallback to plain chunks: ${plainChunks.length}`);

    // Use first 3 chunks as fixture entries
    const entries: ProseApprovedEntry[] = plainChunks.slice(0, 3).map((chunk, i) => {
      const quote = chunk.text.slice(0, 120).replace(/\n/g, " ").trim();
      const naturalKey = `${DOC_ID}:${chunk.chunkId}:${quote}`;
      const entryId = stableId("prose", naturalKey);
      return {
        id: entryId,
        kind: "requirement" as const,
        fields: {
          naturalKey: `ASPEC-PLAIN-${i + 1}`,
          name: `Plain Chunk ${i + 1}`,
          statement: `[Synthetic statement for fixture entry ${i + 1} — real chunk from ANGARS ASPEC]`,
        },
        citation: {
          docId: DOC_ID,
          docSha256: docHash,
          chunkId: chunk.chunkId,
          sectionPath,
          quote: quote.slice(0, 120),
        },
        approvedBy: "fixture-generator",
        approvedAt: "2026-06-10T00:00:00.000Z",
        candidateId: `candidate-aspec-plain-${i + 1}`,
        status: "approved" as const,
      };
    });

    await mkdir(OUT_DIR, { recursive: true });
    await writeFile(OUT_PATH, JSON.stringify({ entries }, null, 2) + "\n", "utf8");
    console.log(`\nWrote fixture (plain chunks) → ${OUT_PATH}`);
    console.log("Real chunkIds cited:");
    for (const e of entries) {
      console.log(`  ${e.citation.chunkId}  (candidateId: ${e.candidateId})`);
    }
    return;
  }

  // Use first 3 requirement chunks
  const entries: ProseApprovedEntry[] = reqChunks.slice(0, 3).map((chunk, i) => {
    // Use only the first 120 chars of the chunk text as the quote (synthetic — no full corpus text)
    const rawQuote = chunk.text.replace(/^\[Section:[^\]]*\]\s*/, "").slice(0, 120).replace(/\n/g, " ").trim();
    const quote = rawQuote.length > 0 ? rawQuote : `[chunk ${chunk.chunkId}]`;
    const naturalKey = `${DOC_ID}:${chunk.chunkId}:${quote}`;
    const entryId = stableId("prose", naturalKey);
    return {
      id: entryId,
      kind: "requirement" as const,
      fields: {
        naturalKey: `ASPEC-REQ-${i + 1}`,
        name: `ASPEC Requirement ${i + 1}`,
        statement: `[Synthetic statement for fixture entry ${i + 1} — real requirement chunk from ANGARS ASPEC]`,
      },
      citation: {
        docId: DOC_ID,
        docSha256: docHash,
        chunkId: chunk.chunkId,
        sectionPath,
        quote: quote.slice(0, 120),
      },
      approvedBy: "fixture-generator",
      approvedAt: "2026-06-10T00:00:00.000Z",
      candidateId: `candidate-aspec-req-${i + 1}`,
      status: "approved" as const,
    };
  });

  await mkdir(OUT_DIR, { recursive: true });
  await writeFile(OUT_PATH, JSON.stringify({ entries }, null, 2) + "\n", "utf8");
  console.log(`\nWrote fixture → ${OUT_PATH}`);
  console.log("Real chunkIds cited:");
  for (const e of entries) {
    console.log(`  ${e.citation.chunkId}  (candidateId: ${e.candidateId})`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
