/**
 * composition-table.ts — the EXPLICIT, small relation-composition table for
 * 2-hop chain enumeration (W2, spec §4).
 *
 * A chain composes two ALREADY-ACCEPTED relations sharing a middle entity:
 *
 *     A --left--> B    ∘    B --right--> C    ⇒    A --result--> C
 *
 * The set of legal `(left, right) → result` compositions is fixed HERE and
 * grows ONLY by adding a row to `COMPOSITION_TABLE` — never by inference, never
 * by an LLM. A composition that is not in the table is REJECTED by the chain
 * type gate (`applyChainTypeGate`); there is no "default" or "fallthrough" that
 * would silently admit an unlisted composition.
 *
 * §9 growth policy: every table entry MUST have a paired type-gate test. The
 * registry-completeness check (`composition-table.test.ts`) mirrors the
 * query-keys pattern — it compares `COMPOSITION_KEYS` (derived from the live
 * table) against the test's declared `TESTED_COMPOSITIONS` set and FAILS if a
 * table entry lacks a test (or a test names a composition the table dropped).
 * So a new row cannot land green without its test.
 */

import { createHash } from "node:crypto";
import type { RelationFamily } from "./types.js";

/**
 * One legal composition. `left`/`right` are constituent relation family NAMES —
 * deliberately typed as `string`, not `RelationFamily`, because a constituent
 * relation may be a corpus family (e.g. `containment`, `interfaceAggregation`)
 * that is NOT one of the four pipeline families. The composed `result`, by
 * contrast, is always a pipeline `RelationFamily` — a chain candidate must be
 * routable through the existing propose/debate/queue stages.
 */
export interface CompositionEntry {
  /** Constituent family of the first hop A→B. */
  left: string;
  /** Constituent family of the second hop B→C. */
  right: string;
  /** The pipeline family the composed A→C candidate is emitted under. */
  result: RelationFamily;
}

/**
 * THE table. Initial entries (spec §4):
 *   - allocation ∘ containment      → allocation
 *   - flowTyping ∘ interfaceAggregation → flowTyping
 *
 * Extend ONLY by adding a row here AND its paired type-gate test (enforced by
 * the registry-completeness check).
 */
export const COMPOSITION_TABLE: readonly CompositionEntry[] = [
  { left: "allocation", right: "containment", result: "allocation" },
  { left: "flowTyping", right: "interfaceAggregation", result: "flowTyping" },
];

/** Canonical registry key for a (left, right) composition. */
export function compositionKey(left: string, right: string): string {
  return `${left}∘${right}`;
}

/**
 * The set of composition keys the live table admits — the canonical registry
 * the completeness check compares the test coverage against.
 */
export const COMPOSITION_KEYS: ReadonlySet<string> = new Set(
  COMPOSITION_TABLE.map((e) => compositionKey(e.left, e.right)),
);

// Fail-fast: the table itself must not contain a duplicate (left,right) whose
// result disagrees — that would make composition non-deterministic.
(() => {
  const byKey = new Map<string, RelationFamily>();
  for (const e of COMPOSITION_TABLE) {
    const k = compositionKey(e.left, e.right);
    const prior = byKey.get(k);
    if (prior !== undefined && prior !== e.result) {
      throw new Error(
        `composition-table: conflicting result for '${k}' ('${prior}' vs '${e.result}')`,
      );
    }
    byKey.set(k, e.result);
  }
})();

/**
 * Look up the composed pipeline family for a `(left, right)` pair.
 * Returns the `RelationFamily` result, or `null` when the composition is NOT in
 * the table (the caller — the chain type gate — rejects a null).
 */
export function composeChain(left: string, right: string): RelationFamily | null {
  for (const e of COMPOSITION_TABLE) {
    if (e.left === left && e.right === right) return e.result;
  }
  return null;
}

/** Whether `(left, right)` is a legal, table-listed composition. */
export function isLegalComposition(left: string, right: string): boolean {
  return composeChain(left, right) !== null;
}

// ── Chain candidate types + chain type gate ──────────────────────────────────

/**
 * A raw 2-hop chain candidate BEFORE the composition (type) gate. Endpoints are
 * canonical entity ids (A, B, C). `premiseIds` are the two constituent relation
 * ids plus their evidence chunk ids — the citable premises for the composed link.
 */
export interface RawChainCandidate {
  stableId: string;
  leftFamily: string;
  rightFamily: string;
  /** A — composed source entity. */
  sourceId: string;
  /** B — shared middle entity. */
  middleId: string;
  /** C — composed target entity. */
  targetId: string;
  /** [leftRelId, rightRelId, …evidenceChunkIds] — resolvable premises. */
  premiseIds: string[];
}

/** A chain candidate that passed the composition gate, carrying its result family. */
export interface TypedChainCandidate {
  id: string;
  relationFamily: RelationFamily;
  sourceId: string;
  targetId: string;
  premiseIds: string[];
  stage: "typed_chain";
}

/** A chain candidate rejected by the composition gate (composition not in table). */
export interface RejectedChainCandidate {
  id: string;
  leftFamily: string;
  rightFamily: string;
  sourceId: string;
  targetId: string;
  stage: "rejected_chain_composition";
  reasonCode: "rejected_chain_composition:not_in_table";
  reason: string;
}

export interface ChainTypeGateResult {
  accepted: TypedChainCandidate[];
  rejected: RejectedChainCandidate[];
}

/**
 * The CHAIN TYPE GATE. A chain candidate is admitted IFF its `(leftFamily,
 * rightFamily)` composition is in `COMPOSITION_TABLE`. An illegal composition
 * (not in the table) is REJECTED here — this is the sole place the composition
 * legality is decided, so the table is the single source of truth.
 */
export function applyChainTypeGate(
  candidates: readonly RawChainCandidate[],
): ChainTypeGateResult {
  const accepted: TypedChainCandidate[] = [];
  const rejected: RejectedChainCandidate[] = [];

  for (const c of candidates) {
    const result = composeChain(c.leftFamily, c.rightFamily);
    if (result === null) {
      rejected.push({
        id: c.stableId,
        leftFamily: c.leftFamily,
        rightFamily: c.rightFamily,
        sourceId: c.sourceId,
        targetId: c.targetId,
        stage: "rejected_chain_composition",
        reasonCode: "rejected_chain_composition:not_in_table",
        reason: `composition '${compositionKey(c.leftFamily, c.rightFamily)}' is not in COMPOSITION_TABLE (extend only via the table)`,
      });
      continue;
    }
    accepted.push({
      id: c.stableId,
      relationFamily: result,
      sourceId: c.sourceId,
      targetId: c.targetId,
      premiseIds: c.premiseIds,
      stage: "typed_chain",
    });
  }

  return { accepted, rejected };
}

/** Deterministic stable id for a chain candidate. */
export function chainStableId(
  leftFamily: string,
  rightFamily: string,
  sourceId: string,
  middleId: string,
  targetId: string,
): string {
  const input = `chain:${leftFamily}:${rightFamily}:${sourceId}:${middleId}:${targetId}`;
  const hex = createHash("sha256").update(input, "utf8").digest("hex");
  return `chain-${hex.slice(0, 16)}`;
}
