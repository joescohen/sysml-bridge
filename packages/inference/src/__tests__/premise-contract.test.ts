/**
 * premise-contract.test.ts — RED-first tests for the premise id contract fix
 * (conductor finding from the live run: all 38 proposals cited premises by NAME,
 * not composed-IR id → 100% dropped unpremised; 412 null-proposals were
 * indistinguishable between declined and parse failure).
 *
 * Covers:
 *   PC-a — prompt builder: the propose user message renders every offered fact as
 *          `[id: <id>] <kind> "<name>" — <detail>`, carries the MUST-cite-ids
 *          instruction verbatim, and includes a worked example
 *   PC-b — premise repair: a premise cited by NAME of an offered fact is repaired
 *          to that fact's id (counted); a name NOT in the offered bundle stays
 *          unresolvable and drops; ambiguous names are not repaired
 *   PC-c — stats split: proposal_declined vs proposal_parse_error are counted
 *          separately per family
 */

import { describe, it, expect } from "vitest";
import type { InferredComposedIR } from "@sysml-bridge/ir";
import { SCHEMA_VERSION } from "@sysml-bridge/ir";
import { buildContextBundle, collectOfferedFacts } from "../neighborhood.js";
import { buildProposeUserMessage, PREMISE_ID_INSTRUCTION } from "../inference-provider.js";
import { repairPremises } from "../premise-repair.js";
import { runInferenceEngine } from "../engine.js";
import type { InferenceProvider } from "../inference-provider.js";
import type { ProposeResult, ContextBundle, OfferedFact, QueuedRecord } from "../types.js";

// ── Fixture IR (mirrors the engine-test fixture, small + deterministic) ───────

const LEAF_FN_ID = "function-leaf-001";
const LEAF_FN_ID_2 = "function-leaf-002";
const COMP_ID = "component-comp-001";
const N2_ID = "n2-flow-001";
const MODE_ID = "prose-mode-001";
const CORPUS_REQ_ID = "requirement-req-001";

function makeIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "TestSub",
    needs: [],
    requirements: [
      {
        id: CORPUS_REQ_ID,
        kind: "requirement",
        naturalKey: "CC-1",
        name: "Authenticate Refueling Requests",
        statement: "The system shall authenticate refueling requests.",
        needIds: [],
      },
    ],
    functions: [
      {
        id: LEAF_FN_ID,
        kind: "function",
        naturalKey: "F1.1",
        name: "Receive & Authenticate Request",
        level: "L3",
        owner: "F1: Manage Refueling Requests",
      },
      {
        id: LEAF_FN_ID_2,
        kind: "function",
        naturalKey: "F1.2",
        name: "Validate Fuel Capacity",
        level: "L3",
        owner: "F1: Manage Refueling Requests",
      },
    ],
    components: [
      { id: COMP_ID, kind: "component", naturalKey: "Operator Console Module", name: "Operator Console Module" },
    ],
    satisfies: [{ reqId: CORPUS_REQ_ID, functionId: LEAF_FN_ID }],
    allocations: [],
    subsystems: [
      {
        id: "subsystem-001",
        kind: "subsystem",
        naturalKey: "Command & Control Subsystem",
        name: "Command & Control Subsystem",
        componentIds: [COMP_ID],
        provenance: { workbook: "t.xlsx", sheet: "S" },
      },
    ],
    n2Interfaces: [
      {
        id: N2_ID,
        kind: "n2",
        scope: "component",
        sourceId: COMP_ID,
        targetId: "component-other-x",
        sourceLabel: "Operator Console Module",
        targetLabel: "Other Module",
        flow: "Operator Commands",
        provenance: { workbook: "t.xlsx", sheet: "S", row: 1, cell: "A1" },
      },
    ],
    kpps: [],
    behaviorDecomp: [],
  };

  return {
    extracted: extracted as any,
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
    ],
    approvedProseIds: new Set([MODE_ID]),
    inferredEntries: [],
    approvedInferredIds: new Set(),
  };
}

// ── PC-a: prompt builder ──────────────────────────────────────────────────────

describe("PC-a — propose prompt offers ids explicitly", () => {
  it("context bundle carries offered facts incl. source, target, and 1-hop neighbors with ids", () => {
    const ir = makeIR();
    const context = buildContextBundle(LEAF_FN_ID, COMP_ID, ir);

    expect(context.offeredFacts.length).toBeGreaterThan(0);
    const ids = context.offeredFacts.map((f) => f.id);
    expect(ids).toContain(LEAF_FN_ID);
    expect(ids).toContain(COMP_ID);
    // 1-hop: the satisfied requirement (source side) and the N2 flow (target side)
    expect(ids).toContain(CORPUS_REQ_ID);
    expect(ids).toContain(N2_ID);
  });

  it("user message renders every offered fact as `[id: <id>] <kind> \"<name>\" — <detail>`", () => {
    const ir = makeIR();
    const context = buildContextBundle(LEAF_FN_ID, COMP_ID, ir);
    const msg = buildProposeUserMessage("allocation", LEAF_FN_ID, COMP_ID, context);

    for (const fact of context.offeredFacts) {
      expect(msg).toContain(`[id: ${fact.id}] ${fact.kind} "${fact.name}"`);
    }
  });

  it("user message carries the MUST-cite-ids instruction and a worked example", () => {
    const ir = makeIR();
    const context = buildContextBundle(LEAF_FN_ID, COMP_ID, ir);
    const msg = buildProposeUserMessage("allocation", LEAF_FN_ID, COMP_ID, context);

    expect(msg).toContain(PREMISE_ID_INSTRUCTION);
    expect(PREMISE_ID_INSTRUCTION).toContain(
      "premises MUST be the bracketed ids EXACTLY as given above"
    );
    expect(msg).toContain("WORKED EXAMPLE");
    // The worked example demonstrates id-form premises
    expect(msg).toMatch(/"premises":\s*\[\s*"[a-z0-9]+-[0-9a-f]{16}"/);
  });
});

// ── PC-b: deterministic name→id premise repair ────────────────────────────────

describe("PC-b — name→id premise repair within the offered bundle", () => {
  const FACTS: OfferedFact[] = [
    {
      id: "function-aaaa111122223333",
      kind: "function",
      name: "Receive & Authenticate Request",
      detail: "level L3",
      aliases: ["F1.1", "F1.1: Receive & Authenticate Request"],
    },
    {
      id: "component-bbbb444455556666",
      kind: "component",
      name: "Operator Console Module",
      detail: "",
    },
    // Two facts sharing a name → ambiguous, must NOT repair
    { id: "n2-cccc777788889999", kind: "n2-flow", name: "Telemetry", detail: "A → B" },
    { id: "n2-dddd000011112222", kind: "n2-flow", name: "Telemetry", detail: "C → D" },
  ];

  it("a premise citing an offered fact's exact name is repaired to its id", () => {
    const { premises, repairedCount } = repairPremises(
      ["Operator Console Module"],
      FACTS
    );
    expect(premises).toEqual(["component-bbbb444455556666"]);
    expect(repairedCount).toBe(1);
  });

  it("matching is case- and whitespace-insensitive", () => {
    const { premises, repairedCount } = repairPremises(
      ["  operator   console MODULE "],
      FACTS
    );
    expect(premises).toEqual(["component-bbbb444455556666"]);
    expect(repairedCount).toBe(1);
  });

  it("a naturalKey alias (e.g. 'F1.1') repairs to the fact's id", () => {
    const { premises, repairedCount } = repairPremises(["F1.1"], FACTS);
    expect(premises).toEqual(["function-aaaa111122223333"]);
    expect(repairedCount).toBe(1);
  });

  it("a name NOT in the offered bundle is left untouched (will drop)", () => {
    const { premises, repairedCount } = repairPremises(
      ["F2: Control Autonomous Docking"],
      FACTS
    );
    expect(premises).toEqual(["F2: Control Autonomous Docking"]);
    expect(repairedCount).toBe(0);
  });

  it("an ambiguous name (two offered facts share it) is NOT repaired", () => {
    const { premises, repairedCount } = repairPremises(["Telemetry"], FACTS);
    expect(premises).toEqual(["Telemetry"]);
    expect(repairedCount).toBe(0);
  });

  it("an already-correct id passes through unrepaired", () => {
    const { premises, repairedCount } = repairPremises(
      ["component-bbbb444455556666"],
      FACTS
    );
    expect(premises).toEqual(["component-bbbb444455556666"]);
    expect(repairedCount).toBe(0);
  });

  it("engine: a proposal citing an offered fact by NAME is repaired, validated, and queued", async () => {
    // Provider cites the requirement by NAME — it IS in the offered bundle for
    // (LEAF_FN_ID → COMP_ID) via the satisfy chain, so repair → CORPUS_REQ_ID.
    class NameCitingProvider implements InferenceProvider {
      async propose(
        family: Parameters<InferenceProvider["propose"]>[0],
        sourceId: string,
        targetId: string,
        _context: ContextBundle
      ): Promise<ProposeResult> {
        // Only propose where the requirement IS in the offered bundle
        // (the satisfy chain offers it only for LEAF_FN_ID-sourced candidates)
        if (sourceId !== LEAF_FN_ID) return { kind: "declined" };
        return {
          kind: "proposal",
          proposal: {
            sourceId,
            targetId,
            relationFamily: family,
            premises: ["Authenticate Refueling Requests"], // NAME, not id
            rationale: "audit-only",
            confidence: 0.9,
          },
        };
      }
      async advocate() {
        return { score: 0.8, summary: "n/a" };
      }
      async challenge() {
        return { score: 0.3, summary: "n/a" };
      }
    }

    const result = await runInferenceEngine(makeIR(), new NameCitingProvider(), {
      dryRun: false,
      log: () => {},
    });

    // The repaired proposals must be QUEUED, not dropped
    expect(result.droppedUnpremised).toBe(0);
    expect(result.emittedUnpremised).toBe(0);
    const queued = result.records.filter((r): r is QueuedRecord => r.stage === "queued");
    expect(queued.length).toBeGreaterThan(0);
    for (const q of queued) {
      expect(q.premises).toEqual([CORPUS_REQ_ID]);
    }
    // The repair count is surfaced in stats
    const totalRepaired = result.stats.reduce((n, s) => n + s.premiseRepaired, 0);
    expect(totalRepaired).toBeGreaterThan(0);
  });

  it("engine: a premise name NOT in the offered bundle still drops (no global repair)", async () => {
    class BadNameProvider implements InferenceProvider {
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
            premises: ["F2: Control Autonomous Docking"], // never offered
            rationale: "audit-only",
            confidence: 0.9,
          },
        };
      }
      async advocate() {
        return { score: 0.8, summary: "n/a" };
      }
      async challenge() {
        return { score: 0.3, summary: "n/a" };
      }
    }

    const result = await runInferenceEngine(makeIR(), new BadNameProvider(), {
      dryRun: false,
      log: () => {},
    });

    expect(result.droppedUnpremised).toBeGreaterThan(0);
    expect(result.emittedUnpremised).toBe(0);
    const queued = result.records.filter((r) => r.stage === "queued");
    expect(queued.length).toBe(0);
  });
});

// ── PC-c: declined vs parse-error stats split ─────────────────────────────────

describe("PC-c — proposal_declined vs proposal_parse_error are counted separately", () => {
  it("declined and parse_error results are tallied in per-family stats", async () => {
    // Alternate: declined, parse_error, declined, parse_error, …
    let call = 0;
    class SplitProvider implements InferenceProvider {
      async propose(): Promise<ProposeResult> {
        call++;
        return call % 2 === 1
          ? { kind: "declined" }
          : { kind: "parse_error", detail: "bad JSON" };
      }
      async advocate() {
        return { score: 0.5, summary: "n/a" };
      }
      async challenge() {
        return { score: 0.5, summary: "n/a" };
      }
    }

    const result = await runInferenceEngine(makeIR(), new SplitProvider(), {
      dryRun: false,
      log: () => {},
    });

    const totalDeclined = result.stats.reduce((n, s) => n + s.proposalDeclined, 0);
    const totalParseErr = result.stats.reduce((n, s) => n + s.proposalParseError, 0);
    expect(totalDeclined).toBeGreaterThan(0);
    expect(totalParseErr).toBeGreaterThan(0);
    expect(totalDeclined + totalParseErr).toBe(call);
    // Nothing proposed → nothing queued/proposed
    const totalProposed = result.stats.reduce((n, s) => n + s.proposed, 0);
    expect(totalProposed).toBe(0);
  });
});
