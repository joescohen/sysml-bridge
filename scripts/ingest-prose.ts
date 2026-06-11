#!/usr/bin/env tsx
/**
 * ingest-prose.ts — CLI driver for the G-C prose ingestion pipeline.
 *
 * Processes the 4 ANGARS corpus PDFs, emits prose-candidates.json for
 * human review.
 *
 * Usage:
 *   pnpm tsx scripts/ingest-prose.ts [--out <path>] [--dry-run]
 *
 * Options:
 *   --out <path>   Output path for prose-candidates.json
 *                  Default: examples/angars/model/prose-candidates.json
 *   --dry-run      Parse and chunk only; skip LLM calls (KEY-REQUIRED guard)
 *   --help         Show this message
 *
 * C4: Proposals with unresolvable chunkIds are DROPPED (logged to stderr).
 * C5: Every chunk is submitted to the LLM exactly once; no retrieval/embedding.
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { parsePdf } from "../packages/prose-ingest/src/parsers/pdf.js";
import { runIngestPipeline } from "../packages/prose-ingest/src/ingest-pipeline.js";
import type { ProseCandidateRecord } from "../packages/prose-ingest/src/ingest-pipeline.js";
import { AnthropicLlmProvider } from "../packages/prose-ingest/src/llm-provider.js";
import type { LlmProvider } from "../packages/prose-ingest/src/llm-provider.js";

// ── Constants ─────────────────────────────────────────────────────────────────

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = join(__dirname, "..");

const CORPUS_DIR = join(REPO_ROOT, "examples/angars/corpus/specs");
const DEFAULT_OUT = join(REPO_ROOT, "examples/angars/model/prose-candidates.json");

/**
 * Zero-dependency .env loader. Reads repo-root `.env` (gitignored) and sets any
 * KEY=VALUE into process.env WITHOUT overriding values already exported in the
 * shell. Lets ANTHROPIC_API_KEY live in .env so the pipeline "just works"
 * without an export dance — no dotenv dependency.
 */
function loadDotEnv(): void {
  const envPath = join(REPO_ROOT, ".env");
  if (!existsSync(envPath)) return;
  for (const raw of readFileSync(envPath, "utf8").split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq === -1) continue;
    const key = line.slice(0, eq).trim();
    if (!key || process.env[key] !== undefined) continue; // shell export wins
    let value = line.slice(eq + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    process.env[key] = value;
  }
}

const DOCS: Array<{ file: string; docId: string }> = [
  { file: "Appendix_B_ANGARS_RAR_CONOPS.pdf", docId: "angars-conops" },
  { file: "Appendix_C_ANGARS_FAR.pdf", docId: "angars-far" },
  { file: "Appendix_E_ANGARS_ConceptDesign.pdf", docId: "angars-concept-design" },
  { file: "Appendix_G_ANGARS_ASPEC.pdf", docId: "angars-aspec" },
];

// ── No-op provider for dry-run ────────────────────────────────────────────────

class NoOpProvider implements LlmProvider {
  async propose(): Promise<[]> {
    return [];
  }
}

// ── CLI arg parsing ───────────────────────────────────────────────────────────

function parseArgs(argv: string[]): { outPath: string; dryRun: boolean } {
  let outPath = DEFAULT_OUT;
  let dryRun = false;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!;
    if (arg === "--out" && argv[i + 1]) {
      outPath = argv[++i]!;
    } else if (arg === "--dry-run") {
      dryRun = true;
    } else if (arg === "--help") {
      console.log(`Usage: pnpm tsx scripts/ingest-prose.ts [--out <path>] [--dry-run]`);
      process.exit(0);
    }
  }

  return { outPath, dryRun };
}

// ── Main ──────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  loadDotEnv(); // load ANTHROPIC_API_KEY from .env before the key check below
  const { outPath, dryRun } = parseArgs(process.argv.slice(2));

  // Verify corpus is present
  for (const { file } of DOCS) {
    if (!existsSync(join(CORPUS_DIR, file))) {
      console.error(`ERROR: corpus file not found: ${join(CORPUS_DIR, file)}`);
      console.error(`Corpus must be present at: ${CORPUS_DIR}`);
      process.exit(1);
    }
  }

  // Resolve LLM provider
  const hasKey = Boolean(process.env["ANTHROPIC_API_KEY"]);
  const ingestModel = process.env["PROSE_INGEST_MODEL"] ?? "claude-haiku-4-5-20251001";
  let provider: LlmProvider;

  if (dryRun) {
    console.log("[KEY-REQUIRED] --dry-run: LLM pass skipped (no proposals generated)");
    provider = new NoOpProvider();
  } else if (!hasKey) {
    console.log(
      "[KEY-REQUIRED] ANTHROPIC_API_KEY not set — running deterministic-only (no LLM proposals)",
    );
    provider = new NoOpProvider();
  } else {
    console.log(`[LLM] ANTHROPIC_API_KEY present — using AnthropicLlmProvider (${ingestModel})`);
    provider = new AnthropicLlmProvider();
  }

  const allCandidates: ProseCandidateRecord[] = [];
  let totalDropped = 0;
  let totalChunks = 0;
  let totalProcessed = 0;

  for (const { file, docId } of DOCS) {
    const filePath = join(CORPUS_DIR, file);
    console.log(`\n[INGEST] ${file} (${docId})`);

    const rawBytes = await readFile(filePath);
    const docSha256 = createHash("sha256").update(rawBytes).digest("hex");
    console.log(`  docSha256: ${docSha256.slice(0, 16)}…`);

    const parsed = await parsePdf(filePath);
    console.log(`  pages: ${parsed.pages.length}, chars: ${parsed.text.length}`);

    const result = await runIngestPipeline({
      text: parsed.text,
      context: {
        documentHash: docSha256,
        sectionId: "sec-root",
        sectionPath: "root",
        pageStart: 0,
        pageEnd: parsed.pages.length - 1,
        documentId: docId,
      },
      provider,
      chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
    });

    console.log(`  chunks: ${result.totalChunks}, processed: ${result.processedChunks}`);
    console.log(`  candidates emitted: ${result.candidates.length}`);
    console.log(`  dropped (uncited): ${result.droppedUncited}`);
    console.log(`  emittedUncited (must=0): ${result.emittedUncited}`);

    // C4 assertion — fail fast if gate is broken
    if (result.emittedUncited !== 0) {
      console.error(`FATAL: C4 gate violated — emittedUncited=${result.emittedUncited} for ${file}`);
      process.exit(2);
    }
    // C5 assertion
    if (result.processedChunks !== result.totalChunks) {
      console.error(
        `FATAL: C5 gate violated — processedChunks(${result.processedChunks}) !== totalChunks(${result.totalChunks}) for ${file}`,
      );
      process.exit(2);
    }

    allCandidates.push(...result.candidates);
    totalDropped += result.droppedUncited;
    totalChunks += result.totalChunks;
    totalProcessed += result.processedChunks;
  }

  // Summary
  console.log(`\n[SUMMARY]`);
  console.log(`  Total candidates: ${allCandidates.length}`);
  console.log(`  Total chunks processed: ${totalProcessed} / ${totalChunks}`);
  console.log(`  Total dropped (uncited): ${totalDropped}`);

  // Write output
  await mkdir(dirname(outPath), { recursive: true });
  const output = {
    generatedAt: new Date().toISOString(),
    totalCandidates: allCandidates.length,
    totalChunks,
    totalDropped,
    llmProviderUsed: hasKey && !dryRun ? `anthropic/${ingestModel}` : "none",
    candidates: allCandidates,
  };
  await writeFile(outPath, JSON.stringify(output, null, 2), "utf8");
  console.log(`\n[OUT] ${outPath}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
