/**
 * verbatim.test.ts — the canonical verbatim-match chokepoint.
 *
 * Locks the normalization contract used by BOTH the ingest pipeline (C6 drop)
 * and the gate (PROSE-unverbatim-quote): whitespace collapse + unicode
 * quote/dash folding, CASE-SENSITIVE otherwise, and a non-vacuous empty-quote.
 */

import { describe, it, expect } from "vitest";
import { normalizeForVerbatim, quoteOccursInChunk } from "../verbatim.js";

describe("normalizeForVerbatim", () => {
  it("collapses all whitespace runs to a single space and trims", () => {
    expect(normalizeForVerbatim("  a\t b\n\n  c  ")).toBe("a b c");
  });

  it("folds smart quotes and dashes to ASCII", () => {
    expect(normalizeForVerbatim("‘a’ “b” ‚c‛ „d‟ e–f e—g e‑h −i")).toBe(
      "'a' \"b\" 'c' \"d\" e-f e-g e-h -i"
    );
  });

  it("preserves case (case-sensitive)", () => {
    expect(normalizeForVerbatim("The System")).toBe("The System");
    expect(normalizeForVerbatim("The System")).not.toBe("the system");
  });

  it("folds NBSP (via NFKC) to a normal space", () => {
    expect(normalizeForVerbatim("a b")).toBe("a b");
  });
});

describe("quoteOccursInChunk", () => {
  const chunk = "The system shall refuel within sixty seconds of contact.";

  it("matches an exact substring", () => {
    expect(quoteOccursInChunk("shall refuel within sixty seconds", chunk)).toBe(true);
  });

  it("matches across whitespace/dash/quote differences", () => {
    expect(quoteOccursInChunk("shall   refuel\nwithin sixty seconds", chunk)).toBe(true);
  });

  it("rejects a mutated quote", () => {
    expect(quoteOccursInChunk("shall refuel within thirty seconds", chunk)).toBe(false);
  });

  it("rejects on case difference (case-sensitive)", () => {
    expect(quoteOccursInChunk("SHALL REFUEL", chunk)).toBe(false);
  });

  it("rejects an empty / whitespace-only quote (non-vacuous)", () => {
    expect(quoteOccursInChunk("", chunk)).toBe(false);
    expect(quoteOccursInChunk("   \n ", chunk)).toBe(false);
  });
});
