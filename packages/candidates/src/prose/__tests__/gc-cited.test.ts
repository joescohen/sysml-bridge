/**
 * G-C / C4 — citation resolution gate.
 *
 * Claims closed:
 *   C4 (neg): every emitted candidate carries a chunkId that resolves into the
 *             chunk store. LLM proposals lacking a resolvable citation are DROPPED
 *             with a logged count. emitted-uncited count == 0.
 *
 * Test:
 *   - Mock provider returns a mix of proposals: some with a chunkId that IS in
 *     the chunk store, some with a chunkId that is NOT.
 *   - After pipeline run:
 *       emitted candidates all have resolvable chunkIds
 *       result.droppedUncited === (number of unresolvable proposals)
 *       result.emittedUncited === 0
 *
 * RED: fails until ingest-pipeline.ts and llm-provider.ts exist.
 */

import { describe, it, expect } from "vitest";
import { chunkWithIds } from "../chunker.js";
import type { ChunkContext } from "../chunker.js";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { IngestPipelineResult } from "../ingest-pipeline.js";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const FAKE_DOC_HASH = "b".repeat(64);
const FAKE_DOC_ID = "gc-cited-fixture";

function makeTwoChunkText(): string {
  // Two clearly distinct sections so chunkWithIds produces ≥ 2 chunks
  return (
    "SECTION A: ".padEnd(1600, "a") +
    "\n\n" +
    "SECTION B: ".padEnd(1600, "b")
  );
}

// ── Controlled mix provider ────────────────────────────────────────────────────

/**
 * For each chunk the provider is called with:
 *   - one VALID proposal using the actual chunkId (resolvable)
 *   - one INVALID proposal using a fabricated unknown chunkId (unresolvable)
 */
class MixedCitationProvider implements LlmProvider {
  async propose(
    chunkId: string,
    _chunkText: string,
    _sectionContext: string,
  ): Promise<CandidateProposal[]> {
    return [
      {
        kind: "requirement" as const,
        fields: { text: "valid req" },
        citedChunkId: chunkId, // resolves — same as the actual chunk
        confidence: 0.9,
        quote: "valid req text",
      },
      {
        kind: "need" as const,
        fields: { text: "ghost need" },
        citedChunkId: "deadbeefdeadbeefdeadbeefdeadbeef", // NOT in store
        confidence: 0.5,
        quote: "ghost need text",
      },
    ];
  }
}

// ── C4 citation gate test ─────────────────────────────────────────────────────

describe("G-C / C4 — citation resolution gate", () => {
  it("drops proposals with unresolvable chunkIds; emitted-uncited == 0", async () => {
    const text = makeTwoChunkText();

    const context: ChunkContext = {
      documentHash: FAKE_DOC_HASH,
      sectionId: "sec-root",
      sectionPath: "root",
      pageStart: 0,
      pageEnd: 1,
      documentId: FAKE_DOC_ID,
    };

    const chunks = await chunkWithIds(text, context, {
      chunkSize: 1500,
      chunkOverlap: 150,
    });
    expect(chunks.length).toBeGreaterThanOrEqual(2);

    const provider = new MixedCitationProvider();

    const result: IngestPipelineResult = await runIngestPipeline({
      text,
      context,
      provider,
      chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
    });

    // C4: no emitted candidate has an unresolvable chunkId
    expect(result.emittedUncited).toBe(0);

    // The invalid ghost proposals were dropped — one per chunk processed
    expect(result.droppedUncited).toBe(result.processedChunks);

    // Every emitted candidate's chunkId is in the chunk store
    const chunkIdSet = new Set(chunks.map((c) => c.chunkId));
    for (const candidate of result.candidates) {
      expect(chunkIdSet.has(candidate.citation.chunkId)).toBe(true);
    }
  });

  it("emits ZERO candidates when all proposals have unresolvable chunkIds", async () => {
    class AllGhostProvider implements LlmProvider {
      async propose(): Promise<CandidateProposal[]> {
        return [
          {
            kind: "requirement" as const,
            fields: { text: "ghost" },
            citedChunkId: "0000000000000000000000000000dead",
            confidence: 0.9,
            quote: "ghost",
          },
        ];
      }
    }

    const context: ChunkContext = {
      documentHash: FAKE_DOC_HASH,
      sectionId: "sec-root",
      sectionPath: "root",
      pageStart: 0,
      pageEnd: 1,
      documentId: FAKE_DOC_ID,
    };

    const result: IngestPipelineResult = await runIngestPipeline({
      text: "ANGARS Autonomous Refueling System Requirements ".padEnd(1600, " "),
      context,
      provider: new AllGhostProvider(),
      chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
    });

    expect(result.candidates).toHaveLength(0);
    expect(result.emittedUncited).toBe(0);
    expect(result.droppedUncited).toBeGreaterThan(0);
  });
});
