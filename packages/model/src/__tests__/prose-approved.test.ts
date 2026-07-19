/**
 * prose-approved.test.ts
 *
 * RED-first tests for G-B gap closure:
 *   C6  — append-only prose-approved.json: superseding entry hides old from composeIR output,
 *          but RETAINS old entry in the file.
 *   C7  — re-running extractor with existing prose layer leaves extracted.json AND
 *          prose-approved.json byte-identical; composeIR output stable.
 *   C8  — composeIR no-prose path === today's behavior (backward-compat).
 *   C9  — tamper source doc hash → entries flip to status:suspect; composeIR still composes
 *          them, but with status:suspect on the returned entry.
 *   C10 — ProseApprovedEntry zod schema validates a well-formed entry and rejects malformed.
 *   C11 — composeIR returns a ProseComposedIR where each prose entry has a status field.
 *   C12 — composeIR(extractedPath) with no proseApprovedPath returns an object byte-identical
 *          to the extracted corpus (the composeIR-no-prose contract).
 */

import { describe, it, expect } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import {
  ProseApprovedEntrySchema,
  composeIR,
  type ProseApprovedEntry,
} from "../prose-approved.js";
import { stableId } from "../stable-id.js";
import { SCHEMA_VERSION } from "../schema.js";

// ---------------------------------------------------------------------------
// Synthetic corpus fixture (no real corpus text — safe to commit)
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

// Synthetic ingest manifest (no real doc content)
function makeManifest(docId: string, docSha256: string) {
  return {
    docId,
    docSha256,
    ingestedAt: "2026-06-10T00:00:00.000Z",
    chunks: [{ chunkId: "chunk-001", sectionPath: "Section 1", offset: 0, length: 50 }],
  };
}

// A well-formed ProseApprovedEntry fixture (synthetic — no corpus text)
function makeApprovedEntry(overrides: Partial<ProseApprovedEntry> = {}): ProseApprovedEntry {
  const id = stableId("prose", "synthetic:req:N1:The system shall do a synthetic thing.");
  return {
    id,
    kind: "requirement",
    fields: { naturalKey: "SYN-1", name: "Synthetic Req" },
    citation: {
      docId: "synthetic-doc-001",
      docSha256: "aabbcc00112233440000000000000000aabbcc00112233440000000000000000",
      chunkId: "chunk-001",
      sectionPath: "Section 1",
      quote: "The system shall do a synthetic thing.",
    },
    approvedBy: "test-user",
    approvedAt: "2026-06-10T00:00:00.000Z",
    candidateId: "candidate-abc",
    status: "approved",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// C10 — ProseApprovedEntrySchema validates / rejects
// ---------------------------------------------------------------------------

describe("ProseApprovedEntrySchema", () => {
  it("accepts a well-formed approved entry", () => {
    const entry = makeApprovedEntry();
    const result = ProseApprovedEntrySchema.safeParse(entry);
    expect(result.success).toBe(true);
  });

  it("accepts a superseded entry (with supersedes field)", () => {
    const old = makeApprovedEntry({ status: "superseded", supersedes: "prose-prev" });
    const result = ProseApprovedEntrySchema.safeParse(old);
    expect(result.success).toBe(true);
  });

  it("accepts a suspect entry", () => {
    const suspect = makeApprovedEntry({ status: "suspect" });
    const result = ProseApprovedEntrySchema.safeParse(suspect);
    expect(result.success).toBe(true);
  });

  it("rejects an entry missing required 'citation' field", () => {
    const bad = { ...makeApprovedEntry() } as Record<string, unknown>;
    delete bad.citation;
    const result = ProseApprovedEntrySchema.safeParse(bad);
    expect(result.success).toBe(false);
  });

  it("rejects an entry with invalid kind", () => {
    const bad = makeApprovedEntry({ kind: "bogusKind" as never });
    const result = ProseApprovedEntrySchema.safeParse(bad);
    expect(result.success).toBe(false);
  });

  it("rejects an entry with invalid status", () => {
    const bad = makeApprovedEntry({ status: "pending" as never });
    const result = ProseApprovedEntrySchema.safeParse(bad);
    expect(result.success).toBe(false);
  });

  it("rejects a citation.quote > 300 chars", () => {
    const longQuote = "x".repeat(301);
    const bad = makeApprovedEntry({ citation: { ...makeApprovedEntry().citation, quote: longQuote } });
    const result = ProseApprovedEntrySchema.safeParse(bad);
    expect(result.success).toBe(false);
  });

  it("accepts a citation.quote of exactly 300 chars", () => {
    const exactQuote = "x".repeat(300);
    const entry = makeApprovedEntry({ citation: { ...makeApprovedEntry().citation, quote: exactQuote } });
    const result = ProseApprovedEntrySchema.safeParse(entry);
    expect(result.success).toBe(true);
  });

  it("all six valid kinds are accepted", () => {
    const kinds: ProseApprovedEntry["kind"][] = [
      "requirement", "need", "mode", "modeTransition", "interface", "component", "function",
    ];
    for (const kind of kinds) {
      const result = ProseApprovedEntrySchema.safeParse(makeApprovedEntry({ kind }));
      expect(result.success, `kind=${kind} should be accepted`).toBe(true);
    }
  });
});

// ---------------------------------------------------------------------------
// C12 — composeIR no-prose backward-compat
// ---------------------------------------------------------------------------

describe("composeIR — backward-compat (no prose layer)", () => {
  it("with no proseApprovedPath returns an object with the extracted corpus data intact", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-ir-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const result = await composeIR(extractedPath);

      // Must have the same structure as the extracted corpus
      expect(result.extracted.schema_version).toBe(SCHEMA_VERSION);
      expect(result.extracted.needs).toHaveLength(1);
      expect(result.extracted.requirements).toHaveLength(1);
      // proseEntries must be empty array (no prose layer)
      expect(result.proseEntries).toHaveLength(0);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("composeIR result extracted is structurally identical to raw extracted parse", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-ir-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const result = await composeIR(extractedPath);
      // The extracted portion must be structurally equal to what was written
      expect(result.extracted).toMatchObject({
        schema_version: SCHEMA_VERSION,
        subsystem: "TestSub",
        needs: expect.any(Array),
        requirements: expect.any(Array),
      });
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// C6 — append-only: superseding entry hides old from composeIR, retains in file
// ---------------------------------------------------------------------------

describe("composeIR — C6 supersede chain", () => {
  it("superseded entry is hidden from composeIR proseEntries, but file retains both", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-supersede-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const oldEntry = makeApprovedEntry({ status: "superseded" });
      const newEntry = makeApprovedEntry({
        id: stableId("prose", "synthetic:req:N1:The system shall do a new thing."),
        status: "approved",
        supersedes: oldEntry.id,
        citation: {
          ...makeApprovedEntry().citation,
          quote: "The system shall do a new thing.",
        },
      });

      const proseApprovedPath = path.join(dir, "prose-approved.json");
      // Append-only: write file with BOTH entries (old first, new second)
      await fs.writeFile(
        proseApprovedPath,
        JSON.stringify({ entries: [oldEntry, newEntry] }, null, 2)
      );

      const result = await composeIR(extractedPath, proseApprovedPath);

      // composeIR output must NOT include the superseded entry
      const supersededInOutput = result.proseEntries.filter(
        (e) => e.status === "superseded"
      );
      expect(supersededInOutput).toHaveLength(0);

      // The new (approved) entry IS in the output
      const approvedInOutput = result.proseEntries.filter(
        (e) => e.id === newEntry.id && e.status === "approved"
      );
      expect(approvedInOutput).toHaveLength(1);

      // The FILE retains BOTH entries
      const fileContent = await fs.readFile(proseApprovedPath, "utf8");
      const parsed = JSON.parse(fileContent) as { entries: ProseApprovedEntry[] };
      expect(parsed.entries).toHaveLength(2);
      expect(parsed.entries.some((e) => e.id === oldEntry.id)).toBe(true);
      expect(parsed.entries.some((e) => e.id === newEntry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// C7 — idempotence: composeIR output is stable on re-run with same inputs
// ---------------------------------------------------------------------------

describe("composeIR — C7 idempotence", () => {
  it("two composeIR calls with same inputs produce identical proseEntries output", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-idem-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const entry = makeApprovedEntry();
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify({ entries: [entry] }, null, 2));

      const result1 = await composeIR(extractedPath, proseApprovedPath);
      const result2 = await composeIR(extractedPath, proseApprovedPath);

      // Both calls must produce identical output
      expect(JSON.stringify(result1)).toBe(JSON.stringify(result2));
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("extracted.json unchanged after two composeIR calls (file not touched)", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-idem-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      const content = JSON.stringify(MINIMAL_EXTRACTED);
      await fs.writeFile(extractedPath, content);

      await composeIR(extractedPath);
      await composeIR(extractedPath);

      const afterContent = await fs.readFile(extractedPath, "utf8");
      expect(afterContent).toBe(content);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// C9 — tampered source doc hash → entries flip to status:suspect; still compose
// ---------------------------------------------------------------------------

describe("composeIR — C9 suspect status on hash mismatch", () => {
  it("entry whose citation.docSha256 doesn't match manifest hash composes with status:suspect", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-suspect-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      // Real hash in the manifest
      const realHash = "aabbcc00112233440000000000000000aabbcc00112233440000000000000000";
      // Entry cites a DIFFERENT hash (tampered)
      const tamperedHash = "deadbeef00000000deadbeef00000000deadbeef00000000deadbeef00000000";

      const manifest = makeManifest("synthetic-doc-001", realHash);
      const manifestPath = path.join(dir, "prose-ingest-manifest.json");
      await fs.writeFile(manifestPath, JSON.stringify({ documents: [manifest] }));

      // Entry has the TAMPERED hash — mismatch against manifest
      const entry = makeApprovedEntry({
        citation: {
          ...makeApprovedEntry().citation,
          docId: "synthetic-doc-001",
          docSha256: tamperedHash,
        },
      });
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify({ entries: [entry] }, null, 2));

      // Pass the manifestPath to composeIR for hash verification
      const result = await composeIR(extractedPath, proseApprovedPath, manifestPath);

      // The entry must be present in output (still composes)
      expect(result.proseEntries).toHaveLength(1);

      // BUT status must be 'suspect'
      expect(result.proseEntries[0].status).toBe("suspect");
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("entry whose citation.docSha256 matches manifest hash retains status:approved", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-suspect-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const realHash = "aabbcc00112233440000000000000000aabbcc00112233440000000000000000";
      const manifest = makeManifest("synthetic-doc-001", realHash);
      const manifestPath = path.join(dir, "prose-ingest-manifest.json");
      await fs.writeFile(manifestPath, JSON.stringify({ documents: [manifest] }));

      // Entry has the CORRECT hash — no mismatch
      const entry = makeApprovedEntry({
        citation: {
          ...makeApprovedEntry().citation,
          docId: "synthetic-doc-001",
          docSha256: realHash,
        },
      });
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify({ entries: [entry] }, null, 2));

      const result = await composeIR(extractedPath, proseApprovedPath, manifestPath);

      expect(result.proseEntries).toHaveLength(1);
      expect(result.proseEntries[0].status).toBe("approved");
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

// ---------------------------------------------------------------------------
// C11 — composeIR returns ProseComposedIR with approved prose ids
// ---------------------------------------------------------------------------

describe("composeIR — C11 ProseComposedIR shape", () => {
  it("result has extracted and proseEntries keys", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-shape-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const entry = makeApprovedEntry();
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify({ entries: [entry] }, null, 2));

      const result = await composeIR(extractedPath, proseApprovedPath);

      expect(result).toHaveProperty("extracted");
      expect(result).toHaveProperty("proseEntries");
      expect(Array.isArray(result.proseEntries)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("approved prose entry id is in the approvedProseIds set", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-shape-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const entry = makeApprovedEntry();
      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(proseApprovedPath, JSON.stringify({ entries: [entry] }, null, 2));

      const result = await composeIR(extractedPath, proseApprovedPath);

      // The approved id must be in the approvedProseIds set
      expect(result.approvedProseIds).toBeDefined();
      expect(result.approvedProseIds.has(entry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("superseded entry id is NOT in approvedProseIds", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "compose-shape-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const oldEntry = makeApprovedEntry({ status: "superseded" });
      const newEntry = makeApprovedEntry({
        id: stableId("prose", "synthetic:req:N2:New thing."),
        status: "approved",
        supersedes: oldEntry.id,
        citation: { ...makeApprovedEntry().citation, quote: "New thing." },
      });

      const proseApprovedPath = path.join(dir, "prose-approved.json");
      await fs.writeFile(
        proseApprovedPath,
        JSON.stringify({ entries: [oldEntry, newEntry] }, null, 2)
      );

      const result = await composeIR(extractedPath, proseApprovedPath);

      // superseded id must NOT be in approvedProseIds
      expect(result.approvedProseIds.has(oldEntry.id)).toBe(false);
      // approved id IS
      expect(result.approvedProseIds.has(newEntry.id)).toBe(true);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});
