/**
 * inferred-approved.test.ts — RED-first tests for F8 inference layer (T1)
 *
 * Covers:
 *   I1  — InferredApprovedEntrySchema: valid/invalid shapes
 *   I2  — composeIR third layer: inferredApprovedPath param exposes approved entries + ids
 *   I3  — supersede chains resolve for inferred entries (same convention as prose)
 *   I4  — suspect premise propagation (A6):
 *           any premise id that resolves to a suspect/superseded prose entry → inferred
 *           entry composes as suspect; any premise id missing from all composed ids →
 *           entry composes as suspect
 *   I5  — A9 backward-compat: absent inferredApprovedPath → output byte-identical to
 *           two-layer compose (inferredEntries: [], approvedInferredIds: Set())
 */

import { describe, it, expect } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import {
  InferredApprovedEntrySchema,
  composeIR,
  type InferredApprovedEntry,
} from "../inferred-approved.js";
import { stableId } from "../stable-id.js";
import { SCHEMA_VERSION } from "../schema.js";

// ---------------------------------------------------------------------------
// Minimal extracted corpus fixture (same as prose tests)
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
    { id: "function-xyz", kind: "function", naturalKey: "F1", name: "Func Thing", level: "L1", owner: "TestSub" },
  ],
  components: [{ id: "component-111", kind: "component", naturalKey: "COMP-1", name: "Widget" }],
  satisfies: [{ reqId: "requirement-abc", functionId: "function-xyz" }],
  allocations: [{ functionId: "function-xyz", componentId: "component-111" }],
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

function makeInferredEntry(overrides: Partial<InferredApprovedEntry> = {}): InferredApprovedEntry {
  const id = stableId("inferred", "allocation:function-xyz:component-111");
  return {
    id,
    relationFamily: "allocation",
    sourceId: "function-xyz",
    targetId: "component-111",
    premises: ["requirement-abc"],
    rationale: "Audit-only: function performs the stated requirement, implying allocation to widget.",
    confidence: 0.85,
    inferenceRunId: "run-test-001",
    approvedBy: "test-user",
    approvedAt: "2026-06-11T00:00:00.000Z",
    status: "approved",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// I1 — InferredApprovedEntrySchema: valid/invalid shapes
// ---------------------------------------------------------------------------

describe("InferredApprovedEntrySchema", () => {
  it("accepts a well-formed approved allocation entry", () => {
    const entry = makeInferredEntry();
    const result = InferredApprovedEntrySchema.safeParse(entry);
    expect(result.success).toBe(true);
  });

  it("accepts all four relation families", () => {
    const families: InferredApprovedEntry["relationFamily"][] = [
      "allocation",
      "modeMembership",
      "flowTyping",
      "controlJoin",
    ];
    for (const relationFamily of families) {
      const result = InferredApprovedEntrySchema.safeParse(
        makeInferredEntry({ relationFamily })
      );
      expect(result.success, `relationFamily=${relationFamily} should be accepted`).toBe(true);
    }
  });

  it("accepts all three status values", () => {
    const statuses: InferredApprovedEntry["status"][] = ["approved", "superseded", "suspect"];
    for (const status of statuses) {
      const result = InferredApprovedEntrySchema.safeParse(makeInferredEntry({ status }));
      expect(result.success, `status=${status} should be accepted`).toBe(true);
    }
  });

  it("accepts optional debate field", () => {
    const result = InferredApprovedEntrySchema.safeParse(
      makeInferredEntry({
        debate: { verdict: "confirmed", advocate: 0.85, challenger: 0.3 },
      })
    );
    expect(result.success).toBe(true);
  });

  it("accepts optional supersedes field", () => {
    const result = InferredApprovedEntrySchema.safeParse(
      makeInferredEntry({ supersedes: "inferred-previous-id" })
    );
    expect(result.success).toBe(true);
  });

  it("rejects an unknown relationFamily", () => {
    const result = InferredApprovedEntrySchema.safeParse(
      makeInferredEntry({ relationFamily: "bogus" as never })
    );
    expect(result.success).toBe(false);
  });

  it("rejects an unknown status value", () => {
    const result = InferredApprovedEntrySchema.safeParse(
      makeInferredEntry({ status: "pending" as never })
    );
    expect(result.success).toBe(false);
  });

  it("rejects empty premises array (min 1)", () => {
    const result = InferredApprovedEntrySchema.safeParse(
      makeInferredEntry({ premises: [] })
    );
    expect(result.success).toBe(false);
  });

  it("rejects entry missing required 'sourceId'", () => {
    const entry = makeInferredEntry() as Record<string, unknown>;
    delete entry.sourceId;
    const result = InferredApprovedEntrySchema.safeParse(entry);
    expect(result.success).toBe(false);
  });

  it("rejects entry missing required 'inferenceRunId'", () => {
    const entry = makeInferredEntry() as Record<string, unknown>;
    delete entry.inferenceRunId;
    const result = InferredApprovedEntrySchema.safeParse(entry);
    expect(result.success).toBe(false);
  });

  it("debate field rejects unknown verdict", () => {
    const result = InferredApprovedEntrySchema.safeParse(
      makeInferredEntry({
        debate: { verdict: "wrong" as never, advocate: 0.8, challenger: 0.2 },
      })
    );
    expect(result.success).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// I2 — composeIR third layer: inferredApprovedPath param
// ---------------------------------------------------------------------------

describe("composeIR — third layer (inferred)", () => {
  it("with inferredApprovedPath: approved entry appears in inferredEntries", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const entry = makeInferredEntry();
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [entry] }, null, 2));

      const result = await composeIR(extractedPath, undefined, undefined, inferredPath);

      expect(result.inferredEntries).toHaveLength(1);
      expect(result.inferredEntries[0].id).toBe(entry.id);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("approved inferred entry id is in approvedInferredIds set", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const entry = makeInferredEntry();
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [entry] }, null, 2));

      const result = await composeIR(extractedPath, undefined, undefined, inferredPath);

      expect(result.approvedInferredIds).toBeDefined();
      expect(result.approvedInferredIds.has(entry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("suspect/superseded inferred entry ids are NOT in approvedInferredIds", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const suspectEntry = makeInferredEntry({
        id: stableId("inferred", "allocation:function-xyz:suspect"),
        status: "suspect",
      });
      const supersededEntry = makeInferredEntry({
        id: stableId("inferred", "allocation:function-xyz:superseded"),
        status: "superseded",
      });
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(
        inferredPath,
        JSON.stringify({ entries: [suspectEntry, supersededEntry] }, null, 2)
      );

      const result = await composeIR(extractedPath, undefined, undefined, inferredPath);

      expect(result.approvedInferredIds.has(suspectEntry.id)).toBe(false);
      expect(result.approvedInferredIds.has(supersededEntry.id)).toBe(false);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// I3 — supersede chains for inferred entries
// ---------------------------------------------------------------------------

describe("composeIR — I3 inferred supersede chain", () => {
  it("superseded inferred entry is hidden from inferredEntries output", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-supersede-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const oldEntry = makeInferredEntry({
        id: stableId("inferred", "allocation:function-xyz:old"),
        status: "superseded",
      });
      const newEntry = makeInferredEntry({
        id: stableId("inferred", "allocation:function-xyz:new"),
        status: "approved",
        supersedes: oldEntry.id,
      });

      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(
        inferredPath,
        JSON.stringify({ entries: [oldEntry, newEntry] }, null, 2)
      );

      const result = await composeIR(extractedPath, undefined, undefined, inferredPath);

      // Superseded entry NOT in output
      expect(result.inferredEntries.some((e) => e.id === oldEntry.id)).toBe(false);
      // New entry IS in output
      expect(result.inferredEntries.some((e) => e.id === newEntry.id)).toBe(true);
      // Only new entry id in approved set
      expect(result.approvedInferredIds.has(oldEntry.id)).toBe(false);
      expect(result.approvedInferredIds.has(newEntry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// I4 — suspect premise propagation (A6)
// ---------------------------------------------------------------------------

describe("composeIR — I4 suspect premise propagation (A6)", () => {
  it("inferred entry whose premise prose id is suspect composes as suspect", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-suspect-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      // Prose entry with status suspect
      const suspectProseId = stableId("prose", "synthetic:suspect-doc");
      const proseFile = {
        entries: [
          {
            id: suspectProseId,
            kind: "requirement",
            fields: {},
            citation: {
              docId: "doc-suspect",
              docSha256: "aa".repeat(32),
              chunkId: "chunk-001",
              sectionPath: "S1",
              quote: "Suspect requirement text.",
            },
            approvedBy: "test",
            approvedAt: "2026-06-11T00:00:00.000Z",
            candidateId: "cand-suspect",
            status: "suspect",
          },
        ],
      };
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify(proseFile, null, 2));

      // Inferred entry whose premise is the suspect prose entry
      const inferredEntry = makeInferredEntry({
        premises: [suspectProseId], // premise is suspect
        status: "approved", // stored as approved, but should compose as suspect
      });
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [inferredEntry] }, null, 2));

      const result = await composeIR(extractedPath, proseApprovedPath, undefined, inferredPath);

      // The inferred entry must compose with status:'suspect'
      expect(result.inferredEntries).toHaveLength(1);
      expect(result.inferredEntries[0].status).toBe("suspect");
      // And must NOT be in approvedInferredIds (suspect != approved)
      expect(result.approvedInferredIds.has(inferredEntry.id)).toBe(false);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("inferred entry whose premise id is superseded composes as suspect", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-superseded-premise-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      // Prose file: oldEntry is superseded by newEntry
      const oldProseId = stableId("prose", "synthetic:old-prose");
      const newProseId = stableId("prose", "synthetic:new-prose");
      const proseFile = {
        entries: [
          {
            id: oldProseId,
            kind: "requirement",
            fields: {},
            citation: {
              docId: "doc-001",
              docSha256: "aa".repeat(32),
              chunkId: "chunk-old",
              sectionPath: "S1",
              quote: "Old requirement.",
            },
            approvedBy: "test",
            approvedAt: "2026-06-11T00:00:00.000Z",
            candidateId: "cand-old",
            status: "superseded",
          },
          {
            id: newProseId,
            kind: "requirement",
            fields: {},
            citation: {
              docId: "doc-001",
              docSha256: "bb".repeat(32),
              chunkId: "chunk-new",
              sectionPath: "S1",
              quote: "New requirement.",
            },
            approvedBy: "test",
            approvedAt: "2026-06-11T00:00:00.000Z",
            candidateId: "cand-new",
            status: "approved",
            supersedes: oldProseId,
          },
        ],
      };
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify(proseFile, null, 2));

      // Inferred entry whose premise points to the OLD (superseded) prose entry
      const inferredEntry = makeInferredEntry({
        premises: [oldProseId],
        status: "approved",
      });
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [inferredEntry] }, null, 2));

      const result = await composeIR(extractedPath, proseApprovedPath, undefined, inferredPath);

      // Inferred entry composes as suspect because its premise is superseded
      expect(result.inferredEntries).toHaveLength(1);
      expect(result.inferredEntries[0].status).toBe("suspect");
      expect(result.approvedInferredIds.has(inferredEntry.id)).toBe(false);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("inferred entry with all premises resolving to approved entries composes as approved", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-ok-premise-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      // Prose entry that is approved and valid
      const approvedProseId = stableId("prose", "synthetic:approved-doc");
      const proseFile = {
        entries: [
          {
            id: approvedProseId,
            kind: "requirement",
            fields: {},
            citation: {
              docId: "doc-001",
              docSha256: "aa".repeat(32),
              chunkId: "chunk-001",
              sectionPath: "S1",
              quote: "The system shall do a thing.",
            },
            approvedBy: "test",
            approvedAt: "2026-06-11T00:00:00.000Z",
            candidateId: "cand-001",
            status: "approved",
          },
        ],
      };
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify(proseFile, null, 2));

      // Inferred entry whose premise is the approved prose entry
      const inferredEntry = makeInferredEntry({
        premises: [approvedProseId],
        status: "approved",
      });
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [inferredEntry] }, null, 2));

      const result = await composeIR(extractedPath, proseApprovedPath, undefined, inferredPath);

      // Must compose as approved (all premises are OK)
      expect(result.inferredEntries).toHaveLength(1);
      expect(result.inferredEntries[0].status).toBe("approved");
      expect(result.approvedInferredIds.has(inferredEntry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("inferred entry with a premise id not in composed IR at all composes as suspect", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-missing-premise-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      // No prose layer — just corpus
      const inferredEntry = makeInferredEntry({
        premises: ["nonexistent-premise-id-xyz"],
        status: "approved",
      });
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [inferredEntry] }, null, 2));

      const result = await composeIR(extractedPath, undefined, undefined, inferredPath);

      // Premise not resolvable → suspect
      expect(result.inferredEntries).toHaveLength(1);
      expect(result.inferredEntries[0].status).toBe("suspect");
      expect(result.approvedInferredIds.has(inferredEntry.id)).toBe(false);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("corpus entity id is a valid premise (extraction ground truth)", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-inferred-corpus-premise-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      // Premise is a corpus entity id — should be valid
      const inferredEntry = makeInferredEntry({
        premises: ["requirement-abc"], // corpus entity id
        status: "approved",
      });
      const inferredPath = path.join(dir, "inferred-approved.json");
      await fs.writeFile(inferredPath, JSON.stringify({ entries: [inferredEntry] }, null, 2));

      const result = await composeIR(extractedPath, undefined, undefined, inferredPath);

      // Corpus entity premise resolves → stays approved
      expect(result.inferredEntries).toHaveLength(1);
      expect(result.inferredEntries[0].status).toBe("approved");
      expect(result.approvedInferredIds.has(inferredEntry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// I5 — A9 backward-compat: absent inferredApprovedPath
// ---------------------------------------------------------------------------

describe("composeIR — I5 backward-compat (no inferred layer)", () => {
  it("without inferredApprovedPath: inferredEntries is empty array", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-compat-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const result = await composeIR(extractedPath);

      expect(result.inferredEntries).toHaveLength(0);
      expect(result.approvedInferredIds.size).toBe(0);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("without inferredApprovedPath: extracted and proseEntries are unchanged", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-compat-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const withoutInferred = await composeIR(extractedPath);

      // Extracted portion is unchanged
      expect(withoutInferred.extracted.schema_version).toBe(SCHEMA_VERSION);
      expect(withoutInferred.extracted.needs).toHaveLength(1);
      // proseEntries empty (no prose layer either)
      expect(withoutInferred.proseEntries).toHaveLength(0);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("with prose layer only (no inferred): two-layer compose identical with and without undefined inferred path", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-compat-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const proseFile = {
        entries: [
          {
            id: stableId("prose", "synthetic:approved"),
            kind: "requirement",
            fields: {},
            citation: {
              docId: "doc-001",
              docSha256: "aa".repeat(32),
              chunkId: "chunk-001",
              sectionPath: "S1",
              quote: "The system shall do a thing.",
            },
            approvedBy: "test",
            approvedAt: "2026-06-11T00:00:00.000Z",
            candidateId: "cand-001",
            status: "approved",
          },
        ],
      };
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify(proseFile, null, 2));

      const withoutInferred = await composeIR(extractedPath, proseApprovedPath);
      const withUndefinedInferred = await composeIR(extractedPath, proseApprovedPath, undefined, undefined);

      // prose layer behavior identical
      expect(withoutInferred.proseEntries).toHaveLength(1);
      expect(withUndefinedInferred.proseEntries).toHaveLength(1);
      expect(withoutInferred.approvedProseIds.size).toBe(1);
      expect(withUndefinedInferred.approvedProseIds.size).toBe(1);
      // both have empty inferred layer
      expect(withoutInferred.inferredEntries).toHaveLength(0);
      expect(withUndefinedInferred.inferredEntries).toHaveLength(0);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});
