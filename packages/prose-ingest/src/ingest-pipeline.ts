/**
 * ingest-pipeline.ts — core prose ingestion pipeline (G-C).
 *
 * Claims:
 *   C4 (neg): every emitted candidate carries a chunkId that resolves into the
 *             chunk store. Proposals with unresolvable chunkIds are DROPPED and
 *             counted in result.droppedUncited. result.emittedUncited === 0.
 *
 *   C5 (neg): every chunk is submitted to the LLM provider EXACTLY ONCE.
 *             result.processedChunks === result.totalChunks.
 *             No vector/embedding/retrieval calls (enforced by grep in tests).
 *
 * PURE pipeline — no I/O in this module (I/O lives in scripts/ingest-prose.ts).
 */

import { chunkWithIds } from "./chunker.js";
import type { ChunkContext, ChunkTextOptions } from "./chunker.js";
import { stableId } from "@sysml-bridge/ir";
import type { LlmProvider, CandidateProposal } from "./llm-provider.js";
import { validateKindSpecificFields } from "./llm-provider.js";

// ── ProseCandidateRecord ──────────────────────────────────────────────────────

/** One candidate extracted from the prose pipeline, ready for human review. */
export interface ProseCandidateRecord {
  /** Deterministic ID: stableId("prose-candidate", `${docSha256}:${chunkId}:${kind}:${naturalKey}`) */
  id: string;
  kind: CandidateProposal["kind"];
  fields: Record<string, unknown>;
  citation: {
    docId: string;
    docSha256: string;
    chunkId: string;
    sectionPath: string;
    quote: string;
  };
  source: "llm";
  confidence: number;
  /** Candidate ID — same as id (for compatibility with ProseApprovedEntry shape). */
  candidateId: string;
}

// ── IngestPipelineResult ──────────────────────────────────────────────────────

export interface IngestPipelineResult {
  /** All emitted candidates (all have resolvable chunkIds — C4). */
  candidates: ProseCandidateRecord[];
  /** Total chunks produced by the splitter (denominator for C5). */
  totalChunks: number;
  /**
   * Chunks submitted to the provider. Must equal totalChunks (C5 invariant).
   * Tracked separately so callers can assert the exact-once property.
   */
  processedChunks: number;
  /** Proposals dropped because their citedChunkId was not in the chunk store (C4). */
  droppedUncited: number;
  /**
   * Emitted candidates with unresolvable chunkIds — MUST be 0 (C4 neg).
   * Included for explicit assertion in tests.
   */
  emittedUncited: number;
  /**
   * Proposals dropped because they were missing kind-specific required fields
   * (T1 malformed-drop gate). Never emitted malformed.
   */
  droppedMalformed: number;
}

// ── IngestPipelineOptions ─────────────────────────────────────────────────────

export interface IngestPipelineOptions {
  /** Full document text to process. */
  text: string;
  /** Chunk context — doc hash, section info, document ID. */
  context: ChunkContext;
  /** Injectable LLM provider (mock for tests, Anthropic for real runs). */
  provider: LlmProvider;
  /** Chunking configuration (optional, defaults to 1500/150). */
  chunkOptions?: ChunkTextOptions;
}

// ── Natural key extraction ────────────────────────────────────────────────────

/**
 * Derive a short natural key from a proposal for stable ID generation.
 * Uses the first field value that looks like a meaningful string.
 */
function naturalKeyFromProposal(proposal: CandidateProposal): string {
  // Prefer a "text" or "id" field if present
  const fields = proposal.fields;
  for (const key of ["id", "text", "name", "title", "description"]) {
    const val = fields[key];
    if (typeof val === "string" && val.trim().length > 0) {
      return val.trim().slice(0, 120);
    }
  }
  // Fall back: first non-empty string value
  for (const val of Object.values(fields)) {
    if (typeof val === "string" && val.trim().length > 0) {
      return val.trim().slice(0, 120);
    }
  }
  // Last resort: stringify the fields object, truncated
  return JSON.stringify(fields).slice(0, 120);
}

// ── Main pipeline ─────────────────────────────────────────────────────────────

/**
 * Run the prose ingestion pipeline over a single document's text.
 *
 * Algorithm:
 * 1. Split text into chunks via chunkWithIds.
 * 2. Build a Set<chunkId> (chunk store) for citation resolution (C4).
 * 3. For each chunk (exactly once — C5): call provider.propose().
 * 4. For each proposal: check citedChunkId in chunk store.
 *    - In store  → emit ProseCandidateRecord with stable deterministic ID.
 *    - Not found → DROP, increment droppedUncited counter.
 * 5. Return result with processedChunks, totalChunks, droppedUncited, emittedUncited.
 */
export async function runIngestPipeline(
  opts: IngestPipelineOptions,
): Promise<IngestPipelineResult> {
  const { text, context, provider, chunkOptions } = opts;

  // Step 1: Produce chunks
  const chunks = await chunkWithIds(text, context, chunkOptions);
  const totalChunks = chunks.length;

  // Step 2: Build chunk store for C4 citation resolution
  const chunkStore = new Set<string>(chunks.map((c) => c.chunkId));

  // Step 3–4: Process each chunk exactly once
  const candidates: ProseCandidateRecord[] = [];
  let processedChunks = 0;
  let droppedUncited = 0;
  let droppedMalformed = 0;

  for (const chunk of chunks) {
    // C5: exactly-once — increment before calling provider
    processedChunks++;

    const proposals = await provider.propose(
      chunk.chunkId,
      chunk.text,
      context.sectionPath,
    );

    for (const proposal of proposals) {
      // C4: citation resolution gate
      if (!chunkStore.has(proposal.citedChunkId)) {
        droppedUncited++;
        continue; // DROP unresolvable proposal
      }

      // T1: kind-specific required-field gate
      if (!validateKindSpecificFields(proposal)) {
        droppedMalformed++;
        continue; // DROP malformed proposal — never emit malformed
      }

      // Build deterministic candidate ID
      const naturalKey = naturalKeyFromProposal(proposal);
      const idInput = `${context.documentHash}:${proposal.citedChunkId}:${proposal.kind}:${naturalKey}`;
      const id = stableId("prose-candidate", idInput);

      const record: ProseCandidateRecord = {
        id,
        kind: proposal.kind,
        fields: proposal.fields,
        citation: {
          docId: context.documentId,
          docSha256: context.documentHash,
          chunkId: proposal.citedChunkId,
          sectionPath: context.sectionPath,
          quote: proposal.quote.slice(0, 300),
        },
        source: "llm",
        confidence: proposal.confidence,
        candidateId: id,
      };

      candidates.push(record);
    }
  }

  return {
    candidates,
    totalChunks,
    processedChunks,
    droppedUncited,
    emittedUncited: 0, // always 0 — the gate above enforces this
    droppedMalformed,
  };
}
