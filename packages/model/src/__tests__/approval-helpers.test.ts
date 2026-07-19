/**
 * approval-helpers.test.ts — TDD RED-first tests for G-D gap closure.
 *
 * Tests appendApproval and recordRejection helpers:
 *
 *   GD-1  appendApproval — file grows: new entry appears, schema-valid
 *   GD-2  appendApproval — stableId assigned deterministically (content-addressed)
 *   GD-3  appendApproval — supersedes field set when candidate.supersedes provided
 *   GD-4  appendApproval — append-only: existing entries retained
 *   GD-5  recordRejection — rejection persisted to rejections file
 *   GD-6  recordRejection — idempotent: recording twice leaves single entry
 *   GD-7  re-ingest skips approved ids (isApproved returns true for approved id)
 *   GD-8  re-ingest skips rejected ids (isRejected returns true for rejected id)
 *   GD-9  composeIR round-trip: appendApproval + composeIR → approved id in set
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import {
  appendApproval,
  recordRejection,
  isApproved,
  isRejected,
  type CandidateEntry,
} from "../approval-helpers.js";
import { ProseApprovedEntrySchema } from "../prose-approved.js";
import { composeIR } from "../prose-approved.js";
import { SCHEMA_VERSION } from "../schema.js";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const MINIMAL_EXTRACTED = {
  schema_version: SCHEMA_VERSION,
  subsystem: "TestSub",
  needs: [{ id: "need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
  requirements: [
    {
      id: "requirement-abc",
      kind: "requirement",
      naturalKey: "CC-1",
      name: "Do Thing",
      statement: "The system shall do a thing.",
      needIds: ["need-001"],
    },
  ],
  functions: [
    { id: "function-xyz", kind: "function", naturalKey: "F1", name: "Func", level: "L1", owner: "TestSub" },
  ],
  components: [{ id: "component-111", kind: "component", naturalKey: "COMP-1", name: "Widget" }],
  satisfies: [{ reqId: "requirement-abc", functionId: "function-xyz" }],
  allocations: [{ functionId: "function-xyz", componentId: "component-111" }],
};

function makeCandidate(overrides: Partial<CandidateEntry> = {}): CandidateEntry {
  return {
    id: "candidate-abc-001",
    kind: "requirement",
    fields: { naturalKey: "SYN-1", name: "Synthetic Req", statement: "The system shall do a synthetic thing." },
    citation: {
      docId: "synthetic-doc-001",
      docSha256: "aabbcc00112233440000000000000000aabbcc00112233440000000000000000",
      chunkId: "chunk-gd-001",
      sectionPath: "3.2 System Requirements",
      quote: "The system shall do a synthetic thing.",
    },
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Test lifecycle: temp directory per test group
// ---------------------------------------------------------------------------

let tmpDir: string;
let approvedPath: string;
let rejectionsPath: string;

beforeEach(async () => {
  tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "approval-helpers-test-"));
  approvedPath = path.join(tmpDir, "prose-approved.json");
  rejectionsPath = path.join(tmpDir, "prose-rejections.json");
});

afterEach(async () => {
  await fs.rm(tmpDir, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// GD-1 — appendApproval creates new file (or appends) and entry is schema-valid
// ---------------------------------------------------------------------------

describe("appendApproval — GD-1 file grows, entry schema-valid", () => {
  it("creates prose-approved.json if it does not exist and adds the entry", async () => {
    const candidate = makeCandidate();
    const entry = await appendApproval(candidate, "test-user", approvedPath, rejectionsPath);

    // File must now exist
    const raw = await fs.readFile(approvedPath, "utf8");
    const parsed = JSON.parse(raw) as { entries: unknown[] };
    expect(parsed.entries).toHaveLength(1);

    // The returned entry must be schema-valid
    const result = ProseApprovedEntrySchema.safeParse(entry);
    expect(result.success, `schema validation failed: ${JSON.stringify(result)}`).toBe(true);
  });

  it("appends to existing file without touching existing entries", async () => {
    const c1 = makeCandidate({ id: "candidate-001" });
    const c2 = makeCandidate({
      id: "candidate-002",
      citation: { ...makeCandidate().citation, quote: "The system shall do another thing." },
    });

    const e1 = await appendApproval(c1, "test-user", approvedPath, rejectionsPath);
    const e2 = await appendApproval(c2, "test-user", approvedPath, rejectionsPath);

    const raw = await fs.readFile(approvedPath, "utf8");
    const parsed = JSON.parse(raw) as { entries: unknown[] };
    expect(parsed.entries).toHaveLength(2);

    // Both entries present by id
    const ids = (parsed.entries as Array<{ id: string }>).map((e) => e.id);
    expect(ids).toContain(e1.id);
    expect(ids).toContain(e2.id);
  });
});

// ---------------------------------------------------------------------------
// GD-2 — stableId assigned deterministically (same input → same id)
// ---------------------------------------------------------------------------

describe("appendApproval — GD-2 stableId deterministic", () => {
  it("two appendApproval calls with the same candidate quote produce the same entry id", async () => {
    const candidate = makeCandidate();

    // First tmp dir
    const dir1 = await fs.mkdtemp(path.join(os.tmpdir(), "det-test-"));
    const dir2 = await fs.mkdtemp(path.join(os.tmpdir(), "det-test-"));
    try {
      const ap1 = path.join(dir1, "prose-approved.json");
      const rp1 = path.join(dir1, "prose-rejections.json");
      const ap2 = path.join(dir2, "prose-approved.json");
      const rp2 = path.join(dir2, "prose-rejections.json");

      const e1 = await appendApproval(candidate, "user-a", ap1, rp1);
      const e2 = await appendApproval(candidate, "user-b", ap2, rp2);

      // The id must be derived from content, not approvedBy or timestamp
      expect(e1.id).toBe(e2.id);
    } finally {
      await fs.rm(dir1, { recursive: true, force: true });
      await fs.rm(dir2, { recursive: true, force: true });
    }
  });

  it("entry status is 'approved'", async () => {
    const entry = await appendApproval(makeCandidate(), "test-user", approvedPath, rejectionsPath);
    expect(entry.status).toBe("approved");
  });

  it("entry candidateId matches candidate.id", async () => {
    const candidate = makeCandidate({ id: "my-candidate-id-xyz" });
    const entry = await appendApproval(candidate, "test-user", approvedPath, rejectionsPath);
    expect(entry.candidateId).toBe("my-candidate-id-xyz");
  });

  it("entry approvedBy matches passed approvedBy", async () => {
    const entry = await appendApproval(makeCandidate(), "alice", approvedPath, rejectionsPath);
    expect(entry.approvedBy).toBe("alice");
  });

  it("entry approvedAt is a valid ISO datetime string", async () => {
    const entry = await appendApproval(makeCandidate(), "test-user", approvedPath, rejectionsPath);
    const d = new Date(entry.approvedAt);
    expect(isNaN(d.getTime())).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// GD-3 — supersedes field set when candidate.supersedes provided
// ---------------------------------------------------------------------------

describe("appendApproval — GD-3 supersedes propagation", () => {
  it("sets supersedes on the new entry when candidate.supersedes is provided", async () => {
    const existing = makeCandidate({ id: "candidate-old" });
    const e1 = await appendApproval(existing, "test-user", approvedPath, rejectionsPath);

    const superseding = makeCandidate({
      id: "candidate-new",
      supersedes: e1.id,
      citation: { ...makeCandidate().citation, quote: "Updated: the system shall do a newer thing." },
    });
    const e2 = await appendApproval(superseding, "test-user", approvedPath, rejectionsPath);

    expect(e2.supersedes).toBe(e1.id);
  });

  it("supersedes is absent when not provided", async () => {
    const entry = await appendApproval(makeCandidate(), "test-user", approvedPath, rejectionsPath);
    expect(entry.supersedes).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// GD-4 — append-only: existing entries retained after second append
// ---------------------------------------------------------------------------

describe("appendApproval — GD-4 append-only", () => {
  it("first entry is still valid after second append (file not clobbered)", async () => {
    const e1 = await appendApproval(makeCandidate({ id: "c-001" }), "u1", approvedPath, rejectionsPath);
    await appendApproval(
      makeCandidate({ id: "c-002", citation: { ...makeCandidate().citation, quote: "Second thing." } }),
      "u2", approvedPath, rejectionsPath
    );

    const raw = await fs.readFile(approvedPath, "utf8");
    const { entries } = JSON.parse(raw) as { entries: Array<{ id: string; status: string }> };
    const first = entries.find((e) => e.id === e1.id);
    expect(first).toBeDefined();
    expect(first!.status).toBe("approved");
  });
});

// ---------------------------------------------------------------------------
// GD-5 — recordRejection persists to rejections file
// ---------------------------------------------------------------------------

describe("recordRejection — GD-5 rejection persisted", () => {
  it("creates rejections file with the candidate id", async () => {
    await recordRejection("candidate-rej-001", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    expect(parsed.rejectedIds).toContain("candidate-rej-001");
  });

  it("records multiple rejections", async () => {
    await recordRejection("candidate-rej-001", rejectionsPath);
    await recordRejection("candidate-rej-002", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    expect(parsed.rejectedIds).toContain("candidate-rej-001");
    expect(parsed.rejectedIds).toContain("candidate-rej-002");
  });
});

// ---------------------------------------------------------------------------
// GD-6 — recordRejection idempotent: recording twice leaves single entry
// ---------------------------------------------------------------------------

describe("recordRejection — GD-6 idempotent", () => {
  it("recording the same id twice does not duplicate it", async () => {
    await recordRejection("candidate-dup-001", rejectionsPath);
    await recordRejection("candidate-dup-001", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    const count = parsed.rejectedIds.filter((id) => id === "candidate-dup-001").length;
    expect(count).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// GD-7 — isApproved returns true for approved candidate id
// ---------------------------------------------------------------------------

describe("isApproved / isRejected — GD-7/8 skip predicates", () => {
  it("isApproved returns true for a candidateId whose approval was recorded", async () => {
    const candidate = makeCandidate({ id: "candidate-skip-001" });
    await appendApproval(candidate, "test-user", approvedPath, rejectionsPath);

    const result = await isApproved("candidate-skip-001", approvedPath);
    expect(result).toBe(true);
  });

  it("isApproved returns false for a candidateId not in the file", async () => {
    // No approvals recorded — file doesn't exist yet
    const result = await isApproved("candidate-unknown", approvedPath);
    expect(result).toBe(false);
  });

  it("isRejected returns true for a rejected candidate id", async () => {
    await recordRejection("candidate-rej-check", rejectionsPath);
    const result = await isRejected("candidate-rej-check", rejectionsPath);
    expect(result).toBe(true);
  });

  it("isRejected returns false for a non-rejected candidate id", async () => {
    const result = await isRejected("candidate-unknown", rejectionsPath);
    expect(result).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// GD-9 — composeIR round-trip: appendApproval → composeIR includes approved id
// ---------------------------------------------------------------------------

describe("appendApproval + composeIR — GD-9 round-trip", () => {
  it("approved entry id appears in composeIR approvedProseIds set", async () => {
    const extractedPath = path.join(tmpDir, "extracted.json");
    await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

    const candidate = makeCandidate();
    const entry = await appendApproval(candidate, "test-user", approvedPath, rejectionsPath);

    const ir = await composeIR(extractedPath, approvedPath);
    expect(ir.approvedProseIds.has(entry.id)).toBe(true);
    expect(ir.proseEntries).toHaveLength(1);
    expect(ir.proseEntries[0].status).toBe("approved");
  });
});
