/**
 * inference-provider.ts — Injectable InferenceProvider interface + implementations.
 *
 * Mirrors the LlmProvider pattern from @sysml-bridge/prose-ingest:
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
import type { RelationFamily, ProposalOutput, ContextBundle } from "./types.js";
import { ProposalOutputSchema } from "./types.js";

// ── InferenceProvider interface ───────────────────────────────────────────────

export interface InferenceProvider {
  /**
   * Propose a link for a single typed candidate.
   *
   * @param family         The relation family being proposed
   * @param sourceId       Source element composed-IR id
   * @param targetId       Target element composed-IR id
   * @param context        Serialized 1-hop neighborhoods + corpus quotes
   * @returns              A proposal (or null if the provider declines to propose)
   */
  propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle
  ): Promise<ProposalOutput | null>;

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
}

// ── Per-family system prompts ────────────────────────────────────────────────

function getAllocationPrompt(): string {
  return `You are an MBSE (Model-Based Systems Engineering) analyst reasoning about component allocation in a SysML v2 model.

Given a leaf function (ActionUsage, level L3) and a component (PartUsage), determine whether the function is performed by that component in the ANGARS (Autonomous Non-stop Ground-based Autonomous Refueling System) system model.

Provide a structured JSON response with:
- sourceId: the function's composed-IR id (copy exactly from input)
- targetId: the component's composed-IR id (copy exactly from input)
- relationFamily: "allocation"
- premises: array of composed-IR ids (corpus entities or prose entries) that support this allocation; MUST include at least 1 id that appears in the provided context
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
- premises: array of composed-IR ids that support this mode membership; MUST be resolvable in the provided context
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
- premises: array of composed-IR ids supporting this typing; MUST be resolvable in context
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
- premises: array of composed-IR ids supporting this ordering; MUST be resolvable in context
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

function getFamilyPrompt(family: RelationFamily): string {
  switch (family) {
    case "allocation": return getAllocationPrompt();
    case "modeMembership": return getModeMembershipPrompt();
    case "flowTyping": return getFlowTypingPrompt();
    case "controlJoin": return getControlJoinPrompt();
  }
}

// ── Response schemas ─────────────────────────────────────────────────────────

const ProposalResponseSchema = z.object({
  proposal: ProposalOutputSchema.nullable(),
});

const DebateResponseSchema = z.object({
  score: z.number().min(0).max(1),
  summary: z.string(),
});

// ── Real Anthropic implementation ────────────────────────────────────────────

const DEFAULT_INFER_MODEL = "claude-haiku-4-5-20251001";

export class AnthropicInferenceProvider implements InferenceProvider {
  private client: Anthropic;
  private model: string;

  constructor(apiKey?: string, model?: string) {
    this.client = new Anthropic({ apiKey: apiKey ?? process.env["ANTHROPIC_API_KEY"] });
    this.model = model ?? process.env["INFER_MODEL"] ?? DEFAULT_INFER_MODEL;
  }

  async propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle
  ): Promise<ProposalOutput | null> {
    const userMsg = [
      `sourceId: ${sourceId}`,
      `targetId: ${targetId}`,
      ``,
      `--- SOURCE 1-HOP NEIGHBORHOOD ---`,
      context.sourceNeighborhood.slice(0, 2000),
      `--- TARGET 1-HOP NEIGHBORHOOD ---`,
      context.targetNeighborhood.slice(0, 2000),
      `--- CORPUS QUOTES ---`,
      context.corpusQuotes.slice(0, 5).join("\n\n").slice(0, 1500),
    ].join("\n");

    const instructionMsg =
      `Respond with JSON matching: {"proposal": {"sourceId":"...","targetId":"...","relationFamily":"${family}","premises":[...],"rationale":"...","confidence":0.0}} or {"proposal": null}`;

    const response = await this.client.messages.create({
      model: this.model,
      max_tokens: 2000,
      system: getFamilyPrompt(family),
      messages: [
        { role: "user", content: userMsg },
        { role: "user", content: instructionMsg },
      ],
    });

    const text = response.content.find((b) => b.type === "text");
    if (!text || text.type !== "text") return null;

    const raw = extractJson(text.text);
    if (!raw) return null;

    const parsed = ProposalResponseSchema.safeParse(raw);
    if (!parsed.success) return null;
    return parsed.data.proposal;
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
    if (!text || text.type !== "text") return { score: 0.5, summary: "advocate error" };

    const raw = extractJson(text.text);
    if (!raw) return { score: 0.5, summary: "advocate parse error" };

    const parsed = DebateResponseSchema.safeParse(raw);
    if (!parsed.success) return { score: 0.5, summary: "advocate schema error" };
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
    if (!text || text.type !== "text") return { score: 0.5, summary: "challenger error" };

    const raw = extractJson(text.text);
    if (!raw) return { score: 0.5, summary: "challenger parse error" };

    const parsed = DebateResponseSchema.safeParse(raw);
    if (!parsed.success) return { score: 0.5, summary: "challenger schema error" };
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
