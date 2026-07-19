/**
 * bm25.test.ts — BM25 lexical index: ranking sanity, determinism, edge cases.
 *
 * Covers:
 *   - tokenizer: lowercase + alphanumeric splits
 *   - ranking: a chunk with BOTH query terms outranks one with a single term
 *     (+ a fail-able positive control: the assertion is non-vacuous)
 *   - deterministic tie-breaking: equal scores → chunkId ascending
 *   - empty store + empty query + zero-k graceful paths
 */

import { describe, it, expect } from "vitest";
import { Bm25Index, tokenize } from "../bm25.js";
import type { RetrievalChunk } from "../bm25.js";

describe("tokenize", () => {
  it("lowercases and splits on non-alphanumeric runs", () => {
    expect(tokenize("Fuel-Pump v2! (status)")).toEqual(["fuel", "pump", "v2", "status"]);
  });
  it("returns [] for empty / symbol-only text", () => {
    expect(tokenize("")).toEqual([]);
    expect(tokenize("!!! --- ...")).toEqual([]);
  });
});

describe("Bm25Index — ranking sanity", () => {
  const chunks: RetrievalChunk[] = [
    { chunkId: "chunk-both", sectionPath: "3.1", text: "the fuel pump moves fuel to the tank" },
    { chunkId: "chunk-fuel", sectionPath: "3.2", text: "the fuel reservoir stores reserves" },
    { chunkId: "chunk-none", sectionPath: "3.3", text: "the operator console shows telemetry" },
  ];

  it("a chunk containing BOTH query terms outranks one containing a single term", () => {
    const index = new Bm25Index(chunks);
    const hits = index.query("fuel pump", 5);

    // Positive control: the single-term chunk MUST be present and scored, so the
    // ranking assertion below is non-vacuous (not passing because it's absent).
    const ids = hits.map((h) => h.chunkId);
    expect(ids).toContain("chunk-both");
    expect(ids).toContain("chunk-fuel");
    expect(ids).not.toContain("chunk-none"); // no query term → score 0 → excluded

    // The two-term chunk ranks first, and strictly above the one-term chunk.
    expect(hits[0]!.chunkId).toBe("chunk-both");
    const both = hits.find((h) => h.chunkId === "chunk-both")!;
    const fuel = hits.find((h) => h.chunkId === "chunk-fuel")!;
    expect(both.score).toBeGreaterThan(fuel.score);
  });

  it("respects topK", () => {
    const index = new Bm25Index(chunks);
    expect(index.query("fuel pump", 1)).toHaveLength(1);
  });
});

describe("Bm25Index — deterministic tie-breaking", () => {
  it("equal-scoring chunks are ordered by chunkId ascending", () => {
    // Two identical texts → identical scores → chunkId decides order.
    const chunks: RetrievalChunk[] = [
      { chunkId: "chunk-zzz", sectionPath: "b", text: "docking latch engaged" },
      { chunkId: "chunk-aaa", sectionPath: "a", text: "docking latch engaged" },
    ];
    const index = new Bm25Index(chunks);
    const hits = index.query("docking latch", 5);
    expect(hits.map((h) => h.chunkId)).toEqual(["chunk-aaa", "chunk-zzz"]);
  });
});

describe("Bm25Index — determinism", () => {
  it("two indexes over the same chunks return byte-identical results", () => {
    const chunks: RetrievalChunk[] = [
      { chunkId: "c1", sectionPath: "1", text: "alpha beta gamma" },
      { chunkId: "c2", sectionPath: "2", text: "beta gamma delta" },
      { chunkId: "c3", sectionPath: "3", text: "gamma delta epsilon" },
    ];
    const a = new Bm25Index(chunks).query("beta gamma", 3);
    const b = new Bm25Index([...chunks]).query("beta gamma", 3);
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});

describe("Bm25Index — edge cases", () => {
  it("empty store returns [] and size 0", () => {
    const index = new Bm25Index([]);
    expect(index.size).toBe(0);
    expect(index.query("anything", 5)).toEqual([]);
    expect([...index.chunkIds()]).toEqual([]);
  });
  it("empty / symbol-only query returns []", () => {
    const index = new Bm25Index([{ chunkId: "c1", sectionPath: "1", text: "hello world" }]);
    expect(index.query("", 5)).toEqual([]);
    expect(index.query("!!!", 5)).toEqual([]);
  });
  it("zero / negative topK returns []", () => {
    const index = new Bm25Index([{ chunkId: "c1", sectionPath: "1", text: "hello world" }]);
    expect(index.query("hello", 0)).toEqual([]);
    expect(index.query("hello", -3)).toEqual([]);
  });
  it("duplicate chunkIds are collapsed (first occurrence wins)", () => {
    const index = new Bm25Index([
      { chunkId: "dup", sectionPath: "1", text: "first body" },
      { chunkId: "dup", sectionPath: "2", text: "second body" },
    ]);
    expect(index.size).toBe(1);
    const hits = index.query("body", 5);
    expect(hits).toHaveLength(1);
    expect(hits[0]!.sectionPath).toBe("1");
  });
});
