/**
 * equivalence.test.ts — the review UI writes byte-compatible disposition
 * records with the /mbse-approve skill path.
 *
 * The plan's three proofs:
 *
 * (a) SHARED-SCHEMA: start createReviewServer on an ephemeral port against tmp
 *     candidate/disposition dirs with small fixture candidate files; POST
 *     approve + reject for BOTH layers over HTTP; parse every written
 *     disposition file with the SAME zod schemas the skill path uses
 *     (ProseApprovedEntrySchema / InferredApprovedEntrySchema + the rejection
 *     shapes) — all parse, statuses correct.
 *
 * (b) EQUIVALENCE: approve candidate X via the HTTP endpoint and the SAME
 *     candidate content via direct appendApproval / appendInferredApproval into
 *     a second tmp dir. The two entries are identical except approvedAt /
 *     approvedBy — assert field-level equality on { id, kind, fields, citation,
 *     candidateId, status } (prose) and the inference analog. The stable id
 *     MUST match: prose ids are content-addressed from the citation; inference
 *     ids are the candidate id. composeProseTwoLayer over each dir yields the
 *     same composed entry modulo approvedAt / approvedBy.
 *
 * (c) READ-ONLY GETs: hash the dispositions dir before/after GET / + GET
 *     /api/state (identical — no GET mutates); an unknown endpoint 404s.
 *
 * The server binds to port 0 (ephemeral) and we read the actual listen address
 * back from server.address() — never a hardcoded port.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { promises as fsp } from "node:fs";
import * as crypto from "node:crypto";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";

import {
  appendApproval,
  appendInferredApproval,
  appendEntityMerge,
  entityMergePairKey,
  composeProseTwoLayer,
  ProseApprovedEntrySchema,
  InferredApprovedEntrySchema,
  EntityMergeApprovedEntrySchema,
  type ProseApprovedEntry,
  type InferredApprovedEntry,
  type EntityMergeApprovedEntry,
} from "@sysml-bridge/model";

import { createReviewServer } from "../server.js";
import {
  DISPOSITION_FILES,
  buildState,
  findProseCandidate,
  findInferenceCandidate,
  findEntityCandidate,
} from "../candidates.js";

// ---------------------------------------------------------------------------
// Disposition-file parsers built from the SAME schemas the skill path validates
// against — the approved entry schemas come straight from @sysml-bridge/model
// (no zod import here → no new dependency for review-ui). A malformed file makes
// `.parse` throw, so every parseXxx call is itself an assertion.
// ---------------------------------------------------------------------------

function parseProseApprovedFile(raw: unknown): { entries: ProseApprovedEntry[] } {
  const entries = (raw as { entries?: unknown[] }).entries ?? [];
  return { entries: entries.map((e) => ProseApprovedEntrySchema.parse(e)) };
}

function parseInferredApprovedFile(raw: unknown): { entries: InferredApprovedEntry[] } {
  const entries = (raw as { entries?: unknown[] }).entries ?? [];
  return { entries: entries.map((e) => InferredApprovedEntrySchema.parse(e)) };
}

function parseEntityApprovedFile(raw: unknown): { entries: EntityMergeApprovedEntry[] } {
  const entries = (raw as { entries?: unknown[] }).entries ?? [];
  return { entries: entries.map((e) => EntityMergeApprovedEntrySchema.parse(e)) };
}

/** The entity rejection-file shape { rejectedPairKeys: string[] } — asserted plainly. */
function parseEntityRejectionsFile(raw: unknown): { rejectedPairKeys: string[] } {
  const keys = (raw as { rejectedPairKeys?: unknown }).rejectedPairKeys;
  expect(Array.isArray(keys)).toBe(true);
  for (const k of keys as unknown[]) expect(typeof k).toBe("string");
  return { rejectedPairKeys: keys as string[] };
}

/** The on-disk rejection-file shape { rejectedIds: string[] } — asserted plainly. */
function parseRejectionsFile(raw: unknown): { rejectedIds: string[] } {
  const ids = (raw as { rejectedIds?: unknown }).rejectedIds;
  expect(Array.isArray(ids)).toBe(true);
  for (const id of ids as unknown[]) expect(typeof id).toBe("string");
  return { rejectedIds: ids as string[] };
}

// ---------------------------------------------------------------------------
// Fixtures — minimal WRAPPED candidate files matching the shapes candidates.ts
// expects: prose = { candidates: [...] }; inference = { irHash, records: [...] }
// where a reviewable record carries premises + confidence + rationale.
// ---------------------------------------------------------------------------

const IR_HASH = "testirhash01";

// Two prose candidates so we can approve one and reject the other.
const PROSE_APPROVE_ID = "prose-candidate-approve";
const PROSE_REJECT_ID = "prose-candidate-reject";

function proseCandidateFixture(id: string, quote: string) {
  return {
    id,
    kind: "requirement" as const,
    fields: { name: "Fixture Req", statement: "The system shall be fixture-grounded." },
    citation: {
      docId: "fixture-doc",
      docSha256: "a".repeat(64),
      chunkId: "chunk-fixture-1",
      sectionPath: "1.2.3",
      quote,
    },
    source: "llm",
    candidateId: id,
  };
}

// Two inference records so we can approve one and reject the other. Reviewable
// records carry premises + confidence + rationale (candidates.ts drops the rest).
const INF_APPROVE_ID = "infer-approve";
const INF_REJECT_ID = "infer-reject";

function inferenceRecordFixture(id: string, targetId: string) {
  return {
    id,
    relationFamily: "allocation" as const,
    sourceId: "function-fixture-a",
    targetId,
    stage: "queued",
    premises: ["requirement-fixture-1"],
    rationale: "fixture rationale (audit-only)",
    confidence: 0.77,
  };
}

// Two entity-merge candidates so we can approve one and reject the other. The
// candidate id IS the content-addressed pair key (== entityMergePairKey(...)).
const ENT_A = "entity-alpha";
const ENT_B = "entity-beta";
const ENT_C = "entity-gamma";
const ENT_APPROVE_KEY = entityMergePairKey(ENT_A, ENT_B);
const ENT_REJECT_KEY = entityMergePairKey(ENT_A, ENT_C);

function entityCandidateFixture(entityIdA: string, entityIdB: string) {
  return {
    id: entityMergePairKey(entityIdA, entityIdB),
    entityIdA,
    entityIdB,
    kind: "component" as const,
    canonicalName: "Fuel Control Module",
    aliases: ["Fuel Control Module", "FCM"],
    mentionIds: ["mention-fixture-1", "mention-fixture-2"],
    reason: "acronym" as const,
    evidence: { aQuotes: ["FCM commands the boom"], bQuotes: ["the Flight Control Module"] },
    confidence: 0.6,
  };
}

async function writeFixtureCandidates(candidatesDir: string): Promise<void> {
  await fsp.mkdir(candidatesDir, { recursive: true });
  await fsp.writeFile(
    path.join(candidatesDir, "prose-candidates.json"),
    JSON.stringify({
      generatedAt: "2026-07-07T00:00:00.000Z",
      candidates: [
        proseCandidateFixture(PROSE_APPROVE_ID, "the system shall be fixture-grounded"),
        proseCandidateFixture(PROSE_REJECT_ID, "a distinct quote that will be rejected"),
      ],
    })
  );
  await fsp.writeFile(
    path.join(candidatesDir, "inference-candidates.json"),
    JSON.stringify({
      generatedAt: "2026-07-07T00:00:00.000Z",
      irHash: IR_HASH,
      records: [
        inferenceRecordFixture(INF_APPROVE_ID, "component-fixture-a"),
        inferenceRecordFixture(INF_REJECT_ID, "component-fixture-b"),
      ],
    })
  );
  await fsp.writeFile(
    path.join(candidatesDir, "entity-candidates.json"),
    JSON.stringify({
      generatedAt: "2026-07-07T00:00:00.000Z",
      proposals: [entityCandidateFixture(ENT_A, ENT_B), entityCandidateFixture(ENT_A, ENT_C)],
    })
  );
}

// ---------------------------------------------------------------------------
// Server + HTTP harness — bind port 0, read the real address back.
// ---------------------------------------------------------------------------

interface Harness {
  server: http.Server;
  baseUrl: string;
  candidatesDir: string;
  dispositionsDir: string;
  root: string;
}

async function startHarness(): Promise<Harness> {
  const root = await fsp.mkdtemp(path.join(os.tmpdir(), "review-ui-equiv-"));
  const candidatesDir = path.join(root, "candidates");
  const dispositionsDir = path.join(root, "dispositions");
  await writeFixtureCandidates(candidatesDir);

  const server = createReviewServer({ candidatesDir, dispositionsDir });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    throw new Error("server did not bind an ephemeral TCP port");
  }
  const baseUrl = `http://127.0.0.1:${addr.port}`;
  return { server, baseUrl, candidatesDir, dispositionsDir, root };
}

async function stopHarness(h: Harness): Promise<void> {
  await new Promise<void>((resolve, reject) =>
    h.server.close((err) => (err ? reject(err) : resolve()))
  );
  await fsp.rm(h.root, { recursive: true, force: true });
}

async function post(
  baseUrl: string,
  route: string,
  body: unknown
): Promise<{ status: number; json: unknown }> {
  const res = await fetch(`${baseUrl}${route}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({}));
  return { status: res.status, json };
}

/** Deterministic hash of every file in a dir (name + bytes) — {} if absent. */
async function hashDir(dir: string): Promise<string> {
  let names: string[];
  try {
    names = (await fsp.readdir(dir)).sort();
  } catch {
    return "EMPTY";
  }
  const h = crypto.createHash("sha256");
  for (const name of names) {
    h.update(name);
    h.update("\0");
    h.update(await fsp.readFile(path.join(dir, name)));
    h.update("\0");
  }
  return h.digest("hex");
}

async function readJson(file: string): Promise<unknown> {
  return JSON.parse(await fsp.readFile(file, "utf8"));
}

// ---------------------------------------------------------------------------
// (a) SHARED-SCHEMA — HTTP-written dispositions parse with the skill's schemas
// ---------------------------------------------------------------------------

describe("(a) shared-schema — UI dispositions validate against the skill's zod schemas", () => {
  let h: Harness;
  beforeEach(async () => {
    h = await startHarness();
  });
  afterEach(async () => {
    await stopHarness(h);
  });

  it("approve + reject on both layers write files that parse with the same schemas", async () => {
    // Approve one + reject one on each layer, all over HTTP.
    const pApprove = await post(h.baseUrl, "/api/approve", {
      layer: "prose",
      candidateId: PROSE_APPROVE_ID,
    });
    const pReject = await post(h.baseUrl, "/api/reject", {
      layer: "prose",
      candidateId: PROSE_REJECT_ID,
    });
    const iApprove = await post(h.baseUrl, "/api/approve", {
      layer: "inference",
      candidateId: INF_APPROVE_ID,
    });
    const iReject = await post(h.baseUrl, "/api/reject", {
      layer: "inference",
      candidateId: INF_REJECT_ID,
    });

    expect(pApprove.status).toBe(200);
    expect(pReject.status).toBe(200);
    expect(iApprove.status).toBe(200);
    expect(iReject.status).toBe(200);

    // --- Prose approved parses; status + candidateId correct ---
    const proseApprovedFile = parseProseApprovedFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.proseApproved))
    );
    expect(proseApprovedFile.entries).toHaveLength(1);
    expect(proseApprovedFile.entries[0].status).toBe("approved");
    expect(proseApprovedFile.entries[0].candidateId).toBe(PROSE_APPROVE_ID);

    // --- Prose rejections parses; the rejected id is present ---
    const proseRej = parseRejectionsFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.proseRejections))
    );
    expect(proseRej.rejectedIds).toContain(PROSE_REJECT_ID);
    expect(proseRej.rejectedIds).not.toContain(PROSE_APPROVE_ID);

    // --- Inferred approved parses; status + id correct ---
    const infApprovedFile = parseInferredApprovedFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.inferredApproved))
    );
    expect(infApprovedFile.entries).toHaveLength(1);
    expect(infApprovedFile.entries[0].status).toBe("approved");
    expect(infApprovedFile.entries[0].id).toBe(INF_APPROVE_ID);

    // --- Inferred rejections parses; the rejected id is present ---
    const infRej = parseRejectionsFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.inferredRejections))
    );
    expect(infRej.rejectedIds).toContain(INF_REJECT_ID);
    expect(infRej.rejectedIds).not.toContain(INF_APPROVE_ID);
  });
});

// ---------------------------------------------------------------------------
// (b) EQUIVALENCE — HTTP approval === direct-helper approval (modulo timestamps)
// ---------------------------------------------------------------------------

describe("(b) equivalence — HTTP approval is byte-compatible with direct appendApproval", () => {
  let h: Harness;
  beforeEach(async () => {
    h = await startHarness();
  });
  afterEach(async () => {
    await stopHarness(h);
  });

  it("prose: same content approved via HTTP and via helper yields the same stable entry", async () => {
    // --- HTTP path ---
    const httpRes = await post(h.baseUrl, "/api/approve", {
      layer: "prose",
      candidateId: PROSE_APPROVE_ID,
    });
    expect(httpRes.status).toBe(200);
    const httpFile = parseProseApprovedFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.proseApproved))
    );
    expect(httpFile.entries).toHaveLength(1);
    const httpEntry = httpFile.entries[0];

    // --- Direct-helper path into a SECOND tmp dir, using the SAME candidate
    //     content the server would have loaded for that id. ---
    const helperDir = path.join(h.root, "helper-prose");
    await fsp.mkdir(helperDir, { recursive: true });
    const candidate = await findProseCandidate(h.candidatesDir, PROSE_APPROVE_ID);
    expect(candidate).toBeDefined();
    const helperApprovedPath = path.join(helperDir, DISPOSITION_FILES.proseApproved);
    const helperRejectionsPath = path.join(helperDir, DISPOSITION_FILES.proseRejections);
    const helperEntry = await appendApproval(
      candidate!,
      "someone-else",
      helperApprovedPath,
      helperRejectionsPath
    );

    // --- Field-level equality on the content-addressed fields ---
    expect(httpEntry.id).toBe(helperEntry.id); // content-addressed — MUST match
    expect(httpEntry.kind).toBe(helperEntry.kind);
    expect(httpEntry.fields).toEqual(helperEntry.fields);
    expect(httpEntry.citation).toEqual(helperEntry.citation);
    expect(httpEntry.candidateId).toBe(helperEntry.candidateId);
    expect(httpEntry.status).toBe(helperEntry.status);
    // The two fields that MAY differ:
    expect(httpEntry.approvedBy).not.toBe(helperEntry.approvedBy);

    // --- composeProseTwoLayer over each dir yields the same composed entry
    //     modulo approvedAt / approvedBy. ---
    const extractedPath = await writeMinimalExtracted(h.root);
    const httpComposed = await composeProseTwoLayer(
      extractedPath,
      path.join(h.dispositionsDir, DISPOSITION_FILES.proseApproved)
    );
    const helperComposed = await composeProseTwoLayer(extractedPath, helperApprovedPath);
    expect(httpComposed.proseEntries).toHaveLength(1);
    expect(helperComposed.proseEntries).toHaveLength(1);
    expect(stripTimestamps(httpComposed.proseEntries[0])).toEqual(
      stripTimestamps(helperComposed.proseEntries[0])
    );
    expect([...httpComposed.approvedProseIds]).toEqual([...helperComposed.approvedProseIds]);
  });

  it("inference: same content approved via HTTP and via helper yields the same entry", async () => {
    // --- HTTP path ---
    const httpRes = await post(h.baseUrl, "/api/approve", {
      layer: "inference",
      candidateId: INF_APPROVE_ID,
    });
    expect(httpRes.status).toBe(200);
    const httpFile = parseInferredApprovedFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.inferredApproved))
    );
    expect(httpFile.entries).toHaveLength(1);
    const httpEntry = httpFile.entries[0];

    // --- Direct-helper path into a second tmp dir, using the SAME adapted
    //     candidate the server would have loaded (synthesized inferenceRunId). ---
    const helperDir = path.join(h.root, "helper-inf");
    await fsp.mkdir(helperDir, { recursive: true });
    const candidate = await findInferenceCandidate(h.candidatesDir, INF_APPROVE_ID);
    expect(candidate).toBeDefined();
    const helperApprovedPath = path.join(helperDir, DISPOSITION_FILES.inferredApproved);
    const helperRejectionsPath = path.join(helperDir, DISPOSITION_FILES.inferredRejections);
    const helperEntry = await appendInferredApproval(
      candidate!,
      "someone-else",
      helperApprovedPath,
      helperRejectionsPath
    );

    // --- Field-level equality on the content fields (id is the candidate id) ---
    expect(httpEntry.id).toBe(helperEntry.id); // === candidate id — MUST match
    expect(httpEntry.relationFamily).toBe(helperEntry.relationFamily);
    expect(httpEntry.sourceId).toBe(helperEntry.sourceId);
    expect(httpEntry.targetId).toBe(helperEntry.targetId);
    expect(httpEntry.premises).toEqual(helperEntry.premises);
    expect(httpEntry.rationale).toBe(helperEntry.rationale);
    expect(httpEntry.confidence).toBe(helperEntry.confidence);
    expect(httpEntry.inferenceRunId).toBe(helperEntry.inferenceRunId);
    expect(httpEntry.status).toBe(helperEntry.status);
    expect(httpEntry.approvedBy).not.toBe(helperEntry.approvedBy);
  });
});

// ---------------------------------------------------------------------------
// (d) ENTITY-MERGE — HTTP approval === direct appendEntityMerge; rejected pair
//     is never re-proposed (content-addressed pair key).
// ---------------------------------------------------------------------------

describe("(d) entity-merge — UI disposition is byte-compatible with the helper", () => {
  let h: Harness;
  beforeEach(async () => {
    h = await startHarness();
  });
  afterEach(async () => {
    await stopHarness(h);
  });

  it("approve via HTTP and via appendEntityMerge yield the same content-addressed entry", async () => {
    const httpRes = await post(h.baseUrl, "/api/approve", {
      layer: "entity",
      candidateId: ENT_APPROVE_KEY,
    });
    expect(httpRes.status).toBe(200);
    const httpFile = parseEntityApprovedFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.entityApproved))
    );
    expect(httpFile.entries).toHaveLength(1);
    const httpEntry = httpFile.entries[0];

    // Direct-helper path into a second tmp dir, using the SAME candidate.
    const helperDir = path.join(h.root, "helper-entity");
    await fsp.mkdir(helperDir, { recursive: true });
    const candidate = await findEntityCandidate(h.candidatesDir, ENT_APPROVE_KEY);
    expect(candidate).toBeDefined();
    const helperEntry = await appendEntityMerge(
      candidate!,
      "someone-else",
      path.join(helperDir, DISPOSITION_FILES.entityApproved),
      path.join(helperDir, DISPOSITION_FILES.entityRejections)
    );

    // Field-level equality on the content-addressed fields; id MUST match.
    expect(httpEntry.id).toBe(helperEntry.id);
    expect(httpEntry.id).toBe(ENT_APPROVE_KEY);
    expect(httpEntry.entityIdA).toBe(helperEntry.entityIdA);
    expect(httpEntry.entityIdB).toBe(helperEntry.entityIdB);
    expect(httpEntry.kind).toBe(helperEntry.kind);
    expect(httpEntry.canonicalName).toBe(helperEntry.canonicalName);
    expect(httpEntry.aliases).toEqual(helperEntry.aliases);
    expect(httpEntry.mentionIds).toEqual(helperEntry.mentionIds);
    expect(httpEntry.reason).toBe(helperEntry.reason);
    expect(httpEntry.confidence).toBe(helperEntry.confidence);
    expect(httpEntry.status).toBe(helperEntry.status);
    // The one field that MAY differ:
    expect(httpEntry.approvedBy).not.toBe(helperEntry.approvedBy);
  });

  it("a rejected pair records its content-addressed key and is never re-proposed", async () => {
    const rejectRes = await post(h.baseUrl, "/api/reject", {
      layer: "entity",
      candidateId: ENT_REJECT_KEY,
    });
    expect(rejectRes.status).toBe(200);

    const rej = parseEntityRejectionsFile(
      await readJson(path.join(h.dispositionsDir, DISPOSITION_FILES.entityRejections))
    );
    expect(rej.rejectedPairKeys).toContain(ENT_REJECT_KEY);
    expect(rej.rejectedPairKeys).not.toContain(ENT_APPROVE_KEY);

    // buildState marks the rejected candidate as 'rejected' (never re-surfaced as pending).
    const state = await buildState(h.candidatesDir, h.dispositionsDir);
    const rejected = state.entity.find((i) => i.candidateId === ENT_REJECT_KEY);
    expect(rejected?.status).toBe("rejected");
    const other = state.entity.find((i) => i.candidateId === ENT_APPROVE_KEY);
    expect(other?.status).toBe("pending");
  });
});

// ---------------------------------------------------------------------------
// (c) NO-AUTO-APPROVE AT THE HTTP LAYER — GETs never mutate; unknown → 404
// ---------------------------------------------------------------------------

describe("(c) read-only GETs — no GET mutates the dispositions dir", () => {
  let h: Harness;
  beforeEach(async () => {
    h = await startHarness();
  });
  afterEach(async () => {
    await stopHarness(h);
  });

  it("GET / and GET /api/state leave the dispositions dir byte-identical", async () => {
    const before = await hashDir(h.dispositionsDir);

    const page = await fetch(`${h.baseUrl}/`);
    expect(page.status).toBe(200);
    await page.text();

    const state = await fetch(`${h.baseUrl}/api/state`);
    expect(state.status).toBe(200);
    await state.json();

    // Read a second time to be thorough — still no writes.
    await (await fetch(`${h.baseUrl}/api/state`)).json();

    const after = await hashDir(h.dispositionsDir);
    expect(after).toBe(before);
  });

  it("an unknown endpoint 404s", async () => {
    const res = await fetch(`${h.baseUrl}/api/nope`);
    expect(res.status).toBe(404);
    const body = (await res.json()) as { error?: string };
    expect(body.error).toBe("not_found");
  });
});

// ---------------------------------------------------------------------------
// helpers for (b) — a minimal extracted.json + a timestamp stripper
// ---------------------------------------------------------------------------

async function writeMinimalExtracted(root: string): Promise<string> {
  const extractedPath = path.join(root, "extracted.json");
  await fsp.writeFile(
    extractedPath,
    JSON.stringify({
      schema_version: "1.0.0",
      subsystem: "FIXTURE",
      needs: [],
      requirements: [],
      functions: [],
      components: [],
      satisfies: [],
      allocations: [],
    })
  );
  return extractedPath;
}

function stripTimestamps<T extends { approvedAt?: unknown; approvedBy?: unknown }>(
  entry: T
): Omit<T, "approvedAt" | "approvedBy"> {
  const { approvedAt: _a, approvedBy: _b, ...rest } = entry;
  return rest;
}
