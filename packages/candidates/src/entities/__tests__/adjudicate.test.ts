/**
 * adjudicate.test.ts — Band 3 mid-band debate adjudication (spec §8 W1).
 *
 * The adjudicator reuses the advocate/challenger debate seam VERBATIM behind the
 * injectable `InferenceProvider` — so it is fully mock-testable with zero API
 * key. The verdict is the SAME deterministic function of the two scores as
 * inference debate (computeDebateVerdict): advocate≥0.7 ∧ challenger<0.5 →
 * confirmed; challenger≥0.7 → auto_rejected; else uncertain.
 */

import { describe, it, expect } from "vitest";
import { adjudicateEntityMerge } from "../adjudicate.js";
import { entityIdFor, type EntityRecord } from "../cluster.js";
import type { InferenceProvider } from "../../inference/inference-provider.js";
import type { ProposeResult } from "../../inference/types.js";

function entity(canonical: string, kind: EntityRecord["kind"]): EntityRecord {
  return {
    entityId: entityIdFor(kind, canonical),
    kind,
    canonicalName: canonical,
    aliases: [canonical],
    mentionIds: [],
    mergeDispositions: [],
  };
}

/** A mock provider returning fixed advocate/challenger scores. */
function mockProvider(advocateScore: number, challengerScore: number): InferenceProvider {
  return {
    propose(): Promise<ProposeResult> {
      return Promise.resolve({ kind: "declined" });
    },
    advocate() {
      return Promise.resolve({ score: advocateScore, summary: "advocate (mock)" });
    },
    challenge() {
      return Promise.resolve({ score: challengerScore, summary: "challenger (mock)" });
    },
  };
}

const A = entity("FCM", "component");
const B = entity("Flight Control Module", "component");

describe("adjudicateEntityMerge — deterministic verdict via the debate seam", () => {
  it("strong advocate + weak challenger → confirmed", async () => {
    const r = await adjudicateEntityMerge(mockProvider(0.9, 0.2), A, B);
    expect(r.verdict).toBe("confirmed");
    expect(r.advocate).toBe(0.9);
    expect(r.challenger).toBe(0.2);
  });

  it("strong challenger → auto_rejected", async () => {
    const r = await adjudicateEntityMerge(mockProvider(0.8, 0.85), A, B);
    expect(r.verdict).toBe("auto_rejected");
  });

  it("mid scores → uncertain (still a proposal for the human)", async () => {
    const r = await adjudicateEntityMerge(mockProvider(0.6, 0.55), A, B);
    expect(r.verdict).toBe("uncertain");
  });

  it("failure isolation: a throwing provider yields a defined uncertain result", async () => {
    const throwing: InferenceProvider = {
      propose: () => Promise.reject(new Error("no")),
      advocate: () => Promise.reject(new Error("advocate down")),
      challenge: () => Promise.reject(new Error("challenger down")),
    };
    const r = await adjudicateEntityMerge(throwing, A, B);
    // Both passes default to 0.5 → uncertain; never throws out of the seam.
    expect(r.verdict).toBe("uncertain");
  });
});
