/**
 * T1 — kind-specific required fields gate.
 *
 * Claims:
 *   - A modeTransition proposal missing fromMode/toMode is DROPPED + counted as droppedMalformed.
 *   - A well-formed modeTransition (fromMode + toMode) passes through.
 *   - A succession proposal missing owningFunction/fromAction/toAction is DROPPED.
 *   - A well-formed succession passes through.
 *   - A decision proposal missing owningFunction/atAction/branches (or < 2 branches) is DROPPED.
 *   - A well-formed decision (≥ 2 branches) passes through.
 *   - A parallel proposal missing owningFunction/branchActions (or < 2) is DROPPED.
 *   - A well-formed parallel (≥ 2 branchActions) passes through.
 *   - Existing kinds (requirement, need, mode, interface, component, function) continue to pass through.
 *   - IngestPipelineResult.droppedMalformed counts all dropped-malformed proposals.
 *
 * RED: fails until llm-provider.ts enforces kind-specific required fields and
 *      ingest-pipeline.ts tracks droppedMalformed.
 */

import { describe, it, expect } from "vitest";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { IngestPipelineResult } from "../ingest-pipeline.js";
import type { ChunkContext } from "../chunker.js";

// ── Shared context fixtures ────────────────────────────────────────────────────

const FAKE_DOC_HASH = "c".repeat(64);
const FAKE_DOC_ID = "kind-specific-fixture";
const CHUNK_CONTEXT: ChunkContext = {
  documentHash: FAKE_DOC_HASH,
  sectionId: "sec-root",
  sectionPath: "root",
  pageStart: 0,
  pageEnd: 1,
  documentId: FAKE_DOC_ID,
};
const CHUNK_OPTS = { chunkSize: 1500, chunkOverlap: 150 };

// Single-chunk text — under chunkSize:1500 so we get exactly 1 chunk.
const SINGLE_CHUNK_TEXT = "ANGARS Operational Modes. ".padEnd(1400, "x");

// ── Provider builder ───────────────────────────────────────────────────────────

function makeProvider(proposals: CandidateProposal[]): LlmProvider {
  return {
    async propose(chunkId: string): Promise<CandidateProposal[]> {
      // Stamp all proposals with the real chunkId so C4 citation gate passes.
      return proposals.map((p) => ({ ...p, citedChunkId: chunkId }));
    },
  };
}

// ── modeTransition ─────────────────────────────────────────────────────────────

describe("T1 — modeTransition kind-specific required fields", () => {
  it("drops modeTransition missing fromMode", async () => {
    const provider = makeProvider([
      {
        kind: "modeTransition",
        fields: { toMode: "Active", trigger: "power on" }, // fromMode missing
        citedChunkId: "",
        confidence: 0.8,
        quote: "transition to Active",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("drops modeTransition missing toMode", async () => {
    const provider = makeProvider([
      {
        kind: "modeTransition",
        fields: { fromMode: "Standby" }, // toMode missing
        citedChunkId: "",
        confidence: 0.8,
        quote: "from Standby",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("passes well-formed modeTransition (fromMode + toMode)", async () => {
    const provider = makeProvider([
      {
        kind: "modeTransition",
        fields: { fromMode: "Standby", toMode: "Active", trigger: "power on" },
        citedChunkId: "",
        confidence: 0.9,
        quote: "from Standby to Active on power on",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(1);
    expect(result.droppedMalformed).toBe(0);
  });
});

// ── succession ─────────────────────────────────────────────────────────────────

describe("T1 — succession kind-specific required fields", () => {
  it("drops succession missing owningFunction", async () => {
    const provider = makeProvider([
      {
        kind: "succession",
        fields: { fromAction: "Receive Request", toAction: "Validate Capacity" },
        citedChunkId: "",
        confidence: 0.7,
        quote: "Receive Request then Validate",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("drops succession missing fromAction", async () => {
    const provider = makeProvider([
      {
        kind: "succession",
        fields: { owningFunction: "F3", toAction: "Validate Capacity" },
        citedChunkId: "",
        confidence: 0.7,
        quote: "into Validate",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("drops succession missing toAction", async () => {
    const provider = makeProvider([
      {
        kind: "succession",
        fields: { owningFunction: "F3", fromAction: "Receive Request" },
        citedChunkId: "",
        confidence: 0.7,
        quote: "after Receive",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("passes well-formed succession (owningFunction + fromAction + toAction)", async () => {
    const provider = makeProvider([
      {
        kind: "succession",
        fields: { owningFunction: "F3", fromAction: "Receive Request", toAction: "Validate Capacity", guard: "isValid" },
        citedChunkId: "",
        confidence: 0.85,
        quote: "F3 receives then validates",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(1);
    expect(result.droppedMalformed).toBe(0);
  });
});

// ── decision ───────────────────────────────────────────────────────────────────

describe("T1 — decision kind-specific required fields", () => {
  it("drops decision missing owningFunction", async () => {
    const provider = makeProvider([
      {
        kind: "decision",
        fields: {
          atAction: "fuelCheck",
          branches: [{ guard: "fuelOk", toAction: "Proceed" }, { guard: "fuelLow", toAction: "Abort" }],
        },
        citedChunkId: "",
        confidence: 0.7,
        quote: "fuel check branches",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("drops decision with fewer than 2 branches", async () => {
    const provider = makeProvider([
      {
        kind: "decision",
        fields: {
          owningFunction: "F3",
          atAction: "fuelCheck",
          branches: [{ guard: "fuelOk", toAction: "Proceed" }], // only 1 branch
        },
        citedChunkId: "",
        confidence: 0.7,
        quote: "only one branch",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("passes well-formed decision (owningFunction + atAction + ≥2 branches)", async () => {
    const provider = makeProvider([
      {
        kind: "decision",
        fields: {
          owningFunction: "F3",
          atAction: "fuelCheck",
          branches: [
            { guard: "fuelOk", toAction: "Proceed" },
            { guard: "fuelLow", toAction: "Abort" },
          ],
        },
        citedChunkId: "",
        confidence: 0.9,
        quote: "fuel check: ok→proceed, low→abort",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(1);
    expect(result.droppedMalformed).toBe(0);
  });
});

// ── parallel ───────────────────────────────────────────────────────────────────

describe("T1 — parallel kind-specific required fields", () => {
  it("drops parallel missing owningFunction", async () => {
    const provider = makeProvider([
      {
        kind: "parallel",
        fields: { branchActions: ["Generate Schedule", "Display Mission Data"] },
        citedChunkId: "",
        confidence: 0.7,
        quote: "parallel branches",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("drops parallel with fewer than 2 branchActions", async () => {
    const provider = makeProvider([
      {
        kind: "parallel",
        fields: { owningFunction: "F3", branchActions: ["Generate Schedule"] }, // only 1
        citedChunkId: "",
        confidence: 0.7,
        quote: "only one branch",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(0);
    expect(result.droppedMalformed).toBeGreaterThan(0);
  });

  it("passes well-formed parallel (owningFunction + ≥2 branchActions)", async () => {
    const provider = makeProvider([
      {
        kind: "parallel",
        fields: {
          owningFunction: "F3",
          branchActions: ["Generate Schedule", "Display Mission Data"],
        },
        citedChunkId: "",
        confidence: 0.85,
        quote: "fork: generate schedule and display mission data",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    expect(result.candidates).toHaveLength(1);
    expect(result.droppedMalformed).toBe(0);
  });
});

// ── Existing kinds pass-through ────────────────────────────────────────────────

describe("T1 — existing kinds still pass through without change", () => {
  for (const kind of ["requirement", "need", "mode", "interface", "component", "function"] as const) {
    it(`passes well-formed ${kind}`, async () => {
      const provider = makeProvider([
        {
          kind,
          fields: { name: "SomeValue", text: "sample text" },
          citedChunkId: "",
          confidence: 0.8,
          quote: "sample quote",
        },
      ]);
      const result: IngestPipelineResult = await runIngestPipeline({
        text: SINGLE_CHUNK_TEXT,
        context: CHUNK_CONTEXT,
        provider,
        chunkOptions: CHUNK_OPTS,
      });
      expect(result.candidates).toHaveLength(1);
      expect(result.droppedMalformed).toBe(0);
    });
  }
});

// ── droppedMalformed counts mixed drop batch ────────────────────────────────────

describe("T1 — droppedMalformed counts all malformed drops in a batch", () => {
  it("counts all malformed proposals across kinds", async () => {
    const provider = makeProvider([
      // well-formed modeTransition
      {
        kind: "modeTransition",
        fields: { fromMode: "Standby", toMode: "Active" },
        citedChunkId: "",
        confidence: 0.9,
        quote: "Standby to Active",
      },
      // malformed modeTransition (missing toMode)
      {
        kind: "modeTransition",
        fields: { fromMode: "Active" },
        citedChunkId: "",
        confidence: 0.5,
        quote: "from Active",
      },
      // malformed succession (missing toAction)
      {
        kind: "succession",
        fields: { owningFunction: "F1", fromAction: "Receive" },
        citedChunkId: "",
        confidence: 0.5,
        quote: "after Receive",
      },
      // well-formed requirement (no kind-specific required fields)
      {
        kind: "requirement",
        fields: { text: "some req" },
        citedChunkId: "",
        confidence: 0.8,
        quote: "some req text",
      },
    ]);
    const result: IngestPipelineResult = await runIngestPipeline({
      text: SINGLE_CHUNK_TEXT,
      context: CHUNK_CONTEXT,
      provider,
      chunkOptions: CHUNK_OPTS,
    });
    // 1 modeTransition + 1 requirement = 2 pass
    expect(result.candidates).toHaveLength(2);
    // 1 bad modeTransition + 1 bad succession = 2 dropped malformed
    expect(result.droppedMalformed).toBe(2);
  });
});
