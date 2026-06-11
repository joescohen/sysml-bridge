/**
 * type-gate.ts — Deterministic pre-LLM type compatibility table.
 *
 * For each relation family, validates that (sourceKind, targetKind) are
 * structurally compatible per the spec §2 type gate rules.
 * Ill-typed candidates receive a structured reason code: rejected_type:<rule>
 *
 * Rules (A1):
 *   allocation   : source ∈ leaf functions (level=L3, ActionUsage-backed)
 *                  target ∈ components (PartUsage-backed)
 *   modeMembership: source ∈ leaf functions (level=L3)
 *                   target ∈ prose modes (kind="mode")
 *   flowTyping   : source ∈ n2 flow entries (kind="n2")
 *                  target ∈ interface entries (kind="interface" prose entries)
 *   controlJoin  : source ∈ leaf functions (level=L3)
 *                  target ∈ leaf functions (level=L3)
 *                  AND source.owner === target.owner (sibling actions of one function)
 */

import type { InferredComposedIR } from "@sysml-bridge/ir";
import type { ProseApprovedEntry } from "@sysml-bridge/ir";
import type { RelationFamily, TypedCandidate, RejectedCandidate } from "./types.js";

// ── Element kind classification helpers ──────────────────────────────────────

export type IrElement =
  | { kind: "function"; id: string; level: string; owner: string; naturalKey: string; name: string }
  | { kind: "component"; id: string; naturalKey: string; name: string }
  | { kind: "n2"; id: string; sourceId: string; targetId: string; flow: string; scope: string }
  | { kind: "mode"; id: string; fields: Record<string, unknown> }
  | { kind: "interface"; id: string; fields: Record<string, unknown> }
  | { kind: "prose"; id: string; proseKind: string; fields: Record<string, unknown> };

function isLeafFunction(el: unknown): el is { kind: "function"; id: string; level: string; owner: string } {
  return (
    typeof el === "object" &&
    el !== null &&
    (el as Record<string, unknown>)["kind"] === "function" &&
    (el as Record<string, unknown>)["level"] === "L3"
  );
}

function isComponent(el: unknown): el is { kind: "component"; id: string } {
  return (
    typeof el === "object" &&
    el !== null &&
    (el as Record<string, unknown>)["kind"] === "component"
  );
}

function isN2Flow(el: unknown): el is { kind: "n2"; id: string } {
  return (
    typeof el === "object" &&
    el !== null &&
    (el as Record<string, unknown>)["kind"] === "n2"
  );
}

function isProseMode(el: unknown): boolean {
  return (
    typeof el === "object" &&
    el !== null &&
    (el as Record<string, unknown>)["kind"] === "mode"
  );
}

function isProseInterface(el: unknown): boolean {
  return (
    typeof el === "object" &&
    el !== null &&
    (el as Record<string, unknown>)["kind"] === "interface"
  );
}

// ── Type Gate ────────────────────────────────────────────────────────────────

export interface TypeGateResult {
  accepted: TypedCandidate[];
  rejected: RejectedCandidate[];
}

/**
 * Build an id-to-element lookup from the composed IR (corpus + prose entries).
 * Returns a Map<id, element> used by the type gate.
 */
export function buildElementMap(ir: InferredComposedIR): Map<string, unknown> {
  const map = new Map<string, unknown>();

  // Corpus entities
  const corpus = ir.extracted;
  for (const e of corpus.functions ?? []) map.set(e.id, e);
  for (const e of corpus.components ?? []) map.set(e.id, e);
  for (const e of corpus.n2Interfaces ?? []) map.set(e.id, e);
  for (const e of corpus.requirements ?? []) map.set(e.id, e);
  for (const e of corpus.needs ?? []) map.set(e.id, e);
  for (const e of corpus.subsystems ?? []) map.set(e.id, e);
  for (const e of corpus.behaviorDecomp ?? []) map.set(e.id, e);
  for (const e of corpus.kpps ?? []) map.set(e.id, e);

  // Prose entries (modes, interfaces, etc.)
  for (const entry of ir.proseEntries) {
    map.set(entry.id, { kind: entry.kind, id: entry.id, fields: entry.fields });
  }

  return map;
}

/**
 * Apply the type gate to a candidate pair.
 *
 * Returns a { pass: true, typed } or { pass: false, reasonCode, reason } result.
 */
export function checkTypeGate(
  family: RelationFamily,
  sourceId: string,
  targetId: string,
  elementMap: Map<string, unknown>
): { pass: true } | { pass: false; reasonCode: string; reason: string } {
  const source = elementMap.get(sourceId);
  const target = elementMap.get(targetId);

  switch (family) {
    case "allocation": {
      // source MUST be a leaf function (level=L3)
      if (!isLeafFunction(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:allocation.source_not_leaf_function",
          reason: `allocation source '${sourceId}' is not a leaf function (L3 ActionUsage); got kind=${source ? (source as Record<string,unknown>)["kind"] : "missing"}, level=${source ? (source as Record<string,unknown>)["level"] : "missing"}`,
        };
      }
      // target MUST be a component
      if (!isComponent(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:allocation.target_not_component",
          reason: `allocation target '${targetId}' is not a component (PartUsage); got kind=${target ? (target as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      return { pass: true };
    }

    case "modeMembership": {
      // source MUST be a leaf function (level=L3)
      if (!isLeafFunction(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:modeMembership.source_not_leaf_function",
          reason: `modeMembership source '${sourceId}' is not a leaf function (L3); got kind=${source ? (source as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      // target MUST be a prose mode entry
      if (!isProseMode(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:modeMembership.target_not_mode",
          reason: `modeMembership target '${targetId}' is not an approved prose mode entry; got kind=${target ? (target as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      return { pass: true };
    }

    case "flowTyping": {
      // source MUST be an N2 flow entry
      if (!isN2Flow(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:flowTyping.source_not_n2_flow",
          reason: `flowTyping source '${sourceId}' is not an N2 flow entry; got kind=${source ? (source as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      // target MUST be a prose interface entry
      if (!isProseInterface(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:flowTyping.target_not_interface",
          reason: `flowTyping target '${targetId}' is not a prose interface entry; got kind=${target ? (target as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      return { pass: true };
    }

    case "controlJoin": {
      // BOTH source and target MUST be leaf functions
      if (!isLeafFunction(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:controlJoin.source_not_leaf_function",
          reason: `controlJoin source '${sourceId}' is not a leaf function (L3); got kind=${source ? (source as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      if (!isLeafFunction(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:controlJoin.target_not_leaf_function",
          reason: `controlJoin target '${targetId}' is not a leaf function (L3); got kind=${target ? (target as Record<string,unknown>)["kind"] : "missing"}`,
        };
      }
      // BOTH must share the same owner (sibling actions of one function)
      const srcOwner = (source as { owner: string }).owner;
      const tgtOwner = (target as { owner: string }).owner;
      if (srcOwner !== tgtOwner) {
        return {
          pass: false,
          reasonCode: "rejected_type:controlJoin.not_sibling_actions",
          reason: `controlJoin source and target have different owners ('${srcOwner}' vs '${tgtOwner}') — not sibling actions of one function`,
        };
      }
      // Disallow self-joins
      if (sourceId === targetId) {
        return {
          pass: false,
          reasonCode: "rejected_type:controlJoin.self_join",
          reason: `controlJoin source and target are the same element '${sourceId}'`,
        };
      }
      return { pass: true };
    }

    default: {
      const exhaustive: never = family;
      return {
        pass: false,
        reasonCode: "rejected_type:unknown_family",
        reason: `unknown relation family '${exhaustive as string}'`,
      };
    }
  }
}

/**
 * Apply the type gate to a batch of raw candidate pairs.
 * Returns accepted typed candidates and rejected (reason-coded) candidates.
 */
export function applyTypeGate(
  candidates: Array<{ family: RelationFamily; sourceId: string; targetId: string; stableId: string }>,
  elementMap: Map<string, unknown>
): TypeGateResult {
  const accepted: TypedCandidate[] = [];
  const rejected: RejectedCandidate[] = [];

  for (const cand of candidates) {
    const result = checkTypeGate(cand.family, cand.sourceId, cand.targetId, elementMap);
    if (result.pass) {
      accepted.push({
        id: cand.stableId,
        relationFamily: cand.family,
        sourceId: cand.sourceId,
        targetId: cand.targetId,
        stage: "typed",
      });
    } else {
      rejected.push({
        id: cand.stableId,
        relationFamily: cand.family,
        sourceId: cand.sourceId,
        targetId: cand.targetId,
        stage: "rejected_type",
        reasonCode: result.reasonCode,
        reason: result.reason,
      });
    }
  }

  return { accepted, rejected };
}
