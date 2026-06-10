/**
 * Corpus loader and GATE-03 resolution set builder.
 *
 * Loads and zod-validates examples/angars/model/extracted.json, then builds the
 * resolution set used by GATE-03 provenance-existence checks.
 *
 * Resolution set = IR entity id ∪ naturalKey ∪ name ∪ ALLOWLIST
 *   - id is the primary key (provenanceSourceId stores the IR id, not naturalKey)
 *   - naturalKey and name are defense-in-depth for future emitters
 *
 * ALLOWLIST (decision A4): six legitimate non-corpus provenance values found in
 * the live ANGARS store. These stay documented info-level conventions; no corpus
 * ids are invented for them.
 */

import { promises as fs } from "node:fs";
import * as path from "node:path";
import { ExtractedSchema, type Extracted } from "@sysml-bridge/ir";

export const ALLOWLIST = new Set([
  "model-asserted",
  "C&C",
  "Demonstration",
  "Test",
  "Analysis",
  "Inspection",
]);

/**
 * Load and zod-validate an extracted.json corpus file.
 * Throws (ZodError or SyntaxError) on malformed input — fail fast before any
 * rule consumes the corpus.
 */
export async function loadCorpus(extractedPath: string): Promise<Extracted> {
  const raw = await fs.readFile(extractedPath, "utf8");
  return ExtractedSchema.parse(JSON.parse(raw)); // fail fast on malformed corpus
}

/**
 * Build the GATE-03 provenance resolution set from a loaded corpus.
 *
 * Seeds with ALLOWLIST, then for each supported entity kind adds:
 *   - e.id       (primary key — provenanceSourceId stores the IR id)
 *   - e.naturalKey (defense-in-depth; optional fields handled via ??)
 *   - e.name     (defense-in-depth; component name is a common provenance form)
 */
export function buildResolutionSet(c: Extracted): Set<string> {
  const s = new Set<string>(ALLOWLIST);
  const kinds = [
    "needs",
    "requirements",
    "functions",
    "components",
    "subsystems",
    "kpps",
    "behaviorDecomp",
  ] as const;
  for (const k of kinds) {
    for (const e of (c as any)[k] ?? []) {
      if (e.id) s.add(e.id);
      if (e.naturalKey) s.add(e.naturalKey);
      if (e.name) s.add(e.name);
    }
  }
  return s;
}

// Module-level cache keyed by resolved absolute path
const _cache = new Map<string, Extracted | null>();

/**
 * Load and cache corpus by resolved absolute path.
 * Returns null (and caches null) when the file is missing or fails zod.
 * Callers degrade gracefully on null (e.g. emit GATE03-corpus-unavailable finding).
 */
export async function loadCorpusCached(extractedPath: string): Promise<Extracted | null> {
  const resolved = path.resolve(extractedPath);
  if (_cache.has(resolved)) {
    return _cache.get(resolved) ?? null;
  }
  try {
    const result = await loadCorpus(resolved);
    _cache.set(resolved, result);
    return result;
  } catch {
    _cache.set(resolved, null);
    return null;
  }
}

/**
 * Clear the in-memory corpus cache. Intended for use in tests only.
 */
export function clearCorpusCache(): void {
  _cache.clear();
}
