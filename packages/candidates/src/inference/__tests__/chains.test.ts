/**
 * chains.test.ts — W2 2-hop chain enumeration + chain type gate.
 *
 * Done-criteria covered (spec §8 W2):
 *   - Chain candidates ONLY from ACCEPTED relations; a pending-proposal input
 *     produces ZERO chain candidates.
 *   - Illegal composition (not in the table) REJECTED by the chain type gate,
 *     with a FAIL-ABLE control: a legal composition passes.
 *   - 2-hop only; constituent relation ids are the composed candidate's premises.
 *   - Deterministic enumeration order.
 */

import { describe, it, expect } from "vitest";
import { enumerateChains, type AcceptedRelation } from "../chains.js";
import { applyChainTypeGate } from "../composition-table.js";

function rel(
  id: string,
  family: string,
  sourceId: string,
  targetId: string,
  status: AcceptedRelation["status"],
  evidenceChunkIds: string[] = [],
): AcceptedRelation {
  return { id, family, sourceId, targetId, status, evidenceChunkIds };
}

// ── Accepted-only composition ─────────────────────────────────────────────────

describe("chains — composes only ACCEPTED relations", () => {
  it("A--allocation-->B ∘ B--containment-->C ⇒ a 2-hop chain A→C with both relation ids as premises", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "accepted", ["chunk-1"]),
      rel("rel-2", "containment", "B", "C", "accepted", ["chunk-2"]),
    ];
    const { candidates, pendingSkipped } = enumerateChains(relations);
    expect(pendingSkipped).toBe(0);
    expect(candidates).toHaveLength(1);
    const c = candidates[0]!;
    expect(c.sourceId).toBe("A");
    expect(c.middleId).toBe("B");
    expect(c.targetId).toBe("C");
    expect(c.leftFamily).toBe("allocation");
    expect(c.rightFamily).toBe("containment");
    // Premises = the two constituent relation ids + their evidence chunks.
    expect(c.premiseIds).toEqual(["rel-1", "rel-2", "chunk-1", "chunk-2"]);
  });

  it("a PENDING-proposal input produces ZERO chain candidates", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "pending"),
      rel("rel-2", "containment", "B", "C", "pending"),
    ];
    const { candidates, pendingSkipped } = enumerateChains(relations);
    expect(candidates).toHaveLength(0);
    expect(pendingSkipped).toBe(2);
  });

  it("a pending middle hop is not composable → zero (accepted+pending mix)", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "accepted"),
      rel("rel-2", "containment", "B", "C", "pending"), // pending → dropped
    ];
    const { candidates } = enumerateChains(relations);
    expect(candidates).toHaveLength(0);
  });

  it("excludes self-chains (A→B→A)", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "accepted"),
      rel("rel-2", "containment", "B", "A", "accepted"),
    ];
    const { candidates } = enumerateChains(relations);
    expect(candidates).toHaveLength(0);
  });

  it("is deterministic: two runs produce byte-identical output", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "accepted"),
      rel("rel-2", "containment", "B", "C", "accepted"),
      rel("rel-3", "flowTyping", "X", "Y", "accepted"),
      rel("rel-4", "interfaceAggregation", "Y", "Z", "accepted"),
    ];
    const a = enumerateChains(relations);
    const b = enumerateChains(relations);
    expect(JSON.stringify(a.candidates)).toEqual(JSON.stringify(b.candidates));
  });
});

// ── Chain type gate: illegal composition rejected (fail-able control) ──────────

describe("chains — illegal composition REJECTED by the type gate (fail-able control)", () => {
  it("REJECTS a composition not in the table: allocation ∘ flowTyping", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "accepted"),
      rel("rel-2", "flowTyping", "B", "C", "accepted"), // (allocation, flowTyping) ∉ table
    ];
    const raw = enumerateChains(relations).candidates;
    expect(raw).toHaveLength(1); // the 2-hop path IS enumerated…
    const { accepted, rejected } = applyChainTypeGate(raw);
    expect(accepted).toHaveLength(0); // …but the type gate rejects it
    expect(rejected).toHaveLength(1);
    expect(rejected[0]!.reasonCode).toBe("rejected_chain_composition:not_in_table");
  });

  it("ACCEPTS the legal control: allocation ∘ containment → allocation", () => {
    const relations = [
      rel("rel-1", "allocation", "A", "B", "accepted"),
      rel("rel-2", "containment", "B", "C", "accepted"),
    ];
    const raw = enumerateChains(relations).candidates;
    const { accepted, rejected } = applyChainTypeGate(raw);
    expect(rejected).toHaveLength(0);
    expect(accepted).toHaveLength(1);
    expect(accepted[0]!.relationFamily).toBe("allocation"); // composed result family
    expect(accepted[0]!.sourceId).toBe("A");
    expect(accepted[0]!.targetId).toBe("C");
  });
});
