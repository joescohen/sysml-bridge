/**
 * crossdoc-engine.test.ts — W2 cross-document candidates end-to-end through the
 * engine's SHARED pipeline (type gate → premise contract → debate → queue).
 *
 * Done-criteria covered (spec §8 W2):
 *   - Premises of BOTH candidate kinds RESOLVE end-to-end: a mock provider that
 *     cites the offered premise ids yields QUEUED records (NOT dropped_unpremised).
 *   - A candidate citing a FABRICATED premise id IS dropped (control).
 *   - ABSENT entityStore → EXACTLY today's behavior: no cross-doc records, no
 *     cross-doc log line (the structural flow is untouched).
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR } from "@sysml-bridge/model";
import { SCHEMA_VERSION } from "@sysml-bridge/model";
import { runInferenceEngine, type EntityStoreInput } from "../engine.js";
import type { InferenceProvider } from "../inference-provider.js";
import type { ContextBundle, ProposeResult, QueuedRecord, RelationFamily } from "../types.js";
import type { EntityRecord } from "../../entities/cluster.js";
import type { MentionRecord } from "../../mentions/index.js";
import type { AcceptedRelation } from "../chains.js";

// ── An EMPTY IR: no structural candidates, so ONLY cross-doc candidates flow ──

function emptyIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "Empty",
    needs: [],
    requirements: [],
    functions: [],
    components: [],
    satisfies: [],
    allocations: [],
    subsystems: [],
    n2Interfaces: [],
    kpps: [],
    behaviorDecomp: [],
  };
  return {
    extracted: extracted as any,
    proseEntries: [],
    approvedProseIds: new Set<string>(),
    inferredEntries: [],
    approvedInferredIds: new Set<string>(),
  };
}

function entity(kind: EntityRecord["kind"], name: string, mentionIds: string[]): EntityRecord {
  return {
    entityId: `entity-${kind}-${name.replace(/\s+/g, "_")}`,
    kind,
    canonicalName: name,
    aliases: [name],
    mentionIds,
    mergeDispositions: [],
  };
}

function mention(
  mentionId: string,
  kind: MentionRecord["kindHint"],
  name: string,
  chunkId: string,
): MentionRecord {
  return {
    mentionId,
    surfaceForm: name,
    kindHint: kind,
    citation: { docId: "doc", docSha256: "aa".repeat(32), chunkId, sectionPath: "S1", quote: name },
    confidence: 0.9,
  };
}

// ── Providers ─────────────────────────────────────────────────────────────────

/** Cites every offered fact id as a premise → all premises resolve → queued. */
class CiteOfferedProvider implements InferenceProvider {
  async propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle,
  ): Promise<ProposeResult> {
    return {
      kind: "proposal",
      proposal: {
        sourceId,
        targetId,
        relationFamily: family,
        premises: context.offeredFacts.map((f) => f.id), // cite the offered ids
        rationale: "audit-only",
        confidence: 0.95, // ≥ 0.70 → queued directly (no debate)
      },
    };
  }
  async advocate() {
    return { score: 0.9, summary: "n/a" };
  }
  async challenge() {
    return { score: 0.1, summary: "n/a" };
  }
}

/** Cites a fabricated premise id that is in NO resolvable set → dropped. */
class FabricatedPremiseProvider implements InferenceProvider {
  async propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
  ): Promise<ProposeResult> {
    return {
      kind: "proposal",
      proposal: {
        sourceId,
        targetId,
        relationFamily: family,
        premises: ["FABRICATED-not-a-real-id-000000"],
        rationale: "audit-only",
        confidence: 0.95,
      },
    };
  }
  async advocate() {
    return { score: 0.9, summary: "n/a" };
  }
  async challenge() {
    return { score: 0.1, summary: "n/a" };
  }
}

// ── Fixtures: a co-occurrence spoke + a chain, sharing the same entities ───────

function makeEntityStore(): EntityStoreInput {
  const mFn = mention("m-fn", "function", "Fuel Pump Ctrl", "chunk-A");
  const mComp = mention("m-comp", "component", "Fuel Pump", "chunk-A"); // co-occur in chunk-A
  const eFn = entity("function", "Fuel Pump Ctrl", ["m-fn"]);
  const eComp = entity("component", "Fuel Pump", ["m-comp"]);
  const eSub = entity("component", "Pump Housing", ["m-comp"]);

  // A chain: eFn --allocation--> eComp ∘ eComp --containment--> eSub ⇒ allocation.
  const acceptedRelations: AcceptedRelation[] = [
    { id: "rel-alloc-1", family: "allocation", sourceId: eFn.entityId, targetId: eComp.entityId, status: "accepted", evidenceChunkIds: ["chunk-A"] },
    { id: "rel-contain-1", family: "containment", sourceId: eComp.entityId, targetId: eSub.entityId, status: "accepted", evidenceChunkIds: ["chunk-A"] },
  ];

  return {
    entities: [eFn, eComp, eSub],
    mentions: [mFn, mComp],
    acceptedRelations,
    families: ["allocation"],
    minCooccur: 1,
  };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("crossdoc engine — premises resolve end-to-end", () => {
  it("co-occurrence + chain candidates whose premises are cited are QUEUED (not dropped)", async () => {
    const result = await runInferenceEngine(emptyIR(), new CiteOfferedProvider(), {
      entityStore: makeEntityStore(),
      log: () => {},
    });

    expect(result.droppedUnpremised).toBe(0);
    expect(result.emittedUnpremised).toBe(0);

    const queued = result.records.filter((r): r is QueuedRecord => r.stage === "queued");
    // At least the co-occurrence spoke AND the composed chain.
    expect(queued.length).toBeGreaterThanOrEqual(2);
    const ids = queued.map((q) => q.id);
    expect(ids.some((id) => id.startsWith("cooccur-"))).toBe(true);
    expect(ids.some((id) => id.startsWith("chain-"))).toBe(true);
  });

  it("a candidate citing a FABRICATED premise id IS dropped (control)", async () => {
    const result = await runInferenceEngine(emptyIR(), new FabricatedPremiseProvider(), {
      entityStore: makeEntityStore(),
      log: () => {},
    });

    expect(result.droppedUnpremised).toBeGreaterThan(0);
    expect(result.emittedUnpremised).toBe(0);
    const queued = result.records.filter((r) => r.stage === "queued");
    expect(queued.length).toBe(0);
  });
});

describe("crossdoc engine — ABSENT entityStore → exactly today's behavior", () => {
  it("no entityStore ⇒ zero records over an empty IR and NO cross-doc log line", async () => {
    const logs: string[] = [];
    const result = await runInferenceEngine(emptyIR(), new CiteOfferedProvider(), {
      log: (m) => logs.push(m),
    });
    expect(result.records).toHaveLength(0);
    expect(logs.some((l) => l.includes("[inference:crossdoc]"))).toBe(false);
  });

  it("entityStore present ⇒ a cross-doc log line IS emitted (gating proof)", async () => {
    const logs: string[] = [];
    await runInferenceEngine(emptyIR(), new CiteOfferedProvider(), {
      entityStore: makeEntityStore(),
      log: (m) => logs.push(m),
    });
    expect(logs.some((l) => l.includes("[inference:crossdoc]"))).toBe(true);
  });
});
