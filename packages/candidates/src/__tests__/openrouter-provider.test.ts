/**
 * openrouter-provider.test.ts — OpenRouter (GLM) providers over a mocked fetch.
 *
 * Covers the transport + shared-parse contract without a live key:
 *   - request shape (URL, auth header, system+user messages, model slug)
 *   - well-formed response → validated proposals / debate scores
 *   - malformed / missing-text → same counted-failure behavior as the Anthropic path
 *   - non-2xx transport error surfaces (not a silent degrade)
 *   - model resolution: explicit → OPENROUTER_MODEL → default z-ai/glm-5.2
 */

import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import {
  OpenRouterLlmProvider,
  proposalsFromResponseText,
} from "../prose/llm-provider.js";
import { OpenRouterInferenceProvider } from "../inference/inference-provider.js";
import {
  openRouterChat,
  resolveOpenRouterModel,
  DEFAULT_OPENROUTER_MODEL,
} from "../openrouter-client.js";
import type { ContextBundle } from "../inference/types.js";

/** Build a fake fetch that returns one canned OpenRouter chat completion. */
function stubFetchContent(content: unknown, ok = true, status = 200): typeof fetch {
  return vi.fn(async () => ({
    ok,
    status,
    statusText: ok ? "OK" : "Error",
    json: async () => ({ choices: [{ message: { content } }] }),
    text: async () => (typeof content === "string" ? content : JSON.stringify(content)),
  })) as unknown as typeof fetch;
}

const EMPTY_CONTEXT: ContextBundle = {
  sourceNeighborhood: "src",
  targetNeighborhood: "tgt",
  corpusQuotes: [],
  offeredFacts: [],
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  delete process.env["OPENROUTER_MODEL"];
});

describe("openRouterChat — transport", () => {
  it("posts to OpenRouter with auth header, model, and system+user messages", async () => {
    const spy = vi.fn(async () => ({
      ok: true,
      status: 200,
      statusText: "OK",
      json: async () => ({ choices: [{ message: { content: "hello" } }] }),
      text: async () => "hello",
    }));
    vi.stubGlobal("fetch", spy);

    const out = await openRouterChat({
      apiKey: "sk-test",
      model: "z-ai/glm-5.2",
      system: "SYS",
      userMessages: ["U1", "U2"],
      maxTokens: 42,
    });

    expect(out).toBe("hello");
    expect(spy).toHaveBeenCalledTimes(1);
    const [url, init] = spy.mock.calls[0]! as unknown as [string, RequestInit];
    expect(url).toBe("https://openrouter.ai/api/v1/chat/completions");
    expect((init.headers as Record<string, string>)["Authorization"]).toBe("Bearer sk-test");
    const body = JSON.parse(init.body as string);
    expect(body.model).toBe("z-ai/glm-5.2");
    expect(body.max_tokens).toBe(42);
    expect(body.messages).toEqual([
      { role: "system", content: "SYS" },
      { role: "user", content: "U1" },
      { role: "user", content: "U2" },
    ]);
  });

  it("returns null when the response carries no text content", async () => {
    vi.stubGlobal("fetch", stubFetchContent(null));
    const out = await openRouterChat({
      apiKey: "k",
      model: "m",
      system: "s",
      userMessages: ["u"],
      maxTokens: 10,
    });
    expect(out).toBeNull();
  });

  it("throws on non-2xx (surfaces bad slug / auth instead of degrading)", async () => {
    vi.stubGlobal("fetch", stubFetchContent("model not found", false, 404));
    await expect(
      openRouterChat({ apiKey: "k", model: "bad", system: "s", userMessages: ["u"], maxTokens: 10 }),
    ).rejects.toThrow(/OpenRouter HTTP 404/);
  });
});

describe("resolveOpenRouterModel", () => {
  it("prefers explicit, then OPENROUTER_MODEL env, then the default", () => {
    expect(resolveOpenRouterModel("explicit/model")).toBe("explicit/model");
    process.env["OPENROUTER_MODEL"] = "env/model";
    expect(resolveOpenRouterModel()).toBe("env/model");
    delete process.env["OPENROUTER_MODEL"];
    expect(resolveOpenRouterModel()).toBe(DEFAULT_OPENROUTER_MODEL);
    expect(DEFAULT_OPENROUTER_MODEL).toBe("z-ai/glm-5.2");
  });
});

describe("OpenRouterLlmProvider", () => {
  it("requires a key", () => {
    const saved = process.env["OPENROUTER_API_KEY"];
    delete process.env["OPENROUTER_API_KEY"];
    expect(() => new OpenRouterLlmProvider()).toThrow(/OPENROUTER_API_KEY/);
    if (saved !== undefined) process.env["OPENROUTER_API_KEY"] = saved;
  });

  it("parses a well-formed proposals response", async () => {
    vi.stubGlobal(
      "fetch",
      stubFetchContent(
        JSON.stringify({
          proposals: [
            { kind: "requirement", fields: { text: "x" }, citedChunkId: "chunk-abc", confidence: 0.9, quote: "x" },
          ],
        }),
      ),
    );
    const provider = new OpenRouterLlmProvider("sk-test");
    const result = await provider.propose("chunk-abc", "text", "root");
    expect(result).toHaveLength(1);
    expect(result[0]!.kind).toBe("requirement");
    expect(provider.counters.total).toBe(0);
  });

  it("malformed JSON → [] and jsonParseError counted (shared parse path)", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal("fetch", stubFetchContent("not json {broken"));
    const provider = new OpenRouterLlmProvider("sk-test");
    const result = await provider.propose("chunk-abc", "text", "root");
    expect(result).toEqual([]);
    expect(provider.counters.snapshot().jsonParseError).toBe(1);
  });

  it("no content → [] and missingText counted", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal("fetch", stubFetchContent(null));
    const provider = new OpenRouterLlmProvider("sk-test");
    const result = await provider.propose("chunk-abc", "text", "root");
    expect(result).toEqual([]);
    expect(provider.counters.snapshot().missingText).toBe(1);
  });
});

describe("proposalsFromResponseText — shared helper", () => {
  it("null → missingText; behaves identically regardless of transport", () => {
    const parsed = proposalsFromResponseText(
      JSON.stringify({ proposals: [{ kind: "mode", fields: {}, citedChunkId: "c", confidence: 0.5, quote: "q" }] }),
      "ctx",
    );
    expect(parsed).toHaveLength(1);
    expect(proposalsFromResponseText(null, "ctx")).toEqual([]);
  });
});

describe("OpenRouterInferenceProvider", () => {
  beforeEach(() => {
    process.env["OPENROUTER_API_KEY"] = "sk-test";
  });

  it("propose parses the wrapped declined form", async () => {
    vi.stubGlobal("fetch", stubFetchContent(JSON.stringify({ proposal: null })));
    const provider = new OpenRouterInferenceProvider("sk-test");
    const out = await provider.propose("allocation", "fn-1", "comp-1", EMPTY_CONTEXT);
    expect(out.kind).toBe("declined");
  });

  it("advocate: schema-invalid → 0.5 default and schemaError counted", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal("fetch", stubFetchContent(JSON.stringify({ score: "not-a-number" })));
    const provider = new OpenRouterInferenceProvider("sk-test");
    const out = await provider.advocate("allocation", { sourceId: "s", targetId: "t", premises: ["p"] } as never, EMPTY_CONTEXT);
    expect(out.score).toBe(0.5);
    expect(provider.counters.snapshot().schemaError).toBe(1);
  });

  it("challenge: well-formed score parsed through", async () => {
    vi.stubGlobal("fetch", stubFetchContent(JSON.stringify({ score: 0.8, summary: "weak" })));
    const provider = new OpenRouterInferenceProvider("sk-test");
    const out = await provider.challenge("allocation", { sourceId: "s", targetId: "t", premises: ["p"] } as never, "adv", EMPTY_CONTEXT);
    expect(out.score).toBe(0.8);
    expect(out.summary).toBe("weak");
  });
});
