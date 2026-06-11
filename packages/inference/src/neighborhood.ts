/**
 * neighborhood.ts — 1-hop neighborhood serialization for context bundles.
 *
 * For a given element id, serializes its immediate IR neighborhood:
 *   - The element itself (name, kind, level, owner, etc.)
 *   - For functions: the parent function (owner), sibling functions, N2 interfaces
 *     connected to components in the same subsystem
 *   - For components: the subsystem(s) it belongs to, N2 interfaces connected to it
 *   - For N2 flows: both endpoint components and their subsystems
 *   - For modes: the prose entry fields
 *   - For interfaces: the prose entry fields
 *
 * Output is a compact plain-text serialization (not JSON) for LLM readability.
 * Capped at 2000 chars per neighborhood.
 */

import type { InferredComposedIR } from "@sysml-bridge/ir";
import type { ContextBundle } from "./types.js";

function serializeElement(el: Record<string, unknown>): string {
  const parts: string[] = [];
  if (el["name"]) parts.push(`name: ${el["name"]}`);
  if (el["kind"]) parts.push(`kind: ${el["kind"]}`);
  if (el["level"]) parts.push(`level: ${el["level"]}`);
  if (el["owner"]) parts.push(`owner: ${el["owner"]}`);
  if (el["naturalKey"]) parts.push(`naturalKey: ${el["naturalKey"]}`);
  if (el["flow"]) parts.push(`flow: ${el["flow"]}`);
  if (el["scope"]) parts.push(`scope: ${el["scope"]}`);
  if (el["fields"] && typeof el["fields"] === "object") {
    const fields = el["fields"] as Record<string, unknown>;
    for (const [k, v] of Object.entries(fields)) {
      if (typeof v === "string" && v.length < 200) parts.push(`${k}: ${v}`);
    }
  }
  return parts.join("; ");
}

/**
 * Build a compact text representation of the 1-hop neighborhood for element id.
 * Returns a string ≤ 2000 chars.
 */
export function serializeNeighborhood(id: string, ir: InferredComposedIR): string {
  const corpus = ir.extracted;
  const lines: string[] = [];

  // Try to find the element in each corpus collection
  const fn = (corpus.functions ?? []).find((f) => f.id === id);
  const comp = (corpus.components ?? []).find((c) => c.id === id);
  const n2 = (corpus.n2Interfaces ?? []).find((n) => n.id === id);
  const prose = ir.proseEntries.find((e) => e.id === id);
  const sub = (corpus.subsystems ?? []).find((s) => s.id === id);

  if (fn) {
    lines.push(`[FUNCTION] ${serializeElement(fn as unknown as Record<string, unknown>)}`);
    // Siblings (same owner)
    const siblings = (corpus.functions ?? [])
      .filter((f) => f.owner === fn.owner && f.id !== fn.id)
      .slice(0, 8);
    if (siblings.length > 0) {
      lines.push(`  siblings (same owner): ${siblings.map((s) => s.name).join(", ")}`);
    }
    // N2 flows involving this function's owner context
    const n2ForOwner = (corpus.n2Interfaces ?? [])
      .filter((n) => n.sourceLabel === fn.owner || n.targetLabel === fn.owner)
      .slice(0, 5);
    for (const n of n2ForOwner) {
      lines.push(`  n2: ${n.sourceLabel} → ${n.targetLabel}: ${n.flow}`);
    }
  } else if (comp) {
    lines.push(`[COMPONENT] ${serializeElement(comp as unknown as Record<string, unknown>)}`);
    // Subsystems containing this component
    const subs = (corpus.subsystems ?? []).filter((s) => s.componentIds.includes(comp.id));
    for (const s of subs) {
      lines.push(`  subsystem: ${s.name}`);
    }
    // N2 flows involving this component
    const n2Flows = (corpus.n2Interfaces ?? [])
      .filter((n) => n.sourceId === comp.id || n.targetId === comp.id)
      .slice(0, 6);
    for (const n of n2Flows) {
      lines.push(`  n2: ${n.sourceLabel} → ${n.targetLabel}: ${n.flow} [scope: ${n.scope}]`);
    }
  } else if (n2) {
    lines.push(`[N2-FLOW] ${serializeElement(n2 as unknown as Record<string, unknown>)}`);
    // Source component
    const srcComp = (corpus.components ?? []).find((c) => c.id === n2.sourceId);
    if (srcComp) lines.push(`  source: ${srcComp.name}`);
    // Target component
    const tgtComp = (corpus.components ?? []).find((c) => c.id === n2.targetId);
    if (tgtComp) lines.push(`  target: ${tgtComp.name}`);
    // Related N2 flows from same source
    const relatedFlows = (corpus.n2Interfaces ?? [])
      .filter((n) => n.sourceId === n2.sourceId && n.id !== n2.id)
      .slice(0, 4);
    for (const n of relatedFlows) {
      lines.push(`  related-flow: ${n.sourceLabel} → ${n.targetLabel}: ${n.flow}`);
    }
  } else if (prose) {
    lines.push(`[PROSE:${prose.kind.toUpperCase()}] id: ${prose.id}`);
    lines.push(`  ${serializeElement(prose as unknown as Record<string, unknown>)}`);
    lines.push(`  status: ${prose.status}`);
  } else if (sub) {
    lines.push(`[SUBSYSTEM] ${serializeElement(sub as unknown as Record<string, unknown>)}`);
    lines.push(`  components: ${sub.componentIds.length} total`);
  } else {
    lines.push(`[UNKNOWN] id: ${id} (not found in composed IR)`);
  }

  const result = lines.join("\n");
  return result.slice(0, 2000);
}

/**
 * Build a ContextBundle for a candidate pair.
 * Includes 1-hop neighborhoods for source and target, plus relevant corpus quotes.
 */
export function buildContextBundle(
  sourceId: string,
  targetId: string,
  ir: InferredComposedIR
): ContextBundle {
  const sourceNeighborhood = serializeNeighborhood(sourceId, ir);
  const targetNeighborhood = serializeNeighborhood(targetId, ir);

  // Collect relevant corpus quotes: prose entry quotes where the element
  // ids are referenced or the names match
  const quotes: string[] = [];
  for (const entry of ir.proseEntries) {
    if (entry.id === sourceId || entry.id === targetId) {
      quotes.push(`[${entry.kind}] ${entry.citation.quote}`);
    }
  }

  return {
    sourceNeighborhood,
    targetNeighborhood,
    corpusQuotes: quotes.slice(0, 10),
  };
}
