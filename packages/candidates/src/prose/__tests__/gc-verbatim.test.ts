/**
 * G-C / C6 — verbatim citation-quote gate (SEPAL-style).
 *
 * Claims closed:
 *   C6 (neg): every emitted candidate's quote verbatim-resolves into the text of
 *             its cited chunk. A proposal whose quote does NOT occur in its cited
 *             chunk (a hallucinated quote over a REAL chunk — passes C4) is
 *             DROPPED and counted in result.droppedUnverbatim.
 *             result.emittedUnverbatim === 0.
 *
 * Matching is normalized: whitespace collapse + unicode quote/dash folding, but
 * case-sensitive otherwise (see @sysml-bridge/model `quoteOccursInChunk`).
 *
 * Tests:
 *   - verbatim quote PASSES (emitted, verbatimVerified true).
 *   - mutated quote is DROPPED (the fail-able positive control: this assertion
 *     fails if the gate is a no-op, because the candidate would be emitted).
 *   - whitespace / smart-quote / dash differences are TOLERATED (still PASS).
 *   - empty quote is DROPPED (no vacuous "".includes("") pass).
 *   - counter accounting across a mixed batch.
 */

import { describe, it, expect } from "vitest";
import type { ChunkContext } from "../chunker.js";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { IngestPipelineResult } from "../ingest-pipeline.js";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const FAKE_DOC_HASH = "e".repeat(64);
const FAKE_DOC_ID = "gc-verbatim-fixture";

// Single chunk (< 1500 chars). Contains an ASCII apostrophe and ASCII hyphens so
// the smart-quote / dash normalization can be exercised from the quote side.
const CHUNK_TEXT =
  "The ANGARS system shall refuel the receiver aircraft autonomously within sixty seconds of contact. " +
  "The pilot's console shows a real-time three-meter separation envelope.";

const CONTEXT: ChunkContext = {
  documentHash: FAKE_DOC_HASH,
  sectionId: "sec-root",
  sectionPath: "root",
  pageStart: 0,
  pageEnd: 0,
  documentId: FAKE_DOC_ID,
};
const OPTS = { chunkSize: 1500, chunkOverlap: 150 };

/** Provider that returns the given proposals, each stamped to the real chunkId. */
function providerReturning(
  proposals: Omit<CandidateProposal, "citedChunkId">[]
): LlmProvider {
  return {
    async propose(chunkId: string): Promise<CandidateProposal[]> {
      return proposals.map((p) => ({ ...p, citedChunkId: chunkId }));
    },
  };
}

async function run(
  proposals: Omit<CandidateProposal, "citedChunkId">[]
): Promise<IngestPipelineResult> {
  return runIngestPipeline({
    text: CHUNK_TEXT,
    context: CONTEXT,
    provider: providerReturning(proposals),
    chunkOptions: OPTS,
  });
}

// ── C6 tests ──────────────────────────────────────────────────────────────────

describe("G-C / C6 — verbatim citation-quote gate", () => {
  it("emits a candidate whose quote occurs verbatim in the cited chunk", async () => {
    const result = await run([
      {
        kind: "requirement",
        fields: { text: "refuel within sixty seconds" },
        confidence: 0.9,
        quote: "shall refuel the receiver aircraft autonomously within sixty seconds",
      },
    ]);

    expect(result.candidates).toHaveLength(1);
    expect(result.droppedUnverbatim).toBe(0);
    expect(result.emittedUnverbatim).toBe(0);
    expect(result.candidates[0]!.verbatimVerified).toBe(true);
  });

  it("DROPS a candidate whose quote does not occur in the cited chunk (positive control)", async () => {
    // A hallucinated quote pointing at a REAL chunk: passes C4 (chunkId resolves)
    // but must be dropped by C6. This assertion FAILS if the gate is a no-op.
    const result = await run([
      {
        kind: "requirement",
        fields: { text: "manual refuel" },
        confidence: 0.9,
        quote: "shall refuel the receiver aircraft MANUALLY within sixty seconds",
      },
    ]);

    expect(result.candidates).toHaveLength(0);
    expect(result.droppedUnverbatim).toBe(1);
    expect(result.emittedUnverbatim).toBe(0);
  });

  it("TOLERATES whitespace, smart-quote, and dash differences (still verbatim)", async () => {
    // Same span as the chunk but with: collapsed→expanded whitespace (double
    // spaces + newline), a curly apostrophe (’), a non-breaking hyphen (‑ U+2011),
    // and an em dash (—). All fold to the chunk's ASCII forms under normalization.
    const mangled =
      "The pilot’s   console\nshows a real‑time  three—meter separation envelope.";
    const result = await run([
      {
        kind: "requirement",
        fields: { text: "separation envelope" },
        confidence: 0.8,
        quote: mangled,
      },
    ]);

    expect(result.candidates).toHaveLength(1);
    expect(result.droppedUnverbatim).toBe(0);
    expect(result.candidates[0]!.verbatimVerified).toBe(true);
  });

  it("DROPS an empty quote (no vacuous match)", async () => {
    const result = await run([
      {
        kind: "requirement",
        fields: { text: "no quote" },
        confidence: 0.5,
        quote: "   ",
      },
    ]);

    expect(result.candidates).toHaveLength(0);
    expect(result.droppedUnverbatim).toBe(1);
  });

  it("counts droppedUnverbatim across a mixed batch and emits only verbatim ones", async () => {
    const result = await run([
      // verbatim → emitted
      {
        kind: "requirement",
        fields: { text: "a" },
        confidence: 0.9,
        quote: "shall refuel the receiver aircraft autonomously",
      },
      // hallucinated → dropped
      {
        kind: "need",
        fields: { text: "b" },
        confidence: 0.9,
        quote: "shall never be refueled by any means",
      },
      // verbatim (dash/whitespace variant) → emitted
      {
        kind: "component",
        fields: { name: "Console" },
        confidence: 0.7,
        quote: "real‑time  three—meter separation envelope",
      },
    ]);

    expect(result.candidates).toHaveLength(2);
    expect(result.droppedUnverbatim).toBe(1);
    expect(result.emittedUnverbatim).toBe(0);
    // Every emitted candidate carries the verified flag.
    expect(result.candidates.every((c) => c.verbatimVerified)).toBe(true);
  });
});
