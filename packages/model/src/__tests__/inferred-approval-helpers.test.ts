/**
 * inferred-approval-helpers.test.ts — TDD RED-first tests for F8 approval helpers.
 *
 * Covers:
 *   IA-1  appendInferredApproval — file grows, entry schema-valid
 *   IA-2  appendInferredApproval — stamps approvedAt/status:'approved'/approvedBy
 *   IA-3  appendInferredApproval — append-only (file grows, prior entries untouched)
 *   IA-4  recordInferredRejection — rejection persisted, idempotent
 *   IA-5  isInferredApproved / isInferredRejected — predicates
 *   IA-6  round-trip: appendInferredApproval → composeIR → approvedInferredIds includes id
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import {
  appendInferredApproval,
  recordInferredRejection,
  isInferredApproved,
  isInferredRejected,
} from "../inferred-approval-helpers.js";
import {
  InferredApprovedEntrySchema,
  composeIR,
  type InferredApprovedEntry,
} from "../inferred-approved.js";
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

/** A minimal inference candidate record (subset of InferredApprovedEntry, pre-approval fields only) */
interface InferenceCandidate {
  id: string;
  relationFamily: InferredApprovedEntry["relationFamily"];
  sourceId: string;
  targetId: string;
  premises: string[];
  rationale: string;
  confidence: number;
  debate?: InferredApprovedEntry["debate"];
  inferenceRunId: string;
  supersedes?: string;
}

function makeCandidate(overrides: Partial<InferenceCandidate> = {}): InferenceCandidate {
  return {
    id: "candidate-infer-001",
    relationFamily: "allocation",
    sourceId: "function-xyz",
    targetId: "component-111",
    premises: ["requirement-abc"],
    rationale: "Audit-only: function performs the stated requirement.",
    confidence: 0.85,
    inferenceRunId: "run-test-001",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Test lifecycle
// ---------------------------------------------------------------------------

let tmpDir: string;
let approvedPath: string;
let rejectionsPath: string;

beforeEach(async () => {
  tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "inferred-approval-test-"));
  approvedPath = path.join(tmpDir, "inferred-approved.json");
  rejectionsPath = path.join(tmpDir, "inferred-rejections.json");
});

afterEach(async () => {
  await fs.rm(tmpDir, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// IA-1 — appendInferredApproval file grows, entry schema-valid
// ---------------------------------------------------------------------------

describe("appendInferredApproval — IA-1 file grows, entry schema-valid", () => {
  it("creates inferred-approved.json if it does not exist and adds the entry", async () => {
    const candidate = makeCandidate();
    const entry = await appendInferredApproval(
      candidate,
      "test-user",
      approvedPath,
      rejectionsPath
    );

    const raw = await fs.readFile(approvedPath, "utf8");
    const parsed = JSON.parse(raw) as { entries: unknown[] };
    expect(parsed.entries).toHaveLength(1);

    const result = InferredApprovedEntrySchema.safeParse(entry);
    expect(result.success, `schema validation failed: ${JSON.stringify(result)}`).toBe(true);
  });

  it("entry id matches candidate id", async () => {
    const candidate = makeCandidate({ id: "candidate-infer-custom-id" });
    const entry = await appendInferredApproval(
      candidate,
      "test-user",
      approvedPath,
      rejectionsPath
    );
    expect(entry.id).toBe("candidate-infer-custom-id");
  });
});

// ---------------------------------------------------------------------------
// IA-2 — stamps approvedAt / status / approvedBy
// ---------------------------------------------------------------------------

describe("appendInferredApproval — IA-2 stamps approval fields", () => {
  it("status is 'approved'", async () => {
    const entry = await appendInferredApproval(
      makeCandidate(),
      "alice",
      approvedPath,
      rejectionsPath
    );
    expect(entry.status).toBe("approved");
  });

  it("approvedBy matches passed approvedBy", async () => {
    const entry = await appendInferredApproval(
      makeCandidate(),
      "alice",
      approvedPath,
      rejectionsPath
    );
    expect(entry.approvedBy).toBe("alice");
  });

  it("approvedAt is a valid ISO datetime", async () => {
    const entry = await appendInferredApproval(
      makeCandidate(),
      "test-user",
      approvedPath,
      rejectionsPath
    );
    const d = new Date(entry.approvedAt);
    expect(isNaN(d.getTime())).toBe(false);
  });

  it("all candidate fields are preserved in the entry", async () => {
    const candidate = makeCandidate({
      id: "candidate-preserve-test",
      relationFamily: "modeMembership",
      sourceId: "function-abc",
      targetId: "component-xyz",
      premises: ["requirement-abc", "requirement-def"],
      confidence: 0.75,
      inferenceRunId: "run-preserve-001",
      debate: { verdict: "confirmed", advocate: 0.85, challenger: 0.3 },
    });
    const entry = await appendInferredApproval(
      candidate,
      "test-user",
      approvedPath,
      rejectionsPath
    );
    expect(entry.relationFamily).toBe("modeMembership");
    expect(entry.sourceId).toBe("function-abc");
    expect(entry.targetId).toBe("component-xyz");
    expect(entry.premises).toEqual(["requirement-abc", "requirement-def"]);
    expect(entry.confidence).toBe(0.75);
    expect(entry.inferenceRunId).toBe("run-preserve-001");
    expect(entry.debate?.verdict).toBe("confirmed");
  });

  it("supersedes field propagated when set", async () => {
    const candidate = makeCandidate({ supersedes: "inferred-old-id-abc" });
    const entry = await appendInferredApproval(
      candidate,
      "test-user",
      approvedPath,
      rejectionsPath
    );
    expect(entry.supersedes).toBe("inferred-old-id-abc");
  });

  it("supersedes is absent when not set", async () => {
    const entry = await appendInferredApproval(
      makeCandidate(),
      "test-user",
      approvedPath,
      rejectionsPath
    );
    expect(entry.supersedes).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// IA-3 — append-only: file grows, prior entries untouched
// ---------------------------------------------------------------------------

describe("appendInferredApproval — IA-3 append-only", () => {
  it("appending twice: file has two entries", async () => {
    const c1 = makeCandidate({ id: "candidate-a1" });
    const c2 = makeCandidate({
      id: "candidate-a2",
      sourceId: "function-abc",
      targetId: "component-222",
    });

    const e1 = await appendInferredApproval(c1, "user-a", approvedPath, rejectionsPath);
    const e2 = await appendInferredApproval(c2, "user-b", approvedPath, rejectionsPath);

    const raw = await fs.readFile(approvedPath, "utf8");
    const parsed = JSON.parse(raw) as { entries: Array<{ id: string }> };
    expect(parsed.entries).toHaveLength(2);
    const ids = parsed.entries.map((e) => e.id);
    expect(ids).toContain(e1.id);
    expect(ids).toContain(e2.id);
  });

  it("first entry is still present and valid after second append", async () => {
    const e1 = await appendInferredApproval(
      makeCandidate({ id: "c-001" }),
      "u1",
      approvedPath,
      rejectionsPath
    );
    await appendInferredApproval(
      makeCandidate({ id: "c-002", targetId: "component-222" }),
      "u2",
      approvedPath,
      rejectionsPath
    );

    const raw = await fs.readFile(approvedPath, "utf8");
    const { entries } = JSON.parse(raw) as { entries: Array<{ id: string; status: string }> };
    const first = entries.find((e) => e.id === e1.id);
    expect(first).toBeDefined();
    expect(first!.status).toBe("approved");
  });

  it("REJECT-persist: rejected id remains in rejections file after multiple calls", async () => {
    await recordInferredRejection("candidate-rej-001", rejectionsPath);
    await recordInferredRejection("candidate-rej-002", rejectionsPath);
    await recordInferredRejection("candidate-rej-003", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    expect(parsed.rejectedIds).toContain("candidate-rej-001");
    expect(parsed.rejectedIds).toContain("candidate-rej-002");
    expect(parsed.rejectedIds).toContain("candidate-rej-003");
    expect(parsed.rejectedIds).toHaveLength(3);
  });
});

// ---------------------------------------------------------------------------
// IA-4 — recordInferredRejection: persisted + idempotent
// ---------------------------------------------------------------------------

describe("recordInferredRejection — IA-4 persisted + idempotent", () => {
  it("creates rejections file with the candidate id", async () => {
    await recordInferredRejection("candidate-rej-001", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    expect(parsed.rejectedIds).toContain("candidate-rej-001");
  });

  it("idempotent: recording same id twice leaves a single entry", async () => {
    await recordInferredRejection("candidate-dup-001", rejectionsPath);
    await recordInferredRejection("candidate-dup-001", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    const count = parsed.rejectedIds.filter((id) => id === "candidate-dup-001").length;
    expect(count).toBe(1);
  });

  it("records multiple distinct ids", async () => {
    await recordInferredRejection("rej-a", rejectionsPath);
    await recordInferredRejection("rej-b", rejectionsPath);

    const raw = await fs.readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    expect(parsed.rejectedIds).toContain("rej-a");
    expect(parsed.rejectedIds).toContain("rej-b");
  });
});

// ---------------------------------------------------------------------------
// IA-5 — isInferredApproved / isInferredRejected predicates
// ---------------------------------------------------------------------------

describe("isInferredApproved / isInferredRejected — IA-5 predicates", () => {
  it("isInferredApproved returns true after appendInferredApproval", async () => {
    const candidate = makeCandidate({ id: "candidate-pred-001" });
    await appendInferredApproval(candidate, "test-user", approvedPath, rejectionsPath);

    const result = await isInferredApproved("candidate-pred-001", approvedPath);
    expect(result).toBe(true);
  });

  it("isInferredApproved returns false for unknown candidate id", async () => {
    const result = await isInferredApproved("candidate-unknown", approvedPath);
    expect(result).toBe(false);
  });

  it("isInferredRejected returns true after recordInferredRejection", async () => {
    await recordInferredRejection("candidate-rej-pred", rejectionsPath);
    const result = await isInferredRejected("candidate-rej-pred", rejectionsPath);
    expect(result).toBe(true);
  });

  it("isInferredRejected returns false for non-rejected id", async () => {
    const result = await isInferredRejected("candidate-unknown", rejectionsPath);
    expect(result).toBe(false);
  });

  it("isInferredApproved returns false when file does not exist", async () => {
    const result = await isInferredApproved("any-id", path.join(tmpDir, "nonexistent.json"));
    expect(result).toBe(false);
  });

  it("isInferredRejected returns false when file does not exist", async () => {
    const result = await isInferredRejected("any-id", path.join(tmpDir, "nonexistent.json"));
    expect(result).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// IA-6 — round-trip: appendInferredApproval → composeIR → approvedInferredIds
// ---------------------------------------------------------------------------

describe("appendInferredApproval + composeIR — IA-6 round-trip", () => {
  it("approved entry id appears in composeIR approvedInferredIds set", async () => {
    const extractedPath = path.join(tmpDir, "extracted.json");
    await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

    const candidate = makeCandidate({ id: "candidate-roundtrip-001" });
    const entry = await appendInferredApproval(
      candidate,
      "test-user",
      approvedPath,
      rejectionsPath
    );

    const ir = await composeIR(extractedPath, undefined, undefined, approvedPath);
    expect(ir.approvedInferredIds.has(entry.id)).toBe(true);
    expect(ir.inferredEntries).toHaveLength(1);
    expect(ir.inferredEntries[0].status).toBe("approved");
  });

  it("candidate id pre-approval (no inferred-approved.json) is NOT in approvedInferredIds", async () => {
    const extractedPath = path.join(tmpDir, "extracted.json");
    await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

    // No inferred-approved.json — composeIR without inferred layer
    const ir = await composeIR(extractedPath);
    expect(ir.approvedInferredIds.size).toBe(0);
    expect(ir.inferredEntries).toHaveLength(0);
  });
});
