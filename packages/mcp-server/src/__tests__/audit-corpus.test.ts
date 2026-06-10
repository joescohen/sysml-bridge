/**
 * Corpus loader + GATE-03 resolution set tests (audit-corpus.test.ts)
 *
 * Tests three behaviors:
 * 1. loadCorpus rejects (zod throw) on malformed JSON missing schema_version
 * 2. buildResolutionSet on a minimal fixture contains: every entity id, every
 *    naturalKey, every entity name, and all six allowlist values
 * 3. The real extracted.json parses clean via loadCorpus
 *    (guards that GATE-03 existence checks are sound against the live file)
 */

import { describe, it, expect, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import {
  loadCorpus,
  loadCorpusCached,
  clearCorpusCache,
  buildResolutionSet,
  ALLOWLIST,
} from "../audit/corpus.js";

// Minimal valid Extracted fixture
const MINIMAL_EXTRACTED = {
  schema_version: "1.0.0",
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

afterEach(() => {
  clearCorpusCache();
});

describe("ALLOWLIST", () => {
  it("contains all six allowlisted values", () => {
    expect(ALLOWLIST.has("model-asserted")).toBe(true);
    expect(ALLOWLIST.has("C&C")).toBe(true);
    expect(ALLOWLIST.has("Demonstration")).toBe(true);
    expect(ALLOWLIST.has("Test")).toBe(true);
    expect(ALLOWLIST.has("Analysis")).toBe(true);
    expect(ALLOWLIST.has("Inspection")).toBe(true);
  });

  it("has exactly six entries", () => {
    expect(ALLOWLIST.size).toBe(6);
  });
});

describe("loadCorpus", () => {
  it("parses a valid Extracted document", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-corpus-test-"));
    const file = path.join(dir, "extracted.json");
    await fs.writeFile(file, JSON.stringify(MINIMAL_EXTRACTED));
    try {
      const result = await loadCorpus(file);
      expect(result.schema_version).toBe("1.0.0");
      expect(result.requirements).toHaveLength(1);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("throws (zod) on malformed JSON missing schema_version", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-corpus-test-"));
    const file = path.join(dir, "extracted.json");
    const malformed = { subsystem: "Bad", needs: [], requirements: [], functions: [], components: [], satisfies: [], allocations: [] };
    await fs.writeFile(file, JSON.stringify(malformed));
    try {
      await expect(loadCorpus(file)).rejects.toThrow();
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });

  it("throws on completely invalid JSON", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-corpus-test-"));
    const file = path.join(dir, "extracted.json");
    await fs.writeFile(file, "not json {{{");
    try {
      await expect(loadCorpus(file)).rejects.toThrow();
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

describe("buildResolutionSet", () => {
  it("contains all six ALLOWLIST values", () => {
    const c = MINIMAL_EXTRACTED as any;
    const s = buildResolutionSet(c);
    expect(s.has("model-asserted")).toBe(true);
    expect(s.has("C&C")).toBe(true);
    expect(s.has("Demonstration")).toBe(true);
    expect(s.has("Test")).toBe(true);
    expect(s.has("Analysis")).toBe(true);
    expect(s.has("Inspection")).toBe(true);
  });

  it("contains the requirement id, naturalKey, and name", () => {
    const c = MINIMAL_EXTRACTED as any;
    const s = buildResolutionSet(c);
    expect(s.has("requirement-abc")).toBe(true);
    expect(s.has("CC-1")).toBe(true);
    expect(s.has("Do Thing")).toBe(true);
  });

  it("contains the need id, naturalKey, and name", () => {
    const c = MINIMAL_EXTRACTED as any;
    const s = buildResolutionSet(c);
    expect(s.has("need-001")).toBe(true);
    expect(s.has("N1")).toBe(true);
    expect(s.has("Test Need")).toBe(true);
  });

  it("contains the function id, naturalKey, and name", () => {
    const c = MINIMAL_EXTRACTED as any;
    const s = buildResolutionSet(c);
    expect(s.has("function-xyz")).toBe(true);
    expect(s.has("F1")).toBe(true);
    expect(s.has("Func Thing")).toBe(true);
  });

  it("contains the component id, naturalKey, and name", () => {
    const c = MINIMAL_EXTRACTED as any;
    const s = buildResolutionSet(c);
    expect(s.has("component-111")).toBe(true);
    expect(s.has("COMP-1")).toBe(true);
    expect(s.has("Widget")).toBe(true);
  });
});

describe("loadCorpusCached", () => {
  it("resolves null for a nonexistent path without throwing", async () => {
    const result = await loadCorpusCached("/nonexistent/path/extracted.json");
    expect(result).toBeNull();
  });

  it("caches null for a nonexistent path (second call also returns null without file access)", async () => {
    const fakePath = "/nonexistent/cached/extracted.json";
    const first = await loadCorpusCached(fakePath);
    const second = await loadCorpusCached(fakePath);
    expect(first).toBeNull();
    expect(second).toBeNull();
  });

  it("returns a valid corpus and caches it on second call", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-corpus-test-"));
    const file = path.join(dir, "extracted.json");
    await fs.writeFile(file, JSON.stringify(MINIMAL_EXTRACTED));
    try {
      const first = await loadCorpusCached(file);
      expect(first).not.toBeNull();
      expect(first!.schema_version).toBe("1.0.0");

      // Second call must return same object (cached)
      const second = await loadCorpusCached(file);
      expect(second).toBe(first);
    } finally {
      await fs.rm(dir, { recursive: true, force: true });
    }
  });
});

describe("real extracted.json parse", () => {
  const EXTRACTED_PATH = path.resolve(
    __dirname,
    "../../../../examples/angars/model/extracted.json"
  );

  it("parses the real extracted.json clean (GATE-03 corpus is loadable)", async () => {
    const result = await loadCorpus(EXTRACTED_PATH);
    expect(result.schema_version).toBe("1.0.0");
    expect(result.subsystem).toBe("ANGARS");
    // Must have all required arrays non-empty
    expect(result.needs.length).toBeGreaterThan(0);
    expect(result.requirements.length).toBeGreaterThan(0);
    expect(result.functions.length).toBeGreaterThan(0);
    expect(result.components.length).toBeGreaterThan(0);
  });

  it("buildResolutionSet on real corpus resolves a known requirement id", async () => {
    const corpus = await loadCorpus(EXTRACTED_PATH);
    const s = buildResolutionSet(corpus);
    // The allowlist values must always be present
    expect(s.has("model-asserted")).toBe(true);
    expect(s.has("C&C")).toBe(true);
    // At least one real entity id must be present (first requirement)
    const firstReqId = corpus.requirements[0].id;
    expect(s.has(firstReqId)).toBe(true);
    // At least one real naturalKey
    const firstNk = corpus.requirements[0].naturalKey;
    expect(s.has(firstNk)).toBe(true);
  });
});
