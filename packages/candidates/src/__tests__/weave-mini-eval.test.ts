/**
 * weave-mini-eval.test.ts — proof-of-recall eval (W4, spec §6, §8 Phase W4).
 *
 * The seeded-defect harness (examples/angars/pipeline/seeded-defects.ts)
 * proves the gates catch planted DEFECTS. This test proves the dual: the
 * weaver FINDS planted FACTS. It runs the full deterministic pipeline —
 * REAL parsers (md/docx/xlsx), REAL chunker, REAL mention derivation (via a
 * recorded fixture LLM provider, zero API key), REAL auto-cluster, REAL
 * merge suggestion, REAL cross-document co-occurrence + chain enumeration —
 * over the corpus in examples/weave-mini/, and asserts the result against
 * examples/weave-mini/answer-key.json by SPECIFIC pinned ids (never counts):
 *
 *   - every cross-document-aliased entity in the key either auto-clusters
 *     (identical spelling) or is proposed for merge (the acronym pair);
 *   - the trap pair (same surface form "Interlock", different kind) does
 *     NOT auto-cluster and is NEVER proposed for merge;
 *   - all three cross-document links (cooccurKind==="section" — no single
 *     document states either link on its own) are enumerated;
 *   - the one planted 2-hop chain is enumerated from two hand-authored
 *     already-accepted relations.
 *
 * Zero API key: mention derivation uses FixtureProvider (recorded
 * responses), never a live LLM call.
 */

import { describe, it, expect, beforeAll } from "vitest";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { runIngestPipeline } from "../prose/ingest-pipeline.js";
import { parseDocument } from "../prose/parsers/dispatch.js";
import type { CandidateProposal, LlmProvider } from "../prose/llm-provider.js";
import { quoteOccursInChunk } from "@sysml-bridge/model";
import { autoCluster, entityIdFor, suggestMerges } from "../entities/index.js";
import type { EntityRecord } from "../entities/index.js";
import {
  enumerateCooccurrence,
  enumerateChains,
  chainStableId,
  type AcceptedRelation,
} from "../inference/index.js";
import type { MentionRecord } from "../mentions/index.js";
import { entityMergePairKey } from "@sysml-bridge/model";

// Fixtures/answer key are DATA files (JSON/md/docx/xlsx) under
// examples/weave-mini/, referenced by path — not imported as TypeScript
// modules — so this test never pulls files outside the `src/` rootDir into
// the package's compiled build graph (mirrors how gc-real-run.test.ts /
// angars-corpus.test.ts reference examples/angars/corpus/*.pdf by path).
const WEAVE_MINI_DIR = join(import.meta.dirname, "../../../../examples/weave-mini");
const CORPUS_DIR = join(WEAVE_MINI_DIR, "corpus");
const ANSWER_KEY_PATH = join(WEAVE_MINI_DIR, "answer-key.json");
const FIXTURE_RESPONSES_PATH = join(WEAVE_MINI_DIR, "fixture-responses.json");

interface WeaveMiniDoc {
  file: string;
  documentId: string;
  sectionPath: string;
}

type FixtureProposal = Omit<CandidateProposal, "citedChunkId">;

interface FixtureResponses {
  docs: WeaveMiniDoc[];
  proposals: Record<string, FixtureProposal[]>;
}

/**
 * A recorded-response `LlmProvider`: returns the fixed proposal batch for
 * this document (from fixture-responses.json), with `citedChunkId` stamped
 * from whatever chunk id the pipeline actually assigned. Zero API key, zero
 * network I/O — see fixture-responses.json's schema doc-comment.
 */
class FixtureProvider implements LlmProvider {
  constructor(
    private readonly documentId: string,
    private readonly proposalsByDoc: Record<string, FixtureProposal[]>,
  ) {}

  async propose(chunkId: string): Promise<CandidateProposal[]> {
    const batch = this.proposalsByDoc[this.documentId] ?? [];
    return batch.map((p) => ({ ...p, citedChunkId: chunkId }));
  }
}

interface AnswerKey {
  corpus: Array<{ file: string; documentId: string; sectionPath: string; chunkId: string }>;
  entities: {
    crossDocumentExactAlias: Array<{ entityId: string; kind: string; canonicalName: string; mentionDocIds: string[] }>;
    acronymPair: {
      full: { entityId: string; kind: string; canonicalName: string };
      acronym: { entityId: string; kind: string; canonicalName: string };
      expectedMergeProposalId: string;
      expectedMergeReason: string;
    };
  };
  trap: {
    surfaceForm: string;
    entityA: { entityId: string; kind: string };
    entityB: { entityId: string; kind: string };
  };
  crossDocumentLinks: {
    links: Array<{
      id: string;
      family: "allocation" | "modeMembership" | "flowTyping" | "controlJoin";
      sourceId: string;
      targetId: string;
      expectedCooccurKind: string;
    }>;
  };
  chain: {
    id: string;
    leftFamily: string;
    rightFamily: string;
    sourceId: string;
    middleId: string;
    targetId: string;
    resultFamily: string;
    acceptedRelations: AcceptedRelation[];
  };
}

let answerKey: AnswerKey;
let fixtures: FixtureResponses;
let allMentions: MentionRecord[];
let totalDroppedUnverbatimMentions = 0;
let entities: EntityRecord[];
let merges: ReturnType<typeof suggestMerges>;
let cooccurrence: ReturnType<typeof enumerateCooccurrence>;
let chains: ReturnType<typeof enumerateChains>;
let chunkIdByDoc: Record<string, string>;

beforeAll(async () => {
  answerKey = JSON.parse(await readFile(ANSWER_KEY_PATH, "utf8")) as AnswerKey;
  fixtures = JSON.parse(await readFile(FIXTURE_RESPONSES_PATH, "utf8")) as FixtureResponses;

  allMentions = [];
  chunkIdByDoc = {};

  for (const d of fixtures.docs) {
    const filePath = join(CORPUS_DIR, d.file);
    const raw = await readFile(filePath);
    const docSha256 = createHash("sha256").update(raw).digest("hex");
    const parsed = await parseDocument(filePath);
    const provider = new FixtureProvider(d.documentId, fixtures.proposals);

    const result = await runIngestPipeline({
      text: parsed.text,
      context: {
        documentHash: docSha256,
        sectionId: "sec-root",
        sectionPath: d.sectionPath,
        pageStart: 0,
        pageEnd: Math.max(parsed.pages.length - 1, 0),
        documentId: d.documentId,
      },
      provider,
      // Large chunkSize: every weave-mini corpus doc is small enough to
      // parse+chunk into exactly ONE chunk — asserted below.
      chunkOptions: { chunkSize: 20_000, chunkOverlap: 0 },
    });

    expect(result.totalChunks).toBe(1);
    expect(result.processedChunks).toBe(1); // C5: exactly once per chunk
    expect(result.emittedUncited).toBe(0); // C4
    expect(result.emittedUnverbatim).toBe(0); // C6
    // Every fixture proposal for this doc must have survived the citation +
    // verbatim gates — a fail-able positive control: if a quote in
    // fixture-responses.json ever drifts from the corpus text, this fails.
    expect(result.candidates.length).toBe(fixtures.proposals[d.documentId]?.length ?? 0);

    totalDroppedUnverbatimMentions += result.droppedUnverbatimMentions;
    allMentions.push(...result.mentions);
    const chunkId = [...result.chunkStore.keys()][0];
    expect(chunkId).toBeDefined();
    chunkIdByDoc[d.documentId] = chunkId!;
  }

  entities = autoCluster(allMentions);
  merges = suggestMerges(entities);
  cooccurrence = enumerateCooccurrence(entities, allMentions, {
    families: ["allocation", "modeMembership"],
  });

  // r1 (allocation) is evidenced by doc1 (system-overview.md), r2
  // (containment) by doc2 (subsystem-spec.docx) — both hand-authored
  // corpus-backed facts for this eval, per answer-key.json's chain section.
  const evidenceChunksByRelationIndex = [chunkIdByDoc["weave-mini-overview"]!, chunkIdByDoc["weave-mini-subsystem"]!];
  const accepted: AcceptedRelation[] = answerKey.chain.acceptedRelations.map((r, i) => ({
    ...r,
    evidenceChunkIds: [evidenceChunksByRelationIndex[i]!],
  }));
  chains = enumerateChains(accepted);
});

// ── Parsing: all 3 real formats, real parsers, zero pre-extracted text ───────

// Synchronous, module-scope read — ONLY used to build the static list of
// `it()` cases below (Vitest collects `describe` bodies synchronously,
// before `beforeAll` has run). All assertions still read the async-loaded
// `answerKey` / `fixtures` populated in `beforeAll`.
const corpusListForCollection = (
  JSON.parse(readFileSync(ANSWER_KEY_PATH, "utf8")) as AnswerKey
).corpus;

describe("weave-mini corpus: real parsers, one chunk per document (pinned chunk ids)", () => {
  for (const d of corpusListForCollection) {
    it(`${d.file} parses via the real dispatch parser and chunks to the pinned chunk id`, async () => {
      const expected = answerKey.corpus.find((c) => c.documentId === d.documentId);
      expect(expected).toBeDefined();
      expect(chunkIdByDoc[d.documentId]).toBe(expected!.chunkId);
    });
  }
});

// ── Positive control: verbatim gate can actually fail ────────────────────────

describe("positive control — the verbatim gate is not vacuous", () => {
  it("a quote NOT present in the chunk text is rejected by quoteOccursInChunk", () => {
    expect(quoteOccursInChunk("this sentence was never written", "The Cargo Handling Controller coordinates all cargo handling activity.")).toBe(false);
  });
  it("droppedUnverbatimMentions is 0 for the real (correct) fixtures", () => {
    expect(totalDroppedUnverbatimMentions).toBe(0);
  });
});

// ── Entities: cross-document exact alias -> auto-cluster ─────────────────────

describe("W1 auto-cluster recall — cross-document EXACT-spelling aliases", () => {
  it("every crossDocumentExactAlias entity in the answer key auto-clusters to its pinned entityId, spanning all its documents", () => {
    for (const expected of answerKey.entities.crossDocumentExactAlias) {
      const found = entities.find((e) => e.entityId === expected.entityId);
      expect(found, `expected entity ${expected.entityId} (${expected.canonicalName})`).toBeDefined();
      expect(found!.kind).toBe(expected.kind);
      expect(found!.canonicalName).toBe(expected.canonicalName);

      const docIds = new Set(
        found!.mentionIds.map((mid) => allMentions.find((m) => m.mentionId === mid)?.citation.docId),
      );
      for (const docId of expected.mentionDocIds) {
        expect(docIds.has(docId), `entity ${expected.entityId} missing mention from doc ${docId}`).toBe(true);
      }
    }
  });

  it("entityIdFor(kind, surface) is order-independent and matches autoCluster's minted id (regression control)", () => {
    const { full, acronym } = answerKey.entities.acronymPair;
    expect(entityIdFor(full.kind as never, full.canonicalName)).toBe(full.entityId);
    expect(entityIdFor(acronym.kind as never, acronym.canonicalName)).toBe(acronym.entityId);
  });
});

// ── Entities: acronym pair -> NOT auto-clustered, IS proposed for merge ──────

describe("W1 merge-suggestion recall — the acronym pair", () => {
  it("the acronym entity ('CHC') does NOT auto-cluster with the full-name entity (different normSurface)", () => {
    const { full, acronym } = answerKey.entities.acronymPair;
    expect(full.entityId).not.toBe(acronym.entityId);
    expect(entities.some((e) => e.entityId === acronym.entityId)).toBe(true);
    expect(entities.some((e) => e.entityId === full.entityId)).toBe(true);
  });

  it("suggestMerges proposes the acronym pair at the pinned content-addressed id", () => {
    const { expectedMergeProposalId, expectedMergeReason, full, acronym } = answerKey.entities.acronymPair;
    expect(entityMergePairKey(full.entityId, acronym.entityId)).toBe(expectedMergeProposalId);
    const found = merges.find((m) => m.id === expectedMergeProposalId);
    expect(found, `expected merge proposal ${expectedMergeProposalId}`).toBeDefined();
    expect(found!.reason).toBe(expectedMergeReason);
  });
});

// ── Trap: same surface form, different kind -> never merges ──────────────────

describe("W1 trap — same surface form, different kind, must NOT merge", () => {
  it("autoCluster keeps the trap pair as two DISTINCT entities", () => {
    const { entityA, entityB } = answerKey.trap;
    expect(entityA.entityId).not.toBe(entityB.entityId);
    const a = entities.find((e) => e.entityId === entityA.entityId);
    const b = entities.find((e) => e.entityId === entityB.entityId);
    expect(a, "trap entity A (mode)").toBeDefined();
    expect(b, "trap entity B (component)").toBeDefined();
    expect(a!.kind).toBe(entityA.kind);
    expect(b!.kind).toBe(entityB.kind);
    expect(a!.canonicalName).toBe(answerKey.trap.surfaceForm);
    expect(b!.canonicalName).toBe(answerKey.trap.surfaceForm);
  });

  it("suggestMerges NEVER proposes a merge for the trap pair (fail-able control: the acronym pair above DOES get proposed)", () => {
    const { entityA, entityB } = answerKey.trap;
    const trapPairKey = entityMergePairKey(entityA.entityId, entityB.entityId);
    expect(merges.some((m) => m.id === trapPairKey)).toBe(false);
    // Every actually-emitted proposal must be the acronym pair — no other
    // proposal (in particular not the trap) was generated.
    expect(merges.map((m) => m.id)).toEqual([answerKey.entities.acronymPair.expectedMergeProposalId]);
  });
});

// ── Cross-document links: co-occurrence stated in NO single document ────────

describe("W2 co-occurrence recall — links stated in no single document", () => {
  it("every answer-key cross-document link is enumerated at its pinned id, with cooccurKind='section' (no shared chunk)", () => {
    for (const link of answerKey.crossDocumentLinks.links) {
      const found = cooccurrence.candidates.find((c) => c.id === link.id);
      expect(found, `expected co-occurrence candidate ${link.id}`).toBeDefined();
      expect(found!.relationFamily).toBe(link.family);
      expect(found!.sourceId).toBe(link.sourceId);
      expect(found!.targetId).toBe(link.targetId);
      // 'section' (not 'chunk' or 'chunk+section') proves the signal came
      // SOLELY from cross-document section-path nesting — no single
      // document's chunk mentions both endpoints together.
      expect(found!.cooccurKind).toBe(link.expectedCooccurKind);
    }
  });
});

// ── 2-hop chain ────────────────────────────────────────────────────────────

describe("W2 chain recall — the planted 2-hop chain", () => {
  it("enumerateChains composes the two accepted relations into the pinned chain id", () => {
    const expectedId = chainStableId(
      answerKey.chain.leftFamily,
      answerKey.chain.rightFamily,
      answerKey.chain.sourceId,
      answerKey.chain.middleId,
      answerKey.chain.targetId,
    );
    expect(expectedId).toBe(answerKey.chain.id);

    const found = chains.candidates.find((c) => c.stableId === answerKey.chain.id);
    expect(found, `expected chain ${answerKey.chain.id}`).toBeDefined();
    expect(found!.sourceId).toBe(answerKey.chain.sourceId);
    expect(found!.middleId).toBe(answerKey.chain.middleId);
    expect(found!.targetId).toBe(answerKey.chain.targetId);
    expect(found!.leftFamily).toBe(answerKey.chain.leftFamily);
    expect(found!.rightFamily).toBe(answerKey.chain.rightFamily);
  });

  it("feeding only PENDING relations yields zero chains (fail-able control)", () => {
    const pending: AcceptedRelation[] = answerKey.chain.acceptedRelations.map((r) => ({
      ...r,
      status: "pending",
    }));
    const result = enumerateChains(pending);
    expect(result.candidates).toHaveLength(0);
    expect(result.pendingSkipped).toBe(pending.length);
  });
});
