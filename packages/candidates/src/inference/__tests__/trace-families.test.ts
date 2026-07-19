/**
 * trace-families.test.ts — W-relations proposer: satisfy + derive as inference
 * RelationFamily values, wired end-to-end.
 *
 * Proves (brief §Deliver + §Report):
 *   - A SATISFY candidate is ENUMERATED (cross-doc co-occurrence), TYPE-GATED
 *     (design element → requirement), PREMISED (co-occurring chunk ids), and
 *     (with a mock provider) QUEUED through the engine's shared pipeline.
 *   - An APPROVED satisfy candidate PROJECTS to a SysmlRelationship with type
 *     `SatisfyRequirementUsage` and SERIALIZES to `satisfy <req> by <element>;`
 *     (a USAGE-correct trace statement — R4).
 *   - Derive lands the same way (requirement → need → DeriveRequirementUsage →
 *     `dependency from <req> to <need>;`).
 *   - Type-gate negative controls: an ill-typed satisfy/derive pair is rejected.
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR, InferredApprovedEntry } from "@sysml-bridge/model";
import { SCHEMA_VERSION, projectInferredTraceRelationships } from "@sysml-bridge/model";
import { runInferenceEngine, type EntityStoreInput } from "../engine.js";
import { enumerateCooccurrence } from "../cooccurrence.js";
import { checkTypeGate, buildEntityElementMap } from "../type-gate.js";
import type { InferenceProvider } from "../inference-provider.js";
import type { ContextBundle, ProposeResult, QueuedRecord, RelationFamily } from "../types.js";
import type { EntityRecord } from "../../entities/cluster.js";
import type { MentionRecord } from "../../mentions/index.js";

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

// Component + requirement co-occur in chunk-A; requirement + need co-occur in chunk-B.
const eComp = entity("component", "Fuel Control Module", ["m-comp"]);
const eReq = entity("requirement", "Fuel Command Requirement", ["m-req-a", "m-req-b"]);
const eNeed = entity("need", "Autonomous Refuel Need", ["m-need"]);
const mComp = mention("m-comp", "component", "Fuel Control Module", "chunk-A");
const mReqA = mention("m-req-a", "requirement", "Fuel Command Requirement", "chunk-A");
const mReqB = mention("m-req-b", "requirement", "Fuel Command Requirement", "chunk-B");
const mNeed = mention("m-need", "need", "Autonomous Refuel Need", "chunk-B");

function makeEntityStore(families: RelationFamily[]): EntityStoreInput {
  return {
    entities: [eComp, eReq, eNeed],
    mentions: [mComp, mReqA, mReqB, mNeed],
    families,
    minCooccur: 1,
  };
}

describe("trace families — enumeration + type gate + premise", () => {
  it("enumerates a SATISFY candidate (component → requirement) with co-occurring chunk premises", () => {
    const { candidates } = enumerateCooccurrence(
      makeEntityStore(["satisfy"]).entities,
      makeEntityStore(["satisfy"]).mentions,
      { families: ["satisfy"], minCooccur: 1, log: () => {} },
    );
    const sat = candidates.find(
      (c) => c.relationFamily === "satisfy" && c.sourceId === eComp.entityId && c.targetId === eReq.entityId,
    );
    expect(sat, "satisfy candidate component→requirement").toBeDefined();
    // PREMISED: the co-occurring chunk id is a resolvable premise.
    expect(sat!.premiseIds).toContain("chunk-A");
    expect(sat!.premiseIds.length).toBeGreaterThan(0);
    // Direction is type-correct: NO reverse (requirement→component) satisfy candidate.
    expect(
      candidates.some((c) => c.relationFamily === "satisfy" && c.sourceId === eReq.entityId),
    ).toBe(false);
  });

  it("enumerates a DERIVE candidate (requirement → need) with premises", () => {
    const store = makeEntityStore(["derive"]);
    const { candidates } = enumerateCooccurrence(store.entities, store.mentions, {
      families: ["derive"], minCooccur: 1, log: () => {},
    });
    const der = candidates.find(
      (c) => c.relationFamily === "derive" && c.sourceId === eReq.entityId && c.targetId === eNeed.entityId,
    );
    expect(der, "derive candidate requirement→need").toBeDefined();
    expect(der!.premiseIds).toContain("chunk-B");
  });

  it("type gate REJECTS an ill-typed satisfy (requirement → component) and derive (need → requirement)", () => {
    const map = buildEntityElementMap([eComp, eReq, eNeed]);
    // satisfy requires design-element source; a requirement source is rejected.
    const badSat = checkTypeGate("satisfy", eReq.entityId, eComp.entityId, map);
    expect(badSat.pass).toBe(false);
    // derive requires requirement source + need target; need→requirement is rejected.
    const badDer = checkTypeGate("derive", eNeed.entityId, eReq.entityId, map);
    expect(badDer.pass).toBe(false);
    // The well-typed forms pass.
    expect(checkTypeGate("satisfy", eComp.entityId, eReq.entityId, map).pass).toBe(true);
    expect(checkTypeGate("derive", eReq.entityId, eNeed.entityId, map).pass).toBe(true);
  });
});

describe("trace families — queued through the engine, approved, serialized", () => {
  it("a satisfy candidate is QUEUED (mock provider), premises resolve", async () => {
    const result = await runInferenceEngine(emptyIR(), new CiteOfferedProvider(), {
      entityStore: makeEntityStore(["satisfy"]),
      log: () => {},
    });
    expect(result.droppedUnpremised).toBe(0);
    const queued = result.records.filter((r): r is QueuedRecord => r.stage === "queued");
    const sat = queued.find((q) => q.relationFamily === "satisfy");
    expect(sat, "queued satisfy record").toBeDefined();
    expect(sat!.sourceId).toBe(eComp.entityId);
    expect(sat!.targetId).toBe(eReq.entityId);
    expect(sat!.premises.length).toBeGreaterThan(0);
  });

  it("an APPROVED satisfy entry projects to a SatisfyRequirementUsage relationship (src=element, tgt=req)", () => {
    // NOTE: the serialize()→`satisfy <req> by <element>;` string assertion lives in
    // packages/sysml (trace-projection-serialize.test.ts) — candidates does not
    // depend on @sysml-bridge/sysml. Here we prove the crux mapping's output.
    const approved: InferredApprovedEntry = {
      id: "infer-satisfy-1", relationFamily: "satisfy",
      sourceId: "part-fcm", targetId: "req-001",
      premises: ["chunk-A"], rationale: "audit-only", confidence: 0.95,
      inferenceRunId: "run-1", approvedBy: "tester", approvedAt: "2026-07-15T00:00:00.000Z",
      status: "approved",
    };
    const rels = projectInferredTraceRelationships([approved]);
    expect(rels).toHaveLength(1);
    expect(rels[0]!.type).toBe("SatisfyRequirementUsage");
    expect(rels[0]!.sourceIds).toEqual(["part-fcm"]);
    expect(rels[0]!.targetIds).toEqual(["req-001"]);
  });

  it("an APPROVED derive entry projects to a DeriveRequirementUsage relationship (src=req, tgt=need)", () => {
    const approved: InferredApprovedEntry = {
      id: "infer-derive-1", relationFamily: "derive",
      sourceId: "req-001", targetId: "need-001",
      premises: ["chunk-B"], rationale: "audit-only", confidence: 0.9,
      inferenceRunId: "run-1", approvedBy: "tester", approvedAt: "2026-07-15T00:00:00.000Z",
      status: "approved",
    };
    const rels = projectInferredTraceRelationships([approved]);
    expect(rels[0]!.type).toBe("DeriveRequirementUsage");
    expect(rels[0]!.sourceIds).toEqual(["req-001"]);
    expect(rels[0]!.targetIds).toEqual(["need-001"]);
  });

  it("non-trace families and non-approved entries do NOT project", () => {
    const modeEntry: InferredApprovedEntry = {
      id: "infer-mode-1", relationFamily: "modeMembership", sourceId: "a", targetId: "b",
      premises: ["p"], rationale: "x", confidence: 0.9, inferenceRunId: "r", approvedBy: "t",
      approvedAt: "2026-07-15T00:00:00.000Z", status: "approved",
    };
    const suspectSatisfy: InferredApprovedEntry = {
      id: "infer-satisfy-suspect", relationFamily: "satisfy", sourceId: "a", targetId: "b",
      premises: ["p"], rationale: "x", confidence: 0.9, inferenceRunId: "r", approvedBy: "t",
      approvedAt: "2026-07-15T00:00:00.000Z", status: "suspect",
    };
    expect(projectInferredTraceRelationships([modeEntry, suspectSatisfy])).toHaveLength(0);
  });
});
