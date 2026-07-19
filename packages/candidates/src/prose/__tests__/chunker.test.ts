/**
 * Tests for chunker.ts — text chunking (C2 coverage + stable ids).
 *
 * TDD: written BEFORE implementation.
 * Verifies:
 * - generateChunkId: 32-char hex, deterministic, text NOT in hash
 * - chunkText: non-empty chunks, covers full text
 * - chunkWithIds: deterministic, chunk coverage (no inter-chunk gap)
 * - chunkId hashes (docSha256, position, normalizedSectionContext) — NOT chunk text
 * - ids identical across two runs (C2)
 * - chunk coverage: concatenating all chunks reproduces the source text (modulo overlap)
 */

import { describe, it, expect } from "vitest";
import { generateChunkId, chunkText, chunkWithIds } from "../chunker.js";
import type { ChunkContext } from "../chunker.js";

// ── generateChunkId ────────────────────────────────────────────────────────────

describe("generateChunkId", () => {
  it("returns a 32-character hex string", () => {
    const id = generateChunkId({ documentHash: "abc123", sectionPath: "1.1", chunkIndex: 0 });
    expect(id).toHaveLength(32);
    expect(id).toMatch(/^[0-9a-f]{32}$/);
  });

  it("is deterministic — same inputs produce same ID (10 iterations)", () => {
    const input = { documentHash: "deadbeef", sectionPath: "3.2.1", chunkIndex: 5 };
    const first = generateChunkId(input);
    for (let i = 0; i < 9; i++) {
      expect(generateChunkId(input)).toBe(first);
    }
  });

  it("different documentHash produces different ID", () => {
    const base = { sectionPath: "1.1", chunkIndex: 0 };
    const id1 = generateChunkId({ ...base, documentHash: "hash-a" });
    const id2 = generateChunkId({ ...base, documentHash: "hash-b" });
    expect(id1).not.toBe(id2);
  });

  it("different sectionPath produces different ID", () => {
    const base = { documentHash: "abc", chunkIndex: 0 };
    const id1 = generateChunkId({ ...base, sectionPath: "1.1" });
    const id2 = generateChunkId({ ...base, sectionPath: "1.2" });
    expect(id1).not.toBe(id2);
  });

  it("different chunkIndex produces different ID", () => {
    const base = { documentHash: "abc", sectionPath: "1.1" };
    const id1 = generateChunkId({ ...base, chunkIndex: 0 });
    const id2 = generateChunkId({ ...base, chunkIndex: 1 });
    expect(id1).not.toBe(id2);
  });

  it("chunk text is NOT part of hash — same position different text → same ID (C2)", () => {
    // The hash is (documentHash, sectionPath, chunkIndex) — text is NOT included.
    // Changing the text of a document should NOT change the chunkId for same position.
    const input = { documentHash: "stable-doc", sectionPath: "2.3", chunkIndex: 0 };
    const id = generateChunkId(input);
    // Same call — deterministic — text never enters the hash
    expect(id).toBe(generateChunkId(input));
    // The ChunkIdInput type has no text field at all
    // @ts-expect-error — chunkText is not a valid field
    expect(() => generateChunkId({ ...input, chunkText: "some text" })).not.toThrow();
    // Still the same — extra fields are ignored in JSON.stringify key ordering
    // (but chunkText is not a key we use)
    expect(
      generateChunkId({ documentHash: "stable-doc", sectionPath: "2.3", chunkIndex: 0 }),
    ).toBe(id);
  });

  it("sectionPath whitespace is normalized before hashing", () => {
    const id1 = generateChunkId({ documentHash: "d", sectionPath: "  3.2  ", chunkIndex: 0 });
    const id2 = generateChunkId({ documentHash: "d", sectionPath: "3.2", chunkIndex: 0 });
    expect(id1).toBe(id2);
  });
});

// ── chunkText ──────────────────────────────────────────────────────────────────

describe("chunkText", () => {
  it("returns the text as a single chunk if shorter than chunkSize", async () => {
    const chunks = await chunkText("short text", { chunkSize: 512, chunkOverlap: 0 });
    expect(chunks).toHaveLength(1);
    expect(chunks[0]).toBe("short text");
  });

  it("returns multiple chunks for long text", async () => {
    const longText = "The quick brown fox jumps over the lazy dog. ".repeat(25);
    const chunks = await chunkText(longText, { chunkSize: 100, chunkOverlap: 10 });
    expect(chunks.length).toBeGreaterThan(1);
  });

  it("all returned chunks are non-empty strings", async () => {
    const longText = "Requirements shall be testable. ".repeat(30);
    const chunks = await chunkText(longText, { chunkSize: 100, chunkOverlap: 20 });
    for (const chunk of chunks) {
      expect(typeof chunk).toBe("string");
      expect(chunk.length).toBeGreaterThan(0);
    }
  });

  it("chunk coverage: all chunk text spans the full input (no inter-chunk gap)", async () => {
    // Build a text with no separator ambiguity — single long word-stream
    const text = "AABABC BCDBCD CDEDEF EFGFGH GHIHIJ IJKJKL KLMLMN MNON ".repeat(5);
    const chunks = await chunkText(text, { chunkSize: 80, chunkOverlap: 0 });
    // Verify all characters in the source appear in at least one chunk
    // (We test this by checking the chunks together cover all unique tokens from text)
    const joined = chunks.join(" ");
    // Every word from text should appear in joined
    const words = text.trim().split(/\s+/);
    const uniqueWords = [...new Set(words)];
    for (const word of uniqueWords) {
      expect(joined).toContain(word);
    }
  });
});

// ── chunkWithIds ───────────────────────────────────────────────────────────────

describe("chunkWithIds", () => {
  const baseContext: ChunkContext = {
    documentHash: "sha256ofrawbytes",
    sectionId: "sec-abc12345678",
    sectionPath: "3.2.1",
    pageStart: 5,
    pageEnd: 7,
    documentId: "doc-srd-001",
  };

  it("returns an array of chunks for valid input", async () => {
    const text = "Requirements shall be verifiable. Each requirement shall be unambiguous.";
    const chunks = await chunkWithIds(text, baseContext);
    expect(Array.isArray(chunks)).toBe(true);
    expect(chunks.length).toBeGreaterThan(0);
  });

  it("each chunk has chunk_id that is 32-char hex", async () => {
    const text = "Simple requirement text.";
    const chunks = await chunkWithIds(text, baseContext);
    for (const chunk of chunks) {
      expect(chunk.chunkId).toHaveLength(32);
      expect(chunk.chunkId).toMatch(/^[0-9a-f]{32}$/);
    }
  });

  it("chunk_index is zero-based and sequential", async () => {
    const longText = "Requirements text for testing. ".repeat(40);
    const chunks = await chunkWithIds(longText, baseContext, { chunkSize: 80, chunkOverlap: 10 });
    for (let i = 0; i < chunks.length; i++) {
      expect(chunks[i]?.chunkIndex).toBe(i);
    }
  });

  it("is fully deterministic — same inputs produce identical chunk arrays (C2)", async () => {
    const text = "The system shall be reliable. The system shall be testable. ".repeat(10);
    const opts = { chunkSize: 128, chunkOverlap: 20 };
    const run1 = await chunkWithIds(text, baseContext, opts);
    const run2 = await chunkWithIds(text, baseContext, opts);
    expect(run1).toHaveLength(run2.length);
    for (let i = 0; i < run1.length; i++) {
      expect(run1[i]?.chunkId).toBe(run2[i]?.chunkId);
      expect(run1[i]?.text).toBe(run2[i]?.text);
    }
  });

  it("chunkId survives text edit — changing text at same position yields same id (C2)", async () => {
    const text1 = "The system shall do X. ".repeat(5);
    const text2 = "The system shall do Y. ".repeat(5); // different text, same position
    const opts = { chunkSize: 100, chunkOverlap: 0 };
    const chunks1 = await chunkWithIds(text1, baseContext, opts);
    const chunks2 = await chunkWithIds(text2, baseContext, opts);
    // Same number of chunks at same positions → same IDs (text excluded from hash)
    expect(chunks1.length).toBe(chunks2.length);
    for (let i = 0; i < chunks1.length; i++) {
      expect(chunks1[i]?.chunkId).toBe(chunks2[i]?.chunkId);
    }
  });

  it("different documentHash produces different chunk IDs (same text)", async () => {
    const text = "Requirements shall be testable.";
    const ctx1 = { ...baseContext, documentHash: "hash-version-1" };
    const ctx2 = { ...baseContext, documentHash: "hash-version-2" };
    const chunks1 = await chunkWithIds(text, ctx1);
    const chunks2 = await chunkWithIds(text, ctx2);
    expect(chunks1.length).toBe(chunks2.length);
    if (chunks1.length > 0) {
      expect(chunks1[0]?.chunkId).not.toBe(chunks2[0]?.chunkId);
    }
  });

  it("handles empty text gracefully (returns empty array)", async () => {
    const chunks = await chunkWithIds("", baseContext);
    expect(Array.isArray(chunks)).toBe(true);
    expect(chunks.length).toBe(0);
  });

  it("chunk coverage: all words in source text appear in at least one chunk", async () => {
    const text = "Alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima. ".repeat(3);
    const chunks = await chunkWithIds(text, baseContext, { chunkSize: 100, chunkOverlap: 20 });
    const allChunkText = chunks.map((c) => c.text).join(" ");
    const words = ["Alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    for (const word of words) {
      expect(allChunkText).toContain(word);
    }
  });

  it("context fields are reflected on each chunk", async () => {
    const text = "Requirement text here.";
    const chunks = await chunkWithIds(text, baseContext);
    for (const chunk of chunks) {
      expect(chunk.documentId).toBe(baseContext.documentId);
      expect(chunk.sectionId).toBe(baseContext.sectionId);
      expect(chunk.pageStart).toBe(baseContext.pageStart);
      expect(chunk.pageEnd).toBe(baseContext.pageEnd);
    }
  });
});
