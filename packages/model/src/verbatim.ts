/**
 * verbatim.ts — SEPAL-style verbatim citation-quote matching.
 *
 * A cited quote is TRUSTWORTHY only if it actually occurs in the text of the
 * chunk it cites. The LLM returns the `quote` field; a hallucinated quote that
 * points at a real chunk otherwise sails through the citation (C4) gate and is
 * what a human reads when approving. This module is the single canonical
 * chokepoint for deciding "does this quote verbatim-resolve into this chunk?".
 *
 * Both the pure ingest pipeline (packages/candidates) and the auditable gate
 * (packages/gates, PROSE-unverbatim-quote) call `quoteOccursInChunk` so the
 * emit-time drop and the after-the-fact re-check use IDENTICAL matching rules —
 * no drift between the two enforcement points.
 *
 * ── Normalization rules (deliberately narrow) ─────────────────────────────────
 * PDF text extraction inserts whitespace artifacts (soft line-wraps, page
 * gutters, non-breaking spaces) and substitutes typographic punctuation, which
 * would cause false rejections against a byte-exact match. Normalization is
 * therefore applied to BOTH the quote and the chunk before an `includes` test:
 *
 *   1. Unicode NFKC — folds compatibility forms (ligatures, full-width chars,
 *      NBSP→space) to a canonical shape.
 *   2. Smart quotes → ASCII: ‘ ’ ‚ ‛ ′ → ' ; “ ” „ ‟ ″ → " .
 *   3. Dashes → ASCII hyphen: ‐ ‑ ‒ – — ― and the minus sign − → - .
 *   4. Collapse ALL whitespace runs (spaces, tabs, newlines) to a single space,
 *      then trim.
 *
 * The match is CASE-SENSITIVE otherwise — only whitespace and the punctuation
 * classes above are normalized; letters are left untouched. An empty (or
 * whitespace-only) quote NEVER verbatim-resolves: `"".includes("")` is vacuously
 * true, so a candidate carrying no supporting span must be treated as unverified,
 * not silently accepted.
 */

/** Smart/curly quotes and primes that collapse to an ASCII single quote. */
const SINGLE_QUOTES = /[‘’‚‛′]/g;
/** Smart/curly double quotes and double prime that collapse to an ASCII quote. */
const DOUBLE_QUOTES = /[“”„‟″]/g;
/** Unicode dash family (hyphen through em/horizontal bar) and minus sign. */
const DASHES = /[‐‑‒–—―−]/g;
/** Any run of whitespace (incl. newlines/tabs/unicode spaces surviving NFKC). */
const WHITESPACE = /\s+/g;

/**
 * Normalize a string for verbatim comparison: NFKC, punctuation folding, and
 * whitespace collapse. Case-preserving.
 */
export function normalizeForVerbatim(s: string): string {
  return s
    .normalize("NFKC")
    .replace(SINGLE_QUOTES, "'")
    .replace(DOUBLE_QUOTES, '"')
    .replace(DASHES, "-")
    .replace(WHITESPACE, " ")
    .trim();
}

/**
 * Does `quote` verbatim-resolve into `chunkText` under the normalization rules?
 *
 * Returns false for an empty/whitespace-only quote (non-vacuous: a candidate
 * with no supporting span is NOT verified).
 */
export function quoteOccursInChunk(quote: string, chunkText: string): boolean {
  const nq = normalizeForVerbatim(quote);
  if (nq.length === 0) return false;
  return normalizeForVerbatim(chunkText).includes(nq);
}

/**
 * Normalize a surface form for cross-document IDENTITY and clustering: the
 * verbatim normalizer's rules (NFKC + quote/dash fold + whitespace collapse)
 * PLUS casefold, then collapse any remaining punctuation runs to a single space.
 * Pure, deterministic, order-independent.
 *
 * This is the single canonical mention/entity normalizer. It lives here beside
 * `normalizeForVerbatim` (whose building blocks it reuses, never weakens) so
 * BOTH the mention/entity layer (packages/candidates) and the ENT-* gate
 * (packages/gates) can share it — the two packages have no direct dependency on
 * each other, and duplicating the normalizer would let emit-time and audit-time
 * identity silently drift. Casefold is layered ON TOP of the case-preserving
 * verbatim matcher; it never mutates verbatim's own case sensitivity.
 */
export function normSurface(s: string): string {
  return normalizeForVerbatim(s)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}
