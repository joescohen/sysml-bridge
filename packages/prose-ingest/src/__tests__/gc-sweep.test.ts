/**
 * G-C / C5 — sweep completeness + no-vector proof.
 *
 * Claims closed:
 *   C5 (neg): every parsed chunk is submitted to the LLM pass EXACTLY ONCE
 *             (processedChunks === totalChunks). The ingest path performs NO
 *             retrieval / top-k / embedding — grep proves it.
 *
 * RED: fails until ingest-pipeline.ts and llm-provider.ts exist.
 */

import { describe, it, expect } from "vitest";
import { execSync } from "node:child_process";
import { join } from "node:path";
import { chunkWithIds } from "../chunker.js";
import type { ChunkContext } from "../chunker.js";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { IngestPipelineResult } from "../ingest-pipeline.js";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const FAKE_DOC_HASH = "a".repeat(64);
const FAKE_DOC_ID = "gc-sweep-fixture";

/** Synthetic multi-chunk text (>3 × 1500 chars) to exercise the sweep counter. */
function makeLongText(chunks: number): string {
  // Each segment is ~1600 chars so we get `chunks` non-trivial chunks
  return Array.from({ length: chunks }, (_, i) =>
    `Chunk ${i} of the synthetic ANGARS fixture. `.padEnd(1600, "x"),
  ).join("\n\n");
}

// ── Counting mock provider ─────────────────────────────────────────────────────

class CountingMockProvider implements LlmProvider {
  calls = 0;
  /** Resolves chunkIds are injected so C5 test can be orthogonal from C4. */
  resolvedIds: Set<string> = new Set();

  async propose(
    chunkId: string,
    _chunkText: string,
    _sectionContext: string,
  ): Promise<CandidateProposal[]> {
    this.calls++;
    // Return one proposal per chunk, re-using the chunk's own id as citation
    return [
      {
        kind: "requirement" as const,
        fields: { text: "synthetic req" },
        citedChunkId: chunkId,
        confidence: 0.9,
        quote: "synthetic req",
      },
    ];
  }
}

// ── C5 sweep test ─────────────────────────────────────────────────────────────

describe("G-C / C5 — processedChunks === totalChunks", () => {
  it("processes every chunk exactly once for a multi-chunk fixture", async () => {
    const targetChunks = 5;
    const text = makeLongText(targetChunks);

    const context: ChunkContext = {
      documentHash: FAKE_DOC_HASH,
      sectionId: "sec-root",
      sectionPath: "root",
      pageStart: 0,
      pageEnd: 9,
      documentId: FAKE_DOC_ID,
    };

    // Pre-compute chunks so we know the expected count
    const chunks = await chunkWithIds(text, context, {
      chunkSize: 1500,
      chunkOverlap: 150,
    });
    expect(chunks.length).toBeGreaterThanOrEqual(targetChunks - 1); // at least 4

    const provider = new CountingMockProvider();

    const result: IngestPipelineResult = await runIngestPipeline({
      text,
      context,
      provider,
      chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
    });

    // C5: every chunk was submitted to the provider exactly once
    expect(result.processedChunks).toBe(result.totalChunks);
    expect(provider.calls).toBe(result.totalChunks);
  });
});

// ── C5 no-vector grep proof ───────────────────────────────────────────────────

describe("G-C / C5 — no vector/embedding/retrieval in ingest path", () => {
  it("grep finds no lancedb|embedding|vector|topK|top-k|rerank in scripts or prose-ingest/src", () => {
    const repoRoot = join(import.meta.dirname, "../../../../..");
    const scriptPath = join(repoRoot, "scripts");
    const srcPath = join(repoRoot, "packages/prose-ingest/src");

    let output = "";
    let errored = false;
    try {
      output = execSync(
        `grep -rEn "lancedb|embedding|vector|topK|top-k|rerank" "${scriptPath}" "${srcPath}" 2>/dev/null || true`,
        { encoding: "utf8" },
      );
    } catch {
      errored = true;
    }

    expect(errored).toBe(false);
    // grep returns nothing when no matches are found — the output must be empty
    expect(output.trim()).toBe("");
  });
});
