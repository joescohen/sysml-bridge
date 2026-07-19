/**
 * retrieval-context.test.ts — BM25 evidence retrieval wired into the context
 * bundle + engine premise resolution.
 *
 * Covers:
 *   R1 — no chunk store → bundle falls back to current behavior (no retrieved
 *        evidence, offeredFacts identical to the pre-retrieval bundle)
 *   R2 — empty chunk store → graceful no-op (same as R1)
 *   R3 — with a chunk store, retrieved passages are merged into the bundle as
 *        clearly-labeled evidence AND as citable `kind: "chunk"` offered facts
 *   R4 — determinism: same IR + same chunk store → byte-identical bundles
 *   R5 — end-to-end: a proposal citing a retrieved chunkId RESOLVES and is queued
 *        (fail-able positive control: citing a chunkId NOT in the store drops it)
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR } from "@sysml-bridge/model";
import { SCHEMA_VERSION } from "@sysml-bridge/model";
import { buildContextBundle } from "../neighborhood.js";
import { buildProposeUserMessage } from "../inference-provider.js";
import { runInferenceEngine } from "../engine.js";
import type { InferenceProvider } from "../inference-provider.js";
import type { ProposeResult, ContextBundle, QueuedRecord } from "../types.js";
import { Bm25Index } from "../../retrieval/bm25.js";
import type { RetrievalChunk } from "../../retrieval/bm25.js";

// ── Fixture IR ────────────────────────────────────────────────────────────────

const LEAF_FN_ID = "function-leaf-001";
const LEAF_FN_ID_2 = "function-leaf-002";
const COMP_ID = "component-comp-001";
const CORPUS_REQ_ID = "requirement-req-001";

function makeIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "TestSub",
    needs: [],
    requirements: [
      {
        id: CORPUS_REQ_ID,
        kind: "requirement",
        naturalKey: "CC-1",
        name: "Authenticate Refueling Requests",
        statement: "The system shall authenticate refueling requests.",
        needIds: [],
      },
    ],
    functions: [
      {
        id: LEAF_FN_ID,
        kind: "function",
        naturalKey: "F1.1",
        name: "Receive Authenticate Request",
        level: "L3",
        owner: "F1: Manage Refueling Requests",
      },
      {
        id: LEAF_FN_ID_2,
        kind: "function",
        naturalKey: "F1.2",
        name: "Validate Fuel Capacity",
        level: "L3",
        owner: "F1: Manage Refueling Requests",
      },
    ],
    components: [
      { id: COMP_ID, kind: "component", naturalKey: "Operator Console Module", name: "Operator Console Module" },
    ],
    satisfies: [{ reqId: CORPUS_REQ_ID, functionId: LEAF_FN_ID }],
    allocations: [],
    subsystems: [
      {
        id: "subsystem-001",
        kind: "subsystem",
        naturalKey: "Command Control Subsystem",
        name: "Command Control Subsystem",
        componentIds: [COMP_ID],
        provenance: { workbook: "t.xlsx", sheet: "S" },
      },
    ],
    n2Interfaces: [],
    kpps: [],
    behaviorDecomp: [],
  };

  return {
    extracted: extracted as any,
    proseEntries: [],
    approvedProseIds: new Set(),
    inferredEntries: [],
    approvedInferredIds: new Set(),
  };
}

// A chunk whose text contains source + target query tokens so BM25 retrieves it.
const EVIDENCE_CHUNK_ID = "abc123def456abc123def456abc12300";
const chunkStore: RetrievalChunk[] = [
  {
    chunkId: EVIDENCE_CHUNK_ID,
    sectionPath: "3.4 Command & Control",
    text: "The Operator Console Module performs receive authenticate request handling and forwards authenticated requests downstream.",
  },
  {
    chunkId: "0000feed0000feed0000feed0000fe00",
    sectionPath: "9.9 Unrelated",
    text: "Ambient thermal envelope and vibration tolerance of the ground segment.",
  },
];

// ── R1 — no chunk store: fallback ─────────────────────────────────────────────

describe("R1 — no chunk store → current behavior", () => {
  it("bundle has no retrievedEvidence and offeredFacts match the pre-retrieval bundle", () => {
    const ir = makeIR();
    const bundle = buildContextBundle(LEAF_FN_ID, COMP_ID, ir);
    expect(bundle.retrievedEvidence).toBeUndefined();
    // No `chunk`-kind facts appear.
    expect(bundle.offeredFacts.some((f) => f.kind === "chunk")).toBe(false);
  });
});

// ── R2 — empty chunk store: graceful ──────────────────────────────────────────

describe("R2 — empty chunk store → graceful no-op", () => {
  it("empty index yields no retrieved evidence, identical to no-index bundle", () => {
    const ir = makeIR();
    const emptyIndex = new Bm25Index([]);
    const withEmpty = buildContextBundle(LEAF_FN_ID, COMP_ID, ir, emptyIndex);
    const withNone = buildContextBundle(LEAF_FN_ID, COMP_ID, ir);
    expect(withEmpty.retrievedEvidence).toBeUndefined();
    expect(JSON.stringify(withEmpty)).toBe(JSON.stringify(withNone));
  });
});

// ── R3 — retrieved evidence merged + citable ──────────────────────────────────

describe("R3 — retrieved evidence is merged as labeled evidence + citable facts", () => {
  it("retrieved chunk appears in retrievedEvidence and as a kind:'chunk' offered fact", () => {
    const ir = makeIR();
    const index = new Bm25Index(chunkStore);
    const bundle = buildContextBundle(LEAF_FN_ID, COMP_ID, ir, index);

    expect(bundle.retrievedEvidence!.length).toBeGreaterThan(0);
    const hit = bundle.retrievedEvidence!.find((h) => h.chunkId === EVIDENCE_CHUNK_ID);
    expect(hit).toBeDefined();
    expect(hit!.sectionPath).toBe("3.4 Command & Control");

    // Offered as a citable fact (id = chunkId), so a premise can resolve to it.
    const fact = bundle.offeredFacts.find((f) => f.id === EVIDENCE_CHUNK_ID);
    expect(fact).toBeDefined();
    expect(fact!.kind).toBe("chunk");

    // The propose message renders it under the RETRIEVED EVIDENCE section with its id.
    const msg = buildProposeUserMessage("allocation", LEAF_FN_ID, COMP_ID, bundle);
    expect(msg).toContain("RETRIEVED EVIDENCE");
    expect(msg).toContain(`[id: ${EVIDENCE_CHUNK_ID}]`);
    // The existing exact-id quotes are KEPT (offered facts still carry IR facts).
    expect(bundle.offeredFacts.some((f) => f.id === LEAF_FN_ID)).toBe(true);
    expect(bundle.offeredFacts.some((f) => f.id === COMP_ID)).toBe(true);
  });
});

// ── R4 — determinism ──────────────────────────────────────────────────────────

describe("R4 — same IR + same chunk store → byte-identical bundle", () => {
  it("two independent builds produce identical JSON", () => {
    const a = buildContextBundle(LEAF_FN_ID, COMP_ID, makeIR(), new Bm25Index(chunkStore));
    const b = buildContextBundle(LEAF_FN_ID, COMP_ID, makeIR(), new Bm25Index([...chunkStore]));
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});

// ── R5 — end-to-end premise resolution ────────────────────────────────────────

describe("R5 — a premise citing a retrieved chunkId resolves through the engine", () => {
  // Proposes for any candidate sourced at LEAF_FN_ID (a controlJoin sibling pair
  // survives the relevance filter), citing a single chunk id as its only premise.
  class ChunkCitingProvider implements InferenceProvider {
    constructor(private readonly citedId: string) {}
    async propose(
      family: Parameters<InferenceProvider["propose"]>[0],
      sourceId: string,
      targetId: string,
      _context: ContextBundle
    ): Promise<ProposeResult> {
      if (sourceId !== LEAF_FN_ID) return { kind: "declined" };
      return {
        kind: "proposal",
        proposal: {
          sourceId,
          targetId,
          relationFamily: family,
          premises: [this.citedId],
          rationale: "audit-only",
          confidence: 0.9,
        },
      };
    }
    async advocate() {
      return { score: 0.8, summary: "n/a" };
    }
    async challenge() {
      return { score: 0.3, summary: "n/a" };
    }
  }

  it("citing a chunkId IN the store → queued (not dropped)", async () => {
    const result = await runInferenceEngine(
      makeIR(),
      new ChunkCitingProvider(EVIDENCE_CHUNK_ID),
      { chunkStore, log: () => {} }
    );
    expect(result.droppedUnpremised).toBe(0);
    expect(result.emittedUnpremised).toBe(0);
    const queued = result.records.filter(
      (r): r is QueuedRecord => r.stage === "queued" && r.sourceId === LEAF_FN_ID
    );
    expect(queued.length).toBeGreaterThan(0);
    for (const q of queued) expect(q.premises).toEqual([EVIDENCE_CHUNK_ID]);
  });

  it("fail-able positive control: citing a chunkId NOT in the store → dropped_unpremised", async () => {
    const result = await runInferenceEngine(
      makeIR(),
      new ChunkCitingProvider("deadbeefdeadbeefdeadbeefdeadbe00"),
      { chunkStore, log: () => {} }
    );
    expect(result.droppedUnpremised).toBeGreaterThan(0);
    const queuedForSource = result.records.filter(
      (r) => r.stage === "queued" && r.sourceId === LEAF_FN_ID
    );
    expect(queuedForSource.length).toBe(0);
  });
});
