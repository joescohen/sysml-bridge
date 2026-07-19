/**
 * parse-failure-telemetry.test.ts — the §6 fix, inference side.
 *
 * Guarantees that the debate 0.5-default fallback sites (advocate/challenge) are
 * now COUNTED and LOGGED, not silent:
 *   - malformed/missing-text response → score STAYS 0.5 (behavior preserved)
 *   - AND provider.counters increments the matching failure class
 *   - AND one console.error line is emitted with provider/method context (no prompt)
 *
 * The live Anthropic client is stubbed (no API key, offline): we replace
 * `messages.create` with a canned malformed response so the parse path runs.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { AnthropicInferenceProvider } from "../inference-provider.js";
import type { ProposalOutput, ContextBundle } from "../types.js";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const PROPOSAL: ProposalOutput = {
  sourceId: "function-leaf-001",
  targetId: "component-comp-001",
  relationFamily: "allocation",
  premises: ["function-leaf-001"],
  rationale: "audit-only",
  confidence: 0.55,
};

const CONTEXT: ContextBundle = {
  sourceNeighborhood: "src",
  targetNeighborhood: "tgt",
  corpusQuotes: [],
  offeredFacts: [],
};

/** Replace the provider's private Anthropic client with a canned text response. */
function stubClientText(provider: AnthropicInferenceProvider, text: string): void {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (provider as any).client = {
    messages: {
      create: async () => ({ content: [{ type: "text", text }] }),
    },
  };
}

/** Replace the client with a response that has NO text block. */
function stubClientNoText(provider: AnthropicInferenceProvider): void {
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

describe("§6 inference telemetry — debate 0.5-default sites are counted + logged", () => {
  it("advocate: malformed JSON → score stays 0.5, jsonParseError counted, console.error called", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicInferenceProvider("test-key");
    stubClientText(provider, "this is not json at all");

    const result = await provider.advocate("allocation", PROPOSAL, CONTEXT);

    // Behavior preserved: 0.5 default still returned.
    expect(result.score).toBe(0.5);
    // Counted.
    expect(provider.counters.snapshot().jsonParseError).toBe(1);
    expect(provider.counters.total).toBe(1);
    // Logged with provider/method context, and NOT the prompt/proposal body.
    expect(spy).toHaveBeenCalledTimes(1);
    const line = String(spy.mock.calls[0]![0]);
    expect(line).toContain("provider=inference");
    expect(line).toContain("method=advocate");
    expect(line).not.toContain("audit-only"); // rationale never logged
  });

  it("advocate: schema-invalid JSON → score stays 0.5, schemaError counted", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicInferenceProvider("test-key");
    // Parseable JSON, but score out of range → zod schema failure.
    stubClientText(provider, '{"score": 5, "summary": "bad range"}');

    const result = await provider.advocate("allocation", PROPOSAL, CONTEXT);

    expect(result.score).toBe(0.5);
    expect(provider.counters.snapshot().schemaError).toBe(1);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(String(spy.mock.calls[0]![0])).toContain("reason=schema_error");
  });

  it("advocate: no text block → score stays 0.5, missingText counted", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicInferenceProvider("test-key");
    stubClientNoText(provider);

    const result = await provider.advocate("allocation", PROPOSAL, CONTEXT);

    expect(result.score).toBe(0.5);
    expect(provider.counters.snapshot().missingText).toBe(1);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(String(spy.mock.calls[0]![0])).toContain("reason=missing_text");
  });

  it("challenge: malformed JSON → score stays 0.5, jsonParseError counted, console.error called", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicInferenceProvider("test-key");
    stubClientText(provider, "definitely not json");

    const result = await provider.challenge("allocation", PROPOSAL, "advocate said yes", CONTEXT);

    expect(result.score).toBe(0.5);
    expect(provider.counters.snapshot().jsonParseError).toBe(1);
    expect(spy).toHaveBeenCalledTimes(1);
    const line = String(spy.mock.calls[0]![0]);
    expect(line).toContain("provider=inference");
    expect(line).toContain("method=challenge");
  });

  it("valid response does NOT increment counters or log", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const provider = new AnthropicInferenceProvider("test-key");
    stubClientText(provider, '{"score": 0.82, "summary": "strong case"}');

    const result = await provider.advocate("allocation", PROPOSAL, CONTEXT);

    expect(result.score).toBe(0.82);
    expect(provider.counters.total).toBe(0);
    expect(spy).not.toHaveBeenCalled();
  });
});
