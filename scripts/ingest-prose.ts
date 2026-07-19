/**
 * ingest-prose.ts
 *
 * I/O entry point for the prose-ingest layer (packages/candidates/src/prose/
 * is a PURE pipeline — see ingest-pipeline.ts's header comment, which names
 * this file as where the I/O lives).
 *
 * Parses one or more corpus files — PDF, DOCX, XLSX, CSV, MD, or TXT — via
 * the format dispatcher (parseDocument), so this script (and any other
 * caller) never hand-picks a parser by extension itself. For each file it
 * prints: format, extracted text length, page/segment count, document
 * SHA-256 (the hash chunk IDs are content-addressed against), and the
 * section map derived from the parser's heading output.
 *
 * This is a summary/inspection tool, not the full candidate-generation
 * pipeline — running the LLM pass (ingest-pipeline.ts#runIngestPipeline)
 * needs a provider (ANTHROPIC_API_KEY) and a chunk-store wiring that's
 * exercised end-to-end in packages/candidates/src/prose/__tests__/
 * gc-real-run.test.ts. This script proves the dispatch/parse/section-map
 * leg of that pipeline against real files on disk.
 *
 * CHUNK-STORE PERSISTENCE (`--out <dir>`): when an output dir is given, each
 * file is deterministically chunked (chunkWithIds — no provider/LLM needed;
 * chunk text and ids are provider-independent) and the union of chunks across
 * all files is written to `<dir>/chunks.json` as { chunkId, sectionPath, text }
 * records. That file is the SINGLE persisted chunk store: it feeds the
 * PROSE-unverbatim-quote audit (as a chunkId→text map — run at ERROR level
 * instead of the degrade-warning path) AND runInferenceEngine's `chunkStore`
 * option (BM25 evidence). See packages/candidates/src/chunk-store/.
 *
 * Usage:
 *   tsx scripts/ingest-prose.ts <file> [<file> ...] [--out <dir>]
 *   pnpm ingest:prose -- <file> [<file> ...] [--out <dir>]
 */

import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

import {
  parseDocument,
  detectFormat,
  extractSectionMapFromPages,
  extractSectionMap,
  chunkWithIds,
  type SectionMap,
} from "../packages/candidates/src/prose/index.js";
import {
  writeChunkStoreFile,
  type ChunkStoreRecord,
} from "../packages/candidates/src/chunk-store/index.js";

/** Section path used for the persisted chunk store, mirroring how the ingest
 *  pipeline is driven per document (one ChunkContext, root section) in
 *  gc-real-run.test.ts. Chunk ids are content-addressed against this path. */
const ROOT_SECTION_PATH = "root";

interface FileSummary {
  file: string;
  format: string;
  parser: unknown;
  textLength: number;
  pageCount: number;
  docSha256: string;
  sectionCount: number;
  topLevelSections: string[];
}

function countSections(map: SectionMap): number {
  let count = 0;
  const walk = (nodes: SectionMap["sections"]) => {
    for (const node of nodes) {
      count++;
      walk(node.children);
    }
  };
  walk(map.sections);
  return count;
}

/** Parse + summarize one file, and deterministically chunk it for persistence. */
async function summarizeFile(
  filePath: string,
): Promise<{ summary: FileSummary; chunks: ChunkStoreRecord[] }> {
  const format = detectFormat(filePath); // fail fast, before any I/O, on an unsupported extension
  const raw = await readFile(filePath);
  const docSha256 = createHash("sha256").update(raw).digest("hex");
  const parsed = await parseDocument(filePath);

  const documentId = `${format}:${docSha256.slice(0, 12)}`;
  const sectionMap =
    format === "pdf"
      ? extractSectionMapFromPages(parsed.pages, documentId)
      : extractSectionMap(
          (parsed.metadata["headings"] as Array<{ title: string; level: number; pageIndex: number }>) ??
            [],
          documentId,
          format,
          parsed.pages.length,
        );

  // Deterministic chunking (no provider/LLM). One ChunkContext per document,
  // root section — the same shape runIngestPipeline is driven with, so ids match.
  const chunks = await chunkWithIds(parsed.text, {
    documentHash: docSha256,
    sectionId: "sec-root",
    sectionPath: ROOT_SECTION_PATH,
    pageStart: 0,
    pageEnd: Math.max(0, parsed.pages.length - 1),
    documentId,
  });
  const records: ChunkStoreRecord[] = chunks.map((c) => ({
    chunkId: c.chunkId,
    sectionPath: ROOT_SECTION_PATH,
    text: c.text,
  }));

  return {
    summary: {
      file: filePath,
      format,
      parser: parsed.metadata["parser"],
      textLength: parsed.text.length,
      pageCount: parsed.pages.length,
      docSha256,
      sectionCount: countSections(sectionMap),
      topLevelSections: sectionMap.sections.map((s) => s.title),
    },
    chunks: records,
  };
}

/** Extract a `--out <dir>` option, returning the dir (or undefined) + remaining args. */
function parseArgs(argv: string[]): { outDir?: string; files: string[] } {
  const files: string[] = [];
  let outDir: string | undefined;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--out") {
      outDir = argv[++i];
      if (outDir === undefined) throw new Error("--out requires a directory argument");
    } else if (argv[i]!.startsWith("--out=")) {
      outDir = argv[i]!.slice("--out=".length);
    } else {
      files.push(argv[i]!);
    }
  }
  return { outDir, files };
}

async function main(): Promise<void> {
  const { outDir, files } = parseArgs(process.argv.slice(2));
  if (files.length === 0) {
    console.error("Usage: tsx scripts/ingest-prose.ts <file> [<file> ...] [--out <dir>]");
    process.exitCode = 1;
    return;
  }

  let hadError = false;
  const allChunks: ChunkStoreRecord[] = [];
  for (const file of files) {
    try {
      const { summary, chunks } = await summarizeFile(file);
      console.log(JSON.stringify(summary, null, 2));
      allChunks.push(...chunks);
    } catch (err) {
      hadError = true;
      console.error(`FAILED: ${file}: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  // Persist the chunk store (chunkId → sectionPath + text) for the audit + BM25.
  if (outDir !== undefined) {
    const outPath = join(outDir, "chunks.json");
    await writeChunkStoreFile(outPath, allChunks);
    console.log(`\nWrote ${allChunks.length} chunk(s) to ${outPath}`);
  }

  if (hadError) process.exitCode = 1;
}

main();
