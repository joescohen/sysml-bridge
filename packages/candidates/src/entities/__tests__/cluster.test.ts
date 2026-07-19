/**
 * cluster.test.ts — Band 1 deterministic auto-cluster (spec §8 W1).
 *
 * Claims closed:
 *   - normalization-EXACT clustering: mentions with equal normSurface + kindHint
 *     collapse to one entity; identity is minted from the first seed and stable.
 *   - the TRAP (same surface form, different kindHint) does NOT auto-merge.
 *   - determinism: same mentions in → byte-identical EntityRecord[] out.
 */

import { describe, it, expect } from "vitest";
import { autoCluster, entityIdFor } from "../cluster.js";
import type { MentionRecord, MentionKind } from "../../mentions/index.js";

let seq = 0;
function mention(
  surfaceForm: string,
  kindHint: MentionKind,
  overrides: Partial<MentionRecord> = {}
): MentionRecord {
  seq += 1;
  return {
    mentionId: overrides.mentionId ?? `mention-${seq}`,
    surfaceForm,
    kindHint,
    confidence: 0.8,
    citation: {
      docId: "doc-1",
      docSha256: "a".repeat(64),
      chunkId: "chunk-1",
      sectionPath: "root",
      quote: surfaceForm,
    },
    ...overrides,
  };
}

describe("autoCluster — normalization-exact clustering", () => {
  it("clusters mentions whose normSurface + kindHint match exactly", () => {
    const entities = autoCluster([
      mention("Fuel Control Module", "component"),
      mention("fuel  control   module", "component"), // whitespace + case variant
      mention("FUEL-CONTROL-MODULE", "component"), // punct variant
    ]);
    expect(entities).toHaveLength(1);
    expect(entities[0]!.aliases).toEqual([
      "Fuel Control Module",
      "fuel  control   module",
      "FUEL-CONTROL-MODULE",
    ]);
    expect(entities[0]!.mentionIds).toHaveLength(3);
    expect(entities[0]!.mergeDispositions).toEqual([]);
  });

  it("does NOT cluster mentions whose normSurface differs", () => {
    const entities = autoCluster([
      mention("Fuel Control Module", "component"),
      mention("Fuel Control Processing Unit", "component"),
    ]);
    expect(entities).toHaveLength(2);
  });

  it("mints entityId from the first seed via stableId('entity', kind:normSurface)", () => {
    const entities = autoCluster([mention("Fuel Control Module", "component")]);
    expect(entities[0]!.entityId).toBe(entityIdFor("component", "Fuel Control Module"));
    // Identity is normalization-stable: a later variant maps to the SAME id.
    expect(entityIdFor("component", "fuel control module")).toBe(entities[0]!.entityId);
  });

  it("canonicalName = most-frequent surface form (ties → first-seen)", () => {
    const entities = autoCluster([
      mention("FCM box", "component"), // appears once
      mention("fcm  box", "component"), // normSurface-equal, different surface
      mention("fcm  box", "component"), // most frequent surface form
    ]);
    expect(entities).toHaveLength(1);
    expect(entities[0]!.canonicalName).toBe("fcm  box");
  });
});

describe("autoCluster — the trap: same surface, different kind", () => {
  it("does NOT auto-merge a component and a mode that share a surface form", () => {
    // "Standby" names a component in one chunk and a mode in another.
    const entities = autoCluster([
      mention("Standby", "component"),
      mention("Standby", "mode"),
    ]);
    expect(entities).toHaveLength(2);
    const kinds = entities.map((e) => e.kind).sort();
    expect(kinds).toEqual(["component", "mode"]);
    // Distinct identities — the surface form alone never merges across kinds.
    expect(entities[0]!.entityId).not.toBe(entities[1]!.entityId);
  });
});

describe("autoCluster — determinism", () => {
  it("same mentions in → byte-identical EntityRecord[] out across two runs", () => {
    const input: MentionRecord[] = [
      mention("Boom Nozzle", "component", { mentionId: "m-1" }),
      mention("boom nozzle", "component", { mentionId: "m-2" }),
      mention("Refuel", "function", { mentionId: "m-3" }),
      mention("Standby", "mode", { mentionId: "m-4" }),
    ];
    const a = autoCluster(input);
    const b = autoCluster(input);
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});
