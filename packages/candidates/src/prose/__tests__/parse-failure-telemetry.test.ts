/**
 * parse-failure-telemetry.test.ts — the §6 fix, prose side.
 *
 * Guarantees that the silent-`[]` return sites in AnthropicLlmProvider.propose()
 * are now COUNTED and LOGGED, not silent:
 *   - malformed / schema-invalid / missing-text response → still returns [] (behavior preserved)
 *   - AND provider.counters increments the matching failure class
 *   - AND one console.error line is emitted with provider/method context (no prompt)
 *
 * Also verifies the ingest pipeline surfaces the provider's counter snapshot in
 * IngestPipelineResult.parseFailures.
 *
 * The live Anthropic client is stubbed (no API key, offline).
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { AnthropicLlmProvider } from "../llm-provider.js";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { ChunkContext } from "../chunker.js";
import { RunCounters } from "../../telemetry.js";

// ── Client stubs ────────────────────────────────────────────────────────────

/** Replace the provider's private Anthropic client with a canned text response. */
function stubClientText(provider: AnthropicLlmProvider, text: string): void {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (provider as any).client = {
    messages: {
      create: async () => ({ content: [{ type: "text", text }] }),
    },
  };
}

/** Replace the client with a response that has NO text block. */
function stubClientNoText(provider: AnthropicLlmProvider): void {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (provider as any).client = {
    messages: {
      create: async () => ({ content: [{ type: "tool_use", id: "x", name: "y", input: {} }] }),
    },
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("§6 prose telemetry — silent-[] sites are counted + logged", () => {
  it("malformed proposals JSON → returns [] AND jsonParseError counted AND console.error called", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicLlmProvider("test-key");
    stubClientText(provider, "not valid json {broken");

    const result = await provider.propose("chunk-abc", "some chunk text", "root/section");

    // Behavior preserved: [] still returned.
    expect(result).toEqual([]);
    // Counted.
    expect(provider.counters.snapshot().jsonParseError).toBe(1);
    expect(provider.counters.total).toBe(1);
    // Logged with provider/method context, and NOT the chunk text.
    expect(spy).toHaveBeenCalledTimes(1);
    const line = String(spy.mock.calls[0]![0]);
    expect(line).toContain("provider=prose");
    expect(line).toContain("method=propose");
    expect(line).not.toContain("some chunk text"); // prompt/chunk never logged
  });

  it("schema-invalid JSON → returns [] AND schemaError counted", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicLlmProvider("test-key");
    // Valid JSON but wrong shape (proposals should be an array of objects).
    stubClientText(provider, '{"proposals": "not-an-array"}');

    const result = await provider.propose("chunk-abc", "text", "root");

    expect(result).toEqual([]);
    expect(provider.counters.snapshot().schemaError).toBe(1);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(String(spy.mock.calls[0]![0])).toContain("reason=schema_error");
  });

  it("no text block → returns [] AND missingText counted", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicLlmProvider("test-key");
    stubClientNoText(provider);

    const result = await provider.propose("chunk-abc", "text", "root");

    expect(result).toEqual([]);
    expect(provider.counters.snapshot().missingText).toBe(1);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(String(spy.mock.calls[0]![0])).toContain("reason=missing_text");
  });

  it("valid JSON response does NOT increment counters or log", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicLlmProvider("test-key");
    stubClientText(
      provider,
      '{"proposals": [{"kind": "requirement", "fields": {"text": "x"}, "citedChunkId": "chunk-abc", "confidence": 0.9, "quote": "x"}]}',
    );

    const result = await provider.propose("chunk-abc", "text", "root");

    expect(result).toHaveLength(1);
    expect(provider.counters.total).toBe(0);
    expect(spy).not.toHaveBeenCalled();
  });
});

// ── Pipeline surfacing ──────────────────────────────────────────────────────

/**
 * Mock provider that always fails to parse: mirrors what AnthropicLlmProvider
 * does at its silent-[] sites (returns [], increments its counters). Used to
 * assert the pipeline surfaces the snapshot in IngestPipelineResult.parseFailures.
 */
class AlwaysParseFailProvider implements LlmProvider {
  readonly counters = new RunCounters();
  async propose(
    chunkId: string,
    _chunkText: string,
    _sectionContext: string,
  ): Promise<CandidateProposal[]> {
    this.counters.recordJsonParseError(`provider=prose method=propose chunkId=${chunkId}`);
    return [];
  }
}

describe("§6 prose telemetry — pipeline surfaces provider counters", () => {
  it("IngestPipelineResult.parseFailures reflects the provider's counts", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const context: ChunkContext = {
      documentHash: "c".repeat(64),
      sectionId: "sec-root",
      sectionPath: "root",
      pageStart: 0,
      pageEnd: 1,
      documentId: "telemetry-fixture",
    };
    // Two distinct sections → ≥ 2 chunks → ≥ 2 failed propose() calls.
    const text = "SECTION A: ".padEnd(1600, "a") + "\n\n" + "SECTION B: ".padEnd(1600, "b");

    const provider = new AlwaysParseFailProvider();
    const result = await runIngestPipeline({ text, context, provider });

    // No candidates emitted (every chunk failed to parse).
    expect(result.candidates).toEqual([]);
    // The pipeline surfaces the provider's counter snapshot.
    expect(result.parseFailures).toBeDefined();
    expect(result.parseFailures!.jsonParseError).toBe(result.processedChunks);
    expect(result.parseFailures!.jsonParseError).toBeGreaterThanOrEqual(2);
    // console.error was called once per failed chunk.
    expect(spy.mock.calls.length).toBe(result.processedChunks);
  });

  it("a provider without counters yields undefined parseFailures (no crash)", async () => {
    const context: ChunkContext = {
      documentHash: "d".repeat(64),
      sectionId: "sec-root",
      sectionPath: "root",
      pageStart: 0,
      pageEnd: 1,
      documentId: "no-counters-fixture",
    };
    const text = "SECTION A: ".padEnd(1600, "a");

    // Minimal mock with NO counters getter.
    const provider: LlmProvider = {
      async propose() {
        return [];
      },
    };
    const result = await runIngestPipeline({ text, context, provider });
    expect(result.parseFailures).toBeUndefined();
  });
});
