/**
 * trace-compare.ts
 *
 * Pure traceability diff logic — testable core for the IEEE 15288 §6.3.3
 * traceability fidelity comparator.
 *
 * Exported for use by:
 *   - scripts/traceability-compare.ts  (the runnable CLI script)
 *   - src/__tests__/traceability-compare.test.ts  (vitest unit tests)
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TracePair {
  reqId: string;
  functionId: string;
}

export interface CompareResult {
  present: TracePair[];
  missing: TracePair[];
  unsupported: TracePair[];
  fidelityPct: number;
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/**
 * Normalize a function id: trim whitespace; if the string contains a colon,
 * take the token before the first colon (e.g. "F1.1: Receive..." → "F1.1").
 */
export function normalizeFunctionId(raw: string): string {
  const trimmed = raw.trim();
  const colonIdx = trimmed.indexOf(":");
  return colonIdx >= 0 ? trimmed.slice(0, colonIdx).trim() : trimmed;
}

function pairKey(p: TracePair): string {
  return `${p.reqId}::${normalizeFunctionId(p.functionId)}`;
}

// ---------------------------------------------------------------------------
// Pure diff function
// ---------------------------------------------------------------------------

/**
 * Compare authoritative trace pairs against generated trace pairs.
 *
 * @param authoritative  Pairs from the human-authored "Satisfied By" sheet
 * @param generated      Pairs extracted from the generated SysML v2 model
 * @returns              { present, missing, unsupported, fidelityPct }
 *
 * Semantics:
 *   present     — links in BOTH (faithfully carried by the pipeline)
 *   missing     — links in authoritative but NOT in generated (DROPPED — pipeline defect)
 *   unsupported — links in generated but NOT in authoritative (FABRICATED — pipeline defect)
 *   fidelityPct — round(present / authoritative * 100), 100 when authoritative is empty
 */
export function compareTrace(
  authoritative: TracePair[],
  generated: TracePair[]
): CompareResult {
  const authKeys = new Map<string, TracePair>();
  for (const p of authoritative) {
    authKeys.set(pairKey(p), p);
  }

  const genKeys = new Map<string, TracePair>();
  for (const p of generated) {
    genKeys.set(pairKey(p), p);
  }

  const present: TracePair[] = [];
  const missing: TracePair[] = [];
  for (const [key, pair] of authKeys) {
    if (genKeys.has(key)) {
      present.push(pair);
    } else {
      missing.push(pair);
    }
  }

  const unsupported: TracePair[] = [];
  for (const [key, pair] of genKeys) {
    if (!authKeys.has(key)) {
      unsupported.push(pair);
    }
  }

  const fidelityPct =
    authoritative.length === 0
      ? 100
      : Math.round((present.length / authoritative.length) * 100);

  return { present, missing, unsupported, fidelityPct };
}
