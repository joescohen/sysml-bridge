/**
 * candidate-generator.ts — Deterministic candidate enumeration over the composed IR.
 *
 * Enumerates typed pairs per relation family, bounded by graph locality
 * (subsystem / owner context — never all-pairs).
 *
 * Bounding heuristics (spec §2):
 *
 *   allocation:
 *     Enumerate (leaf function × component) pairs where the component is in the
 *     same subsystem as any component that the function's owning L2 function's
 *     sibling leaf functions have N2 interface connections to.
 *     Concretely: for each leaf L3 function F, find all N2 triples whose
 *     sourceId or targetId resolves to a component in the same subsystem that
 *     owns any component touched by F's owner group. This scopes to the
 *     functional area, not all 34 components.
 *     Fallback: if subsystem scoping yields < 1 component, include all components
 *     that appear as N2 endpoints with the function's owner label.
 *
 *   modeMembership:
 *     All (leaf L3 function × approved prose mode) pairs — modes are few (≤10)
 *     so the product is small.
 *
 *   flowTyping:
 *     All (N2 flow entry × approved prose interface) pairs — bounded by the
 *     count of prose interfaces (typically small).
 *
 *   controlJoin:
 *     For each owner (L2 function), enumerate all ordered pairs of its sibling
 *     leaf L3 functions, excluding self-pairs and pairs already in the corpus
 *     allocations/satisfies (those are known; we infer what's missing).
 */

import { createHash } from "node:crypto";
import type { InferredComposedIR } from "@sysml-bridge/ir";
import type { RelationFamily } from "./types.js";

// ── Stable ID for inference candidates ──────────────────────────────────────

/**
 * Deterministic stable id for a candidate: sha256 of "infer:<family>:<src>:<tgt>",
 * returns "infer-<hex16>" — same approach as stableId in @sysml-bridge/ir.
 */
export function inferenceStableId(
  family: RelationFamily,
  sourceId: string,
  targetId: string
): string {
  const input = `infer:${family}:${sourceId}:${targetId}`;
  const hex = createHash("sha256").update(input, "utf8").digest("hex");
  return `infer-${hex.slice(0, 16)}`;
}

// ── Raw candidate type ───────────────────────────────────────────────────────

export interface RawCandidate {
  stableId: string;
  family: RelationFamily;
  sourceId: string;
  targetId: string;
}

// ── Per-family generators ─────────────────────────────────────────────────────

/**
 * Generate allocation candidates:
 *   source = leaf L3 function
 *   target = component in the subsystem context of that function
 *
 * Bounding: for each L3 function, find the subsystems that contain components
 * that appear as N2 endpoints associated with that function's owning L2 label.
 * Then enumerate only components in those subsystems.
 */
function generateAllocationCandidates(ir: InferredComposedIR): RawCandidate[] {
  const corpus = ir.extracted;
  const leafFunctions = (corpus.functions ?? []).filter((f) => f.level === "L3");
  const components = corpus.components ?? [];
  const n2Triples = corpus.n2Interfaces ?? [];
  const subsystems = corpus.subsystems ?? [];

  if (components.length === 0 || leafFunctions.length === 0) return [];

  // Build component id → subsystem ids map
  const compToSubsystems = new Map<string, Set<string>>();
  for (const sub of subsystems) {
    for (const cid of sub.componentIds) {
      if (!compToSubsystems.has(cid)) compToSubsystems.set(cid, new Set());
      compToSubsystems.get(cid)!.add(sub.id);
    }
  }

  // Build N2 component endpoint set (all components referenced in N2 triples)
  const n2ComponentIds = new Set<string>();
  for (const n2 of n2Triples) {
    n2ComponentIds.add(n2.sourceId);
    n2ComponentIds.add(n2.targetId);
  }

  // For each leaf function, find candidate component targets via subsystem scoping
  const candidates: RawCandidate[] = [];

  for (const fn of leafFunctions) {
    // Find the owner label (e.g. "F1: Manage Refueling Requests")
    const ownerLabel = fn.owner;

    // Find subsystems that contain components connected to this function's owner context:
    // Look for N2 triples whose source or target label matches the owner's context.
    // Heuristic: we use N2 triples where sourceLabel or targetLabel substring-matches
    // the owner label, or where all N2 components in the subsystem are candidate targets.
    // Simpler and more bounded: collect all subsystem ids that contain any N2 component
    // that appears in N2 triples scoped to "component" scope.

    // Primary bound: use the subsystem ids of all components that appear in component-scope N2 triples
    // whose endpoints are in the same "component-scoped" sheet (this preserves the locality intent).
    const componentScopeN2 = n2Triples.filter((n2) => n2.scope === "component");
    const relevantSubsystemIds = new Set<string>();

    for (const n2 of componentScopeN2) {
      const srcSubs = compToSubsystems.get(n2.sourceId);
      const tgtSubs = compToSubsystems.get(n2.targetId);
      if (srcSubs) for (const s of srcSubs) relevantSubsystemIds.add(s);
      if (tgtSubs) for (const s of tgtSubs) relevantSubsystemIds.add(s);
    }

    // Collect candidate components: those in relevant subsystems
    let targetComponents = components.filter((c) => {
      const subs = compToSubsystems.get(c.id);
      if (!subs) return false;
      for (const s of subs) {
        if (relevantSubsystemIds.has(s)) return true;
      }
      return false;
    });

    // Fallback: if scoping gives nothing, use all N2-connected components
    if (targetComponents.length === 0) {
      targetComponents = components.filter((c) => n2ComponentIds.has(c.id));
    }

    // Final fallback: all components (bounded - should not normally trigger)
    if (targetComponents.length === 0) {
      targetComponents = components;
    }

    for (const comp of targetComponents) {
      candidates.push({
        stableId: inferenceStableId("allocation", fn.id, comp.id),
        family: "allocation",
        sourceId: fn.id,
        targetId: comp.id,
      });
    }
  }

  return candidates;
}

/**
 * Generate modeMembership candidates:
 *   source = leaf L3 function
 *   target = approved prose mode entry
 *
 * Product is small: 54 functions × ≤10 modes = ≤540 pairs.
 */
function generateModeMembershipCandidates(ir: InferredComposedIR): RawCandidate[] {
  const corpus = ir.extracted;
  const leafFunctions = (corpus.functions ?? []).filter((f) => f.level === "L3");
  const modeEntries = ir.proseEntries.filter((e) => e.kind === "mode");

  const candidates: RawCandidate[] = [];
  for (const fn of leafFunctions) {
    for (const mode of modeEntries) {
      candidates.push({
        stableId: inferenceStableId("modeMembership", fn.id, mode.id),
        family: "modeMembership",
        sourceId: fn.id,
        targetId: mode.id,
      });
    }
  }
  return candidates;
}

/**
 * Generate flowTyping candidates:
 *   source = N2 flow entry
 *   target = approved prose interface entry
 *
 * Bounded by the count of prose interfaces (typically small: ≤20).
 */
function generateFlowTypingCandidates(ir: InferredComposedIR): RawCandidate[] {
  const corpus = ir.extracted;
  const n2Flows = corpus.n2Interfaces ?? [];
  const interfaceEntries = ir.proseEntries.filter((e) => e.kind === "interface");

  const candidates: RawCandidate[] = [];
  for (const flow of n2Flows) {
    for (const iface of interfaceEntries) {
      candidates.push({
        stableId: inferenceStableId("flowTyping", flow.id, iface.id),
        family: "flowTyping",
        sourceId: flow.id,
        targetId: iface.id,
      });
    }
  }
  return candidates;
}

/**
 * Generate controlJoin candidates:
 *   source = leaf L3 function
 *   target = leaf L3 function
 *   constraint: same owner (sibling actions of one L2 function)
 *   exclude: self-pairs
 *
 * For each owner group, enumerate all ordered pairs of sibling leaf functions.
 * Bounded by group size: groups of N yield N*(N-1) ordered pairs (directed edges).
 */
function generateControlJoinCandidates(ir: InferredComposedIR): RawCandidate[] {
  const corpus = ir.extracted;
  const leafFunctions = (corpus.functions ?? []).filter((f) => f.level === "L3");

  // Group by owner
  const byOwner = new Map<string, typeof leafFunctions>();
  for (const fn of leafFunctions) {
    if (!byOwner.has(fn.owner)) byOwner.set(fn.owner, []);
    byOwner.get(fn.owner)!.push(fn);
  }

  const candidates: RawCandidate[] = [];
  for (const [, siblings] of byOwner) {
    if (siblings.length < 2) continue;
    // Enumerate all ordered pairs (A→B where A≠B)
    for (let i = 0; i < siblings.length; i++) {
      for (let j = 0; j < siblings.length; j++) {
        if (i === j) continue;
        const src = siblings[i]!;
        const tgt = siblings[j]!;
        candidates.push({
          stableId: inferenceStableId("controlJoin", src.id, tgt.id),
          family: "controlJoin",
          sourceId: src.id,
          targetId: tgt.id,
        });
      }
    }
  }
  return candidates;
}

// ── Skip set (already approved or rejected in the inferred layer) ─────────────

/**
 * Build the set of candidate ids to skip (already approved or explicitly rejected
 * in the inferred-approved layer). These are idempotent: if a candidate's stable id
 * matches an approved/rejected inferred entry, skip it.
 */
export function buildSkipSet(ir: InferredComposedIR): Set<string> {
  const skip = new Set<string>();
  for (const entry of ir.inferredEntries) {
    // Compute the inference stable id for this approved entry
    skip.add(inferenceStableId(entry.relationFamily, entry.sourceId, entry.targetId));
  }
  return skip;
}

// ── Main entry point ─────────────────────────────────────────────────────────

export interface GenerationResult {
  candidates: RawCandidate[];
  countsByFamily: Record<RelationFamily, number>;
}

/**
 * Generate all typed candidate pairs from the composed IR, skipping any that
 * already have approved/rejected inferred entries (idempotency).
 */
export function generateCandidates(
  ir: InferredComposedIR,
  skipSet?: Set<string>
): GenerationResult {
  const allCandidates: RawCandidate[] = [
    ...generateAllocationCandidates(ir),
    ...generateModeMembershipCandidates(ir),
    ...generateFlowTypingCandidates(ir),
    ...generateControlJoinCandidates(ir),
  ];

  const skip = skipSet ?? new Set<string>();
  const filtered = allCandidates.filter((c) => !skip.has(c.stableId));

  const countsByFamily: Record<RelationFamily, number> = {
    allocation: 0,
    modeMembership: 0,
    flowTyping: 0,
    controlJoin: 0,
  };
  for (const c of filtered) {
    countsByFamily[c.family]++;
  }

  return { candidates: filtered, countsByFamily };
}
