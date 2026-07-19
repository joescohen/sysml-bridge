/**
 * no-auto-approve.test.ts — the human-gate invariant, proven three ways.
 *
 * (a) NO-DISPOSITION: candidates present but zero disposition records →
 *     nothing from the candidate layer enters the composed IR.
 * (b) EXPLICIT-APPROVAL: an appendApproval / appendInferredApproval record →
 *     exactly that entry appears; a rejection record never appears.
 * (c) RATCHET: a source-scanning test walks the LIVE tree (never a hardcoded
 *     file list) and asserts the approval-writer helpers are called from no
 *     production source file — the helpers execute a human decision made in
 *     the /mbse-approve skill; any new call site in src/ fails this test
 *     until reviewed.
 */
import { describe, it, expect } from "vitest";
import { promises as fsp } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { fileURLToPath } from "node:url";

import { sourceScanRatchet } from "@sysml-bridge/invariants";

import {
  composeProseTwoLayer,
  composeIR as composeInferredThreeLayer,
  appendApproval,
  recordRejection,
  isRejected,
  appendInferredApproval,
  recordInferredRejection,
  type CandidateEntry,
  type InferenceCandidate,
} from "@sysml-bridge/model";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const MINIMAL_EXTRACTED = {
  schema_version: "1.0.0",
  subsystem: "TEST",
  needs: [{ id: "need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
  requirements: [
    {
      id: "req-001",
      kind: "requirement",
      naturalKey: "R1",
      name: "Test Requirement",
      statement: "The system shall do something.",
      needIds: ["need-001"],
    },
  ],
  functions: [],
  components: [],
  satisfies: [],
  allocations: [],
};

function proseCandidate(id: string): CandidateEntry {
  return {
    id,
    kind: "requirement",
    fields: { name: "Prose Req", statement: "The system shall be prose-grounded." },
    citation: {
      docId: "doc-1",
      docSha256: "0".repeat(64),
      chunkId: "chunk-1",
      sectionPath: "3.2.1",
      quote: "the system shall be prose-grounded",
    },
  };
}

function inferenceCandidate(id: string): InferenceCandidate {
  return {
    id,
    relationFamily: "allocation",
    sourceId: "req-001",
    targetId: "need-001",
    premises: ["req-001"],
    rationale: "test rationale (audit-only)",
    confidence: 0.9,
    inferenceRunId: "run-001",
  };
}

async function mkFixtureDir(): Promise<{ dir: string; extractedPath: string }> {
  const dir = await fsp.mkdtemp(path.join(os.tmpdir(), "no-auto-approve-"));
  const extractedPath = path.join(dir, "extracted.json");
  await fsp.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));
  return { dir, extractedPath };
}

// ---------------------------------------------------------------------------
// (a) NO-DISPOSITION — candidates never leak without a disposition record
// ---------------------------------------------------------------------------

describe("no-auto-approve — candidates without dispositions never compose", () => {
  it("prose: candidates file present, no approved file → composed == base extraction", async () => {
    const { dir, extractedPath } = await mkFixtureDir();
    // A candidates file exists on disk — composeIR must not even look at it.
    await fsp.writeFile(
      path.join(dir, "prose-candidates.json"),
      JSON.stringify({ candidates: [proseCandidate("cand-1")] })
    );

    const composed = await composeProseTwoLayer(extractedPath);
    expect(composed.extracted).toEqual(MINIMAL_EXTRACTED);
    expect(composed.proseEntries).toEqual([]);
    expect(composed.approvedProseIds.size).toBe(0);
  });

  it("prose: EMPTY approved file (entries: []) → nothing composes", async () => {
    const { dir, extractedPath } = await mkFixtureDir();
    const approvedPath = path.join(dir, "prose-approved.json");
    await fsp.writeFile(approvedPath, JSON.stringify({ entries: [] }));

    const composed = await composeProseTwoLayer(extractedPath, approvedPath);
    expect(composed.extracted).toEqual(MINIMAL_EXTRACTED);
    expect(composed.proseEntries).toEqual([]);
  });

  it("inferred: candidates present, no approved file → composed == base", async () => {
    const { dir, extractedPath } = await mkFixtureDir();
    await fsp.writeFile(
      path.join(dir, "inference-candidates.json"),
      JSON.stringify({ candidates: [inferenceCandidate("inf-1")] })
    );

    const composed = await composeInferredThreeLayer(extractedPath);
    expect(composed.extracted).toEqual(MINIMAL_EXTRACTED);
    expect(composed.inferredEntries).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// (b) EXPLICIT-APPROVAL — the human's decision, and only it, composes
// ---------------------------------------------------------------------------

describe("no-auto-approve — explicit dispositions", () => {
  it("prose: appendApproval composes exactly that entry; rejection composes nothing", async () => {
    const { dir, extractedPath } = await mkFixtureDir();
    const approvedPath = path.join(dir, "prose-approved.json");
    const rejectionsPath = path.join(dir, "prose-rejections.json");

    const approvedEntry = await appendApproval(
      proseCandidate("cand-approved"),
      "test-human",
      approvedPath,
      rejectionsPath
    );
    await recordRejection("cand-rejected", rejectionsPath);

    const composed = await composeProseTwoLayer(extractedPath, approvedPath);
    expect(composed.proseEntries).toHaveLength(1);
    expect(composed.proseEntries[0].id).toBe(approvedEntry.id);
    expect(composed.proseEntries[0].candidateId).toBe("cand-approved");
    expect(composed.proseEntries[0].approvedBy).toBe("test-human");
    // The rejected candidate exists only in the rejections record.
    expect(await isRejected("cand-rejected", rejectionsPath)).toBe(true);
    expect(
      composed.proseEntries.some((e) => e.candidateId === "cand-rejected")
    ).toBe(false);
  });

  it("inferred: appendInferredApproval composes exactly that entry", async () => {
    const { dir, extractedPath } = await mkFixtureDir();
    const approvedPath = path.join(dir, "inferred-approved.json");
    const rejectionsPath = path.join(dir, "inferred-rejections.json");

    const entry = await appendInferredApproval(
      inferenceCandidate("inf-approved"),
      "test-human",
      approvedPath,
      rejectionsPath
    );
    // A DISTINCT candidate (different target triple) gets rejected — it must
    // never appear. Inferred entries are content-addressed by
    // (relationFamily, sourceId, targetId), so the triple is the identity.
    const rejected = { ...inferenceCandidate("inf-rejected"), targetId: "req-001" };
    await recordInferredRejection(rejected.id, rejectionsPath);

    const composed = await composeInferredThreeLayer(
      extractedPath,
      undefined,
      undefined,
      approvedPath
    );
    const entries = composed.inferredEntries;
    expect(entries).toHaveLength(1);
    expect(entries[0].id).toBe(entry.id);
    expect(
      entries.some(
        (e) =>
          e.relationFamily === rejected.relationFamily &&
          e.sourceId === rejected.sourceId &&
          e.targetId === rejected.targetId
      )
    ).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// (c) RATCHET — no production call sites of the approval writers
// ---------------------------------------------------------------------------

describe("no-auto-approve — approval-writer ratchet (source scan)", () => {
  const HERE = path.dirname(fileURLToPath(import.meta.url));
  const REPO_ROOT = path.resolve(HERE, "../../../..");

  /** Files allowed to mention the writers as CALL SITES. The helpers execute a
   *  human decision from the /mbse-approve skill at runtime; no production
   *  module may call them. Only the defining modules (declarations, not calls),
   *  the one allowlisted human-gate surface, and test files are exempt — and
   *  tests are excluded from the scan anyway. */
  const DEFINING_MODULES = [
    path.join("packages/model/src", "approval-helpers.ts"),
    path.join("packages/model/src", "inferred-approval-helpers.ts"),
    // W1 entity-merge writer — the defining module for appendEntityMerge.
    path.join("packages/model/src", "entity-approval-helpers.ts"),
    // The review UI is a human-gate surface — every write happens on an explicit
    // user click; no automated path reaches these endpoints. server.ts is the
    // ONLY module in packages/review-ui allowed to call the approval writers; a
    // rogue call in any other review-ui file still fails this ratchet.
    path.join("packages/review-ui/src", "server.ts"),
  ];

  it("appendApproval / appendInferredApproval / appendEntityMerge have zero production call sites", () => {
    // Scan scope is DERIVED from the live tree (every packages/<pkg>/src) by the
    // shared ratchet — a new package or file is automatically in scope; a
    // hardcoded list would silently go blind. The floor asserts the derivation
    // actually found the packages (candidates, gates, mcp-server, model,
    // review-ui, sysml, invariants ⇒ 7).
    const { scanRoots, offenders } = sourceScanRatchet({
      repoRoot: REPO_ROOT,
      tokens: ["appendApproval", "appendInferredApproval", "appendEntityMerge"],
      allowlist: DEFINING_MODULES,
    });

    expect(scanRoots.length).toBeGreaterThanOrEqual(7);
    expect(
      offenders,
      `production call sites of approval writers:\n${offenders.join("\n")}`
    ).toEqual([]);
  });
});
