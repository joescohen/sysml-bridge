/**
 * G-C / idempotent — candidate ID stability across re-runs.
 *
 * Claims closed:
 *   Candidate IDs are derived deterministically from (docSha256, chunkId, kind,
 *   naturalKey). Two runs with identical inputs produce identical candidate IDs.
 *
 * RED: fails until ingest-pipeline.ts and llm-provider.ts exist.
 */

import { describe, it, expect } from "vitest";
import type { ChunkContext } from "../chunker.js";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { IngestPipelineResult } from "../ingest-pipeline.js";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const FAKE_DOC_HASH = "c".repeat(64);
const FAKE_DOC_ID = "gc-idempotent-fixture";

const FIXTURE_TEXT =
  "SYS-REQ-001: The system shall operate autonomously. " +
  "The ANGARS refueling unit shall maintain a 3-meter separation envelope. ".padEnd(1600, " ");

const CONTEXT: ChunkContext = {
  documentHash: FAKE_DOC_HASH,
  sectionId: "sec-root",
  sectionPath: "root",
  pageStart: 0,
  pageEnd: 0,
  documentId: FAKE_DOC_ID,
};

// ── Deterministic mock provider ────────────────────────────────────────────────

/** Always returns the same proposal shape regardless of call order. */
class DeterministicProvider implements LlmProvider {
  async propose(
    chunkId: string,
    _chunkText: string,
    _sectionContext: string,
  ): Promise<CandidateProposal[]> {
    return [
      {
        kind: "requirement" as const,
        fields: { text: "The system shall operate autonomously." },
        citedChunkId: chunkId,
        confidence: 0.95,
        quote: "The system shall operate autonomously.",
      },
    ];
  }
}

// ── Idempotent candidate ID tests ─────────────────────────────────────────────

describe("G-C / idempotent — candidate IDs stable across re-runs", () => {
  it("two runs with identical inputs produce identical candidate IDs", async () => {
    const opts = { chunkSize: 1500, chunkOverlap: 150 };

    const run1: IngestPipelineResult = await runIngestPipeline({
      text: FIXTURE_TEXT,
      context: CONTEXT,
      provider: new DeterministicProvider(),
      chunkOptions: opts,
    });

    const run2: IngestPipelineResult = await runIngestPipeline({
      text: FIXTURE_TEXT,
      context: CONTEXT,
      provider: new DeterministicProvider(),
      chunkOptions: opts,
    });

    expect(run1.candidates.length).toBeGreaterThan(0);
    expect(run1.candidates).toHaveLength(run2.candidates.length);

    for (let i = 0; i < run1.candidates.length; i++) {
      const c1 = run1.candidates[i]!;
      const c2 = run2.candidates[i]!;
      // Candidate IDs must be identical across runs
      expect(c1.id).toBe(c2.id);
      // chunkId citations must be identical
      expect(c1.citation.chunkId).toBe(c2.citation.chunkId);
      // kind must match
      expect(c1.kind).toBe(c2.kind);
    }
  });

  it("candidate ID changes when doc hash changes (different document)", async () => {
    const opts = { chunkSize: 1500, chunkOverlap: 150 };

    const ctxA: ChunkContext = { ...CONTEXT, documentHash: "a".repeat(64), documentId: "doc-a" };
    const ctxB: ChunkContext = { ...CONTEXT, documentHash: "b".repeat(64), documentId: "doc-b" };

    const runA: IngestPipelineResult = await runIngestPipeline({
      text: FIXTURE_TEXT,
      context: ctxA,
      provider: new DeterministicProvider(),
      chunkOptions: opts,
    });

    const runB: IngestPipelineResult = await runIngestPipeline({
      text: FIXTURE_TEXT,
      context: ctxB,
      provider: new DeterministicProvider(),
      chunkOptions: opts,
    });

    expect(runA.candidates.length).toBeGreaterThan(0);
    expect(runA.candidates).toHaveLength(runB.candidates.length);

    // IDs must differ because the doc hash is different
    for (let i = 0; i < runA.candidates.length; i++) {
      expect(runA.candidates[i]!.id).not.toBe(runB.candidates[i]!.id);
    }
  });

  it("candidate ID is deterministic format: 'prose-candidate-<16-hex-chars>'", async () => {
    const result: IngestPipelineResult = await runIngestPipeline({
      text: FIXTURE_TEXT,
      context: CONTEXT,
      provider: new DeterministicProvider(),
      chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
    });

    expect(result.candidates.length).toBeGreaterThan(0);
    for (const candidate of result.candidates) {
      // stableId("prose-candidate", ...) → "prose-candidate-<16 hex>"
      expect(candidate.id).toMatch(/^prose-candidate-[0-9a-f]{16}$/);
    }
  });
});
