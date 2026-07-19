/**
 * store.test.ts — entities.json round-trip + throw-on-malformed (spec §8 W1),
 * mirroring the chunk-store / mention-store persistence discipline.
 */

import { describe, it, expect } from "vitest";
import { promises as fsp } from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import {
  ENTITY_STORE_SCHEMA,
  serializeEntityStore,
  parseEntityStore,
  writeEntityStoreFile,
  loadEntityStoreFile,
} from "../store.js";
import type { EntityRecord } from "../cluster.js";

const FIXED = new Date("2026-07-14T00:00:00.000Z");

const RECORDS: EntityRecord[] = [
  {
    entityId: "entity-abc123",
    kind: "component",
    canonicalName: "Fuel Control Module",
    aliases: ["Fuel Control Module", "FCM"],
    mentionIds: ["mention-1", "mention-2"],
    mergeDispositions: ["entity-merge-xyz"],
  },
  {
    entityId: "entity-def456",
    kind: "mode",
    canonicalName: "Standby",
    aliases: ["Standby"],
    mentionIds: ["mention-3"],
    mergeDispositions: [],
  },
];

describe("entity store — round-trip", () => {
  it("write → load → deep-equal", async () => {
    const dir = await fsp.mkdtemp(path.join(os.tmpdir(), "entity-store-"));
    const file = path.join(dir, "entities.json");
    await writeEntityStoreFile(file, RECORDS, FIXED);
    const loaded = await loadEntityStoreFile(file);
    expect(loaded).toEqual(RECORDS);
    await fsp.rm(dir, { recursive: true, force: true });
  });

  it("serialization is byte-stable for a fixed generatedAt", () => {
    expect(serializeEntityStore(RECORDS, FIXED)).toBe(serializeEntityStore(RECORDS, FIXED));
    expect(serializeEntityStore(RECORDS, FIXED)).toContain(ENTITY_STORE_SCHEMA);
  });
});

describe("entity store — throw on malformed (never a vacuous empty store)", () => {
  it("invalid JSON throws", () => {
    expect(() => parseEntityStore("{ not json")).toThrow(/not valid JSON/);
  });

  it("wrong schema throws", () => {
    const bad = JSON.stringify({ schema: "sysml-foundry/wrong@9", generatedAt: "x", entities: [] });
    expect(() => parseEntityStore(bad)).toThrow(/unexpected schema/);
  });

  it("a bad field throws (kind out of the enum)", () => {
    const bad = JSON.stringify({
      schema: ENTITY_STORE_SCHEMA,
      generatedAt: "x",
      entities: [
        {
          entityId: "e1",
          kind: "gadget",
          canonicalName: "n",
          aliases: [],
          mentionIds: [],
          mergeDispositions: [],
        },
      ],
    });
    expect(() => parseEntityStore(bad)).toThrow(/kind must be one of/);
  });

  it("a non-string alias throws", () => {
    const bad = JSON.stringify({
      schema: ENTITY_STORE_SCHEMA,
      generatedAt: "x",
      entities: [
        {
          entityId: "e1",
          kind: "component",
          canonicalName: "n",
          aliases: [42],
          mentionIds: [],
          mergeDispositions: [],
        },
      ],
    });
    expect(() => parseEntityStore(bad)).toThrow(/aliases\[0\] must be a string/);
  });
});
