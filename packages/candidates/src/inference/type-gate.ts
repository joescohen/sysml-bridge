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

import type { InferredComposedIR } from "@sysml-bridge/model";
import type { ProseApprovedEntry } from "@sysml-bridge/model";
import type { RelationFamily, TypedCandidate, RejectedCandidate } from "./types.js";
// Type-only import from the LEAF cluster module (not the entities barrel) to
// avoid an inference→entities→inference import cycle: entities/adjudicate.ts
// already imports from ../inference. cluster.ts imports only mentions + model.
import type { EntityRecord } from "../entities/cluster.js";

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

/** Element `kind` accessor shared by the trace-family predicates. Works on both
 *  composed-IR corpus entities (which carry a `kind` literal from the Extracted
 *  schema) and the entity-store synthetic elements from `buildEntityElementMap`. */
function kindOf(el: unknown): string | undefined {
  if (typeof el !== "object" || el === null) return undefined;
  const k = (el as Record<string, unknown>)["kind"];
  return typeof k === "string" ? k : undefined;
}

/** A design element that can SATISFY a requirement: a component/part or a function. */
function isDesignElement(el: unknown): boolean {
  const k = kindOf(el);
  return k === "component" || k === "function";
}

function isRequirement(el: unknown): boolean {
  return kindOf(el) === "requirement";
}

/** A containment PARENT: a component or a subsystem (a subsystem contains
 *  components; a component contains sub-components). The cross-document entity
 *  path only produces `component` entities, so the `subsystem` arm matters for
 *  the corpus element map (`buildElementMap`), where subsystems carry
 *  `kind:"subsystem"` — kept for completeness. */
function isContainmentParent(el: unknown): boolean {
  const k = kindOf(el);
  return k === "component" || k === "subsystem";
}

function isNeed(el: unknown): boolean {
  return kindOf(el) === "need";
}

function isVerification(el: unknown): boolean {
  return kindOf(el) === "verification";
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

    case "satisfy": {
      // source = design element (component/part or function) that SATISFIES;
      // target = the requirement being satisfied. Serializes as
      // `satisfy <target-req> by <source-element>;` (R4: operands are usages).
      if (!isDesignElement(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:satisfy.source_not_design_element",
          reason: `satisfy source '${sourceId}' is not a design element (component/part or function); got kind=${kindOf(source) ?? "missing"}`,
        };
      }
      if (!isRequirement(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:satisfy.target_not_requirement",
          reason: `satisfy target '${targetId}' is not a requirement; got kind=${kindOf(target) ?? "missing"}`,
        };
      }
      return { pass: true };
    }

    case "derive": {
      // source = system requirement; target = stakeholder need. Serializes as
      // `dependency from <source-req> to <target-need>;` (the backward-trace edge
      // and the mechanism by which a need is covered).
      if (!isRequirement(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:derive.source_not_requirement",
          reason: `derive source '${sourceId}' is not a requirement; got kind=${kindOf(source) ?? "missing"}`,
        };
      }
      if (!isNeed(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:derive.target_not_need",
          reason: `derive target '${targetId}' is not a stakeholder need; got kind=${kindOf(target) ?? "missing"}`,
        };
      }
      return { pass: true };
    }

    case "verify": {
      // source = verification case; target = requirement. NOTE: `verify` is NOT
      // enumerated by any proposer (no `verification` mention/proposal source
      // exists, and per R3 the serializer emits VerifyRequirementUsage as a nested
      // `objective { verify <req>; }` body on a VerificationCaseDefinition element,
      // not a flat trace line — the entity/co-occurrence path cannot supply that
      // element). The case is kept so the family is well-typed if a verification
      // endpoint is ever supplied structurally; it is a no-op in the weave loop.
      if (!isVerification(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:verify.source_not_verification",
          reason: `verify source '${sourceId}' is not a verification case; got kind=${kindOf(source) ?? "missing"}`,
        };
      }
      if (!isRequirement(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:verify.target_not_requirement",
          reason: `verify target '${targetId}' is not a requirement; got kind=${kindOf(target) ?? "missing"}`,
        };
      }
      return { pass: true };
    }

    case "containment": {
      // Structural composition: source = parent (component or subsystem),
      // target = child component. Direction is parent(source) CONTAINS
      // child(target) — the direction a chain composes through
      // (`A --allocation--> B ∘ B --containment--> C`). Co-occurrence is
      // symmetric, so BOTH directions are enumerated and the HUMAN GATE resolves
      // which is the real parent→child; the type gate only keeps well-typed
      // (component/subsystem → component) pairs. Self-containment is rejected.
      if (!isContainmentParent(source)) {
        return {
          pass: false,
          reasonCode: "rejected_type:containment.source_not_component",
          reason: `containment source '${sourceId}' is not a component or subsystem (containment parent); got kind=${kindOf(source) ?? "missing"}`,
        };
      }
      if (!isComponent(target)) {
        return {
          pass: false,
          reasonCode: "rejected_type:containment.target_not_component",
          reason: `containment target '${targetId}' is not a component (containment child); got kind=${kindOf(target) ?? "missing"}`,
        };
      }
      if (sourceId === targetId) {
        return {
          pass: false,
          reasonCode: "rejected_type:containment.self_containment",
          reason: `containment source and target are the same element '${sourceId}' (a component cannot contain itself)`,
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

// ── Entity-aware element map (cross-document co-occurrence typing) ────────────

/**
 * Build an element map keyed by canonical entity id, mapping each entity's
 * `kind` onto the SAME element shapes the existing family predicates already
 * understand — so cross-document co-occurrence candidates are typed by the
 * EXISTING `checkTypeGate`/`applyTypeGate`, with no change to the gate rules.
 *
 * Kind → synthetic element:
 *   function   → { kind:"function", level:"L3", owner:"<per-entity>" }
 *   component  → { kind:"component" }
 *   mode       → { kind:"mode" }
 *   interface  → { kind:"interface" }
 *   flow       → { kind:"n2" }        (types as an N2 flow source)
 *   requirement/unknown → { kind:"prose" }  (matches no pipeline family → rejected)
 *
 * The per-entity `owner` (unique to each entity) means a controlJoin between two
 * DISTINCT function entities is rejected by the existing `not_sibling_actions`
 * rule — correct, since sibling-action control joins are an intra-document
 * structural notion, not a cross-document one.
 */
export function buildEntityElementMap(
  entities: readonly EntityRecord[],
): Map<string, unknown> {
  const map = new Map<string, unknown>();
  for (const e of entities) {
    switch (e.kind) {
      case "function":
        map.set(e.entityId, {
          kind: "function",
          id: e.entityId,
          level: "L3",
          owner: `entity:${e.entityId}`,
          naturalKey: e.canonicalName,
          name: e.canonicalName,
        });
        break;
      case "component":
        map.set(e.entityId, {
          kind: "component",
          id: e.entityId,
          naturalKey: e.canonicalName,
          name: e.canonicalName,
        });
        break;
      case "mode":
        map.set(e.entityId, { kind: "mode", id: e.entityId, fields: { name: e.canonicalName } });
        break;
      case "interface":
        map.set(e.entityId, { kind: "interface", id: e.entityId, fields: { name: e.canonicalName } });
        break;
      case "flow":
        map.set(e.entityId, {
          kind: "n2",
          id: e.entityId,
          sourceId: "",
          targetId: "",
          flow: e.canonicalName,
          scope: "component",
        });
        break;
      case "requirement":
        // Typed for the satisfy (target) and derive (source) trace families.
        map.set(e.entityId, {
          kind: "requirement",
          id: e.entityId,
          naturalKey: e.canonicalName,
          name: e.canonicalName,
        });
        break;
      case "need":
        // Typed for the derive (target) trace family. `need` is a distinct
        // MentionKind so stakeholder needs cluster into their own entities.
        map.set(e.entityId, {
          kind: "need",
          id: e.entityId,
          naturalKey: e.canonicalName,
          name: e.canonicalName,
        });
        break;
      case "verification":
        // Typed for the verify trace family's SOURCE. No proposer enumerates
        // verify (see the verify case in checkTypeGate), so in practice no
        // verification entities are produced — kept for type completeness.
        map.set(e.entityId, {
          kind: "verification",
          id: e.entityId,
          naturalKey: e.canonicalName,
          name: e.canonicalName,
        });
        break;
      default:
        // unknown — no pipeline family types this; a candidate over such an
        // endpoint is rejected by the existing gate (correct).
        map.set(e.entityId, { kind: "prose", id: e.entityId, proseKind: e.kind, fields: {} });
        break;
    }
  }
  return map;
}
