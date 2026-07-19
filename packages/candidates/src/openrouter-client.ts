/**
 * openrouter-client.ts — OpenAI-compatible transport for OpenRouter.
 *
 * OpenRouter (https://openrouter.ai) speaks the OpenAI Chat Completions API, NOT
 * the Anthropic Messages API, so it cannot be driven through `@anthropic-ai/sdk`.
 * This module is the single, dependency-free (global `fetch`) transport the
 * OpenRouter-backed providers share.
 *
 * It carries NO retrieval/embedding machinery — the C5 ingest invariant
 * (no `lancedb|embedding|vector|topK|top-k|rerank` in scripts or prose) is
 * untouched by anything here.
 *
 * Config (env, overridable per-constructor):
 *   - OPENROUTER_API_KEY : required for a live call.
 *   - OPENROUTER_MODEL   : model slug; defaults to `z-ai/glm-5.2`.
 */

/** Default OpenRouter model slug. Override with OPENROUTER_MODEL or the ctor. */
export const DEFAULT_OPENROUTER_MODEL = "z-ai/glm-5.2";

const OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions";

export interface OpenRouterChatRequest {
  apiKey: string;
  model: string;
  /** System prompt (role: "system"). */
  system: string;
  /** One or more user turns (role: "user"), in order. */
  userMessages: readonly string[];
  maxTokens: number;
}

/**
 * Fire one Chat Completions request and return the assistant message text, or
 * `null` when the response carries no text (mirrors the "no text block" branch
 * of the Anthropic providers so callers count it as a missing-text failure).
 *
 * Throws on transport / HTTP error (non-2xx) with a status + body snippet so a
 * live run surfaces a bad slug or auth failure clearly instead of silently
 * degrading.
 */
export async function openRouterChat(req: OpenRouterChatRequest): Promise<string | null> {
  const res = await fetch(OPENROUTER_URL, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${req.apiKey}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: req.model,
      max_tokens: req.maxTokens,
      messages: [
        { role: "system", content: req.system },
        ...req.userMessages.map((content) => ({ role: "user", content })),
      ],
    }),
  });

  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`OpenRouter HTTP ${res.status} ${res.statusText}: ${body.slice(0, 500)}`);
  }

  const data = (await res.json()) as {
    choices?: { message?: { content?: unknown } }[];
  };
  const content = data?.choices?.[0]?.message?.content;
  return typeof content === "string" && content.length > 0 ? content : null;
}

/** Resolve the OpenRouter model slug from an explicit value → env → default. */
export function resolveOpenRouterModel(explicit?: string): string {
  return explicit ?? process.env["OPENROUTER_MODEL"] ?? DEFAULT_OPENROUTER_MODEL;
}
