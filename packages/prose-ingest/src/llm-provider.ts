/**
 * LlmProvider — injectable interface for the prose ingestion LLM pass.
 *
 * The interface is kept minimal so that:
 *   - Tests use a deterministic mock (no API key required, offline)
 *   - The real Anthropic implementation is swapped in for live runs
 *
 * C5 guarantee: the pipeline calls propose() exactly ONCE per chunk.
 *              No retrieval / embedding / vector DB calls here.
 */

import Anthropic from "@anthropic-ai/sdk";
import { z } from "zod";

// ── CandidateProposal ─────────────────────────────────────────────────────────

/** One candidate proposed by the LLM for a single chunk. */
export interface CandidateProposal {
  kind: "requirement" | "need" | "mode" | "modeTransition" | "interface" | "component" | "function";
  /** Arbitrary structured fields extracted from the prose. */
  fields: Record<string, unknown>;
  /**
   * The chunkId this candidate cites. The pipeline resolves this against the
   * chunk store and DROPS any proposal whose chunkId is not found (C4 gate).
   */
  citedChunkId: string;
  /** 0.0–1.0 model confidence. May be 0.0 for mock/offline runs. */
  confidence: number;
  /** Verbatim quote from the source chunk (≤ 300 chars). */
  quote: string;
}

// ── LlmProvider interface ─────────────────────────────────────────────────────

export interface LlmProvider {
  /**
   * Propose zero or more candidates from a single chunk.
   *
   * @param chunkId        The content-addressed ID of this chunk (32-hex).
   * @param chunkText      The full text of this chunk.
   * @param sectionContext Human-readable section path / heading context.
   * @returns              Array of proposals (may be empty).
   *
   * Called EXACTLY ONCE per chunk by the pipeline (C5 invariant).
   */
  propose(
    chunkId: string,
    chunkText: string,
    sectionContext: string,
  ): Promise<CandidateProposal[]>;
}

// ── Zod schema for structured output ─────────────────────────────────────────

const ProposalSchema = z.object({
  kind: z.enum([
    "requirement",
    "need",
    "mode",
    "modeTransition",
    "interface",
    "component",
    "function",
  ]),
  fields: z.record(z.unknown()),
  citedChunkId: z.string(),
  confidence: z.number().min(0).max(1),
  quote: z.string().max(300),
});

const ProposalsResponseSchema = z.object({
  proposals: z.array(ProposalSchema),
});

// ── System prompt ─────────────────────────────────────────────────────────────

const SYSTEM_PROMPT = `You are an MBSE (Model-Based Systems Engineering) analyst extracting structured
requirements and engineering concepts from prose documents.

Given a text chunk from an ANGARS (Autonomous Non-stop Ground-based Autonomous Refueling System)
specification document, identify any discrete engineering artifacts present in the chunk.

For each artifact found, return:
- kind: one of requirement|need|mode|modeTransition|interface|component|function
- fields: an object with relevant extracted fields (e.g. text, id, rationale, constraint)
- citedChunkId: MUST be exactly the chunkId provided in the user message — do NOT invent a different value
- confidence: 0.0–1.0 float reflecting your confidence in the extraction
- quote: the verbatim span from the chunk supporting this extraction (≤ 300 chars)

If the chunk contains no extractable MBSE artifacts, return an empty proposals array.
Return only well-formed artifacts — do not hallucinate IDs or references not present in the text.`;

// ── Anthropic implementation ──────────────────────────────────────────────────

/**
 * Real LLM provider using the Anthropic API (claude-opus-4-8).
 *
 * Requires ANTHROPIC_API_KEY environment variable.
 * Uses structured output (client.messages.parse) to enforce the JSON schema.
 */
export class AnthropicLlmProvider implements LlmProvider {
  private client: Anthropic;

  constructor(apiKey?: string) {
    this.client = new Anthropic({ apiKey: apiKey ?? process.env["ANTHROPIC_API_KEY"] });
  }

  async propose(
    chunkId: string,
    chunkText: string,
    sectionContext: string,
  ): Promise<CandidateProposal[]> {
    const userMessage = [
      `chunkId: ${chunkId}`,
      `sectionContext: ${sectionContext}`,
      ``,
      `--- CHUNK TEXT START ---`,
      chunkText.slice(0, 4000), // guard against oversized chunks
      `--- CHUNK TEXT END ---`,
    ].join("\n");

    // Use structured output with JSON response format
    const response = await this.client.messages.create({
      model: "claude-opus-4-8",
      max_tokens: 16000,
      thinking: { type: "adaptive" },
      system: SYSTEM_PROMPT,
      messages: [
        {
          role: "user",
          content: userMessage,
        },
        {
          role: "user",
          content:
            'Respond with a JSON object matching this schema: {"proposals": [{"kind": "...", "fields": {...}, "citedChunkId": "...", "confidence": 0.0, "quote": "..."}]}',
        },
      ],
    });

    // Extract text content from response
    const textBlock = response.content.find((b) => b.type === "text");
    if (!textBlock || textBlock.type !== "text") {
      return [];
    }

    // Parse JSON from response text — extract JSON block if wrapped in markdown
    const raw = textBlock.text.trim();
    let jsonStr = raw;
    const jsonMatch = raw.match(/```(?:json)?\s*([\s\S]*?)```/);
    if (jsonMatch) {
      jsonStr = jsonMatch[1]!.trim();
    } else {
      // Find first { to last }
      const start = raw.indexOf("{");
      const end = raw.lastIndexOf("}");
      if (start !== -1 && end !== -1) {
        jsonStr = raw.slice(start, end + 1);
      }
    }

    let parsed: unknown;
    try {
      parsed = JSON.parse(jsonStr);
    } catch {
      return [];
    }

    const result = ProposalsResponseSchema.safeParse(parsed);
    if (!result.success) {
      return [];
    }

    return result.data.proposals;
  }
}
