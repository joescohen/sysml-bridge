/**
 * cooccurrence.test.ts — W2 co-occurrence ("spokes") enumeration.
 *
 * Done-criteria covered (spec §8 W2):
 *   - Co-occurrence enumeration DETERMINISTIC (order test).
 *   - Per-family caps LOGGED with dropped counts (assert the log LINE, not just
 *     the cap length) — no silent caps.
 *   - Typed by the EXISTING type gate (fail-able control: an ill-typed pair
 *     yields zero candidates; a well-typed pair yields one).
 */

import { describe, it, expect } from "vitest";
import type { EntityRecord } from "../../entities/cluster.js";
import type { MentionRecord } from "../../mentions/index.js";
import { enumerateCooccurrence } from "../cooccurrence.js";

// ── Fixture builders ──────────────────────────────────────────────────────────

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
  sectionPath: string,
): MentionRecord {
  return {
    mentionId,
    surfaceForm: name,
    kindHint: kind,
    citation: { docId: "doc", docSha256: "aa".repeat(32), chunkId, sectionPath, quote: name },
    confidence: 0.9,
  };
}

// ── Determinism + basic chunk co-occurrence ───────────────────────────────────

describe("cooccurrence — deterministic chunk co-occurrence", () => {
  const mFn = mention("m-fn", "function", "Fuel Pump Ctrl", "chunk-A", "S1 > Design");
  const mComp = mention("m-comp", "component", "Fuel Pump", "chunk-A", "S1 > Design");
  const eFn = entity("function", "Fuel Pump Ctrl", ["m-fn"]);
  const eComp = entity("component", "Fuel Pump", ["m-comp"]);

  it("emits exactly one well-typed allocation spoke (function→component) with the shared chunk as premise", () => {
    const { candidates } = enumerateCooccurrence([eFn, eComp], [mFn, mComp], {
      families: ["allocation"],
      minCooccur: 1,
      log: () => {},
    });
    expect(candidates).toHaveLength(1);
    const c = candidates[0]!;
    expect(c.relationFamily).toBe("allocation");
    expect(c.sourceId).toBe(eFn.entityId); // function is the allocation SOURCE
    expect(c.targetId).toBe(eComp.entityId); // component is the TARGET
    expect(c.premiseIds).toContain("chunk-A"); // co-occurring chunk is the premise
    expect(c.cooccurKind).toBe("chunk+section"); // same chunk AND same section prefix
  });

  it("is deterministic: two runs produce byte-identical candidate sequences", () => {
    const opts = { families: ["allocation", "modeMembership"] as const, minCooccur: 1, log: () => {} };
    const a = enumerateCooccurrence([eFn, eComp], [mFn, mComp], opts);
    const b = enumerateCooccurrence([eFn, eComp], [mFn, mComp], opts);
    expect(JSON.stringify(a.candidates)).toEqual(JSON.stringify(b.candidates));
  });
});

// ── No silent caps: per-family cap logs a dropped-count LINE ───────────────────

describe("cooccurrence — per-family cap is LOGGED with dropped counts", () => {
  it("caps allocation at familyCap and logs the dropped count line", () => {
    // One function entity co-occurs (chunk-A) with THREE component entities →
    // 3 allocation spokes → cap to 1 → dropped 2, LOGGED.
    const mFn = mention("m-fn", "function", "Ctrl", "chunk-A", "S1");
    const mC1 = mention("m-c1", "component", "Comp One", "chunk-A", "S1");
    const mC2 = mention("m-c2", "component", "Comp Two", "chunk-A", "S1");
    const mC3 = mention("m-c3", "component", "Comp Three", "chunk-A", "S1");
    const entities = [
      entity("function", "Ctrl", ["m-fn"]),
      entity("component", "Comp One", ["m-c1"]),
      entity("component", "Comp Two", ["m-c2"]),
      entity("component", "Comp Three", ["m-c3"]),
    ];
    const mentions = [mFn, mC1, mC2, mC3];

    const logs: string[] = [];
    const { candidates, droppedByFamily } = enumerateCooccurrence(entities, mentions, {
      families: ["allocation"],
      minCooccur: 1,
      familyCap: 1,
      log: (m) => logs.push(m),
    });

    expect(candidates).toHaveLength(1); // capped
    expect(droppedByFamily["allocation"]).toBe(2);
    // The LINE must be present — no silent cap.
    const capLine = logs.find((l) => l.includes("[cooccurrence] cap:") && l.includes("family=allocation"));
    expect(capLine).toBeDefined();
    expect(capLine).toContain("kept=1");
    expect(capLine).toContain("dropped=2");
  });
});

// ── Existing type gate: fail-able control ─────────────────────────────────────

describe("cooccurrence — typed by the existing type gate (fail-able control)", () => {
  const mC1 = mention("m-c1", "component", "Comp A", "chunk-A", "S1");
  const mC2 = mention("m-c2", "component", "Comp B", "chunk-A", "S1");
  const eC1 = entity("component", "Comp A", ["m-c1"]);
  const eC2 = entity("component", "Comp B", ["m-c2"]);

  it("REJECTS an ill-typed pair: two components co-occur but allocation needs a function source → zero", () => {
    const { candidates } = enumerateCooccurrence([eC1, eC2], [mC1, mC2], {
      families: ["allocation"],
      minCooccur: 1,
      log: () => {},
    });
    expect(candidates).toHaveLength(0);
  });

  it("ACCEPTS the well-typed control: function+component co-occur → one allocation spoke", () => {
    const mFn = mention("m-fn", "function", "Ctrl", "chunk-A", "S1");
    const eFn = entity("function", "Ctrl", ["m-fn"]);
    const { candidates } = enumerateCooccurrence([eFn, eC1], [mFn, mC1], {
      families: ["allocation"],
      minCooccur: 1,
      log: () => {},
    });
    expect(candidates).toHaveLength(1);
    expect(candidates[0]!.relationFamily).toBe("allocation");
  });

  it("section-prefix co-occurrence (no shared chunk) still enumerates", () => {
    const mFn = mention("m-fn", "function", "Ctrl", "chunk-A", "S1 > Sub");
    const mComp = mention("m-comp", "component", "Comp A", "chunk-B", "S1 > Sub > Deep");
    const eFn = entity("function", "Ctrl", ["m-fn"]);
    const eComp = entity("component", "Comp A", ["m-comp"]);
    const { candidates } = enumerateCooccurrence([eFn, eComp], [mFn, mComp], {
      families: ["allocation"],
      minCooccur: 1,
      log: () => {},
    });
    expect(candidates).toHaveLength(1);
    expect(candidates[0]!.cooccurKind).toBe("section");
  });
});
