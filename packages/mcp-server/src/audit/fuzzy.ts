/**
 * Fuzzy string matcher with token-set normalization for GATE-04 fidelity reconciliation.
 *
 * Key design choice: token-set normalization (sort tokens before distance) defeats
 * token reorder, which raw Levenshtein misscores catastrophically (e.g.
 * "Aircraft ID Verification" vs "ID Verification Aircraft" → raw 0.25, sorted → 1.0).
 *
 * Band constants (evidence-tuned, pinned by fuzzy-calibration.test.ts):
 *   confident >= 0.90
 *   review    >= 0.78
 *   unmatched  < 0.78
 */

import { distance } from "fastest-levenshtein";

/**
 * Normalize a string for fuzzy comparison:
 * 1. Lowercase + trim
 * 2. Replace & → and
 * 3. Strip non-alphanumeric chars to space
 * 4. Collapse whitespace
 * 5. Sort tokens (token-set: defeats word reorder)
 */
export function normalize(s: string): string {
  return s
    .toLowerCase()
    .trim()
    .replace(/&/g, "and")
    .replace(/[^a-z0-9\s]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .sort()
    .join(" ");
}

/**
 * Compute normalized similarity between two strings (0–1).
 * Uses Levenshtein edit distance on the token-set normalized forms.
 */
export function similarity(a: string, b: string): number {
  const na = normalize(a);
  const nb = normalize(b);
  const mx = Math.max(na.length, nb.length);
  return mx === 0 ? 1 : 1 - distance(na, nb) / mx;
}

export type Band = "confident" | "review" | "unmatched";

/**
 * Map a similarity score to a fidelity band.
 * Thresholds calibrated on real ANGARS name pairs (see 05-RESEARCH.md Fuzzy Calibration).
 */
export function band(sim: number): Band {
  if (sim >= 0.90) return "confident";
  if (sim >= 0.78) return "review";
  return "unmatched";
}
