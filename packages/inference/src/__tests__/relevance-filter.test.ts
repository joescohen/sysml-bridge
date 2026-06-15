/**
 * relevance-filter.test.ts — RED-first tests for the relevance filter stage
 * (conductor finding: allocation generation must be bounded, not all-pairs).
 *
 * Covers:
 *   RF-a — all-pairs fixture → filter shrinks it; rejections carry structured
 *          reason codes (rejected_unbounded:<signal-miss>)
 *   RF-b — a pair with name-token overlap (signal a) survives
 *   RF-c — a pair with NO corpus signal is rejected_unbounded
 *   RF-d — per-family cap (INFER_FAMILY_CAP, default 150): post-filter count
 *          above the cap keeps highest-signal first, excess → rejected_capped
 *   RF-e — controlJoin: pair already stated by an approved prose succession is
 *          rejected; unstated sibling pair survives
 *   RF-f — flow co-occurrence (signal b) survives: L2 parent functional-N2 flow
 *          exactly matches a component-scope N2 flow touching the component
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR } from "@sysml-bridge/ir";
import { SCHEMA_VERSION } from "@sysml-bridge/ir";
import { applyRelevanceFilter, scoreAllocationSignals } from "../relevance-filter.js";
import { generateCandidates } from "../candidate-generator.js";
import { runInferenceEngine } from "../engine.js";
import type { InferenceProvider } from "../inference-provider.js";
import type { ProposeResult } from "../types.js";

// ── Fixture: 2 leaf functions × 3 components, deliberately partial signals ────
//
// fn-fuel  "Validate Fuel Capacity"   (owner "F1: Manage Requests")
// fn-recv  "Receive Request"          (owner "F1: Manage Requests")
// comp-fuel "Fuel Pump"               — name-token "fuel" overlaps fn-fuel (signal a)
// comp-flow "Data Bus Controller"     — component-scope N2 flow "Telemetry" matches
//                                       functional-N2 flow of F1 (signal b)
// comp-none "Hydraulic Actuator"      — NO signal at all

const FN_FUEL = "function-fuel-0001";
const FN_RECV = "function-recv-0002";
const COMP_FUEL = "component-fuel-01";
const COMP_FLOW = "component-flow-02";
const COMP_NONE = "component-none-03";

function makeFixtureIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "TestSub",
    needs: [],
    requirements: [],
    functions: [
      {
        id: FN_FUEL,
        kind: "function",
        naturalKey: "F1.1",
        name: "Validate Fuel Capacity",
        level: "L3",
        owner: "F1: Manage Requests",
      },
      {
        id: FN_RECV,
        kind: "function",
        naturalKey: "F1.2",
        name: "Receive Request",
        level: "L3",
        owner: "F1: Manage Requests",
      },
      {
        id: "function-l2-01",
        kind: "function",
        naturalKey: "F1",
        name: "Manage Requests",
        level: "L2",
        owner: "Top",
      },
    ],
    components: [
      { id: COMP_FUEL, kind: "component", naturalKey: "Fuel Pump", name: "Fuel Pump" },
      { id: COMP_FLOW, kind: "component", naturalKey: "Data Bus Controller", name: "Data Bus Controller" },
      { id: COMP_NONE, kind: "component", naturalKey: "Hydraulic Actuator", name: "Hydraulic Actuator" },
    ],
    satisfies: [],
    allocations: [],
    subsystems: [
      {
        id: "subsystem-01",
        kind: "subsystem",
        naturalKey: "Test Subsystem",
        name: "Test Subsystem",
        componentIds: [COMP_FUEL, COMP_FLOW, COMP_NONE],
        provenance: { workbook: "t.xlsx", sheet: "S" },
      },
    ],
    n2Interfaces: [
      // functional-N2: F1 emits "Telemetry"
      {
        id: "n2-functional-01",
        kind: "n2",
        scope: "functional",
        sourceId: "function-l2-01",
        targetId: "function-l2-01",
        sourceLabel: "F1",
        targetLabel: "F2",
        flow: "Telemetry",
        provenance: { workbook: "t.xlsx", sheet: "S", row: 1, cell: "A1" },
      },
      // component-scope N2: COMP_FLOW carries "Telemetry" → signal b bridge to F1
      {
        id: "n2-component-01",
        kind: "n2",
        scope: "component",
        sourceId: COMP_FLOW,
        targetId: COMP_NONE_NEVER_MATCHES_PLACEHOLDER(),
        sourceLabel: "Data Bus Controller",
        targetLabel: "Elsewhere",
        flow: "Telemetry",
        provenance: { workbook: "t.xlsx", sheet: "S", row: 2, cell: "A2" },
      },
    ],
    kpps: [],
    behaviorDecomp: [],
  };

  return {
    extracted: extracted as any,
    proseEntries: [],
    approvedProseIds: new Set(),
    inferredEntries: [],
    approvedInferredIds: new Set(),
  };
}

// COMP_NONE must NOT appear as an N2 endpoint with a flow matching F1's functional
// flows — use a component id that exists but has no matching flow. We point the
// second endpoint at a non-existent id so only COMP_FLOW gets the "Telemetry" flow.
function COMP_NONE_NEVER_MATCHES_PLACEHOLDER(): string {
  return "component-external-x";
}

// ── Mock provider (never proposes — these tests are pre-LLM) ─────────────────

class NeverProvider implements InferenceProvider {
  async propose(): Promise<ProposeResult> {
    return { kind: "declined" };
  }
  async advocate(): Promise<{ score: number; summary: string }> {
    return { score: 0.5, summary: "n/a" };
  }
  async challenge(): Promise<{ score: number; summary: string }> {
    return { score: 0.5, summary: "n/a" };
  }
}

// ── RF-a: all-pairs fixture shrinks + reasons coded ───────────────────────────

describe("RF-a — relevance filter shrinks all-pairs allocation generation", () => {
  it("filter output is strictly smaller than the all-pairs product and rejections are reason-coded", () => {
    const ir = makeFixtureIR();
    const { candidates } = generateCandidates(ir);
    const allocationCandidates = candidates.filter((c) => c.family === "allocation");
    // all-pairs would be 2 leaf fns × 3 comps = 6
    expect(allocationCandidates.length).toBe(6);

    const { kept, rejected } = applyRelevanceFilter(allocationCandidates, ir);

    // The filter must shrink the set (comp-none has no signal for either function)
    expect(kept.length).toBeLessThan(allocationCandidates.length);
    expect(rejected.length).toBeGreaterThan(0);

    // Every rejection carries a structured rejected_unbounded reason code
    for (const r of rejected) {
      expect(r.stage).toBe("rejected_unbounded");
      expect(r.reasonCode).toMatch(/^rejected_unbounded:/);
      expect(r.reason.length).toBeGreaterThan(0);
    }
  });
});

// ── RF-b: name-token overlap survives ─────────────────────────────────────────

describe("RF-b — signal a: name-token overlap survives the filter", () => {
  it("'Validate Fuel Capacity' → 'Fuel Pump' (shared token 'fuel') is kept", () => {
    const ir = makeFixtureIR();
    const { candidates } = generateCandidates(ir);
    const allocationCandidates = candidates.filter((c) => c.family === "allocation");

    const { kept } = applyRelevanceFilter(allocationCandidates, ir);

    const survivor = kept.find((c) => c.sourceId === FN_FUEL && c.targetId === COMP_FUEL);
    expect(survivor).toBeDefined();
  });

  it("scoreAllocationSignals reports the matched token for signal a", () => {
    const ir = makeFixtureIR();
    const score = scoreAllocationSignals(FN_FUEL, COMP_FUEL, ir);
    expect(score.signalA).toBe(true);
    expect(score.matchedTokens).toContain("fuel");
    expect(score.strength).toBeGreaterThan(0);
  });
});

// ── RF-f: flow co-occurrence survives ─────────────────────────────────────────

describe("RF-f — signal b: L2-parent functional flow ∩ component-scope flow survives", () => {
  it("F1 'Telemetry' functional flow + 'Data Bus Controller' carrying 'Telemetry' is kept", () => {
    const ir = makeFixtureIR();
    const { candidates } = generateCandidates(ir);
    const allocationCandidates = candidates.filter((c) => c.family === "allocation");

    const { kept } = applyRelevanceFilter(allocationCandidates, ir);

    // Both leaf functions share the F1 parent → both pair with COMP_FLOW via signal b
    const survivor = kept.find((c) => c.sourceId === FN_RECV && c.targetId === COMP_FLOW);
    expect(survivor).toBeDefined();
  });

  it("scoreAllocationSignals reports signal b for the flow-bridged pair", () => {
    const ir = makeFixtureIR();
    const score = scoreAllocationSignals(FN_RECV, COMP_FLOW, ir);
    expect(score.signalB).toBe(true);
    expect(score.strength).toBeGreaterThan(0);
  });
});

// ── RF-c: no-signal pair is rejected_unbounded ────────────────────────────────

describe("RF-c — a pair with no corpus signal is rejected_unbounded", () => {
  it("'Receive Request' → 'Hydraulic Actuator' (no token, no flow) is rejected with reason code", () => {
    const ir = makeFixtureIR();
    const { candidates } = generateCandidates(ir);
    const allocationCandidates = candidates.filter((c) => c.family === "allocation");

    const { kept, rejected } = applyRelevanceFilter(allocationCandidates, ir);

    const keptPair = kept.find((c) => c.sourceId === FN_RECV && c.targetId === COMP_NONE);
    expect(keptPair).toBeUndefined();

    const rejectedPair = rejected.find(
      (c) => c.sourceId === FN_RECV && c.targetId === COMP_NONE
    );
    expect(rejectedPair).toBeDefined();
    expect(rejectedPair!.reasonCode).toBe("rejected_unbounded:no_signal");
  });
});

// ── RF-d: per-family cap ──────────────────────────────────────────────────────

describe("RF-d — per-family cap keeps highest-signal first, excess → rejected_capped", () => {
  it("cap=1 on the fixture keeps the strongest allocation candidate and caps the rest", () => {
    const ir = makeFixtureIR();
    const { candidates } = generateCandidates(ir);
    const allocationCandidates = candidates.filter((c) => c.family === "allocation");

    const { kept, rejected, capped } = applyRelevanceFilter(allocationCandidates, ir, {
      familyCap: 1,
    });

    expect(kept.length).toBe(1);
    expect(capped.length).toBeGreaterThan(0);
    for (const c of capped) {
      expect(c.stage).toBe("rejected_capped");
    }
    // kept + capped + rejected partitions the input
    expect(kept.length + capped.length + rejected.length).toBe(allocationCandidates.length);
  });

  it("engine dry-run honors INFER_FAMILY_CAP via options and reports capped counts in stats", async () => {
    const ir = makeFixtureIR();
    const result = await runInferenceEngine(ir, new NeverProvider(), {
      dryRun: true,
      familyCap: 1,
      log: () => {},
    });

    const allocStats = result.stats.find((s) => s.family === "allocation")!;
    expect(allocStats.rejectedCapped).toBeGreaterThan(0);
    // typed candidates surviving for allocation == 1 (the cap)
    const typedAllocation = result.records.filter(
      (r) => r.stage === "typed" && (r as { relationFamily: string }).relationFamily === "allocation"
    );
    expect(typedAllocation.length).toBe(1);
  });
});

// ── RF-e: controlJoin succession-already-stated skip ──────────────────────────

describe("RF-e — controlJoin pairs already stated by an approved prose succession are rejected", () => {
  function makeIRWithSuccession(): InferredComposedIR {
    const ir = makeFixtureIR();
    return {
      ...ir,
      proseEntries: [
        {
          id: "prose-succ-001",
          kind: "succession",
          fields: {
            owningFunction: "F1",
            fromAction: "Validate Fuel Capacity",
            toAction: "Receive Request",
          },
          citation: {
            docId: "doc-001",
            docSha256: "aa".repeat(32),
            chunkId: "chunk-001",
            sectionPath: "S1",
            quote: "After validating fuel capacity, the system receives the request.",
          },
          approvedBy: "test",
          approvedAt: "2026-06-11T00:00:00.000Z",
          candidateId: "cand-succ-001",
          status: "approved",
        },
      ],
      approvedProseIds: new Set(["prose-succ-001"]),
    };
  }

  it("the stated pair (fuel→recv) is rejected_unbounded:succession_already_stated", () => {
    const ir = makeIRWithSuccession();
    const { candidates } = generateCandidates(ir);
    const cjCandidates = candidates.filter((c) => c.family === "controlJoin");
    // sibling pairs: fuel→recv and recv→fuel = 2
    expect(cjCandidates.length).toBe(2);

    const { kept, rejected } = applyRelevanceFilter(cjCandidates, ir);

    const stated = rejected.find((c) => c.sourceId === FN_FUEL && c.targetId === FN_RECV);
    expect(stated).toBeDefined();
    expect(stated!.reasonCode).toBe("rejected_unbounded:succession_already_stated");

    // The reverse (unstated) pair survives
    const unstated = kept.find((c) => c.sourceId === FN_RECV && c.targetId === FN_FUEL);
    expect(unstated).toBeDefined();
  });

  it("without any prose succession, both sibling pairs survive", () => {
    const ir = makeFixtureIR();
    const { candidates } = generateCandidates(ir);
    const cjCandidates = candidates.filter((c) => c.family === "controlJoin");

    const { kept, rejected } = applyRelevanceFilter(cjCandidates, ir);
    expect(kept.length).toBe(2);
    expect(rejected.length).toBe(0);
  });
});

// ── Engine integration: stats carry rejectedUnbounded ─────────────────────────

describe("engine integration — relevance stats in dry run", () => {
  it("dry-run stats include rejectedUnbounded for allocation", async () => {
    const ir = makeFixtureIR();
    const result = await runInferenceEngine(ir, new NeverProvider(), {
      dryRun: true,
      log: () => {},
    });

    const allocStats = result.stats.find((s) => s.family === "allocation")!;
    expect(allocStats.rejectedUnbounded).toBeGreaterThan(0);
    // generated = 6 all-pairs; some rejected by relevance
    expect(allocStats.generated).toBe(6);

    // rejected_unbounded records present in output
    const unboundedRecords = result.records.filter((r) => r.stage === "rejected_unbounded");
    expect(unboundedRecords.length).toBeGreaterThan(0);
  });
});
