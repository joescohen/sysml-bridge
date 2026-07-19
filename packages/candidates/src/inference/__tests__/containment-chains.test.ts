/**
 * containment-chains.test.ts — the WHOLE POINT of the weaver-containment work:
 * prove a hub-and-spoke CHAIN forms from a pure document corpus that could NOT
 * form before a containment relation existed.
 *
 * The gap (brief §Why): the chain enumerator composes `allocation ∘ containment
 * → allocation`, but NOTHING ever produced a containment relation — so from a
 * document corpus (no pre-extracted structural containment) chains could never
 * form. This proves the closed loop, end to end:
 *
 *   1. ENUMERATE  — two components CO-OCCUR in a chunk (no pre-existing
 *      containment). `containment` is proposed in BOTH directions (co-occurrence
 *      is symmetric); the HUMAN GATE resolves which is the real parent→child.
 *   2. TYPE-GATE  — component→component passes; self-containment and
 *      component→requirement are rejected (fail-able controls).
 *   3. APPROVE    — a human approves ONE direction (mock): an InferredApprovedEntry
 *      (status "approved") PROJECTS to an AcceptedRelation (family "containment",
 *      status "accepted") via projectApprovedInferredToRelations (§B).
 *   4. BEFORE     — with only an accepted allocation and NO containment, the chain
 *      enumerator yields ZERO chains: the chain literally cannot form.
 *   5. AFTER      — feed the approved containment alongside the allocation → a NEW
 *      allocation chain forms (`fn --allocation--> parent ∘ parent --containment-->
 *      child ⇒ fn --allocation--> child`), premised on the two constituent
 *      relation ids + evidence, and it ROUTES through the engine pipeline (queued).
 *
 * Invariants asserted: no auto-approve (both directions are proposals only);
 * determinism; premise contract (chain premises = constituent rel ids + evidence).
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR, InferredApprovedEntry } from "@sysml-bridge/model";
import { SCHEMA_VERSION } from "@sysml-bridge/model";
import { runInferenceEngine, type EntityStoreInput } from "../engine.js";
import { enumerateCooccurrence } from "../cooccurrence.js";
import { checkTypeGate, buildEntityElementMap } from "../type-gate.js";
import {
  enumerateChains,
  projectApprovedInferredToRelations,
  type AcceptedRelation,
} from "../chains.js";
import { applyChainTypeGate } from "../composition-table.js";
import type { InferenceProvider } from "../inference-provider.js";
import type { ContextBundle, ProposeResult, QueuedRecord, RelationFamily } from "../types.js";
import type { EntityRecord } from "../../entities/cluster.js";
import type { MentionRecord } from "../../mentions/index.js";

// ── Fixtures ───────────────────────────────────────────────────────────────────

function emptyIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "Empty",
    needs: [], requirements: [], functions: [], components: [],
    satisfies: [], allocations: [], subsystems: [], n2Interfaces: [],
    kpps: [], behaviorDecomp: [],
  };
  return {
    extracted: extracted as never,
    proseEntries: [],
    approvedProseIds: new Set<string>(),
    inferredEntries: [],
    approvedInferredIds: new Set<string>(),
  };
}

function entity(kind: EntityRecord["kind"], name: string, mentionIds: string[]): EntityRecord {
  return {
    entityId: `entity-${kind}-${name.replace(/\s+/g, "_")}`,
    kind, canonicalName: name, aliases: [name], mentionIds, mergeDispositions: [],
  };
}

function mention(mentionId: string, kind: MentionRecord["kindHint"], name: string, chunkId: string): MentionRecord {
  return {
    mentionId, surfaceForm: name, kindHint: kind,
    citation: { docId: "doc", docSha256: "aa".repeat(32), chunkId, sectionPath: "S1", quote: name },
    confidence: 0.9,
  };
}

/** Cites every offered fact id as a premise → all premises resolve → queued. */
class CiteOfferedProvider implements InferenceProvider {
  async propose(family: RelationFamily, sourceId: string, targetId: string, context: ContextBundle): Promise<ProposeResult> {
    return {
      kind: "proposal",
      proposal: {
        sourceId, targetId, relationFamily: family,
        premises: context.offeredFacts.map((f) => f.id),
        rationale: "audit-only", confidence: 0.95,
      },
    };
  }
  async advocate() { return { score: 0.9, summary: "n/a" }; }
  async challenge() { return { score: 0.1, summary: "n/a" }; }
}

// A function, a PARENT component, and a CHILD component. The two components
// co-occur in chunk-1 (a "the Position Sensor Array consists of a Sensing
// Element" sentence) — the ONLY structural signal; there is NO pre-extracted
// containment anywhere.
const eFn = entity("function", "Detect Cargo Presence", ["m-fn"]);
const eParent = entity("component", "Position Sensor Array", ["m-parent"]);
const eChild = entity("component", "Sensing Element", ["m-child"]);
const eReq = entity("requirement", "Cargo Detection Requirement", ["m-req"]);
const mFn = mention("m-fn", "function", "Detect Cargo Presence", "chunk-1");
const mParent = mention("m-parent", "component", "Position Sensor Array", "chunk-1");
const mChild = mention("m-child", "component", "Sensing Element", "chunk-1");

const CHILD_CHUNK = "chunk-1";

// ── 1 + 2: enumerate + type-gate ───────────────────────────────────────────────

describe("containment — enumerated from co-occurring components, both directions, human-resolved", () => {
  it("proposes BOTH directions (parent→child AND child→parent) — no auto-approve; human resolves", () => {
    const { candidates } = enumerateCooccurrence(
      [eParent, eChild],
      [mParent, mChild],
      { families: ["containment"], minCooccur: 1, log: () => {} },
    );
    const forward = candidates.find(
      (c) => c.relationFamily === "containment" && c.sourceId === eParent.entityId && c.targetId === eChild.entityId,
    );
    const reverse = candidates.find(
      (c) => c.relationFamily === "containment" && c.sourceId === eChild.entityId && c.targetId === eParent.entityId,
    );
    // Co-occurrence is symmetric → BOTH directions are proposed. Which one is the
    // real parent→child is a HUMAN decision (no directional auto-approve).
    expect(forward, "parent→child containment candidate").toBeDefined();
    expect(reverse, "child→parent containment candidate").toBeDefined();
    // PREMISED on the co-occurring chunk (resolvable evidence).
    expect(forward!.premiseIds).toContain(CHILD_CHUNK);
    // Both are PROPOSALS only — the enumerator writes no disposition.
    expect(forward!.stage).toBe("typed_cooccurrence");
  });

  it("type gate: component→component passes; self-containment and component→requirement REJECTED", () => {
    const map = buildEntityElementMap([eFn, eParent, eChild, eReq]);
    // Well-typed parent→child passes.
    expect(checkTypeGate("containment", eParent.entityId, eChild.entityId, map).pass).toBe(true);
    // Self-containment rejected.
    const self = checkTypeGate("containment", eParent.entityId, eParent.entityId, map);
    expect(self.pass).toBe(false);
    if (!self.pass) expect(self.reasonCode).toBe("rejected_type:containment.self_containment");
    // component → requirement rejected (target not a component).
    const badTgt = checkTypeGate("containment", eParent.entityId, eReq.entityId, map);
    expect(badTgt.pass).toBe(false);
    if (!badTgt.pass) expect(badTgt.reasonCode).toBe("rejected_type:containment.target_not_component");
    // function → component rejected (source not a containment parent).
    const badSrc = checkTypeGate("containment", eFn.entityId, eChild.entityId, map);
    expect(badSrc.pass).toBe(false);
    if (!badSrc.pass) expect(badSrc.reasonCode).toBe("rejected_type:containment.source_not_component");
  });
});

// ── 3: approve one direction → AcceptedRelation (§B) ────────────────────────────

/** A human approves the parent→child direction (mock). */
function approvedContainment(): InferredApprovedEntry {
  return {
    id: "infer-containment-1",
    relationFamily: "containment",
    sourceId: eParent.entityId,
    targetId: eChild.entityId,
    premises: [CHILD_CHUNK],
    rationale: "audit-only: 'the Position Sensor Array consists of a Sensing Element'",
    confidence: 0.95,
    inferenceRunId: "run-1",
    approvedBy: "tester",
    approvedAt: "2026-07-15T00:00:00.000Z",
    status: "approved",
  };
}

describe("containment — approved inferred entry projects to an AcceptedRelation (§B)", () => {
  it("an APPROVED containment projects to AcceptedRelation(family=containment, status=accepted)", () => {
    const rels = projectApprovedInferredToRelations([approvedContainment()]);
    expect(rels).toHaveLength(1);
    expect(rels[0]!.family).toBe("containment");
    expect(rels[0]!.status).toBe("accepted");
    expect(rels[0]!.sourceId).toBe(eParent.entityId);
    expect(rels[0]!.targetId).toBe(eChild.entityId);
    // Evidence carried through so the composed chain's premises stay resolvable.
    expect(rels[0]!.evidenceChunkIds).toEqual([CHILD_CHUNK]);
    // The relation id is the entry id (content-addressed upstream, deterministic).
    expect(rels[0]!.id).toBe("infer-containment-1");
  });

  it("does NOT project suspect / superseded entries (never composes non-approved)", () => {
    const suspect: InferredApprovedEntry = { ...approvedContainment(), id: "s1", status: "suspect" };
    const superseder: InferredApprovedEntry = { ...approvedContainment(), id: "s3", supersedes: "s2" };
    const superseded: InferredApprovedEntry = { ...approvedContainment(), id: "s2", status: "approved" };
    // suspect excluded; s2 excluded because s3 supersedes it; only s3 survives.
    const rels = projectApprovedInferredToRelations([suspect, superseded, superseder]);
    expect(rels.map((r) => r.id)).toEqual(["s3"]);
  });
});

// ── 4 + 5: the chain could NOT form before; forms after ─────────────────────────

/** A pre-existing accepted allocation fn → parent (corpus-backed, say). */
const acceptedAllocation: AcceptedRelation = {
  id: "rel-alloc-1",
  family: "allocation",
  sourceId: eFn.entityId,
  targetId: eParent.entityId,
  status: "accepted",
  evidenceChunkIds: [CHILD_CHUNK],
};

describe("containment — the CHAIN could not form before; forms after (the whole point)", () => {
  it("BEFORE: allocation with NO containment ⇒ ZERO chains (the chain cannot form)", () => {
    const { candidates } = enumerateChains([acceptedAllocation]);
    expect(candidates).toHaveLength(0); // no second hop to compose through
  });

  it("AFTER: approved containment + allocation ⇒ a NEW allocation chain fn→child forms", () => {
    const [containRel] = projectApprovedInferredToRelations([approvedContainment()]);
    const relations = [acceptedAllocation, containRel!];

    const raw = enumerateChains(relations).candidates;
    // Exactly the one 2-hop path: fn --alloc--> parent ∘ parent --contain--> child.
    const composed = raw.find(
      (c) => c.sourceId === eFn.entityId && c.middleId === eParent.entityId && c.targetId === eChild.entityId,
    );
    expect(composed, "the fn→parent→child 2-hop path").toBeDefined();
    expect(composed!.leftFamily).toBe("allocation");
    expect(composed!.rightFamily).toBe("containment");
    // Premise contract: the two constituent relation ids + their evidence chunks.
    expect(composed!.premiseIds).toEqual([
      "rel-alloc-1", "infer-containment-1", CHILD_CHUNK, CHILD_CHUNK,
    ]);

    // The chain type gate accepts it as a composed `allocation` (allocation ∘
    // containment → allocation) — the composed link fn --allocation--> child.
    const { accepted, rejected } = applyChainTypeGate(raw);
    expect(rejected).toHaveLength(0);
    const chain = accepted.find((c) => c.sourceId === eFn.entityId && c.targetId === eChild.entityId);
    expect(chain, "typed allocation chain fn→child").toBeDefined();
    expect(chain!.relationFamily).toBe("allocation");
  });
});

// ── 5 (routability): the chain routes through the engine pipeline ───────────────

function makeStore(acceptedRelations: AcceptedRelation[]): EntityStoreInput {
  return {
    entities: [eFn, eParent, eChild],
    mentions: [mFn, mParent, mChild],
    acceptedRelations,
    // Restrict co-occurrence to allocation so the ONLY chain-family output is the
    // composed one (keeps the assertion precise).
    families: ["allocation"],
    minCooccur: 1,
  };
}

describe("containment — the composed chain ROUTES through the engine pipeline", () => {
  it("BEFORE (no containment): the engine emits NO chain record", async () => {
    const result = await runInferenceEngine(emptyIR(), new CiteOfferedProvider(), {
      entityStore: makeStore([acceptedAllocation]),
      log: () => {},
    });
    const chainRecords = result.records.filter((r) => r.id.startsWith("chain-"));
    expect(chainRecords).toHaveLength(0);
  });

  it("AFTER (approved containment): the engine QUEUES the composed chain (routable)", async () => {
    const [containRel] = projectApprovedInferredToRelations([approvedContainment()]);
    const result = await runInferenceEngine(emptyIR(), new CiteOfferedProvider(), {
      entityStore: makeStore([acceptedAllocation, containRel!]),
      log: () => {},
    });
    expect(result.droppedUnpremised).toBe(0);
    expect(result.emittedUnpremised).toBe(0);
    const queued = result.records.filter((r): r is QueuedRecord => r.stage === "queued");
    const chain = queued.find((q) => q.id.startsWith("chain-"));
    expect(chain, "queued composed chain record").toBeDefined();
    expect(chain!.relationFamily).toBe("allocation"); // composed result family
    expect(chain!.sourceId).toBe(eFn.entityId);
    expect(chain!.targetId).toBe(eChild.entityId);
    expect(chain!.premises.length).toBeGreaterThan(0); // premises resolved end-to-end
  });
});

// ── Determinism ─────────────────────────────────────────────────────────────────

describe("containment — deterministic enumeration", () => {
  it("two enumeration runs produce byte-identical containment candidates", () => {
    const opts = { families: ["containment"] as RelationFamily[], minCooccur: 1, log: () => {} };
    const a = enumerateCooccurrence([eParent, eChild], [mParent, mChild], opts);
    const b = enumerateCooccurrence([eParent, eChild], [mParent, mChild], opts);
    expect(JSON.stringify(a.candidates)).toEqual(JSON.stringify(b.candidates));
  });
});
