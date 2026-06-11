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
import type { ContextBundle, OfferedFact } from "./types.js";

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

// ── Offered facts (the premise id contract) ──────────────────────────────────

/** Cap on offered facts per candidate pair — keeps the prompt bounded. */
const MAX_OFFERED_FACTS = 25;

/**
 * Collect the facts offered for premise citation for a candidate pair:
 * the source element, the target element, and their 1-hop IR neighbors —
 * each with its composed-IR id, a display name, and a short detail string.
 *
 * The propose prompt renders each fact as `[id: <id>] <kind> "<name>" — <detail>`;
 * the deterministic premise repair matches ONLY within this list (name + aliases).
 */
export function collectOfferedFacts(
  sourceId: string,
  targetId: string,
  ir: InferredComposedIR
): OfferedFact[] {
  const corpus = ir.extracted;
  const facts: OfferedFact[] = [];
  const seen = new Set<string>();

  const add = (fact: OfferedFact) => {
    if (seen.has(fact.id) || facts.length >= MAX_OFFERED_FACTS) return;
    seen.add(fact.id);
    facts.push(fact);
  };

  const addElementAndNeighbors = (id: string) => {
    const fn = (corpus.functions ?? []).find((f) => f.id === id);
    const comp = (corpus.components ?? []).find((c) => c.id === id);
    const n2 = (corpus.n2Interfaces ?? []).find((n) => n.id === id);
    const prose = ir.proseEntries.find((e) => e.id === id);

    if (fn) {
      add({
        id: fn.id,
        kind: "function",
        name: fn.name,
        detail: `level ${fn.level}, owner ${fn.owner}`,
        aliases: [fn.naturalKey, `${fn.naturalKey}: ${fn.name}`],
      });
      // Requirements this function satisfies (satisfy chain)
      for (const sat of corpus.satisfies ?? []) {
        if (sat.functionId !== fn.id) continue;
        const req = (corpus.requirements ?? []).find((r) => r.id === sat.reqId);
        if (req) {
          add({
            id: req.id,
            kind: "requirement",
            name: req.name,
            detail: req.statement.slice(0, 120),
            aliases: [req.naturalKey],
          });
        }
      }
      // Sibling leaf functions (same owner)
      for (const sib of (corpus.functions ?? []).filter(
        (f) => f.owner === fn.owner && f.id !== fn.id
      ).slice(0, 4)) {
        add({
          id: sib.id,
          kind: "function",
          name: sib.name,
          detail: `sibling of "${fn.name}" (owner ${fn.owner})`,
          aliases: [sib.naturalKey, `${sib.naturalKey}: ${sib.name}`],
        });
      }
      // Functional-N2 flows touching the function's owner context
      for (const t of (corpus.n2Interfaces ?? []).filter(
        (n) => n.sourceLabel === fn.owner || n.targetLabel === fn.owner
      ).slice(0, 4)) {
        add({
          id: t.id,
          kind: "n2-flow",
          name: t.flow,
          detail: `${t.sourceLabel} → ${t.targetLabel} [${t.scope}]`,
        });
      }
    } else if (comp) {
      add({
        id: comp.id,
        kind: "component",
        name: comp.name,
        detail: `naturalKey ${comp.naturalKey}`,
        aliases: [comp.naturalKey],
      });
      // Subsystems containing this component
      for (const sub of (corpus.subsystems ?? []).filter((s) =>
        s.componentIds.includes(comp.id)
      )) {
        add({
          id: sub.id,
          kind: "subsystem",
          name: sub.name,
          detail: `contains "${comp.name}"`,
          aliases: [sub.naturalKey],
        });
      }
      // Component-scope N2 flows touching this component
      for (const t of (corpus.n2Interfaces ?? []).filter(
        (n) => n.sourceId === comp.id || n.targetId === comp.id
      ).slice(0, 6)) {
        add({
          id: t.id,
          kind: "n2-flow",
          name: t.flow,
          detail: `${t.sourceLabel} → ${t.targetLabel} [${t.scope}]`,
        });
      }
    } else if (n2) {
      add({
        id: n2.id,
        kind: "n2-flow",
        name: n2.flow,
        detail: `${n2.sourceLabel} → ${n2.targetLabel} [${n2.scope}]`,
      });
      // Endpoint components
      for (const endpointId of [n2.sourceId, n2.targetId]) {
        const endpoint = (corpus.components ?? []).find((c) => c.id === endpointId);
        if (endpoint) {
          add({
            id: endpoint.id,
            kind: "component",
            name: endpoint.name,
            detail: `endpoint of flow "${n2.flow}"`,
            aliases: [endpoint.naturalKey],
          });
        }
      }
    } else if (prose) {
      const name = typeof prose.fields["name"] === "string" ? (prose.fields["name"] as string) : prose.id;
      add({
        id: prose.id,
        kind: prose.kind,
        name,
        detail: prose.citation.quote.slice(0, 120),
      });
    }
  };

  addElementAndNeighbors(sourceId);
  addElementAndNeighbors(targetId);

  return facts;
}

/**
 * Build a ContextBundle for a candidate pair.
 * Includes 1-hop neighborhoods for source and target, the offered fact list
 * (the premise id contract), plus relevant corpus quotes.
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
    offeredFacts: collectOfferedFacts(sourceId, targetId, ir),
  };
}
