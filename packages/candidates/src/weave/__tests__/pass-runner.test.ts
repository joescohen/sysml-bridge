/**
 * pass-runner.test.ts — the core W3 done-criterion (spec §8 W3):
 *
 *   "A pass on a fixture with a known gap proposes ≥1 candidate targeting that
 *    gap's element id (mock provider)."
 *
 * A gap requirement's name lexically retrieves two co-occurring entities; the
 * cross-document co-occurrence spoke between them is queued by a mock provider
 * and attributed back to the gap element id. Also proves determinism (twice-run
 * identical) and that a candidate touching NO retrieved entity is not attributed.
 */
import { describe, it, expect } from "vitest";
import type { InferredComposedIR } from "@sysml-bridge/model";
import { SCHEMA_VERSION } from "@sysml-bridge/model";

import { runTargetedInference, scopeEntitiesToQuery } from "../pass-runner.js";
import type { WeaveQuery } from "../gap-queue.js";
import type { InferenceProvider } from "../../inference/inference-provider.js";
import type { ContextBundle, ProposeResult, RelationFamily } from "../../inference/types.js";
import type { EntityRecord } from "../../entities/cluster.js";
import type { MentionRecord } from "../../mentions/index.js";

function emptyIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "T",
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

function entity(entityId: string, kind: EntityRecord["kind"], name: string, mentionIds: string[]): EntityRecord {
  return { entityId, kind, canonicalName: name, aliases: [name], mentionIds, mergeDispositions: [] };
}

function mention(mentionId: string, kind: MentionRecord["kindHint"], name: string, chunkId: string): MentionRecord {
  return {
    mentionId,
    surfaceForm: name,
    kindHint: kind,
    citation: { docId: "d", docSha256: "aa".repeat(32), chunkId, sectionPath: "S1", quote: name },
    confidence: 0.9,
  };
}

/** Mock: cites every offered fact id → premises resolve → queued (0.95, no debate). */
class MockCiteProvider implements InferenceProvider {
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
        premises: context.offeredFacts.map((f) => f.id),
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

// The gap: a requirement "Fuel Pump Control" with no satisfy edge (GATE02-unsatisfied).
const GAP_QUERY: WeaveQuery = {
  findingRuleId: "GATE02-unsatisfied",
  gapElementId: "req-fpc",
  gapElementName: "Fuel Pump Control",
  family: "satisfy",
  bm25Query: "Fuel Pump Control The system shall control the fuel pump.",
};

// Two entities that co-occur in chunk-A and both lexically match the gap query.
const ENTITIES = [
  entity("entity-fn-fpc", "function", "Fuel Pump Control", ["m-fpc"]),
  entity("entity-comp-fp", "component", "Fuel Pump", ["m-fp"]),
];
const MENTIONS = [
  mention("m-fpc", "function", "Fuel Pump Control", "chunk-A"),
  mention("m-fp", "component", "Fuel Pump", "chunk-A"),
];

describe("scopeEntitiesToQuery", () => {
  it("retrieves entities sharing a significant token with the query", () => {
    const scoped = scopeEntitiesToQuery(GAP_QUERY, ENTITIES);
    expect(scoped).toEqual(["entity-fn-fpc", "entity-comp-fp"]);
  });

  it("retrieves nothing for an unrelated query", () => {
    const q: WeaveQuery = { ...GAP_QUERY, bm25Query: "Avionics Display Subsystem" };
    expect(scopeEntitiesToQuery(q, ENTITIES)).toEqual([]);
  });
});

describe("runTargetedInference — proposes ≥1 candidate targeting the gap element id", () => {
  it("attributes the co-occurrence spoke to the gap requirement", async () => {
    const result = await runTargetedInference({
      ir: emptyIR(),
      provider: new MockCiteProvider(),
      queries: [GAP_QUERY],
      entities: ENTITIES,
      mentions: MENTIONS,
      log: () => {},
    });

    expect(result.proposedCandidates.length).toBeGreaterThanOrEqual(1);
    const hit = result.proposedCandidates.find((c) => c.targetsGapElementIds.includes("req-fpc"));
    expect(hit).toBeDefined();
    expect(hit!.id.startsWith("cooccur-")).toBe(true);
    expect(result.scopeByGap["req-fpc"]).toEqual(["entity-fn-fpc", "entity-comp-fp"]);
  });

  it("is deterministic: twice-run identical proposals", async () => {
    const run = () =>
      runTargetedInference({
        ir: emptyIR(),
        provider: new MockCiteProvider(),
        queries: [GAP_QUERY],
        entities: ENTITIES,
        mentions: MENTIONS,
        log: () => {},
      });
    const a = await run();
    const b = await run();
    expect(JSON.stringify(a.proposedCandidates)).toBe(JSON.stringify(b.proposedCandidates));
  });

  it("does NOT attribute candidates when no query retrieves their endpoints", async () => {
    const result = await runTargetedInference({
      ir: emptyIR(),
      provider: new MockCiteProvider(),
      queries: [{ ...GAP_QUERY, bm25Query: "Totally Unrelated Concept" }],
      entities: ENTITIES,
      mentions: MENTIONS,
      log: () => {},
    });
    expect(result.proposedCandidates).toEqual([]);
  });
});
