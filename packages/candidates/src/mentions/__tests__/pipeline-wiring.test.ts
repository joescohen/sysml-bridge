/**
 * pipeline-wiring.test.ts — mentions flow through runIngestPipeline harvested
 * from the SAME provider call as candidates (C5: no extra call).
 *
 * Claim closed (W0 done-criteria 2): C5 untouched — provider called exactly
 * once per chunk — verified here alongside a populated `result.mentions`, so
 * mention harvesting is proven NOT to add a call.
 */

import { describe, it, expect } from "vitest";
import { chunkWithIds } from "../../prose/chunker.js";
import type { ChunkContext } from "../../prose/chunker.js";
import type { LlmProvider, CandidateProposal } from "../../prose/llm-provider.js";
import { runIngestPipeline } from "../../prose/ingest-pipeline.js";

const FAKE_DOC_HASH = "b".repeat(64);
const FAKE_DOC_ID = "mentions-wiring-fixture";

function makeLongText(chunks: number): string {
  return Array.from({ length: chunks }, (_, i) =>
    `Chunk ${i} of the synthetic ANGARS fixture. `.padEnd(1600, "x"),
  ).join("\n\n");
}

/** Counts calls AND emits one candidate + one explicit mention per chunk. */
class MentionHarvestingMockProvider implements LlmProvider {
  calls = 0;

  async propose(chunkId: string, chunkText: string): Promise<CandidateProposal[]> {
    this.calls++;
    const quote = chunkText.slice(0, 20);
    return [
      {
        kind: "component" as const,
        fields: { name: `Component ${chunkId.slice(0, 6)}` },
        citedChunkId: chunkId,
        confidence: 0.9,
        quote,
        mentions: [
          {
            surfaceForm: `Extra ${chunkId.slice(0, 6)}`,
            kindHint: "flow" as const,
            citedChunkId: chunkId,
            quote,
            confidence: 0.4,
          },
        ],
      },
    ];
  }
}

describe("runIngestPipeline — mentions harvested in the same provider call (C5)", () => {
  it("provider is still called exactly once per chunk, and mentions are populated", async () => {
    const targetChunks = 4;
    const text = makeLongText(targetChunks);
    const context: ChunkContext = {
      documentHash: FAKE_DOC_HASH,
      sectionId: "sec-root",
      sectionPath: "root",
      pageStart: 0,
      pageEnd: 9,
      documentId: FAKE_DOC_ID,
    };

    const chunks = await chunkWithIds(text, context, { chunkSize: 1500, chunkOverlap: 150 });
    const provider = new MentionHarvestingMockProvider();

    const result = await runIngestPipeline({
      text,
      context,
      provider,
      chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
    });

    // C5 held: exactly one call per chunk, unchanged by mention harvesting.
    expect(provider.calls).toBe(result.totalChunks);
    expect(result.processedChunks).toBe(result.totalChunks);
    expect(chunks.length).toBe(result.totalChunks);

    // Mentions populated: 1 implicit (component name) + 1 explicit per chunk.
    expect(result.droppedUnverbatimMentions).toBe(0);
    expect(result.mentions.length).toBe(result.totalChunks * 2);
    expect(result.mentions.some((m) => m.kindHint === "component")).toBe(true);
    expect(result.mentions.some((m) => m.kindHint === "flow")).toBe(true);
    // Every emitted mention's citation resolves into the run's own chunk store (C4).
    for (const m of result.mentions) {
      expect(result.chunkStore.has(m.citation.chunkId)).toBe(true);
    }
  });
});
