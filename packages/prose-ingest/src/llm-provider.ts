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
  kind:
    | "requirement"
    | "need"
    | "mode"
    | "modeTransition"
    | "interface"
    | "component"
    | "function"
    | "succession"
    | "decision"
    | "parallel";
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

// ── Kind-specific required-field validation ───────────────────────────────────

/**
 * Validate kind-specific required fields for a proposal.
 * Returns true if the proposal is well-formed; false if it should be DROPPED
 * (counted as droppedMalformed — never emitted malformed).
 *
 * Rules:
 *   - modeTransition: fields.fromMode (string) + fields.toMode (string) REQUIRED
 *   - succession: fields.owningFunction (string) + fields.fromAction (string) + fields.toAction (string) REQUIRED
 *   - decision: fields.owningFunction (string) + fields.atAction (string) + fields.branches (array, ≥2) REQUIRED
 *   - parallel: fields.owningFunction (string) + fields.branchActions (string[], ≥2) REQUIRED
 *   - all other kinds: no additional required fields (pass through)
 */
export function validateKindSpecificFields(proposal: CandidateProposal): boolean {
  const f = proposal.fields;
  switch (proposal.kind) {
    case "modeTransition":
      return typeof f["fromMode"] === "string" && f["fromMode"].length > 0 &&
             typeof f["toMode"] === "string" && f["toMode"].length > 0;
    case "succession":
      return typeof f["owningFunction"] === "string" && f["owningFunction"].length > 0 &&
             typeof f["fromAction"] === "string" && f["fromAction"].length > 0 &&
             typeof f["toAction"] === "string" && f["toAction"].length > 0;
    case "decision": {
      if (typeof f["owningFunction"] !== "string" || f["owningFunction"].length === 0) return false;
      if (typeof f["atAction"] !== "string" || f["atAction"].length === 0) return false;
      const branches = f["branches"];
      return Array.isArray(branches) && branches.length >= 2;
    }
    case "parallel": {
      if (typeof f["owningFunction"] !== "string" || f["owningFunction"].length === 0) return false;
      const branchActions = f["branchActions"];
      return Array.isArray(branchActions) && branchActions.length >= 2;
    }
    default:
      return true; // requirement, need, mode, interface, component, function — no extra required fields
  }
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
    "succession",
    "decision",
    "parallel",
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
- kind: one of requirement|need|mode|modeTransition|interface|component|function|succession|decision|parallel
- fields: an object with the REQUIRED fields for that kind (see below)
- citedChunkId: MUST be exactly the chunkId provided in the user message — do NOT invent a different value
- confidence: 0.0–1.0 float reflecting your confidence in the extraction
- quote: the verbatim span from the chunk supporting this extraction (≤ 300 chars)

KIND-SPECIFIC REQUIRED FIELDS (proposals missing required fields will be rejected):

requirement: { text: string }
  Example: { "kind": "requirement", "fields": { "text": "The system shall refuel within 60 seconds" } }

need: { text: string }
  Example: { "kind": "need", "fields": { "text": "Autonomous refueling without human intervention" } }

mode: { name: string }
  Example: { "kind": "mode", "fields": { "name": "Standby" } }

modeTransition: { fromMode: string (REQUIRED), toMode: string (REQUIRED), trigger?: string, guard?: string }
  Example: { "kind": "modeTransition", "fields": { "fromMode": "Standby", "toMode": "Active", "trigger": "power on" } }

interface: { name: string, source?: string, target?: string }
  Example: { "kind": "interface", "fields": { "name": "Fuel Interface", "source": "Pump", "target": "Tank" } }

component: { name: string }
  Example: { "kind": "component", "fields": { "name": "Fuel Pump" } }

function: { name: string, level?: string }
  Example: { "kind": "function", "fields": { "name": "Receive Refueling Request", "level": "L2" } }

succession: { owningFunction: string (REQUIRED), fromAction: string (REQUIRED), toAction: string (REQUIRED), guard?: string }
  (Intra-function sequential ordering explicitly stated in prose)
  Example: { "kind": "succession", "fields": { "owningFunction": "F3", "fromAction": "Receive Request", "toAction": "Validate Capacity" } }

decision: { owningFunction: string (REQUIRED), atAction: string (REQUIRED), branches: [{guard: string, toAction: string}, ...] (REQUIRED, ≥2 branches) }
  (A branching control-flow decision node explicitly described in prose)
  Example: { "kind": "decision", "fields": { "owningFunction": "F3", "atAction": "fuelCheck", "branches": [{ "guard": "fuelOk", "toAction": "Proceed" }, { "guard": "fuelLow", "toAction": "Abort" }] } }

parallel: { owningFunction: string (REQUIRED), branchActions: string[] (REQUIRED, ≥2 elements) }
  (Explicitly parallel/concurrent activities stated in prose)
  Example: { "kind": "parallel", "fields": { "owningFunction": "F3", "branchActions": ["Generate Schedule", "Display Mission Data"] } }

If the chunk contains no extractable MBSE artifacts, return an empty proposals array.
Return only well-formed artifacts — do not hallucinate IDs or references not present in the text.
Only emit succession/decision/parallel when the prose EXPLICITLY states the ordering or branching.
Do NOT infer ordering from narrative sequence; require explicit control-flow language.`;

// ── Anthropic implementation ──────────────────────────────────────────────────

/**
 * Default extraction model — Haiku (light, fast, cheap; structured extraction
 * does not need a frontier model). Override with PROSE_INGEST_MODEL.
 */
const DEFAULT_MODEL = "claude-haiku-4-5-20251001";

/**
 * Real LLM provider using the Anthropic API.
 *
 * Requires ANTHROPIC_API_KEY. Model defaults to Haiku (DEFAULT_MODEL) and is
 * overridable via the PROSE_INGEST_MODEL env var or the constructor.
 * Uses structured output to enforce the JSON schema.
 */
export class AnthropicLlmProvider implements LlmProvider {
  private client: Anthropic;
  private model: string;

  constructor(apiKey?: string, model?: string) {
    this.client = new Anthropic({ apiKey: apiKey ?? process.env["ANTHROPIC_API_KEY"] });
    this.model = model ?? process.env["PROSE_INGEST_MODEL"] ?? DEFAULT_MODEL;
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
    // No extended thinking: structured JSON extraction doesn't need it, it adds
    // cost/latency, and "adaptive" thinking is unsupported on lighter models (Haiku).
    const response = await this.client.messages.create({
      model: this.model,
      max_tokens: 8000,
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
