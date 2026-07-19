/**
 * suggest.test.ts — Band 2 merge suggesters (spec §8 W1).
 *
 * Both suggesters ship with a FAIL-ABLE positive control: a matching pair IS
 * proposed AND a non-matching pair is NOT — so a suggester that fired on
 * everything (or nothing) would fail loudly.
 *   - acronym: "FCM" ↔ "Flight Control Module" proposes; "FCM" ↔ "Ground Station"
 *     does not.
 *   - token-overlap: high-Jaccard pair proposes; disjoint-token pair does not.
 *   - rejected pair (content-addressed skip key) is NEVER re-proposed.
 */

import { describe, it, expect } from "vitest";
import { entityMergePairKey } from "@sysml-bridge/model";
import {
  suggestMerges,
  acronymMatch,
  isAcronymExpansion,
  tokenOverlap,
} from "../suggest.js";
import { entityIdFor, type EntityRecord } from "../cluster.js";

function entity(canonical: string, kind: EntityRecord["kind"], aliases?: string[]): EntityRecord {
  return {
    entityId: entityIdFor(kind, canonical),
    kind,
    canonicalName: canonical,
    aliases: aliases ?? [canonical],
    mentionIds: [`men-${entityIdFor(kind, canonical)}`],
    mergeDispositions: [],
  };
}

describe("acronym / expansion suggester", () => {
  it("isAcronymExpansion: initials of a multi-word name", () => {
    expect(isAcronymExpansion("FCM", "Flight Control Module")).toBe(true);
    expect(isAcronymExpansion("fcm", "Flight Control Module")).toBe(true);
    expect(isAcronymExpansion("FCM", "Ground Station")).toBe(false); // "GS" != "fcm"
    expect(isAcronymExpansion("FCM", "Fuel")).toBe(false); // single word, not an expansion
  });

  it("proposes an acronym pair and NOT a non-matching pair (fail-able control)", () => {
    const fcm = entity("FCM", "component");
    const flight = entity("Flight Control Module", "component");
    const ground = entity("Ground Station", "component");

    expect(acronymMatch(fcm, flight)).toBe(true);
    expect(acronymMatch(fcm, ground)).toBe(false);

    const proposals = suggestMerges([fcm, flight, ground]);
    const pairKeys = proposals.map((p) => p.id);
    expect(pairKeys).toContain(entityMergePairKey(fcm.entityId, flight.entityId));
    expect(pairKeys).not.toContain(entityMergePairKey(fcm.entityId, ground.entityId));
    const acr = proposals.find((p) => p.id === entityMergePairKey(fcm.entityId, flight.entityId));
    expect(acr!.reason).toBe("acronym");
  });
});

describe("token-overlap suggester", () => {
  it("scores Jaccard over nameTokens and proposes only above the threshold", () => {
    const a = entity("Fuel Control Module", "component");
    const b = entity("Fuel Control Processing", "component");
    const c = entity("Weather Radar Antenna", "component");

    // a∩b share the token "fuel" (control/module/processing are stopwords or shared);
    // a∩c are disjoint.
    expect(tokenOverlap(a, b)).toBeGreaterThanOrEqual(0.5);
    expect(tokenOverlap(a, c)).toBe(0);

    const proposals = suggestMerges([a, b, c]);
    const ids = proposals.map((p) => p.id);
    expect(ids).toContain(entityMergePairKey(a.entityId, b.entityId));
    expect(ids).not.toContain(entityMergePairKey(a.entityId, c.entityId));
  });

  it("does NOT propose across incompatible kinds", () => {
    const comp = entity("FCM", "component");
    const mode = entity("Flight Control Module", "mode");
    // Acronym would match on surface, but the kinds are incompatible.
    const proposals = suggestMerges([comp, mode]);
    expect(proposals).toHaveLength(0);
  });
});

describe("rejected pair is never re-proposed", () => {
  it("a pair whose content-addressed key is in skipPairKeys is filtered out", () => {
    const fcm = entity("FCM", "component");
    const flight = entity("Flight Control Module", "component");
    const key = entityMergePairKey(fcm.entityId, flight.entityId);

    const withoutSkip = suggestMerges([fcm, flight]);
    expect(withoutSkip.map((p) => p.id)).toContain(key);

    const withSkip = suggestMerges([fcm, flight], { skipPairKeys: new Set([key]) });
    expect(withSkip.map((p) => p.id)).not.toContain(key);
  });
});

describe("determinism", () => {
  it("same entities in → byte-identical proposals out", () => {
    const ents = [
      entity("FCM", "component"),
      entity("Flight Control Module", "component"),
      entity("Fuel Control Processing", "component"),
    ];
    expect(JSON.stringify(suggestMerges(ents))).toBe(JSON.stringify(suggestMerges(ents)));
  });
});
