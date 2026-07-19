/**
 * entities.test.ts — the ENT-* rule pack (W1). Each rule is shown to FIRE on its
 * defect AND to stay silent on the clean control (non-vacuous).
 */

import { describe, it, expect } from "vitest";
import { entityFindings, type EntityRecordLike } from "../entities.js";
import { audit } from "../index.js";

function ent(overrides: Partial<EntityRecordLike>): EntityRecordLike {
  return {
    entityId: "entity-1",
    kind: "component",
    canonicalName: "Fuel Control Module",
    aliases: ["Fuel Control Module"],
    mentionIds: ["m-1"],
    mergeDispositions: [],
    ...overrides,
  };
}

const ids = (fs: { ruleId: string }[]) => fs.map((f) => f.ruleId);

describe("entityFindings — clean control (non-vacuous)", () => {
  it("a pure auto-cluster entity (single normSurface, resolvable mentions) → no findings", () => {
    const findings = entityFindings(
      [ent({ aliases: ["Fuel Control Module", "fuel  control module"] })],
      new Set(["m-1"])
    );
    expect(findings).toEqual([]);
  });

  it("empty entity set → genuine no-op", () => {
    expect(entityFindings([], undefined)).toEqual([]);
  });
});

describe("ENT-unapproved-merge (error)", () => {
  it("fires when aliases span 2 normSurface groups with no disposition", () => {
    const findings = entityFindings(
      [ent({ aliases: ["Fuel Control Module", "FCM"], mergeDispositions: [] })],
      new Set(["m-1"])
    );
    const merge = findings.filter((f) => f.ruleId === "ENT-unapproved-merge");
    expect(merge).toHaveLength(1);
    expect(merge[0]!.severity).toBe("error");
    expect(merge[0]!.elementId).toBe("entity-1");
  });

  it("does NOT fire when the divergent group is covered by a disposition", () => {
    const findings = entityFindings(
      [ent({ aliases: ["Fuel Control Module", "FCM"], mergeDispositions: ["entity-merge-x"] })],
      new Set(["m-1"])
    );
    expect(ids(findings)).not.toContain("ENT-unapproved-merge");
  });
});

describe("ENT-dangling-mention-ref (error)", () => {
  it("fires when a referenced mentionId is absent from the store", () => {
    const findings = entityFindings(
      [ent({ mentionIds: ["m-1", "m-absent"] })],
      new Set(["m-1"])
    );
    const dangling = findings.filter((f) => f.ruleId === "ENT-dangling-mention-ref");
    expect(dangling).toHaveLength(1);
    expect(dangling[0]!.severity).toBe("error");
  });

  it("does NOT fire when every referenced mentionId resolves", () => {
    const findings = entityFindings([ent({ mentionIds: ["m-1"] })], new Set(["m-1"]));
    expect(ids(findings)).not.toContain("ENT-dangling-mention-ref");
  });
});

describe("ENT-mention-store-unavailable (degrade warning, never vacuous)", () => {
  it("warns instead of passing when the store is undefined but entities reference mentions", () => {
    const findings = entityFindings([ent({ mentionIds: ["m-1"] })], undefined);
    const degrade = findings.filter((f) => f.ruleId === "ENT-mention-store-unavailable");
    expect(degrade).toHaveLength(1);
    expect(degrade[0]!.severity).toBe("warning");
    // Crucially, dangling-ref is NOT emitted (cannot verify) and NOT vacuously passed.
    expect(ids(findings)).not.toContain("ENT-dangling-mention-ref");
  });
});

describe("ENT-duplicate-suspect (warning)", () => {
  it("fires for two same-kind entities whose canonical names auto-cluster-match", () => {
    const findings = entityFindings(
      [
        ent({ entityId: "e-a", canonicalName: "Fuel Control Module" }),
        ent({ entityId: "e-b", canonicalName: "fuel  control  module" }),
      ],
      new Set(["m-1"])
    );
    const dup = findings.filter((f) => f.ruleId === "ENT-duplicate-suspect");
    expect(dup).toHaveLength(1);
    expect(dup[0]!.severity).toBe("warning");
  });
});

describe("audit() wiring", () => {
  it("skips ENT-* entirely when no entity input is supplied", () => {
    const res = audit([], [], null);
    expect(ids(res.findings).filter((r) => r.startsWith("ENT-"))).toEqual([]);
  });

  it("runs ENT-* when an entity input is supplied", () => {
    const res = audit([], [], null, {
      entities: [ent({ aliases: ["Fuel Control Module", "FCM"] })],
      mentionIds: new Set(["m-1"]),
    });
    expect(ids(res.findings)).toContain("ENT-unapproved-merge");
  });
});
