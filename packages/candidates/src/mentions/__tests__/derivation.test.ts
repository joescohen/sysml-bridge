/**
 * derivation.test.ts — pure MentionRecord derivation from raw proposals.
 *
 * Claims closed (W0 done-criteria):
 *   1. Determinism: same input → identical mentionIds, byte-identical across
 *      two independent runs of `deriveMentions`.
 *   3. Mentions with an unresolvable chunkId OR a non-verbatim quote are
 *      DROPPED and COUNTED (`droppedUnverbatimMentions`) — with a FAIL-ABLE
 *      positive control (a fixture that, if the drop logic were removed,
 *      would produce a different mention count / a zero drop counter).
 */

import { describe, it, expect } from "vitest";
import { deriveMentions, normSurface } from "../index.js";
import type { MentionDerivationContext } from "../index.js";
import type { CandidateProposal } from "../../prose/llm-provider.js";

const CONTEXT: MentionDerivationContext = {
  documentId: "doc-1",
  documentHash: "a".repeat(64),
  sectionPath: "root/1",
};

const CHUNK_A = "chunk-a";
const CHUNK_B = "chunk-b";

const CHUNK_STORE = new Map<string, string>([
  [CHUNK_A, "The Fuel Control Module shall command the boom within sixty seconds."],
  [CHUNK_B, "Standby mode transitions to Active mode on power-on."],
]);

function proposal(overrides: Partial<CandidateProposal>): CandidateProposal {
  return {
    kind: "component",
    fields: { name: "Fuel Control Module" },
    citedChunkId: CHUNK_A,
    confidence: 0.8,
    quote: "The Fuel Control Module shall command the boom within sixty seconds.",
    ...overrides,
  };
}

// ── Implicit derivation (candidate's own name field) ────────────────────────

describe("deriveMentions — implicit derivation from each proposal kind", () => {
  it("component: derives a mention from fields.name", () => {
    const { mentions, droppedUnverbatimMentions } = deriveMentions(
      [proposal({ kind: "component", fields: { name: "Fuel Control Module" } })],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(droppedUnverbatimMentions).toBe(0);
    expect(mentions).toHaveLength(1);
    expect(mentions[0]!.surfaceForm).toBe("Fuel Control Module");
    expect(mentions[0]!.kindHint).toBe("component");
    expect(mentions[0]!.citation.chunkId).toBe(CHUNK_A);
    expect(mentions[0]!.citation.docSha256).toBe(CONTEXT.documentHash);
  });

  it("requirement: derives a mention from fields.text", () => {
    const { mentions } = deriveMentions(
      [
        proposal({
          kind: "requirement",
          fields: { text: "The Fuel Control Module shall command the boom within sixty seconds." },
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(mentions).toHaveLength(1);
    expect(mentions[0]!.kindHint).toBe("requirement");
  });

  it("modeTransition: derives TWO mode mentions (fromMode, toMode)", () => {
    const { mentions } = deriveMentions(
      [
        proposal({
          kind: "modeTransition",
          fields: { fromMode: "Standby", toMode: "Active" },
          citedChunkId: CHUNK_B,
          quote: "Standby mode transitions to Active mode on power-on.",
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(mentions).toHaveLength(2);
    expect(mentions.map((m) => m.surfaceForm).sort()).toEqual(["Active", "Standby"]);
    expect(mentions.every((m) => m.kindHint === "mode")).toBe(true);
  });

  it("succession/decision/parallel: derive owningFunction as a function mention", () => {
    const { mentions } = deriveMentions(
      [
        proposal({
          kind: "succession",
          fields: { owningFunction: "F3", fromAction: "Receive", toAction: "Validate" },
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(mentions).toHaveLength(1);
    expect(mentions[0]!.surfaceForm).toBe("F3");
    expect(mentions[0]!.kindHint).toBe("function");
  });

  it("emits zero mentions when a proposal has no name-like field", () => {
    const { mentions } = deriveMentions(
      [proposal({ kind: "requirement", fields: {} })],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(mentions).toHaveLength(0);
  });
});

// ── Explicit provider-harvested mentions ────────────────────────────────────

describe("deriveMentions — explicit proposal.mentions[]", () => {
  it("derives an additional mention alongside the implicit one", () => {
    const { mentions } = deriveMentions(
      [
        proposal({
          kind: "requirement",
          fields: { text: "The Fuel Control Module shall command the boom within sixty seconds." },
          mentions: [
            {
              surfaceForm: "boom",
              kindHint: "component",
              citedChunkId: CHUNK_A,
              quote: "command the boom within sixty seconds",
              confidence: 0.5,
            },
          ],
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(mentions).toHaveLength(2);
    const surfaces = mentions.map((m) => m.surfaceForm).sort();
    expect(surfaces).toEqual([
      "The Fuel Control Module shall command the boom within sixty seconds.",
      "boom",
    ]);
  });
});

// ── C4/C6 discipline extended to mentions (claim 3, fail-able) ─────────────

describe("deriveMentions — unresolvable chunkId or non-verbatim quote is DROPPED and COUNTED", () => {
  it("drops an explicit mention citing an unresolvable chunkId", () => {
    const result = deriveMentions(
      [
        proposal({
          kind: "requirement",
          fields: { text: "irrelevant" },
          mentions: [
            {
              surfaceForm: "Ghost Component",
              kindHint: "component",
              citedChunkId: "chunk-does-not-exist",
              quote: "anything",
              confidence: 0.5,
            },
          ],
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    // FAIL-ABLE POSITIVE CONTROL: if the citation gate in deriveMentions were
    // removed (i.e. every candidate mention were pushed unconditionally),
    // "Ghost Component" would appear in `mentions` and `droppedUnverbatimMentions`
    // would read 0 — both assertions below would fail.
    expect(result.mentions.some((m) => m.surfaceForm === "Ghost Component")).toBe(false);
    expect(result.droppedUnverbatimMentions).toBe(1);
  });

  it("drops an explicit mention whose quote does not verbatim-resolve into its cited chunk", () => {
    const result = deriveMentions(
      [
        proposal({
          kind: "requirement",
          fields: { text: "irrelevant" },
          mentions: [
            {
              surfaceForm: "Hallucinated Thing",
              kindHint: "component",
              citedChunkId: CHUNK_A, // real chunk…
              quote: "this exact phrase is not present in chunk A at all", // …but hallucinated quote
              confidence: 0.5,
            },
          ],
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(result.mentions.some((m) => m.surfaceForm === "Hallucinated Thing")).toBe(false);
    expect(result.droppedUnverbatimMentions).toBe(1);
  });

  it("an implicit mention inherits its parent proposal's drop when the proposal itself is uncited", () => {
    const result = deriveMentions(
      [
        proposal({
          kind: "component",
          fields: { name: "Orphan Component" },
          citedChunkId: "chunk-does-not-exist",
          quote: "irrelevant",
        }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(result.mentions).toHaveLength(0);
    expect(result.droppedUnverbatimMentions).toBe(1);
  });
});

// ── Determinism (claim 1) ────────────────────────────────────────────────────

describe("deriveMentions — determinism", () => {
  it("running derivation twice on identical input yields byte-identical (deep-equal) results", () => {
    const proposals: CandidateProposal[] = [
      proposal({ kind: "component", fields: { name: "Fuel Control Module" } }),
      proposal({
        kind: "modeTransition",
        fields: { fromMode: "Standby", toMode: "Active" },
        citedChunkId: CHUNK_B,
        quote: "Standby mode transitions to Active mode on power-on.",
      }),
      proposal({
        kind: "requirement",
        fields: { text: "irrelevant" },
        mentions: [
          {
            surfaceForm: "boom",
            kindHint: "component",
            citedChunkId: CHUNK_A,
            quote: "command the boom within sixty seconds",
            confidence: 0.5,
          },
        ],
      }),
    ];

    const run1 = deriveMentions(proposals, CHUNK_STORE, CONTEXT);
    const run2 = deriveMentions(proposals, CHUNK_STORE, CONTEXT);

    expect(run1).toEqual(run2);
    expect(JSON.stringify(run1)).toBe(JSON.stringify(run2));
    expect(run1.mentions.map((m) => m.mentionId)).toEqual(run2.mentions.map((m) => m.mentionId));
  });

  it("mentionId is stable across separate invocations for the same natural key", () => {
    const p = proposal({ kind: "component", fields: { name: "Fuel Control Module" } });
    const a = deriveMentions([p], CHUNK_STORE, CONTEXT);
    const b = deriveMentions([p], CHUNK_STORE, CONTEXT);
    expect(a.mentions[0]!.mentionId).toBe(b.mentions[0]!.mentionId);
  });
});

// ── Dedup ─────────────────────────────────────────────────────────────────

describe("deriveMentions — dedup by mentionId", () => {
  it("collapses two proposals naming the same surface/kind in the same chunk into one mention", () => {
    const { mentions } = deriveMentions(
      [
        proposal({ kind: "component", fields: { name: "Fuel Control Module" } }),
        proposal({ kind: "component", fields: { name: "Fuel Control Module" } }),
      ],
      CHUNK_STORE,
      CONTEXT,
    );
    expect(mentions).toHaveLength(1);
  });
});

// ── normSurface ──────────────────────────────────────────────────────────────

describe("normSurface", () => {
  it("collapses whitespace/punctuation and casefolds", () => {
    expect(normSurface("Flight-Control  Module")).toBe(normSurface("flight control module"));
    expect(normSurface("  FCM.  ")).toBe("fcm");
  });

  it("is pure and order-independent for the same input", () => {
    expect(normSurface("Fuel Pump")).toBe(normSurface("Fuel Pump"));
  });
});
