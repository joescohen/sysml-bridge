/**
 * chunk-store — persist and reload the prose ingest chunk store.
 *
 * ONE representation flows through the whole system. The prose ingest pipeline
 * builds a `Map<chunkId, text>` (C4 citation resolution + C6 verbatim checking);
 * BM25 evidence retrieval wants `RetrievalChunk[]` ({ chunkId, sectionPath,
 * text }). Rather than keep two stores, we persist the RICHER record
 * (`RetrievalChunk`) to disk once and DERIVE the `Map<chunkId, text>` at the
 * boundary where the audit needs it:
 *
 *   ingest pipeline ─┐
 *   (chunkId → text) │   scripts/ingest-prose.ts
 *   ChunkContext ────┴──► writeChunkStoreFile(<out>/chunks.json)   ← RetrievalChunk[]
 *                                     │
 *                    loadChunkStoreFile(...) ──► RetrievalChunk[]
 *                                     │
 *              ┌──────────────────────┴───────────────────────┐
 *              ▼                                               ▼
 *   chunkStoreTextMap(records)                    runInferenceEngine({ chunkStore })
 *   → Map<chunkId,text>                           → Bm25Index (BM25 evidence)
 *   → ProseComposedIR.chunkStore                  premises citing retrieved chunks resolve
 *   → PROSE-unverbatim-quote at ERROR level
 *
 * File format (`chunks.json`) — a self-describing envelope:
 *   {
 *     "schema": "sysml-foundry/chunk-store@1",
 *     "generatedAt": "<ISO-8601>",
 *     "chunks": [ { "chunkId": "…", "sectionPath": "…", "text": "…" }, … ]
 *   }
 *
 * The record shape is exactly `RetrievalChunk`, so a loaded store is fed to the
 * inference engine's `chunkStore` option verbatim, and projected to a text map
 * for the gate. LEXICAL/plain data only — no retrieval CALLS happen here, so the
 * C5 no-retrieval ingest invariant is untouched (this module lives outside
 * prose/ and scripts/, and imports no vector/embedding machinery).
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";

import type { RetrievalChunk } from "../retrieval/bm25.js";

/**
 * The canonical persisted chunk record. Structurally identical to
 * `RetrievalChunk` (chunkId + sectionPath + text) — the single representation
 * that feeds both the audit (as a chunkId→text map) and BM25 retrieval.
 */
export type ChunkStoreRecord = RetrievalChunk;

/** Schema tag stamped into every chunks.json envelope. */
export const CHUNK_STORE_SCHEMA = "sysml-foundry/chunk-store@1";

/** The on-disk envelope wrapping the chunk records. */
export interface ChunkStoreFile {
  schema: typeof CHUNK_STORE_SCHEMA;
  generatedAt: string;
  chunks: ChunkStoreRecord[];
}

/**
 * Build persistable chunk records from a chunkId→text map plus the section path
 * they were chunked under. This is the boundary adapter: the pipeline's
 * `Map<chunkId,text>` (C4/C6 store) becomes the richer `RetrievalChunk[]` by
 * attaching the run's `sectionPath`. First-seen order is preserved; ids are
 * emitted sorted so the file is byte-stable across runs with the same content.
 */
export function chunkRecordsFromMap(
  chunkTextById: ReadonlyMap<string, string>,
  sectionPath: string,
): ChunkStoreRecord[] {
  return [...chunkTextById.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([chunkId, text]) => ({ chunkId, sectionPath, text }));
}

/**
 * Derive the `Map<chunkId, text>` the PROSE-unverbatim-quote audit consumes
 * (ProseComposedIR.chunkStore) from persisted records. Last-write-wins on a
 * duplicate id (records are normally unique; content-addressed).
 */
export function chunkStoreTextMap(
  records: readonly ChunkStoreRecord[],
): Map<string, string> {
  const map = new Map<string, string>();
  for (const r of records) map.set(r.chunkId, r.text);
  return map;
}

/** Validate a single decoded record, throwing on any missing/mistyped field. */
function assertRecord(value: unknown, index: number): ChunkStoreRecord {
  if (typeof value !== "object" || value === null) {
    throw new Error(`chunk-store: chunks[${index}] is not an object`);
  }
  const rec = value as Record<string, unknown>;
  for (const key of ["chunkId", "sectionPath", "text"] as const) {
    if (typeof rec[key] !== "string") {
      throw new Error(
        `chunk-store: chunks[${index}].${key} must be a string (got ${typeof rec[key]})`,
      );
    }
  }
  return {
    chunkId: rec["chunkId"] as string,
    sectionPath: rec["sectionPath"] as string,
    text: rec["text"] as string,
  };
}

/**
 * Serialize chunk records to the on-disk envelope JSON string.
 *
 * @param generatedAt Override the timestamp (tests pass a fixed value for
 *                    byte-stable fixtures). Defaults to `new Date()`.
 */
export function serializeChunkStore(
  records: readonly ChunkStoreRecord[],
  generatedAt: Date = new Date(),
): string {
  const file: ChunkStoreFile = {
    schema: CHUNK_STORE_SCHEMA,
    generatedAt: generatedAt.toISOString(),
    chunks: records.map((r) => ({
      chunkId: r.chunkId,
      sectionPath: r.sectionPath,
      text: r.text,
    })),
  };
  return `${JSON.stringify(file, null, 2)}\n`;
}

/**
 * Parse a chunks.json string into validated records. Throws on a malformed
 * envelope or record — a corrupt chunk store must fail loudly, never silently
 * yield an empty store that would let the audit degrade to a vacuous pass.
 */
export function parseChunkStore(json: string): ChunkStoreRecord[] {
  let decoded: unknown;
  try {
    decoded = JSON.parse(json);
  } catch (err) {
    throw new Error(
      `chunk-store: file is not valid JSON: ${err instanceof Error ? err.message : String(err)}`,
    );
  }
  if (typeof decoded !== "object" || decoded === null) {
    throw new Error("chunk-store: top-level value is not an object");
  }
  const env = decoded as Record<string, unknown>;
  if (env["schema"] !== CHUNK_STORE_SCHEMA) {
    throw new Error(
      `chunk-store: unexpected schema '${String(env["schema"])}' (want '${CHUNK_STORE_SCHEMA}')`,
    );
  }
  if (!Array.isArray(env["chunks"])) {
    throw new Error("chunk-store: 'chunks' must be an array");
  }
  return env["chunks"].map((c, i) => assertRecord(c, i));
}

/**
 * Write chunk records to `filePath` (creating parent dirs). Byte-stable content
 * for a fixed `generatedAt`.
 */
export async function writeChunkStoreFile(
  filePath: string,
  records: readonly ChunkStoreRecord[],
  generatedAt?: Date,
): Promise<void> {
  await mkdir(dirname(filePath), { recursive: true });
  await writeFile(filePath, serializeChunkStore(records, generatedAt), "utf8");
}

/**
 * Load and validate a chunks.json file into `RetrievalChunk[]` — feed directly
 * to `runInferenceEngine({ chunkStore })`, or to `chunkStoreTextMap` for the
 * PROSE-unverbatim-quote audit.
 */
export async function loadChunkStoreFile(
  filePath: string,
): Promise<ChunkStoreRecord[]> {
  const json = await readFile(filePath, "utf8");
  return parseChunkStore(json);
}
