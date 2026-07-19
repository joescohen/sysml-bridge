/**
 * persistence.test.ts — mentions.json round-trip + malformed-store throws.
 *
 * Mirrors `../../chunk-store/__tests__/persistence.test.ts` exactly for the
 * store-plumbing shape: envelope schema tag, byte-stable serialize for a
 * fixed timestamp, write → load → deep-equal round-trip, and loud throws on
 * a malformed file (never a silent empty-store degrade).
 *
 * Claim closed (W0 done-criteria 4): mentions.json round-trips; malformed
 * store throws.
 */

import { describe, it, expect } from "vitest";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  MENTION_STORE_SCHEMA,
  serializeMentionStore,
  parseMentionStore,
  writeMentionStoreFile,
  loadMentionStoreFile,
  type MentionRecord,
} from "../index.js";

const RECORDS: MentionRecord[] = [
  {
    mentionId: "mention-aaaaaaaaaaaaaaaa",
    surfaceForm: "Fuel Control Module",
    kindHint: "component",
    citation: {
      docId: "doc-1",
      docSha256: "a".repeat(64),
      chunkId: "chunk-a",
      sectionPath: "root/1",
      quote: "The Fuel Control Module shall command the boom.",
    },
    confidence: 0.8,
  },
  {
    mentionId: "mention-bbbbbbbbbbbbbbbb",
    surfaceForm: "Standby",
    kindHint: "mode",
    citation: {
      docId: "doc-1",
      docSha256: "a".repeat(64),
      chunkId: "chunk-b",
      sectionPath: "root/2",
      quote: "Standby mode transitions to Active mode on power-on.",
    },
    confidence: 0.6,
  },
];

const FIXED_TS = new Date("2026-07-14T00:00:00.000Z");

describe("mention-store — round trip", () => {
  it("write → load → deep-equal", async () => {
    const dir = await mkdtemp(join(tmpdir(), "mention-store-"));
    const filePath = join(dir, "mentions.json");
    await writeMentionStoreFile(filePath, RECORDS, FIXED_TS);

    const reloaded = await loadMentionStoreFile(filePath);
    expect(reloaded).toEqual(RECORDS);
  });

  it("the on-disk file carries the schema envelope", async () => {
    const dir = await mkdtemp(join(tmpdir(), "mention-store-"));
    const filePath = join(dir, "mentions.json");
    await writeMentionStoreFile(filePath, RECORDS, FIXED_TS);

    const raw = await readFile(filePath, "utf8");
    const parsed = JSON.parse(raw);
    expect(parsed.schema).toBe(MENTION_STORE_SCHEMA);
    expect(Array.isArray(parsed.mentions)).toBe(true);
    expect(parsed.generatedAt).toBe(FIXED_TS.toISOString());
  });

  it("serialize→parse is a pure identity on the records", () => {
    expect(parseMentionStore(serializeMentionStore(RECORDS, FIXED_TS))).toEqual(RECORDS);
  });

  it("serialization is byte-stable for a fixed generatedAt", () => {
    const a = serializeMentionStore(RECORDS, FIXED_TS);
    const b = serializeMentionStore(RECORDS, FIXED_TS);
    expect(a).toBe(b);
  });
});

describe("mention-store — malformed store throws loudly", () => {
  it("rejects invalid JSON", () => {
    expect(() => parseMentionStore("{ not json")).toThrow();
  });

  it("rejects the wrong schema tag", () => {
    expect(() =>
      parseMentionStore(JSON.stringify({ schema: "wrong", mentions: [] })),
    ).toThrow(/unexpected schema/);
  });

  it("rejects a non-array mentions field", () => {
    expect(() =>
      parseMentionStore(JSON.stringify({ schema: MENTION_STORE_SCHEMA, mentions: "nope" })),
    ).toThrow(/must be an array/);
  });

  it("rejects a record missing a required string field", () => {
    expect(() =>
      parseMentionStore(
        JSON.stringify({
          schema: MENTION_STORE_SCHEMA,
          mentions: [{ mentionId: 1 }],
        }),
      ),
    ).toThrow(/must be a string/);
  });

  it("rejects a record with an invalid kindHint", () => {
    expect(() =>
      parseMentionStore(
        JSON.stringify({
          schema: MENTION_STORE_SCHEMA,
          mentions: [
            {
              mentionId: "m-1",
              surfaceForm: "X",
              kindHint: "not-a-real-kind",
              confidence: 0.5,
              citation: {
                docId: "d",
                docSha256: "a".repeat(64),
                chunkId: "c",
                sectionPath: "root",
                quote: "q",
              },
            },
          ],
        }),
      ),
    ).toThrow(/kindHint/);
  });

  it("rejects a record with a malformed citation", () => {
    expect(() =>
      parseMentionStore(
        JSON.stringify({
          schema: MENTION_STORE_SCHEMA,
          mentions: [
            {
              mentionId: "m-1",
              surfaceForm: "X",
              kindHint: "component",
              confidence: 0.5,
              citation: { docId: "d" }, // missing required citation fields
            },
          ],
        }),
      ),
    ).toThrow(/citation/);
  });
});
