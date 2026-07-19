/**
 * entity-approval-helpers.test.ts — the W1 entity-merge human gate.
 *
 * Claims closed:
 *   - appendEntityMerge writes a CONTENT-ADDRESSED disposition id (== the
 *     unordered pair key), append-only, independent of approvedBy/timestamp.
 *   - recordEntityRejection records the pair key idempotently; a rejected pair
 *     key is never re-added.
 *   - the merge disposition id is order-insensitive (merge(A,B) == merge(B,A)).
 *   - canonical-name / kind overrides are honored WITHOUT changing identity (§9).
 */

import { describe, it, expect } from "vitest";
import { promises as fsp } from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import {
  appendEntityMerge,
  recordEntityRejection,
  isEntityMergeApproved,
  isEntityMergeRejected,
  entityMergePairKey,
  type EntityMergeCandidate,
} from "../entity-approval-helpers.js";

function candidate(overrides: Partial<EntityMergeCandidate> = {}): EntityMergeCandidate {
  const entityIdA = overrides.entityIdA ?? "entity-aaa";
  const entityIdB = overrides.entityIdB ?? "entity-bbb";
  return {
    id: entityMergePairKey(entityIdA, entityIdB),
    entityIdA,
    entityIdB,
    kind: "component",
    canonicalName: "Fuel Control Module",
    aliases: ["Fuel Control Module", "FCM"],
    mentionIds: ["m-1", "m-2"],
    reason: "acronym",
    confidence: 0.6,
    ...overrides,
  };
}

async function tmpDir(): Promise<string> {
  return fsp.mkdtemp(path.join(os.tmpdir(), "entity-approve-"));
}

describe("appendEntityMerge — content-addressed, append-only", () => {
  it("derives the disposition id from the unordered entity-id pair", async () => {
    const dir = await tmpDir();
    const approved = path.join(dir, "entity-approved.json");
    const rejections = path.join(dir, "entity-rejections.json");
    const entry = await appendEntityMerge(candidate(), "human", approved, rejections);
    expect(entry.id).toBe(entityMergePairKey("entity-aaa", "entity-bbb"));
    expect(entry.candidateId).toBe(entry.id);
    expect(entry.status).toBe("approved");
    expect(entry.approvedBy).toBe("human");
    await fsp.rm(dir, { recursive: true, force: true });
  });

  it("the merge id is order-insensitive: merge(A,B) === merge(B,A)", () => {
    expect(entityMergePairKey("entity-aaa", "entity-bbb")).toBe(
      entityMergePairKey("entity-bbb", "entity-aaa")
    );
  });

  it("honors canonicalName / kind overrides without changing identity (§9)", async () => {
    const dir = await tmpDir();
    const approved = path.join(dir, "entity-approved.json");
    const rejections = path.join(dir, "entity-rejections.json");
    const entry = await appendEntityMerge(candidate(), "human", approved, rejections, {
      canonicalName: "Flight Control Module",
      kind: "function",
    });
    expect(entry.canonicalName).toBe("Flight Control Module");
    expect(entry.kind).toBe("function");
    expect(entry.id).toBe(entityMergePairKey("entity-aaa", "entity-bbb")); // unchanged
    await fsp.rm(dir, { recursive: true, force: true });
  });

  it("is append-only across two approvals", async () => {
    const dir = await tmpDir();
    const approved = path.join(dir, "entity-approved.json");
    const rejections = path.join(dir, "entity-rejections.json");
    await appendEntityMerge(candidate(), "h1", approved, rejections);
    await appendEntityMerge(
      candidate({ entityIdA: "entity-ccc", entityIdB: "entity-ddd" }),
      "h2",
      approved,
      rejections
    );
    const file = JSON.parse(await fsp.readFile(approved, "utf8"));
    expect(file.entries).toHaveLength(2);
    await fsp.rm(dir, { recursive: true, force: true });
  });
});

describe("recordEntityRejection — content-addressed pair key, idempotent", () => {
  it("records the pair key and never re-asks", async () => {
    const dir = await tmpDir();
    const rejections = path.join(dir, "entity-rejections.json");
    const pairKey = entityMergePairKey("entity-aaa", "entity-bbb");

    expect(await isEntityMergeRejected(pairKey, rejections)).toBe(false);
    await recordEntityRejection(pairKey, rejections);
    await recordEntityRejection(pairKey, rejections); // idempotent
    expect(await isEntityMergeRejected(pairKey, rejections)).toBe(true);

    const file = JSON.parse(await fsp.readFile(rejections, "utf8"));
    expect(file.rejectedPairKeys).toEqual([pairKey]);
    await fsp.rm(dir, { recursive: true, force: true });
  });
});

describe("skip predicates", () => {
  it("isEntityMergeApproved reflects an approved pair; false when file absent", async () => {
    const dir = await tmpDir();
    const approved = path.join(dir, "entity-approved.json");
    const rejections = path.join(dir, "entity-rejections.json");
    const pairKey = entityMergePairKey("entity-aaa", "entity-bbb");
    expect(await isEntityMergeApproved(pairKey, approved)).toBe(false);
    await appendEntityMerge(candidate(), "human", approved, rejections);
    expect(await isEntityMergeApproved(pairKey, approved)).toBe(true);
    await fsp.rm(dir, { recursive: true, force: true });
  });
});
