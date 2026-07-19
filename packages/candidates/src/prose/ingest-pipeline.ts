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
 *   C6 (neg): every emitted candidate's quote verbatim-resolves into the text of
 *             its cited chunk (SEPAL-style verbatim citation enforcement). A
 *             proposal whose quote does NOT occur in its cited chunk — under the
 *             normalization rules in @sysml-bridge/model's `quoteOccursInChunk`
 *             (collapse whitespace, fold unicode quotes/dashes, case-sensitive
 *             otherwise) — is DROPPED and counted in result.droppedUnverbatim.
 *             result.emittedUnverbatim === 0. This closes the hole where a
 *             hallucinated quote pointing at a REAL chunk (passes C4) would be
 *             the text a human reads in the review UI when approving.
 *
 * PURE pipeline — no I/O in this module (I/O lives in scripts/ingest-prose.ts).
 */

import { chunkWithIds } from "./chunker.js";
import type { ChunkContext, ChunkTextOptions } from "./chunker.js";
import { stableId, quoteOccursInChunk } from "@sysml-bridge/model";
import type { LlmProvider, CandidateProposal } from "./llm-provider.js";
import { validateKindSpecificFields } from "./llm-provider.js";
import type { RunCountersSnapshot } from "../telemetry.js";
import { deriveMentions } from "../mentions/index.js";
import type { MentionRecord } from "../mentions/index.js";

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
  /**
   * True iff this candidate's quote verbatim-resolves into its cited chunk (C6).
   * Every emitted candidate has this true by construction (the gate below drops
   * the rest); it is stamped explicitly so the record is self-auditing and the
   * review UI can surface a "verbatim-verified" signal without re-deriving it.
   */
  verbatimVerified: boolean;
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
   * Proposals dropped because their quote does NOT verbatim-resolve into the
   * text of their cited chunk (C6). Mirrors droppedUncited — a hallucinated quote
   * over a real chunk is dropped here, not emitted.
   */
  droppedUnverbatim: number;
  /**
   * Emitted candidates whose quote does NOT verbatim-resolve — MUST be 0 (C6 neg).
   * Included for explicit assertion in tests.
   */
  emittedUnverbatim: number;
  /**
   * Proposals dropped because they were missing kind-specific required fields
   * (T1 malformed-drop gate). Never emitted malformed.
   */
  droppedMalformed: number;
  /**
   * LLM parse/schema-failure counts from the provider (the silent-`[]` sites in
   * llm-provider.ts, now counted + logged). A non-zero count means chunks were
   * silently yielding nothing due to unparseable responses — a run-degradation
   * signal that was previously invisible. Undefined if the provider does not
   * expose a `counters` getter (e.g. simple mocks).
   */
  parseFailures?: RunCountersSnapshot;
  /**
   * The chunk store this run built and processed: chunkId → chunk text, for
   * exactly the chunks that were submitted to the provider (C4/C6 resolution
   * domain). Exposed so a caller (e.g. scripts/ingest-prose.ts) can PERSIST the
   * store to disk alongside the emitted candidates — the same store later feeds
   * the PROSE-unverbatim-quote audit (ProseComposedIR.chunkStore) and BM25
   * evidence retrieval. This is the single source of truth for chunk text; the
   * on-disk record's sectionPath comes from the run's ChunkContext.
   */
  chunkStore: ReadonlyMap<string, string>;
  /**
   * Mentions derived from this run's raw proposals (W0 mention substrate) —
   * every candidate's own name field PLUS any explicit `proposal.mentions[]`
   * the provider returned, gated by the same citation discipline (C4/C6)
   * candidates use. See `../mentions/index.ts#deriveMentions`.
   */
  mentions: MentionRecord[];
  /**
   * Candidate mentions dropped because their citation's chunkId did not
   * resolve, or their quote did not verbatim-resolve into that chunk's text
   * — mirrors droppedUnverbatim for candidates, extended to mentions. Never
   * silently emitted, never silently capped.
   */
  droppedUnverbatimMentions: number;
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
 * 2. Build a Map<chunkId, chunkText> (the chunk store) for citation resolution
 *    (C4) and verbatim quote checking (C6).
 * 3. For each chunk (exactly once — C5): call provider.propose().
 * 4. For each proposal, in order:
 *    - citedChunkId not in store → DROP, increment droppedUncited (C4).
 *    - missing kind-specific fields → DROP, increment droppedMalformed (T1).
 *    - quote does not occur in the CITED chunk's text → DROP, increment
 *      droppedUnverbatim (C6). Checked against the cited chunk (which may differ
 *      from the chunk being processed), using the shared normalization rules.
 *    - otherwise → emit ProseCandidateRecord with stable deterministic ID.
 * 5. Return result with processedChunks, totalChunks, and the drop/emit counters.
 */
export async function runIngestPipeline(
  opts: IngestPipelineOptions,
): Promise<IngestPipelineResult> {
  const { text, context, provider, chunkOptions } = opts;

  // Step 1: Produce chunks
  const chunks = await chunkWithIds(text, context, chunkOptions);
  const totalChunks = chunks.length;

  // Step 2: Build chunk store (chunkId → chunk text) for C4 citation resolution
  // and C6 verbatim quote checking. First occurrence wins on the vanishingly
  // rare id collision — chunkIds are content-addressed and unique in practice.
  const chunkStore = new Map<string, string>();
  for (const c of chunks) {
    if (!chunkStore.has(c.chunkId)) chunkStore.set(c.chunkId, c.text);
  }

  // Step 3–4: Process each chunk exactly once
  const candidates: ProseCandidateRecord[] = [];
  // Raw proposals from every chunk, kept for W0 mention derivation below — the
  // SAME provider call candidates come from (C5: no extra call). Mentions are
  // derived from the raw stream, not the filtered `candidates`, so a mention
  // gets its own independent C4/C6-style citation gate (see deriveMentions).
  const allProposals: CandidateProposal[] = [];
  let processedChunks = 0;
  let droppedUncited = 0;
  let droppedMalformed = 0;
  let droppedUnverbatim = 0;

  for (const chunk of chunks) {
    // C5: exactly-once — increment before calling provider
    processedChunks++;

    const proposals = await provider.propose(
      chunk.chunkId,
      chunk.text,
      context.sectionPath,
    );
    allProposals.push(...proposals);

    for (const proposal of proposals) {
      // C4: citation resolution gate
      const citedText = chunkStore.get(proposal.citedChunkId);
      if (citedText === undefined) {
        droppedUncited++;
        continue; // DROP unresolvable proposal
      }

      // T1: kind-specific required-field gate
      if (!validateKindSpecificFields(proposal)) {
        droppedMalformed++;
        continue; // DROP malformed proposal — never emit malformed
      }

      // C6: verbatim quote gate — the quote MUST occur in the cited chunk's text.
      // A hallucinated quote pointing at a real chunk passes C4 but is dropped
      // here so it never reaches the human reviewer as trusted evidence.
      if (!quoteOccursInChunk(proposal.quote, citedText)) {
        droppedUnverbatim++;
        continue; // DROP unverbatim proposal
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
        verbatimVerified: true, // C6: enforced by the gate above
      };

      candidates.push(record);
    }
  }

  // Surface the provider's LLM parse-failure counts (silent-`[]` sites), if exposed.
  const parseFailures = provider.counters?.snapshot();

  // Step 5 (W0): derive mentions from the raw proposal stream — pure, no I/O,
  // gated by the same C4/C6 citation discipline as candidates (unresolvable
  // chunkId or non-verbatim quote is dropped and counted, never emitted).
  const { mentions, droppedUnverbatimMentions } = deriveMentions(
    allProposals,
    chunkStore,
    {
      documentId: context.documentId,
      documentHash: context.documentHash,
      sectionPath: context.sectionPath,
    },
  );

  return {
    candidates,
    totalChunks,
    processedChunks,
    droppedUncited,
    emittedUncited: 0, // always 0 — the gate above enforces this
    droppedUnverbatim,
    emittedUnverbatim: 0, // always 0 — the C6 gate above enforces this
    droppedMalformed,
    chunkStore, // expose for persistence (chunkId → text; source of truth)
    mentions,
    droppedUnverbatimMentions,
    ...(parseFailures ? { parseFailures } : {}),
  };
}
