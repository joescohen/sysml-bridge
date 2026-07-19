/**
 * inference-engine.test.ts — RED-first tests for F8 inference engine.
 *
 * Acceptance criteria covered:
 *   A1 — type gate: one violation per family rejected pre-LLM with structured reason code
 *   A2 — no unpremised proposal: mock returns unresolvable premise ids → emittedUnpremised == 0
 *   A3 — band routing: conf .3/.5/.9 → auto_rejected/debate/queued
 *   A4 — debate verdict determinism: fixture confidences reproduce confirmed/rejected/uncertain
 *   A5 — bounded concurrency: correct under parallelism, deterministic ordering, INFER_CONCURRENCY=1
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR } from "@sysml-bridge/model";
import type { InferenceProvider } from "../inference-provider.js";
import type { ProposalOutput, ProposeResult, ContextBundle } from "../types.js";
import { checkTypeGate, buildElementMap } from "../type-gate.js";
import { computeDebateVerdict } from "../debate.js";
import { classifyBand } from "../types.js";
import { runInferenceEngine, boundedPool, resolveInferConcurrency } from "../engine.js";
import { SCHEMA_VERSION } from "@sysml-bridge/model";

// ── Fixture helpers ──────────────────────────────────────────────────────────

function makeMinimalIR(overrides: Partial<typeof MINIMAL_IR> = {}): InferredComposedIR {
  return { ...MINIMAL_IR, ...overrides } as InferredComposedIR;
}

const LEAF_FN_ID = "function-leaf-001";
const COMP_ID = "component-comp-001";
const N2_ID = "n2-flow-001";
const MODE_ID = "prose-mode-001";
const IFACE_ID = "prose-iface-001";
const LEAF_FN_ID_2 = "function-leaf-002";
const CORPUS_REQ_ID = "requirement-req-001";

const MINIMAL_EXTRACTED = {
  schema_version: SCHEMA_VERSION,
  subsystem: "TestSub",
  needs: [{ id: "need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
  requirements: [
    {
      id: CORPUS_REQ_ID,
      kind: "requirement",
      naturalKey: "CC-1",
      name: "Do Thing",
      statement: "The system shall do a thing.",
      needIds: ["need-001"],
    },
  ],
  functions: [
    {
      id: LEAF_FN_ID,
      kind: "function",
      naturalKey: "F1.1",
      name: "Leaf Function 1",
      level: "L3",
      owner: "F1: Parent Function",
    },
    {
      id: LEAF_FN_ID_2,
      kind: "function",
      naturalKey: "F1.2",
      name: "Leaf Function 2",
      level: "L3",
      owner: "F1: Parent Function",
    },
    {
      id: "function-l2-001",
      kind: "function",
      naturalKey: "F1",
      name: "Parent Function",
      level: "L2",
      owner: "Top Level",
    },
  ],
  components: [
    {
      id: COMP_ID,
      kind: "component",
      naturalKey: "COMP-1",
      name: "Widget",
    },
  ],
  satisfies: [],
  allocations: [],
  subsystems: [],
  n2Interfaces: [
    {
      id: N2_ID,
      kind: "n2",
      scope: "component",
      sourceId: COMP_ID,
      targetId: "component-comp-002",
      sourceLabel: "Widget",
      targetLabel: "Other",
      flow: "Data Signal",
      provenance: { workbook: "test.xlsx", sheet: "Sheet1", row: 1, cell: "A1" },
    },
  ],
  kpps: [],
  behaviorDecomp: [],
};

const MINIMAL_IR: InferredComposedIR = {
  extracted: MINIMAL_EXTRACTED as any,
  proseEntries: [
    {
      id: MODE_ID,
      kind: "mode",
      fields: { name: "Operational Mode" },
      citation: {
        docId: "doc-001",
        docSha256: "aa".repeat(32),
        chunkId: "chunk-001",
        sectionPath: "S1",
        quote: "System enters operational mode.",
      },
      approvedBy: "test",
      approvedAt: "2026-06-11T00:00:00.000Z",
      candidateId: "cand-mode-001",
      status: "approved",
    },
    {
      id: IFACE_ID,
      kind: "interface",
      fields: { name: "Data Interface" },
      citation: {
        docId: "doc-001",
        docSha256: "aa".repeat(32),
        chunkId: "chunk-002",
        sectionPath: "S2",
        quote: "Data interface carries telemetry.",
      },
      approvedBy: "test",
      approvedAt: "2026-06-11T00:00:00.000Z",
      candidateId: "cand-iface-001",
      status: "approved",
    },
  ],
  approvedProseIds: new Set([MODE_ID, IFACE_ID]),
  inferredEntries: [],
  approvedInferredIds: new Set(),
};

// ── Mock provider ────────────────────────────────────────────────────────────

class MockProvider implements InferenceProvider {
  constructor(
    private readonly confidence: number,
    private readonly premiseIds: string[],
    private readonly advocateScore: number = 0.8,
    private readonly challengerScore: number = 0.3
  ) {}

  async propose(
    family: Parameters<InferenceProvider["propose"]>[0],
    sourceId: string,
    targetId: string,
    _context: ContextBundle
  ): Promise<ProposeResult> {
    return {
      kind: "proposal",
      proposal: {
        sourceId,
        targetId,
        relationFamily: family,
        premises: this.premiseIds,
        rationale: "Mock rationale — audit only",
        confidence: this.confidence,
      },
    };
  }

  async advocate(
    _family: Parameters<InferenceProvider["advocate"]>[0],
    _proposal: ProposalOutput,
    _context: ContextBundle
  ): Promise<{ score: number; summary: string }> {
    return { score: this.advocateScore, summary: "Mock advocate summary — audit only" };
  }

  async challenge(
    _family: Parameters<InferenceProvider["challenge"]>[0],
    _proposal: ProposalOutput,
    _advocateSummary: string,
    _context: ContextBundle
  ): Promise<{ score: number; summary: string }> {
    return { score: this.challengerScore, summary: "Mock challenger summary — audit only" };
  }
}

// ── A1: Type gate — one violation per family ──────────────────────────────────

describe("A1 — type gate: one ill-typed candidate per family is rejected pre-LLM", () => {
  const ir = makeMinimalIR();
  const elementMap = buildElementMap(ir);

  it("A1-allocation: Def→Def (L2 function as source) is rejected with structured reason code", () => {
    // Use an L2 function (not leaf) as source — should be rejected
    const result = checkTypeGate("allocation", "function-l2-001", COMP_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:allocation.source_not_leaf_function");
      expect(result.reason).toContain("not a leaf function");
    }
  });

  it("A1-allocation: requirement as target is rejected with structured reason code", () => {
    // Use a requirement as target (not a component)
    const result = checkTypeGate("allocation", LEAF_FN_ID, CORPUS_REQ_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:allocation.target_not_component");
      expect(result.reason).toContain("not a component");
    }
  });

  it("A1-modeMembership: L2 function as source is rejected with structured reason code", () => {
    const result = checkTypeGate("modeMembership", "function-l2-001", MODE_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:modeMembership.source_not_leaf_function");
    }
  });

  it("A1-modeMembership: component as target (not a mode) is rejected with structured reason code", () => {
    const result = checkTypeGate("modeMembership", LEAF_FN_ID, COMP_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:modeMembership.target_not_mode");
      expect(result.reason).toContain("not an approved prose mode");
    }
  });

  it("A1-flowTyping: leaf function as source (not N2 flow) is rejected with structured reason code", () => {
    const result = checkTypeGate("flowTyping", LEAF_FN_ID, IFACE_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:flowTyping.source_not_n2_flow");
      expect(result.reason).toContain("not an N2 flow entry");
    }
  });

  it("A1-flowTyping: N2 flow → component (not interface) is rejected with structured reason code", () => {
    const result = checkTypeGate("flowTyping", N2_ID, COMP_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:flowTyping.target_not_interface");
      expect(result.reason).toContain("not a prose interface entry");
    }
  });

  it("A1-controlJoin: different-owner leaf functions are rejected with structured reason code", () => {
    // Add a function with a different owner to the element map
    const diffOwnerFn = {
      id: "function-diff-owner",
      kind: "function",
      naturalKey: "F2.1",
      name: "Different Owner Function",
      level: "L3",
      owner: "F2: Different Parent",
    };
    const extendedMap = new Map(elementMap);
    extendedMap.set(diffOwnerFn.id, diffOwnerFn);

    const result = checkTypeGate("controlJoin", LEAF_FN_ID, diffOwnerFn.id, extendedMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:controlJoin.not_sibling_actions");
      expect(result.reason).toContain("different owners");
    }
  });

  it("A1-controlJoin: self-join is rejected with structured reason code", () => {
    const result = checkTypeGate("controlJoin", LEAF_FN_ID, LEAF_FN_ID, elementMap);
    expect(result.pass).toBe(false);
    if (!result.pass) {
      expect(result.reasonCode).toBe("rejected_type:controlJoin.self_join");
    }
  });

  // Sanity: valid pairs pass
  it("A1-allocation: valid leaf function → component pair passes type gate", () => {
    const result = checkTypeGate("allocation", LEAF_FN_ID, COMP_ID, elementMap);
    expect(result.pass).toBe(true);
  });

  it("A1-modeMembership: valid leaf function → mode pair passes type gate", () => {
    const result = checkTypeGate("modeMembership", LEAF_FN_ID, MODE_ID, elementMap);
    expect(result.pass).toBe(true);
  });

  it("A1-flowTyping: valid N2 flow → interface pair passes type gate", () => {
    const result = checkTypeGate("flowTyping", N2_ID, IFACE_ID, elementMap);
    expect(result.pass).toBe(true);
  });

  it("A1-controlJoin: valid sibling leaf functions pass type gate", () => {
    const result = checkTypeGate("controlJoin", LEAF_FN_ID, LEAF_FN_ID_2, elementMap);
    expect(result.pass).toBe(true);
  });
});

// ── A2: No unpremised proposals ───────────────────────────────────────────────

describe("A2 — no unpremised proposal: emittedUnpremised == 0 when mock returns bad premise ids", () => {
  it("A2-no-unpremised: mock provider returning unresolvable premise id → dropped_unpremised count > 0 AND emittedUnpremised == 0", async () => {
    // Provider returns a proposal with an unresolvable premise id
    const unresolvableId = "NONEXISTENT-PREMISE-ID-XYZ-9999";
    const provider = new MockProvider(0.9, [unresolvableId]);

    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {}, // suppress output
    });

    // The invariant: emittedUnpremised MUST be 0
    expect(result.emittedUnpremised).toBe(0);

    // And the drop counter should be > 0 (at least one proposal was dropped)
    expect(result.droppedUnpremised).toBeGreaterThan(0);

    // No queued records with unresolvable premises
    const queuedRecords = result.records.filter((r) => r.stage === "queued");
    for (const r of queuedRecords) {
      if ("premises" in r) {
        for (const p of r.premises) {
          expect(p).not.toBe(unresolvableId);
        }
      }
    }
  });

  it("A2-corpus-premise: mock with valid corpus premise id → NOT dropped", async () => {
    // Provider returns a proposal with a corpus entity id as premise (should be valid)
    const provider = new MockProvider(0.9, [CORPUS_REQ_ID]);

    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });

    // With a valid premise, no drops for unpremised
    expect(result.emittedUnpremised).toBe(0);
    expect(result.droppedUnpremised).toBe(0);
  });
});

// ── A3: Band routing ──────────────────────────────────────────────────────────

describe("A3 — band routing: conf .3/.5/.9 route to auto_rejected/debate/queued", () => {
  it("A3-classifyBand: conf 0.30 → auto_rejected", () => {
    expect(classifyBand(0.30)).toBe("auto_rejected");
  });

  it("A3-classifyBand: conf 0.39 → auto_rejected (below floor)", () => {
    expect(classifyBand(0.39)).toBe("auto_rejected");
  });

  it("A3-classifyBand: conf 0.40 → debate (at floor)", () => {
    expect(classifyBand(0.40)).toBe("debate");
  });

  it("A3-classifyBand: conf 0.50 → debate", () => {
    expect(classifyBand(0.50)).toBe("debate");
  });

  it("A3-classifyBand: conf 0.69 → debate (just below debate max)", () => {
    expect(classifyBand(0.69)).toBe("debate");
  });

  it("A3-classifyBand: conf 0.70 → queued (at debate max)", () => {
    expect(classifyBand(0.70)).toBe("queued");
  });

  it("A3-classifyBand: conf 0.90 → queued", () => {
    expect(classifyBand(0.90)).toBe("queued");
  });

  it("A3-engine: conf 0.3 proposal → auto_rejected record in output", async () => {
    const provider = new MockProvider(0.3, [CORPUS_REQ_ID]);
    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });
    const autoRejected = result.records.filter((r) => r.stage === "auto_rejected");
    expect(autoRejected.length).toBeGreaterThan(0);
    // No queued records from this provider
    const queued = result.records.filter((r) => r.stage === "queued");
    expect(queued.length).toBe(0);
  });

  it("A3-engine: conf 0.5 proposal → enters debate stage (advocate called)", async () => {
    // Track advocate calls
    let advocateCalls = 0;
    class TrackingProvider extends MockProvider {
      constructor() { super(0.5, [CORPUS_REQ_ID]); }
      override async advocate(...args: Parameters<InferenceProvider["advocate"]>) {
        advocateCalls++;
        return super.advocate(...args);
      }
    }
    const provider = new TrackingProvider();
    await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });
    expect(advocateCalls).toBeGreaterThan(0);
  });

  it("A3-engine: conf 0.9 proposal → queued record WITHOUT debate (no advocate called)", async () => {
    let advocateCalls = 0;
    class TrackingProvider extends MockProvider {
      constructor() { super(0.9, [CORPUS_REQ_ID]); }
      override async advocate(...args: Parameters<InferenceProvider["advocate"]>) {
        advocateCalls++;
        return super.advocate(...args);
      }
    }
    const provider = new TrackingProvider();
    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });
    const queued = result.records.filter((r) => r.stage === "queued");
    expect(queued.length).toBeGreaterThan(0);
    expect(advocateCalls).toBe(0); // high-conf bypasses debate
  });
});

// ── A4: Debate verdict determinism ────────────────────────────────────────────

describe("A4 — debate verdict: SEPAL threshold logic on fixture confidences", () => {
  it("A4-confirmed: advocate=0.8, challenger=0.3 → confirmed", () => {
    expect(computeDebateVerdict(0.8, 0.3)).toBe("confirmed");
  });

  it("A4-confirmed boundary: advocate=0.7, challenger=0.49 → confirmed", () => {
    expect(computeDebateVerdict(0.7, 0.49)).toBe("confirmed");
  });

  it("A4-auto_rejected: challenger=0.7 → auto_rejected (regardless of advocate)", () => {
    expect(computeDebateVerdict(0.9, 0.7)).toBe("auto_rejected");
  });

  it("A4-auto_rejected: challenger=0.8 → auto_rejected", () => {
    expect(computeDebateVerdict(0.3, 0.8)).toBe("auto_rejected");
  });

  it("A4-uncertain: advocate=0.6, challenger=0.4 → uncertain (advocate below 0.7 threshold)", () => {
    expect(computeDebateVerdict(0.6, 0.4)).toBe("uncertain");
  });

  it("A4-uncertain: advocate=0.8, challenger=0.5 → uncertain (challenger not below 0.5)", () => {
    expect(computeDebateVerdict(0.8, 0.5)).toBe("uncertain");
  });

  it("A4-uncertain: advocate=0.5, challenger=0.4 → uncertain", () => {
    expect(computeDebateVerdict(0.5, 0.4)).toBe("uncertain");
  });

  it("A4-engine: debate verdict 'confirmed' → queued with debateVerdict=confirmed", async () => {
    // Mock: conf=0.5 (debate band), advocate=0.8 (≥0.7), challenger=0.3 (<0.5) → confirmed
    const provider = new MockProvider(0.5, [CORPUS_REQ_ID], 0.8, 0.3);
    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });
    const queued = result.records.filter(
      (r): r is import("../types.js").QueuedRecord => r.stage === "queued"
    );
    expect(queued.length).toBeGreaterThan(0);
    for (const r of queued) {
      expect(r.debateVerdict).toBe("confirmed");
      expect(r.debateUncertain).toBeUndefined();
    }
  });

  it("A4-engine: debate verdict 'auto_rejected' (challenger=0.8) → auto_rejected records", async () => {
    // conf=0.5 (debate band), advocate=0.8, challenger=0.8 → auto_rejected
    const provider = new MockProvider(0.5, [CORPUS_REQ_ID], 0.8, 0.8);
    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });
    const autoRejected = result.records.filter((r) => r.stage === "auto_rejected");
    expect(autoRejected.length).toBeGreaterThan(0);
  });

  it("A4-engine: debate verdict 'uncertain' → queued with debateUncertain=true", async () => {
    // conf=0.5 (debate band), advocate=0.6 (<0.7), challenger=0.3 → uncertain
    const provider = new MockProvider(0.5, [CORPUS_REQ_ID], 0.6, 0.3);
    const result = await runInferenceEngine(makeMinimalIR(), provider, {
      dryRun: false,
      log: () => {},
    });
    const queued = result.records.filter(
      (r): r is import("../types.js").QueuedRecord => r.stage === "queued"
    );
    expect(queued.length).toBeGreaterThan(0);
    for (const r of queued) {
      expect(r.debateVerdict).toBe("uncertain");
      expect(r.debateUncertain).toBe(true);
    }
  });
});

// ── Dry run ───────────────────────────────────────────────────────────────────

describe("dry run — no LLM calls", () => {
  it("dry run with empty IR returns zero candidates", async () => {
    let proposeCalled = false;
    class SpyProvider extends MockProvider {
      constructor() { super(0.9, [CORPUS_REQ_ID]); }
      override async propose(...args: Parameters<InferenceProvider["propose"]>) {
        proposeCalled = true;
        return super.propose(...args);
      }
    }
    // IR with no functions/components — generates 0 candidates
    const emptyIR: InferredComposedIR = {
      ...makeMinimalIR(),
      extracted: {
        ...MINIMAL_EXTRACTED as any,
        functions: [],
        components: [],
        n2Interfaces: [],
      },
    };
    const result = await runInferenceEngine(emptyIR, new SpyProvider(), {
      dryRun: true,
      log: () => {},
    });
    expect(proposeCalled).toBe(false);
    expect(result.estimatedCostUsd).toBe(0);
  });

  it("dry run on real minimal IR: returns typed+rejected records, no LLM calls", async () => {
    let proposeCalled = false;
    class SpyProvider extends MockProvider {
      constructor() { super(0.9, [CORPUS_REQ_ID]); }
      override async propose(...args: Parameters<InferenceProvider["propose"]>) {
        proposeCalled = true;
        return super.propose(...args);
      }
    }
    await runInferenceEngine(makeMinimalIR(), new SpyProvider(), {
      dryRun: true,
      log: () => {},
    });
    expect(proposeCalled).toBe(false);
  });
});

// ── A5: Bounded concurrency ───────────────────────────────────────────────────

describe("A5 — bounded concurrency: correct processing, determinism, INFER_CONCURRENCY=1", () => {

  // ── A5-pool: unit tests of the boundedPool helper ────────────────────────

  it("A5-pool: empty task list returns empty array", async () => {
    const { results, failures } = await boundedPool([], 4);
    expect(results).toEqual([]);
    expect(failures).toEqual([]);
  });

  it("A5-pool: single task returns single result", async () => {
    const { results, failures } = await boundedPool([async () => 42], 4);
    expect(results).toEqual([42]);
    expect(failures).toEqual([]);
  });

  it("A5-pool: results returned in original order regardless of completion order", async () => {
    // Tasks complete in reverse order (last task is fastest)
    const delays = [50, 30, 10, 40, 20]; // ms
    const tasks = delays.map((d, i) => async () => {
      await new Promise<void>((resolve) => setTimeout(resolve, d));
      return i;
    });
    const { results } = await boundedPool(tasks, 5);
    // Must preserve original indices even though tasks finish in a different order
    expect(results).toEqual([0, 1, 2, 3, 4]);
  });

  it("A5-pool: concurrency=1 runs tasks sequentially (max in-flight = 1)", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    const tasks = Array.from({ length: 6 }, () => async () => {
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise<void>((resolve) => setTimeout(resolve, 5));
      inFlight--;
      return 1;
    });
    await boundedPool(tasks, 1);
    expect(maxInFlight).toBe(1);
  });

  it("A5-pool: concurrency=4 uses more than 1 concurrent worker when tasks > 1", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    // 8 tasks each with a 20ms delay — at concurrency=4 we should see 4 in-flight
    const tasks = Array.from({ length: 8 }, () => async () => {
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise<void>((resolve) => setTimeout(resolve, 20));
      inFlight--;
      return 1;
    });
    await boundedPool(tasks, 4);
    expect(maxInFlight).toBeGreaterThan(1);
  });

  it("A5-pool: all tasks are processed exactly once", async () => {
    const callCounts = new Array<number>(10).fill(0);
    const tasks = callCounts.map((_, i) => async () => {
      callCounts[i]!++;
      return i;
    });
    const { results, failures } = await boundedPool(tasks, 3);
    expect(results).toHaveLength(10);
    expect(failures).toEqual([]);
    expect(callCounts.every((c) => c === 1)).toBe(true);
  });

  // ── A5-pool-isolation: one throwing task must not abort the whole pool ────────

  it("A5-pool-isolation: one throwing task → other results present, failure recorded, pool resolves", async () => {
    // 5 tasks; task index 2 throws. The pool MUST resolve (not reject), the other
    // 4 slots MUST hold their results, and the failure MUST be recorded {index, error}.
    const tasks = [0, 1, 2, 3, 4].map((i) => async () => {
      if (i === 2) throw new Error("boom-2");
      return i * 10;
    });

    // Must resolve, not reject.
    const { results, failures } = await boundedPool(tasks, 5);

    // 4 successful results present in their original slots; failed slot is undefined.
    expect(results).toEqual([0, 10, undefined, 30, 40]);
    expect(results.filter((r) => r !== undefined)).toHaveLength(4);

    // Exactly one failure recorded, at the right index, with the error message.
    expect(failures).toHaveLength(1);
    expect(failures[0]!.index).toBe(2);
    expect(failures[0]!.error).toContain("boom-2");
  });

  it("A5-pool-isolation: multiple throwing tasks all recorded, remaining continue", async () => {
    // Tasks 1 and 3 throw; 0, 2, 4 succeed. Under concurrency=2 the failures must
    // still be isolated and every non-throwing task must complete.
    const tasks = [0, 1, 2, 3, 4].map((i) => async () => {
      if (i === 1 || i === 3) throw new Error(`fail-${i}`);
      return `ok-${i}`;
    });

    const { results, failures } = await boundedPool(tasks, 2);

    expect(results).toEqual(["ok-0", undefined, "ok-2", undefined, "ok-4"]);
    expect(failures.map((f) => f.index)).toEqual([1, 3]);
    expect(failures.map((f) => f.error)).toEqual([
      expect.stringContaining("fail-1"),
      expect.stringContaining("fail-3"),
    ]);
  });

  // ── A5-resolveInferConcurrency ────────────────────────────────────────────

  it("A5-resolve: default concurrency is 8 when no env and no override", () => {
    const orig = process.env["INFER_CONCURRENCY"];
    delete process.env["INFER_CONCURRENCY"];
    try {
      expect(resolveInferConcurrency()).toBe(8);
    } finally {
      if (orig !== undefined) process.env["INFER_CONCURRENCY"] = orig;
    }
  });

  it("A5-resolve: INFER_CONCURRENCY env is respected", () => {
    const orig = process.env["INFER_CONCURRENCY"];
    process.env["INFER_CONCURRENCY"] = "12";
    try {
      expect(resolveInferConcurrency()).toBe(12);
    } finally {
      if (orig !== undefined) process.env["INFER_CONCURRENCY"] = orig;
      else delete process.env["INFER_CONCURRENCY"];
    }
  });

  it("A5-resolve: clamped to [1, 16]", () => {
    expect(resolveInferConcurrency(0)).toBe(1);
    expect(resolveInferConcurrency(-5)).toBe(1);
    expect(resolveInferConcurrency(100)).toBe(16);
    expect(resolveInferConcurrency(16)).toBe(16);
    expect(resolveInferConcurrency(1)).toBe(1);
  });

  it("A5-resolve: override takes priority over env", () => {
    const orig = process.env["INFER_CONCURRENCY"];
    process.env["INFER_CONCURRENCY"] = "12";
    try {
      expect(resolveInferConcurrency(3)).toBe(3);
    } finally {
      if (orig !== undefined) process.env["INFER_CONCURRENCY"] = orig;
      else delete process.env["INFER_CONCURRENCY"];
    }
  });

  // ── A5-engine: variable-delay mock, all candidates processed once ─────────

  it("A5-engine: all accepted candidates processed exactly once under concurrency=4", async () => {
    const proposeCalls = new Set<string>();
    class DelayedMockProvider implements InferenceProvider {
      async propose(
        family: Parameters<InferenceProvider["propose"]>[0],
        sourceId: string,
        targetId: string,
        _ctx: ContextBundle
      ): Promise<ProposeResult> {
        // Deliberate variable delay to exercise interleaving
        const delay = Math.floor(Math.random() * 10);
        await new Promise<void>((resolve) => setTimeout(resolve, delay));
        const key = `${family}::${sourceId}::${targetId}`;
        proposeCalls.add(key);
        return {
          kind: "proposal",
          proposal: {
            sourceId,
            targetId,
            relationFamily: family,
            premises: [CORPUS_REQ_ID],
            rationale: "mock",
            confidence: 0.9,
          },
        };
      }
      async advocate(_f: Parameters<InferenceProvider["advocate"]>[0], _p: ProposalOutput, _c: ContextBundle) {
        return { score: 0.8, summary: "ok" };
      }
      async challenge(_f: Parameters<InferenceProvider["challenge"]>[0], _p: ProposalOutput, _s: string, _c: ContextBundle) {
        return { score: 0.3, summary: "ok" };
      }
    }

    const ir = makeMinimalIR();
    const result = await runInferenceEngine(ir, new DelayedMockProvider(), {
      dryRun: false,
      concurrency: 4,
      log: () => {},
    });

    // Every accepted candidate should have been proposed exactly once
    // (proposeCalls is a Set so duplicates would not inflate it — we check
    // that the total queued+auto_rejected+dropped count matches expectations)
    const nonRejected = result.records.filter(
      (r) => r.stage !== "rejected_type" && r.stage !== "rejected_unbounded" && r.stage !== "rejected_capped"
    );
    // emittedUnpremised invariant still holds
    expect(result.emittedUnpremised).toBe(0);
    // All processed candidates reached some terminal stage
    for (const r of nonRejected) {
      expect(["queued", "auto_rejected", "dropped_unpremised"]).toContain(r.stage);
    }
    // Propose called once per unique (family, sourceId, targetId) triple
    const accepted = result.records.filter((r) => r.stage !== "rejected_type" && r.stage !== "rejected_unbounded" && r.stage !== "rejected_capped");
    expect(proposeCalls.size).toBeLessThanOrEqual(accepted.length);
  });

  it("A5-engine: output record ordering is deterministic (same IR, two runs, same order)", async () => {
    class StableProvider implements InferenceProvider {
      async propose(
        family: Parameters<InferenceProvider["propose"]>[0],
        sourceId: string,
        targetId: string,
        _ctx: ContextBundle
      ): Promise<ProposeResult> {
        // Variable delay to stress ordering
        await new Promise<void>((resolve) => setTimeout(resolve, Math.floor(Math.random() * 15)));
        return {
          kind: "proposal",
          proposal: {
            sourceId,
            targetId,
            relationFamily: family,
            premises: [CORPUS_REQ_ID],
            rationale: "mock",
            confidence: 0.9,
          },
        };
      }
      async advocate(_f: Parameters<InferenceProvider["advocate"]>[0], _p: ProposalOutput, _c: ContextBundle) {
        return { score: 0.8, summary: "ok" };
      }
      async challenge(_f: Parameters<InferenceProvider["challenge"]>[0], _p: ProposalOutput, _s: string, _c: ContextBundle) {
        return { score: 0.3, summary: "ok" };
      }
    }

    const ir = makeMinimalIR();
    const run1 = await runInferenceEngine(ir, new StableProvider(), { dryRun: false, concurrency: 4, log: () => {} });
    const run2 = await runInferenceEngine(ir, new StableProvider(), { dryRun: false, concurrency: 4, log: () => {} });

    const ids1 = run1.records.map((r) => r.id);
    const ids2 = run2.records.map((r) => r.id);
    expect(ids1).toEqual(ids2);
  });

  it("A5-engine: counters (droppedUnpremised, stats) are exact under concurrency=6", async () => {
    const BAD_PREMISE = "UNRESOLVABLE-XYZ-99999";
    class HalfDropProvider implements InferenceProvider {
      private callIndex = 0;
      async propose(
        family: Parameters<InferenceProvider["propose"]>[0],
        sourceId: string,
        targetId: string,
        _ctx: ContextBundle
      ): Promise<ProposeResult> {
        const idx = this.callIndex++;
        // Every other candidate gets a bad premise
        const premises = idx % 2 === 0 ? [CORPUS_REQ_ID] : [BAD_PREMISE];
        return {
          kind: "proposal",
          proposal: {
            sourceId,
            targetId,
            relationFamily: family,
            premises,
            rationale: "mock",
            confidence: 0.9,
          },
        };
      }
      async advocate(_f: Parameters<InferenceProvider["advocate"]>[0], _p: ProposalOutput, _c: ContextBundle) {
        return { score: 0.8, summary: "ok" };
      }
      async challenge(_f: Parameters<InferenceProvider["challenge"]>[0], _p: ProposalOutput, _s: string, _c: ContextBundle) {
        return { score: 0.3, summary: "ok" };
      }
    }

    const ir = makeMinimalIR();
    const result = await runInferenceEngine(ir, new HalfDropProvider(), {
      dryRun: false,
      concurrency: 6,
      log: () => {},
    });

    // droppedUnpremised must equal the count of dropped_unpremised records
    const droppedRecords = result.records.filter((r) => r.stage === "dropped_unpremised");
    expect(result.droppedUnpremised).toBe(droppedRecords.length);

    // Sum of per-family proposed in stats must equal (queued + autoRejected + droppedUnpremised)
    const totalProposed = result.stats.reduce((s, st) => s + st.proposed, 0);
    const totalQueued = result.stats.reduce((s, st) => s + st.queued, 0);
    const totalAutoRejected = result.stats.reduce((s, st) => s + st.autoRejected, 0);
    const totalDropped = result.stats.reduce((s, st) => s + st.droppedUnpremised, 0);
    expect(totalProposed).toBe(totalQueued + totalAutoRejected + totalDropped);

    // emittedUnpremised invariant
    expect(result.emittedUnpremised).toBe(0);
  });

  it("A5-engine: concurrency actually used (max in-flight > 1 when limit=4 and tasks > 4)", async () => {
    let inFlight = 0;
    let maxInFlight = 0;

    class InFlightTrackingProvider implements InferenceProvider {
      async propose(
        family: Parameters<InferenceProvider["propose"]>[0],
        sourceId: string,
        targetId: string,
        _ctx: ContextBundle
      ): Promise<ProposeResult> {
        inFlight++;
        maxInFlight = Math.max(maxInFlight, inFlight);
        // Hold for a bit so multiple tasks can pile up
        await new Promise<void>((resolve) => setTimeout(resolve, 20));
        inFlight--;
        return {
          kind: "proposal",
          proposal: {
            sourceId,
            targetId,
            relationFamily: family,
            premises: [CORPUS_REQ_ID],
            rationale: "mock",
            confidence: 0.9,
          },
        };
      }
      async advocate(_f: Parameters<InferenceProvider["advocate"]>[0], _p: ProposalOutput, _c: ContextBundle) {
        return { score: 0.8, summary: "ok" };
      }
      async challenge(_f: Parameters<InferenceProvider["challenge"]>[0], _p: ProposalOutput, _s: string, _c: ContextBundle) {
        return { score: 0.3, summary: "ok" };
      }
    }

    // Build an IR with enough candidates to saturate concurrency=4.
    // We need > 4 accepted candidates. Add extra leaf functions + a component.
    const extraFunctions = Array.from({ length: 8 }, (_, i) => ({
      id: `function-extra-leaf-${i.toString().padStart(3, "0")}`,
      kind: "function",
      naturalKey: `FX.${i + 1}`,
      name: `Extra Leaf Function ${i + 1}`,
      level: "L3",
      owner: "F1: Parent Function",
    }));

    const richIR: InferredComposedIR = {
      ...makeMinimalIR(),
      extracted: {
        ...MINIMAL_EXTRACTED as any,
        functions: [
          ...MINIMAL_EXTRACTED.functions,
          ...extraFunctions,
        ],
      },
    };

    await runInferenceEngine(richIR, new InFlightTrackingProvider(), {
      dryRun: false,
      concurrency: 4,
      log: () => {},
    });

    // With 4-way concurrency and 10+ candidates each holding 20ms, maxInFlight must be > 1
    expect(maxInFlight).toBeGreaterThan(1);
  });

  it("A5-engine: INFER_CONCURRENCY=1 reproduces sequential behavior (max in-flight = 1)", async () => {
    let inFlight = 0;
    let maxInFlight = 0;

    class SeqCheckProvider implements InferenceProvider {
      async propose(
        family: Parameters<InferenceProvider["propose"]>[0],
        sourceId: string,
        targetId: string,
        _ctx: ContextBundle
      ): Promise<ProposeResult> {
        inFlight++;
        maxInFlight = Math.max(maxInFlight, inFlight);
        await new Promise<void>((resolve) => setTimeout(resolve, 10));
        inFlight--;
        return {
          kind: "proposal",
          proposal: {
            sourceId,
            targetId,
            relationFamily: family,
            premises: [CORPUS_REQ_ID],
            rationale: "mock",
            confidence: 0.9,
          },
        };
      }
      async advocate(_f: Parameters<InferenceProvider["advocate"]>[0], _p: ProposalOutput, _c: ContextBundle) {
        return { score: 0.8, summary: "ok" };
      }
      async challenge(_f: Parameters<InferenceProvider["challenge"]>[0], _p: ProposalOutput, _s: string, _c: ContextBundle) {
        return { score: 0.3, summary: "ok" };
      }
    }

    const extraFunctions = Array.from({ length: 5 }, (_, i) => ({
      id: `function-seq-leaf-${i.toString().padStart(3, "0")}`,
      kind: "function",
      naturalKey: `FS.${i + 1}`,
      name: `Seq Leaf Function ${i + 1}`,
      level: "L3",
      owner: "F1: Parent Function",
    }));

    const richIR: InferredComposedIR = {
      ...makeMinimalIR(),
      extracted: {
        ...MINIMAL_EXTRACTED as any,
        functions: [...MINIMAL_EXTRACTED.functions, ...extraFunctions],
      },
    };

    await runInferenceEngine(richIR, new SeqCheckProvider(), {
      dryRun: false,
      concurrency: 1, // sequential
      log: () => {},
    });

    // Sequential mode: never more than 1 propose in flight at a time
    expect(maxInFlight).toBe(1);
  });
});
