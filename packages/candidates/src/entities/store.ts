/**
 * store.ts — persist and reload the entity store (entities.json).
 *
 * Mirrors ../chunk-store/index.ts and ../mentions/index.ts EXACTLY: a
 * self-describing envelope (`sysml-foundry/entity-store@1`), byte-stable for a
 * fixed `generatedAt`, and a THROW-ON-MALFORMED loader — a corrupt entity store
 * must fail loudly, never silently yield an empty store that would let the ENT-*
 * gate degrade to a vacuous pass. Sibling of chunks.json / mentions.json under
 * the same `<out>` directory. Pure data + fs only — no retrieval, no embeddings.
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";

import type { MentionKind } from "../mentions/index.js";
import type { EntityRecord } from "./cluster.js";

/** Schema tag stamped into every entities.json envelope. */
export const ENTITY_STORE_SCHEMA = "sysml-foundry/entity-store@1";

/** The on-disk envelope wrapping the entity records. */
export interface EntityStoreFile {
  schema: typeof ENTITY_STORE_SCHEMA;
  generatedAt: string;
  entities: EntityRecord[];
}

const ENTITY_KINDS: readonly MentionKind[] = [
  "component",
  "function",
  "requirement",
  "mode",
  "interface",
  "flow",
  "unknown",
];

function assertStringArray(value: unknown, index: number, field: string): string[] {
  if (!Array.isArray(value)) {
    throw new Error(`entity-store: entities[${index}].${field} must be an array`);
  }
  for (const [k, v] of value.entries()) {
    if (typeof v !== "string") {
      throw new Error(
        `entity-store: entities[${index}].${field}[${k}] must be a string (got ${typeof v})`
      );
    }
  }
  return value as string[];
}

/** Validate a single decoded record, throwing on any missing/mistyped field. */
function assertRecord(value: unknown, index: number): EntityRecord {
  if (typeof value !== "object" || value === null) {
    throw new Error(`entity-store: entities[${index}] is not an object`);
  }
  const rec = value as Record<string, unknown>;
  for (const key of ["entityId", "canonicalName"] as const) {
    if (typeof rec[key] !== "string") {
      throw new Error(
        `entity-store: entities[${index}].${key} must be a string (got ${typeof rec[key]})`
      );
    }
  }
  if (
    typeof rec["kind"] !== "string" ||
    !ENTITY_KINDS.includes(rec["kind"] as MentionKind)
  ) {
    throw new Error(
      `entity-store: entities[${index}].kind must be one of ${ENTITY_KINDS.join("|")} (got ${String(rec["kind"])})`
    );
  }
  return {
    entityId: rec["entityId"] as string,
    kind: rec["kind"] as MentionKind,
    canonicalName: rec["canonicalName"] as string,
    aliases: assertStringArray(rec["aliases"], index, "aliases"),
    mentionIds: assertStringArray(rec["mentionIds"], index, "mentionIds"),
    mergeDispositions: assertStringArray(rec["mergeDispositions"], index, "mergeDispositions"),
  };
}

/**
 * Serialize entity records to the on-disk envelope JSON string.
 *
 * @param generatedAt Override the timestamp (tests pass a fixed value for
 *                    byte-stable fixtures). Defaults to `new Date()`.
 */
export function serializeEntityStore(
  records: readonly EntityRecord[],
  generatedAt: Date = new Date()
): string {
  const file: EntityStoreFile = {
    schema: ENTITY_STORE_SCHEMA,
    generatedAt: generatedAt.toISOString(),
    entities: records.map((r) => ({
      entityId: r.entityId,
      kind: r.kind,
      canonicalName: r.canonicalName,
      aliases: [...r.aliases],
      mentionIds: [...r.mentionIds],
      mergeDispositions: [...r.mergeDispositions],
    })),
  };
  return `${JSON.stringify(file, null, 2)}\n`;
}

/**
 * Parse an entities.json string into validated records. Throws on a malformed
 * envelope or record.
 */
export function parseEntityStore(json: string): EntityRecord[] {
  let decoded: unknown;
  try {
    decoded = JSON.parse(json);
  } catch (err) {
    throw new Error(
      `entity-store: file is not valid JSON: ${err instanceof Error ? err.message : String(err)}`
    );
  }
  if (typeof decoded !== "object" || decoded === null) {
    throw new Error("entity-store: top-level value is not an object");
  }
  const env = decoded as Record<string, unknown>;
  if (env["schema"] !== ENTITY_STORE_SCHEMA) {
    throw new Error(
      `entity-store: unexpected schema '${String(env["schema"])}' (want '${ENTITY_STORE_SCHEMA}')`
    );
  }
  if (!Array.isArray(env["entities"])) {
    throw new Error("entity-store: 'entities' must be an array");
  }
  return env["entities"].map((e, i) => assertRecord(e, i));
}

/**
 * Write entity records to `filePath` (creating parent dirs). Byte-stable content
 * for a fixed `generatedAt`.
 */
export async function writeEntityStoreFile(
  filePath: string,
  records: readonly EntityRecord[],
  generatedAt?: Date
): Promise<void> {
  await mkdir(dirname(filePath), { recursive: true });
  await writeFile(filePath, serializeEntityStore(records, generatedAt), "utf8");
}

/**
 * Load and validate an entities.json file into `EntityRecord[]` — the input W2
 * cross-document enumeration consumes.
 */
export async function loadEntityStoreFile(filePath: string): Promise<EntityRecord[]> {
  const json = await readFile(filePath, "utf8");
  return parseEntityStore(json);
}
