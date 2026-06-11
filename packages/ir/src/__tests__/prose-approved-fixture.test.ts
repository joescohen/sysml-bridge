/**
 * prose-approved-fixture.test.ts — Validate the committed prose-approved-fixture.json.
 *
 * This fixture is the deterministic approved-set used by G-E/C10/C11 downstream tests.
 * Tests verify:
 *   - File parses as valid JSON
 *   - All entries pass ProseApprovedEntrySchema validation
 *   - chunkIds are genuine 32-char hex strings (content-addressed from real PDF runs)
 *   - docSha256 is a valid 64-char hex string matching Appendix_G_ANGARS_ASPEC.pdf
 *   - Fixture has exactly 3 requirement-kind entries
 *   - composeIR over a synthetic extracted + the fixture produces approvedProseIds
 *
 * The chunkIds and docSha256 in the fixture are derived from the real ANGARS ASPEC PDF.
 * The quote and fields text is synthetic — no corpus text committed.
 */

import { describe, it, expect } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { ProseApprovedEntrySchema } from "../prose-approved.js";
import { composeIR } from "../prose-approved.js";
import { SCHEMA_VERSION } from "../schema.js";

const FIXTURE_PATH = path.resolve(
  __dirname,
  "fixtures/prose-approved-fixture.json"
);

// Real docSha256 for Appendix_G_ANGARS_ASPEC.pdf (derived by gen-prose-fixture.ts)
const ASPEC_DOC_HASH =
  "aa1fb0130f2632e1be256e4a024b1232c1f425d774a41e389cdcd6e6b9734012";

// Real chunkIds derived by the chunker over the ASPEC PDF (gen-prose-fixture.ts)
const EXPECTED_CHUNK_IDS = [
  "a4845108f5366629fc25e81c4a20d898",
  "e6b7b60cfc8881b559bf4ce2cbd4318c",
  "2e1eb28fd38997c88ab6c20af49131de",
];

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

describe("prose-approved-fixture.json — structural validity", () => {
  it("file parses as valid JSON", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    expect(() => JSON.parse(raw)).not.toThrow();
  });

  it("has exactly 3 entries", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as { entries: unknown[] };
    expect(entries).toHaveLength(3);
  });

  it("all entries pass ProseApprovedEntrySchema validation", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as { entries: unknown[] };
    for (const [i, entry] of entries.entries()) {
      const result = ProseApprovedEntrySchema.safeParse(entry);
      expect(result.success, `entry[${i}] failed schema: ${JSON.stringify(result)}`).toBe(true);
    }
  });

  it("all entries have kind='requirement'", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as { entries: Array<{ kind: string }> };
    for (const [i, entry] of entries.entries()) {
      expect(entry.kind, `entry[${i}].kind`).toBe("requirement");
    }
  });

  it("all entries have status='approved'", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as { entries: Array<{ status: string }> };
    for (const [i, entry] of entries.entries()) {
      expect(entry.status, `entry[${i}].status`).toBe("approved");
    }
  });
});

describe("prose-approved-fixture.json — genuine chunk IDs (ANGARS ASPEC)", () => {
  it("docSha256 matches the known Appendix_G_ANGARS_ASPEC.pdf hash", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as {
      entries: Array<{ citation: { docSha256: string } }>;
    };
    for (const [i, entry] of entries.entries()) {
      expect(entry.citation.docSha256, `entry[${i}].citation.docSha256`).toBe(ASPEC_DOC_HASH);
    }
  });

  it("chunkIds are 32-char lowercase hex strings", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as {
      entries: Array<{ citation: { chunkId: string } }>;
    };
    for (const [i, entry] of entries.entries()) {
      expect(entry.citation.chunkId, `entry[${i}].citation.chunkId`).toMatch(
        /^[0-9a-f]{32}$/
      );
    }
  });

  it("chunkIds match the three expected content-addressed ids from gen-prose-fixture.ts", async () => {
    const raw = await fs.readFile(FIXTURE_PATH, "utf8");
    const { entries } = JSON.parse(raw) as {
      entries: Array<{ citation: { chunkId: string } }>;
    };
    const chunkIds = entries.map((e) => e.citation.chunkId);
    for (const expectedId of EXPECTED_CHUNK_IDS) {
      expect(chunkIds).toContain(expectedId);
    }
  });
});

describe("prose-approved-fixture.json — composeIR integration", () => {
  it("composeIR over fixture produces 3 active entries all in approvedProseIds", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "fixture-compose-test-"));
    try {
      const extractedPath = path.join(dir, "extracted.json");
      await fs.writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

      const ir = await composeIR(extractedPath, FIXTURE_PATH);

      expect(ir.proseEntries).toHaveLength(3);
      for (const entry of ir.proseEntries) {
        expect(ir.approvedProseIds.has(entry.id)).toBe(true);
      }
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});
