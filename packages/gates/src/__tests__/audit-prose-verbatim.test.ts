/**
 * PROSE-unverbatim-quote — audit re-check of approved prose entries' quotes.
 *
 * Claims:
 *   - An approved prose entry whose citation.quote does NOT occur in its cited
 *     chunk yields a PROSE-unverbatim-quote ERROR on the entry id.
 *   - An approved entry whose quote DOES occur (verbatim, incl. whitespace/dash
 *     normalization) yields NO finding.
 *   - Chunk store ABSENT with approved entries present → a
 *     PROSE-unverbatim-quote-unavailable WARNING (degrade path, never a vacuous
 *     pass — the fail-able control: this fails if the rule silently passes).
 *   - A chunkId present in the store but a quote mismatch is caught through the
 *     full audit() orchestrator (prose-layer wiring), and it is the sole error.
 */

import { describe, it, expect } from "vitest";
import type { Extracted, ProseApprovedEntry, ProseComposedIR } from "@sysml-bridge/model";
import { proseVerbatimFindings } from "../prose-verbatim.js";
import { audit } from "../index.js";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const CHUNK_ID = "chunk-verbatim-000000000000000000";
const CHUNK_TEXT =
  "The ANGARS system shall refuel the receiver aircraft autonomously within sixty seconds. " +
  "The pilot's console shows a real-time three-meter separation envelope.";

const EXTRACTED: Extracted = {
  schema_version: "1.0.0",
  subsystem: "test",
  needs: [],
  requirements: [],
  functions: [],
  components: [],
  satisfies: [],
  allocations: [],
};

function entry(overrides: Partial<ProseApprovedEntry> = {}): ProseApprovedEntry {
  return {
    id: "prose-entry-1",
    kind: "requirement",
    fields: { text: "x" },
    citation: {
      docId: "doc-1",
      docSha256: "a".repeat(64),
      chunkId: CHUNK_ID,
      sectionPath: "root",
      quote: "shall refuel the receiver aircraft autonomously",
    },
    approvedBy: "tester",
    approvedAt: "2026-01-01T00:00:00Z",
    candidateId: "cand-1",
    status: "approved",
    ...overrides,
  };
}

const STORE = new Map<string, string>([[CHUNK_ID, CHUNK_TEXT]]);

// ── proseVerbatimFindings (unit) ────────────────────────────────────────────────

describe("proseVerbatimFindings — verbatim re-check", () => {
  it("flags an approved entry whose quote is absent from its chunk (error)", () => {
    const findings = proseVerbatimFindings(
      [entry({ citation: { ...entry().citation, quote: "shall self-destruct on command" } })],
      STORE
    );
    expect(findings).toHaveLength(1);
    expect(findings[0]!.ruleId).toBe("PROSE-unverbatim-quote");
    expect(findings[0]!.severity).toBe("error");
    expect(findings[0]!.elementId).toBe("prose-entry-1");
  });

  it("passes an entry whose quote occurs verbatim (no finding)", () => {
    const findings = proseVerbatimFindings([entry()], STORE);
    expect(findings).toHaveLength(0);
  });

  it("tolerates whitespace / smart-quote / dash differences (no finding)", () => {
    const findings = proseVerbatimFindings(
      [
        entry({
          citation: {
            ...entry().citation,
            quote: "The pilot’s   console shows a real‑time  three—meter separation envelope.",
          },
        }),
      ],
      STORE
    );
    expect(findings).toHaveLength(0);
  });

  it("degrades to a warning (never a vacuous pass) when the chunk store is absent", () => {
    const findings = proseVerbatimFindings([entry()], undefined);
    expect(findings).toHaveLength(1);
    expect(findings[0]!.ruleId).toBe("PROSE-unverbatim-quote-unavailable");
    expect(findings[0]!.severity).toBe("warning");
  });

  it("only re-checks approved entries (suspect/superseded skipped)", () => {
    const findings = proseVerbatimFindings(
      [entry({ status: "suspect", citation: { ...entry().citation, quote: "absent quote" } })],
      STORE
    );
    expect(findings).toHaveLength(0);
  });
});

// ── audit() orchestrator wiring ─────────────────────────────────────────────────

describe("audit() — PROSE-unverbatim-quote through the prose-layer path", () => {
  it("emits the error as the sole error finding for a mismatched approved quote", () => {
    const composed: ProseComposedIR = {
      extracted: EXTRACTED,
      proseEntries: [
        entry({ citation: { ...entry().citation, quote: "a quote that is not in the chunk" } }),
      ],
      approvedProseIds: new Set(["prose-entry-1"]),
      chunkStore: STORE,
    };
    const { findings } = audit([], [], composed);
    const errors = findings.filter((f) => f.severity === "error");
    expect(errors).toHaveLength(1);
    expect(errors[0]!.ruleId).toBe("PROSE-unverbatim-quote");
    expect(errors[0]!.elementId).toBe("prose-entry-1");
  });

  it("emits no verbatim finding when the quote resolves", () => {
    const composed: ProseComposedIR = {
      extracted: EXTRACTED,
      proseEntries: [entry()],
      approvedProseIds: new Set(["prose-entry-1"]),
      chunkStore: STORE,
    };
    const { findings } = audit([], [], composed);
    expect(findings.some((f) => f.ruleId.startsWith("PROSE-unverbatim-quote"))).toBe(false);
  });
});
