/**
 * persistence.test.ts — chunk-store plumbing, end to end.
 *
 * Proves the connective plumbing that lets a PERSISTED chunk store feed both
 * downstream consumers from ONE file on disk:
 *
 *   ingest a tiny fixture doc (mock provider — no LLM/key, like gc-real-run)
 *     → persist the run's chunkStore to <tmp>/chunks.json
 *     → reload it
 *     → (a) run PROSE-unverbatim-quote at ERROR level over it (not the degrade
 *           warning path), with a paired positive control that the gate DOES
 *           degrade when the store is absent — so the ERROR is not vacuous
 *     → (b) feed the SAME reloaded records to a Bm25Index and confirm a
 *           retrieved chunk id resolves as an inference premise
 *
 * This is the fixture-based proof the integration task calls for: the committed
 * ANGARS candidates predate chunk persistence (no chunks.json, regeneration
 * needs an API key), so the degrade-warning path is correct THERE; here we prove
 * the ERROR path works when a real store IS present.
 */

import { describe, it, expect, beforeAll } from "vitest";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { runIngestPipeline } from "../../prose/ingest-pipeline.js";
import type { IngestPipelineResult } from "../../prose/ingest-pipeline.js";
import type { LlmProvider, CandidateProposal } from "../../prose/llm-provider.js";
import { Bm25Index } from "../../retrieval/bm25.js";
import { validatePremises } from "../../inference/engine.js";
import type { InferredComposedIR } from "@sysml-bridge/model";
import { proseVerbatimFindings } from "@sysml-bridge/gates";
import type { ProseApprovedEntry } from "@sysml-bridge/model";
import {
  chunkRecordsFromMap,
  writeChunkStoreFile,
  loadChunkStoreFile,
  chunkStoreTextMap,
  parseChunkStore,
  serializeChunkStore,
  CHUNK_STORE_SCHEMA,
  type ChunkStoreRecord,
} from "../index.js";

// ── Fixture document + deterministic mock provider ──────────────────────────

const DOC_TEXT =
  "The refuel controller shall complete boom contact within sixty seconds.\n\n" +
  "The fuel pump shall sustain a transfer rate of at least two hundred gallons per minute.\n\n" +
  "The receiver aircraft shall maintain formation station-keeping during transfer.";

const DOC_SHA = "a".repeat(64);
const SECTION_PATH = "root";
const CONTEXT = {
  documentHash: DOC_SHA,
  sectionId: "sec-root",
  sectionPath: SECTION_PATH,
  pageStart: 0,
  pageEnd: 0,
  documentId: "fixture-doc",
};

/** Emits one valid proposal per chunk whose quote is a REAL span of the chunk. */
class VerbatimMockProvider implements LlmProvider {
  async propose(chunkId: string, chunkText: string): Promise<CandidateProposal[]> {
    return [
      {
        kind: "requirement" as const,
        fields: { text: chunkText.slice(0, 60) },
        citedChunkId: chunkId,
        confidence: 0.5,
        quote: chunkText.slice(0, 40), // genuine verbatim span
      },
    ];
  }
}

// ── Shared run state ────────────────────────────────────────────────────────

let result: IngestPipelineResult;
let reloaded: ChunkStoreRecord[];
let chunksJsonPath: string;

beforeAll(async () => {
  result = await runIngestPipeline({
    text: DOC_TEXT,
    context: CONTEXT,
    provider: new VerbatimMockProvider(),
    chunkOptions: { chunkSize: 120, chunkOverlap: 20 },
  });

  const records = chunkRecordsFromMap(result.chunkStore, SECTION_PATH);
  const dir = await mkdtemp(join(tmpdir(), "chunk-store-"));
  chunksJsonPath = join(dir, "chunks.json");
  // Fixed timestamp → byte-stable file.
  await writeChunkStoreFile(chunksJsonPath, records, new Date("2026-07-14T00:00:00.000Z"));
  reloaded = await loadChunkStoreFile(chunksJsonPath);
});

// ── The store round-trips ───────────────────────────────────────────────────

describe("chunk-store persistence — round trip", () => {
  it("pipeline exposes a non-empty chunkStore that all resolve to chunk text", () => {
    expect(result.chunkStore.size).toBeGreaterThanOrEqual(2);
    expect(result.candidates.length).toBeGreaterThanOrEqual(2);
    expect(result.emittedUnverbatim).toBe(0); // C6 held during the run
  });

  it("reloaded records equal the persisted records (round-trips losslessly)", () => {
    expect(reloaded.length).toBe(result.chunkStore.size);
    for (const r of reloaded) {
      expect(r.sectionPath).toBe(SECTION_PATH);
      expect(result.chunkStore.get(r.chunkId)).toBe(r.text);
    }
  });

  it("the on-disk file carries the schema envelope", async () => {
    const raw = await readFile(chunksJsonPath, "utf8");
    const parsed = JSON.parse(raw);
    expect(parsed.schema).toBe(CHUNK_STORE_SCHEMA);
    expect(Array.isArray(parsed.chunks)).toBe(true);
    // serialize→parse is a pure identity on the records
    expect(parseChunkStore(serializeChunkStore(reloaded))).toEqual(reloaded);
  });

  it("rejects a malformed store loudly (no silent empty-store degrade)", () => {
    expect(() => parseChunkStore("{ not json")).toThrow();
    expect(() => parseChunkStore(JSON.stringify({ schema: "wrong", chunks: [] }))).toThrow(
      /unexpected schema/,
    );
    expect(() =>
      parseChunkStore(JSON.stringify({ schema: CHUNK_STORE_SCHEMA, chunks: [{ chunkId: 1 }] })),
    ).toThrow(/must be a string/);
  });
});

// ── (a) Gate path: PROSE-unverbatim-quote at ERROR level ────────────────────

describe("chunk-store persistence — feeds the PROSE-unverbatim-quote gate", () => {
  // Pick a real chunk to build both a verbatim-OK entry and a hallucinated one.
  function entriesForGate(): { good: ProseApprovedEntry; bad: ProseApprovedEntry; realChunkId: string } {
    const realChunkId = reloaded[0]!.chunkId;
    const realText = reloaded[0]!.text;
    const base = {
      kind: "requirement" as const,
      fields: {},
      approvedBy: "tester",
      approvedAt: "2026-07-14T00:00:00.000Z",
      status: "approved" as const,
    };
    const citation = (chunkId: string, quote: string) => ({
      docId: CONTEXT.documentId,
      docSha256: DOC_SHA,
      chunkId,
      sectionPath: SECTION_PATH,
      quote,
    });
    return {
      realChunkId,
      good: {
        ...base,
        id: "entry-good",
        candidateId: "entry-good",
        citation: citation(realChunkId, realText.slice(0, 30)), // verbatim span
      },
      bad: {
        ...base,
        id: "entry-bad",
        candidateId: "entry-bad",
        citation: citation(realChunkId, "this hallucinated span is not in the chunk at all"),
      },
    };
  }

  it("emits a PROSE-unverbatim-quote ERROR on a drifted quote against the loaded store", () => {
    const { good, bad } = entriesForGate();
    const map = chunkStoreTextMap(reloaded);
    const findings = proseVerbatimFindings([good, bad], map);

    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(1);
    expect(errors[0]!.ruleId).toBe("PROSE-unverbatim-quote");
    expect(errors[0]!.elementId).toBe("entry-bad");
    // The verbatim-OK entry produced no finding at all (not even a warning).
    expect(findings.some((f) => f.elementId === "entry-good")).toBe(false);
    // NOT the degrade path — no -unavailable warning when the store is present.
    expect(findings.some((f) => f.ruleId === "PROSE-unverbatim-quote-unavailable")).toBe(false);
  });

  it("paired control: with NO store the gate degrades to a warning (never vacuous)", () => {
    const { good, bad } = entriesForGate();
    const findings = proseVerbatimFindings([good, bad], undefined);
    // The exact same drifted entry produces zero errors and one degrade warning —
    // proving the ERROR above is real, and absence-of-store never silently passes.
    expect(findings.some((f) => f.severity === "error")).toBe(false);
    expect(findings.filter((f) => f.ruleId === "PROSE-unverbatim-quote-unavailable")).toHaveLength(1);
  });
});

// ── (b) Retrieval path: reloaded store feeds BM25 + premise resolution ───────

describe("chunk-store persistence — feeds BM25 evidence retrieval", () => {
  it("a reloaded chunk is retrievable by BM25 and citable as a resolvable premise", () => {
    const index = new Bm25Index(reloaded); // RetrievalChunk[] fed verbatim
    expect(index.size).toBe(reloaded.length);

    const hits = index.query("fuel pump transfer rate", 3);
    expect(hits.length).toBeGreaterThanOrEqual(1);
    const topId = hits[0]!.chunkId;
    expect(index.chunkIds().has(topId)).toBe(true);

    // Minimal IR with no corpus/prose/inferred ids of its own.
    const emptyIr = {
      extracted: {},
      proseEntries: [],
      inferredEntries: [],
      approvedProseIds: new Set<string>(),
      approvedInferredIds: new Set<string>(),
    } as unknown as InferredComposedIR;

    // Without the retrieved ids, citing the chunk is unpremised…
    expect(validatePremises([topId], emptyIr).valid).toBe(false);
    // …with the BM25 index's chunk ids admitted, the same premise resolves.
    expect(validatePremises([topId], emptyIr, index.chunkIds()).valid).toBe(true);
  });
});
