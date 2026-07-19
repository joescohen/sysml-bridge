/**
 * inference-provider.ts — Injectable InferenceProvider interface + implementations.
 *
 * Mirrors the LlmProvider pattern from ../prose/llm-provider.ts:
 *   - Interface is minimal for easy mocking in tests (no API key needed)
 *   - Real impl uses Anthropic SDK; model defaults to claude-haiku-4-5-20251001
 *     (INFER_MODEL env override)
 *   - NO thinking param (structured extraction does not need it; unsupported on Haiku)
 *
 * Per-relation-family prompts: each family has a tailored system prompt focused on
 * the specific reasoning needed for that link type.
 *
 * Output is zod-enforced: { sourceId, targetId, relationFamily, premises: string[],
 * rationale, confidence }. A proposal with ANY unresolvable premise id is dropped
 * + counted dropped_unpremised (A2).
 */

import Anthropic from "@anthropic-ai/sdk";
import { z } from "zod";
import type { RelationFamily, ProposalOutput, ProposeResult, ContextBundle } from "./types.js";
import { ProposalOutputSchema } from "./types.js";
import { RunCounters } from "../telemetry.js";
import { openRouterChat, resolveOpenRouterModel } from "../openrouter-client.js";

// ── InferenceProvider interface ───────────────────────────────────────────────

export interface InferenceProvider {
  /**
   * Propose a link for a single typed candidate.
   *
   * @param family         The relation family being proposed
   * @param sourceId       Source element composed-IR id
   * @param targetId       Target element composed-IR id
   * @param context        Offered facts (id contract) + neighborhoods + quotes
   * @returns              ProposeResult — "proposal" | "declined" (model
   *                       explicitly returned none) | "parse_error" (JSON or
   *                       schema failure); declined and parse errors are
   *                       counted separately in the run stats
   */
  propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle
  ): Promise<ProposeResult>;

  /**
   * Advocate pass: argue FOR the proposed link.
   * Returns { score: 0.0-1.0, summary: string (audit-only) }
   */
  advocate(
    family: RelationFamily,
    proposal: ProposalOutput,
    context: ContextBundle
  ): Promise<{ score: number; summary: string }>;

  /**
   * Challenger pass: argue AGAINST the proposed link.
   * Sees the advocate's summary.
   * Returns { score: 0.0-1.0, summary: string (audit-only) }
   */
  challenge(
    family: RelationFamily,
    proposal: ProposalOutput,
    advocateSummary: string,
    context: ContextBundle
  ): Promise<{ score: number; summary: string }>;

  /**
   * Optional per-run parse-failure counters. When present, the engine surfaces
   * the snapshot in EngineResult.parseFailures. Mock providers may omit it.
   */
  readonly counters?: RunCounters;
}

// ── Per-family system prompts ────────────────────────────────────────────────

function getAllocationPrompt(): string {
  return `You are an MBSE (Model-Based Systems Engineering) analyst reasoning about component allocation in a SysML v2 model.

Given a leaf function (ActionUsage, level L3) and a component (PartUsage), determine whether the function is performed by that component in the ANGARS (Autonomous Non-stop Ground-based Autonomous Refueling System) system model.

Provide a structured JSON response with:
- sourceId: the function's composed-IR id (copy exactly from input)
- targetId: the component's composed-IR id (copy exactly from input)
- relationFamily: "allocation"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation of why the function should be allocated to this component (audit-only)
- confidence: float 0.0-1.0 reflecting your confidence

Only propose an allocation if there is genuine evidence in the provided context. If no evidence supports this specific allocation, return null (as JSON: {"proposal": null}).
Return null if you are uncertain or the evidence is weak.`;
}

function getModeMembershipPrompt(): string {
  return `You are an MBSE analyst reasoning about operational mode membership in a SysML v2 model.

Given a leaf function (ActionUsage, level L3) and an operational mode, determine whether this function is active during that mode in the ANGARS system.

Provide a structured JSON response with:
- sourceId: the function's composed-IR id (copy exactly from input)
- targetId: the mode entry's id (copy exactly from input)
- relationFamily: "modeMembership"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation (audit-only)
- confidence: float 0.0-1.0

Only propose membership if there is genuine evidence. Return {"proposal": null} if insufficient evidence.`;
}

function getFlowTypingPrompt(): string {
  return `You are an MBSE analyst reasoning about interface flow typing in a SysML v2 model.

Given an N2 flow entry (a data/signal/material flow between components) and a prose interface entry, determine whether the N2 flow carries the interface item described by the prose entry in the ANGARS system.

Provide a structured JSON response with:
- sourceId: the N2 flow entry's id (copy exactly from input)
- targetId: the interface entry's id (copy exactly from input)
- relationFamily: "flowTyping"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation (audit-only)
- confidence: float 0.0-1.0

Return {"proposal": null} if the N2 flow and interface entry are clearly unrelated.`;
}

function getControlJoinPrompt(): string {
  return `You are an MBSE analyst reasoning about control flow between sibling actions in a SysML v2 activity.

Given two sibling leaf functions (ActionUsage, same owning activity/function), determine whether the first function leads to (precedes / triggers / joins to) the second in the ANGARS operational behavior.

Provide a structured JSON response with:
- sourceId: the preceding function's id (copy exactly from input)
- targetId: the succeeding function's id (copy exactly from input)
- relationFamily: "controlJoin"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation (audit-only)
- confidence: float 0.0-1.0

Return {"proposal": null} if no evidence supports a direct control-flow connection.`;
}

function getAdvocatePrompt(family: RelationFamily): string {
  return `You are an MBSE analyst making the strongest possible case FOR a proposed ${family} link in the ANGARS SysML v2 model.

Given the proposal details and context, provide:
1. The best evidence FROM THE PROVIDED PREMISES supporting this link
2. Why this link is structurally sound for SysML v2
3. A confidence score 0.0-1.0 for how strongly the evidence supports the link

Respond as JSON: { "score": float, "summary": "concise case FOR (1-3 sentences)" }`;
}

function getChallengerPrompt(family: RelationFamily): string {
  return `You are an MBSE analyst making the strongest possible case AGAINST a proposed ${family} link in the ANGARS SysML v2 model.

You have seen the advocate's summary. Your job is to identify weaknesses, alternative explanations, or reasons this link should NOT be in the model.

Respond as JSON: { "score": float (0.0-1.0 where 1.0 = strong case against), "summary": "concise case AGAINST (1-3 sentences)" }`;
}

function getSatisfyPrompt(): string {
  return `You are an MBSE analyst reasoning about requirement satisfaction in a SysML v2 model.

Given a design element (a component/part or a function) and a system requirement, determine whether that design element SATISFIES the requirement in the ANGARS system model.

Provide a structured JSON response with:
- sourceId: the design element's composed-IR/entity id (copy exactly from input)
- targetId: the requirement's id (copy exactly from input)
- relationFamily: "satisfy"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation of why the element satisfies the requirement (audit-only)
- confidence: float 0.0-1.0

Only propose a satisfy link if there is genuine evidence. Return {"proposal": null} if the evidence is weak or the pair is unrelated.`;
}

function getDerivePrompt(): string {
  return `You are an MBSE analyst reasoning about requirement derivation in a SysML v2 model.

Given a system requirement and a stakeholder need, determine whether the requirement DERIVES FROM (traces back to) that stakeholder need in the ANGARS system model.

Provide a structured JSON response with:
- sourceId: the requirement's id (copy exactly from input)
- targetId: the stakeholder need's id (copy exactly from input)
- relationFamily: "derive"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation of why the requirement derives from this need (audit-only)
- confidence: float 0.0-1.0

Only propose a derive link if there is genuine evidence. Return {"proposal": null} if the requirement and need are unrelated.`;
}

function getContainmentPrompt(): string {
  return `You are an MBSE analyst reasoning about structural composition (containment) in a SysML v2 model.

Given two components, determine whether the FIRST component (source) STRUCTURALLY CONTAINS the SECOND (target) as a part — i.e. source is the parent/whole and target is a sub-component/part of it ("consists of", "comprises", "is composed of", "subcomponent of", "part of").

Direction matters: propose the link ONLY in the parent→child direction (source contains target). If the evidence instead supports target containing source, return {"proposal": null} — the reverse-direction candidate is enumerated separately and the human reviewer resolves the true direction.

Provide a structured JSON response with:
- sourceId: the PARENT component's id (copy exactly from input)
- targetId: the CHILD component's id (copy exactly from input)
- relationFamily: "containment"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation of why the parent contains the child (audit-only)
- confidence: float 0.0-1.0

Only propose a containment link if there is genuine part-of evidence. Return {"proposal": null} if the components are merely related, siblings, or the direction is unclear.`;
}

function getVerifyPrompt(): string {
  // No proposer enumerates `verify` (see type-gate.ts); this prompt exists only
  // so getFamilyPrompt is total over RelationFamily.
  return `You are an MBSE analyst reasoning about requirement verification in a SysML v2 model.

Given a verification case and a system requirement, determine whether the case VERIFIES that requirement in the ANGARS system model.

Provide a structured JSON response with:
- sourceId: the verification case's id (copy exactly from input)
- targetId: the requirement's id (copy exactly from input)
- relationFamily: "verify"
- premises: array of the bracketed [id: …] ids from the OFFERED FACTS in the user message — ids ONLY, never names; ≥1 required
- rationale: brief explanation (audit-only)
- confidence: float 0.0-1.0

Return {"proposal": null} if the case and requirement are unrelated.`;
}

function getFamilyPrompt(family: RelationFamily): string {
  switch (family) {
    case "allocation": return getAllocationPrompt();
    case "modeMembership": return getModeMembershipPrompt();
    case "flowTyping": return getFlowTypingPrompt();
    case "controlJoin": return getControlJoinPrompt();
    case "satisfy": return getSatisfyPrompt();
    case "derive": return getDerivePrompt();
    case "verify": return getVerifyPrompt();
    case "containment": return getContainmentPrompt();
  }
}

// ── Premise id contract: prompt builder (exported, pure, unit-testable) ──────

/**
 * The non-negotiable premise citation rule, rendered verbatim into every
 * propose user message. The live-run failure mode was Haiku citing premises by
 * NAME — this instruction + the [id: …] fact lines are the contract fix.
 */
export const PREMISE_ID_INSTRUCTION =
  `premises MUST be the bracketed ids EXACTLY as given above; proposals citing anything else are discarded`;

/** Render the offered facts as `[id: <id>] <kind> "<name>" — <detail>` lines. */
export function renderOfferedFacts(context: ContextBundle): string {
  return context.offeredFacts
    .map((f) => `[id: ${f.id}] ${f.kind} "${f.name}"${f.detail ? ` — ${f.detail}` : ""}`)
    .join("\n");
}

/**
 * Render BM25-retrieved corpus passages as a clearly-labeled evidence block.
 * Each line carries the chunkId (a resolvable premise id) and its sectionPath so
 * the LLM can cite the passage as a premise. Returns "" when no evidence.
 */
export function renderRetrievedEvidence(context: ContextBundle): string {
  const hits = context.retrievedEvidence ?? [];
  if (hits.length === 0) return "";
  return hits
    .map(
      (h) =>
        `[id: ${h.chunkId}] (${h.sectionPath}) ${h.text.replace(/\s+/g, " ").trim().slice(0, 400)}`
    )
    .join("\n\n");
}

/**
 * Build the full propose user message for a candidate pair: the source/target
 * ids, the offered fact lines (premise id contract), corpus quotes, the
 * MUST-cite-ids instruction, and one worked example demonstrating id-form
 * premises.
 */
export function buildProposeUserMessage(
  family: RelationFamily,
  sourceId: string,
  targetId: string,
  context: ContextBundle
): string {
  return [
    `sourceId: ${sourceId}`,
    `targetId: ${targetId}`,
    ``,
    `--- OFFERED FACTS (cite premises ONLY from these bracketed ids) ---`,
    renderOfferedFacts(context),
    `--- RETRIEVED EVIDENCE (BM25 corpus passages; cite the bracketed chunk id as a premise) ---`,
    renderRetrievedEvidence(context) || "(none)",
    `--- CORPUS QUOTES ---`,
    context.corpusQuotes.slice(0, 5).join("\n\n").slice(0, 1500) || "(none)",
    ``,
    `RULE: ${PREMISE_ID_INSTRUCTION}.`,
    ``,
    `WORKED EXAMPLE (format only — cite ids from the OFFERED FACTS above, not these):`,
    `  Offered: [id: function-1a2b3c4d5e6f7080] function "Validate Fuel Capacity" — level L3`,
    `           [id: n2-9f8e7d6c5b4a3920] n2-flow "Fuel Status" — Fuel Pump → Controller`,
    `  Correct: {"proposal": {"sourceId": "function-1a2b3c4d5e6f7080", "targetId": "component-0011223344556677",`,
    `            "relationFamily": "${family}", "premises": ["n2-9f8e7d6c5b4a3920"],`,
    `            "rationale": "...", "confidence": 0.72}}`,
    `  WRONG:   "premises": ["Fuel Status"]   ← cited by name, will be DISCARDED`,
    ``,
    `Respond with JSON: {"proposal": {"sourceId":"...","targetId":"...","relationFamily":"${family}","premises":["<bracketed id>", ...],"rationale":"...","confidence":0.0}} or {"proposal": null}`,
  ].join("\n");
}

// ── Response schemas ─────────────────────────────────────────────────────────

const ProposalResponseSchema = z.object({
  proposal: ProposalOutputSchema.nullable(),
});

const DebateResponseSchema = z.object({
  score: z.number().min(0).max(1),
  summary: z.string(),
});

/**
 * Parse a raw model response text into a ProposeResult.
 * Tolerant: accepts the wrapped form {"proposal": {...}|null} and the bare
 * proposal object {...}; anything else is a parse_error with detail.
 * Exported for testing.
 */
export function parseProposeResponse(rawText: string): ProposeResult {
  const raw = extractJson(rawText);
  if (raw === null) {
    return { kind: "parse_error", detail: `no parseable JSON in response: ${rawText.slice(0, 120)}` };
  }

  // Wrapped form: {"proposal": {...} | null}
  const wrapped = ProposalResponseSchema.safeParse(raw);
  if (wrapped.success) {
    if (wrapped.data.proposal === null) return { kind: "declined" };
    return { kind: "proposal", proposal: wrapped.data.proposal };
  }

  // Bare form: the proposal object directly
  const bare = ProposalOutputSchema.safeParse(raw);
  if (bare.success) {
    return { kind: "proposal", proposal: bare.data };
  }

  return {
    kind: "parse_error",
    detail: `schema validation failed: ${wrapped.error.issues
      .slice(0, 3)
      .map((i) => `${i.path.join(".")}: ${i.message}`)
      .join("; ")}`,
  };
}

// ── Real Anthropic implementation ────────────────────────────────────────────

const DEFAULT_INFER_MODEL = "claude-haiku-4-5-20251001";

export class AnthropicInferenceProvider implements InferenceProvider {
  private client: Anthropic;
  private model: string;
  /** Per-run parse-failure counters for the debate 0.5-default sites. */
  readonly counters = new RunCounters();

  constructor(apiKey?: string, model?: string) {
    this.client = new Anthropic({ apiKey: apiKey ?? process.env["ANTHROPIC_API_KEY"] });
    this.model = model ?? process.env["INFER_MODEL"] ?? DEFAULT_INFER_MODEL;
  }

  async propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle
  ): Promise<ProposeResult> {
    const userMsg = buildProposeUserMessage(family, sourceId, targetId, context);

    const response = await this.client.messages.create({
      model: this.model,
      max_tokens: 2000,
      system: getFamilyPrompt(family),
      messages: [{ role: "user", content: userMsg }],
    });

    const text = response.content.find((b) => b.type === "text");
    if (!text || text.type !== "text") {
      return { kind: "parse_error", detail: "response contained no text block" };
    }

    return parseProposeResponse(text.text);
  }

  async advocate(
    family: RelationFamily,
    proposal: ProposalOutput,
    context: ContextBundle
  ): Promise<{ score: number; summary: string }> {
    const userMsg = [
      `Proposal: ${JSON.stringify({ sourceId: proposal.sourceId, targetId: proposal.targetId, premises: proposal.premises })}`,
      `Source neighborhood: ${context.sourceNeighborhood.slice(0, 1500)}`,
      `Target neighborhood: ${context.targetNeighborhood.slice(0, 1500)}`,
    ].join("\n");

    const response = await this.client.messages.create({
      model: this.model,
      max_tokens: 1000,
      system: getAdvocatePrompt(family),
      messages: [{ role: "user", content: userMsg }],
    });

    const text = response.content.find((b) => b.type === "text");
    if (!text || text.type !== "text") {
      // 0.5 default preserved — now counted + logged (was silent).
      this.counters.recordMissingText(`provider=inference method=advocate family=${family}`);
      return { score: 0.5, summary: "advocate error" };
    }

    const raw = extractJson(text.text);
    if (!raw) {
      this.counters.recordJsonParseError(`provider=inference method=advocate family=${family}`);
      return { score: 0.5, summary: "advocate parse error" };
    }

    const parsed = DebateResponseSchema.safeParse(raw);
    if (!parsed.success) {
      this.counters.recordSchemaError(
        `provider=inference method=advocate family=${family}`,
        parsed.error.issues.slice(0, 3).map((i) => `${i.path.join(".")}: ${i.message}`).join("; ")
      );
      return { score: 0.5, summary: "advocate schema error" };
    }
    return parsed.data;
  }

  async challenge(
    family: RelationFamily,
    proposal: ProposalOutput,
    advocateSummary: string,
    context: ContextBundle
  ): Promise<{ score: number; summary: string }> {
    const userMsg = [
      `Proposal: ${JSON.stringify({ sourceId: proposal.sourceId, targetId: proposal.targetId, premises: proposal.premises })}`,
      `Advocate summary: ${advocateSummary}`,
      `Source neighborhood: ${context.sourceNeighborhood.slice(0, 1000)}`,
      `Target neighborhood: ${context.targetNeighborhood.slice(0, 1000)}`,
    ].join("\n");

    const response = await this.client.messages.create({
      model: this.model,
      max_tokens: 1000,
      system: getChallengerPrompt(family),
      messages: [{ role: "user", content: userMsg }],
    });

    const text = response.content.find((b) => b.type === "text");
    if (!text || text.type !== "text") {
      // 0.5 default preserved — now counted + logged (was silent).
      this.counters.recordMissingText(`provider=inference method=challenge family=${family}`);
      return { score: 0.5, summary: "challenger error" };
    }

    const raw = extractJson(text.text);
    if (!raw) {
      this.counters.recordJsonParseError(`provider=inference method=challenge family=${family}`);
      return { score: 0.5, summary: "challenger parse error" };
    }

    const parsed = DebateResponseSchema.safeParse(raw);
    if (!parsed.success) {
      this.counters.recordSchemaError(
        `provider=inference method=challenge family=${family}`,
        parsed.error.issues.slice(0, 3).map((i) => `${i.path.join(".")}: ${i.message}`).join("; ")
      );
      return { score: 0.5, summary: "challenger schema error" };
    }
    return parsed.data;
  }
}

// ── JSON extraction helper ───────────────────────────────────────────────────

function extractJson(text: string): unknown {
  const raw = text.trim();
  const jsonMatch = raw.match(/```(?:json)?\s*([\s\S]*?)```/);
  let jsonStr = jsonMatch ? jsonMatch[1]!.trim() : raw;
  if (!jsonMatch) {
    const start = raw.indexOf("{");
    const end = raw.lastIndexOf("}");
    if (start !== -1 && end !== -1) jsonStr = raw.slice(start, end + 1);
  }
  try {
    return JSON.parse(jsonStr);
  } catch {
    return null;
  }
}

/**
 * Parse a debate (advocate/challenger) response, preserving the 0.5-default +
 * counted-failure behavior. `rawText === null` → the transport had no text.
 */
function parseDebateResponse(
  rawText: string | null,
  ctx: string,
  role: "advocate" | "challenger",
  counters?: RunCounters,
): { score: number; summary: string } {
  if (rawText === null) {
    counters?.recordMissingText(ctx);
    return { score: 0.5, summary: `${role} error` };
  }
  const raw = extractJson(rawText);
  if (!raw) {
    counters?.recordJsonParseError(ctx);
    return { score: 0.5, summary: `${role} parse error` };
  }
  const parsed = DebateResponseSchema.safeParse(raw);
  if (!parsed.success) {
    counters?.recordSchemaError(
      ctx,
      parsed.error.issues.slice(0, 3).map((i) => `${i.path.join(".")}: ${i.message}`).join("; "),
    );
    return { score: 0.5, summary: `${role} schema error` };
  }
  return parsed.data;
}

// ── OpenRouter implementation ─────────────────────────────────────────────────

/**
 * Real inference provider using OpenRouter's OpenAI-compatible Chat Completions
 * API. Requires OPENROUTER_API_KEY. Model defaults to `z-ai/glm-5.2`, overridable
 * via OPENROUTER_MODEL or the constructor. Shares every prompt + schema + parse
 * path with {@link AnthropicInferenceProvider}; only the transport differs.
 */
export class OpenRouterInferenceProvider implements InferenceProvider {
  private apiKey: string;
  private model: string;
  readonly counters = new RunCounters();

  constructor(apiKey?: string, model?: string) {
    const key = apiKey ?? process.env["OPENROUTER_API_KEY"];
    if (!key) throw new Error("OpenRouterInferenceProvider requires OPENROUTER_API_KEY");
    this.apiKey = key;
    this.model = resolveOpenRouterModel(model);
  }

  async propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle,
  ): Promise<ProposeResult> {
    const userMsg = buildProposeUserMessage(family, sourceId, targetId, context);
    const text = await openRouterChat({
      apiKey: this.apiKey,
      model: this.model,
      system: getFamilyPrompt(family),
      userMessages: [userMsg],
      maxTokens: 2000,
    });
    if (text === null) return { kind: "parse_error", detail: "response contained no text" };
    return parseProposeResponse(text);
  }

  async advocate(
    family: RelationFamily,
    proposal: ProposalOutput,
    context: ContextBundle,
  ): Promise<{ score: number; summary: string }> {
    const userMsg = [
      `Proposal: ${JSON.stringify({ sourceId: proposal.sourceId, targetId: proposal.targetId, premises: proposal.premises })}`,
      `Source neighborhood: ${context.sourceNeighborhood.slice(0, 1500)}`,
      `Target neighborhood: ${context.targetNeighborhood.slice(0, 1500)}`,
    ].join("\n");
    const text = await openRouterChat({
      apiKey: this.apiKey,
      model: this.model,
      system: getAdvocatePrompt(family),
      userMessages: [userMsg],
      maxTokens: 1000,
    });
    return parseDebateResponse(
      text,
      `provider=inference-openrouter method=advocate family=${family}`,
      "advocate",
      this.counters,
    );
  }

  async challenge(
    family: RelationFamily,
    proposal: ProposalOutput,
    advocateSummary: string,
    context: ContextBundle,
  ): Promise<{ score: number; summary: string }> {
    const userMsg = [
      `Proposal: ${JSON.stringify({ sourceId: proposal.sourceId, targetId: proposal.targetId, premises: proposal.premises })}`,
      `Advocate summary: ${advocateSummary}`,
      `Source neighborhood: ${context.sourceNeighborhood.slice(0, 1000)}`,
      `Target neighborhood: ${context.targetNeighborhood.slice(0, 1000)}`,
    ].join("\n");
    const text = await openRouterChat({
      apiKey: this.apiKey,
      model: this.model,
      system: getChallengerPrompt(family),
      userMessages: [userMsg],
      maxTokens: 1000,
    });
    return parseDebateResponse(
      text,
      `provider=inference-openrouter method=challenge family=${family}`,
      "challenger",
      this.counters,
    );
  }
}
