/**
 * G-C / real-run — full pipeline over the ANGARS corpus.
 *
 * Skipped if corpus PDFs are absent (CI without corpus).
 *
 * Behaviour:
 *   - Always runs the DETERMINISTIC path (detectAndChunkRequirements).
 *   - Runs the LLM path only when ANTHROPIC_API_KEY is set.
 *   - When the key is absent, marks the LLM step as KEY-REQUIRED and does NOT
 *     fabricate LLM output.
 *
 * Claims tested:
 *   - Deterministic candidates emitted ≥ 1 per document
 *   - LLM processedChunks === totalChunks (if key present)
 *   - emittedUncited === 0 (C4, always)
 *   - All candidate IDs match prose-candidate-<16-hex> format
 *
 * RED: fails until ingest-pipeline.ts + parsers exist.
 */

import { describe, it, expect } from "vitest";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { parsePdf } from "../parsers/pdf.js";
import type { LlmProvider, CandidateProposal } from "../llm-provider.js";
import { runIngestPipeline } from "../ingest-pipeline.js";
import type { IngestPipelineResult } from "../ingest-pipeline.js";

// ── Corpus config ─────────────────────────────────────────────────────────────

const CORPUS_DIR =
  "/home/joescohen/Engineering/projects/sysml-bridge/examples/angars/corpus/specs";

const DOCS = [
  { file: "Appendix_B_ANGARS_RAR_CONOPS.pdf", docId: "angars-conops" },
  { file: "Appendix_C_ANGARS_FAR.pdf", docId: "angars-far" },
  { file: "Appendix_E_ANGARS_ConceptDesign.pdf", docId: "angars-concept-design" },
  { file: "Appendix_G_ANGARS_ASPEC.pdf", docId: "angars-aspec" },
];

const corpusPresent = DOCS.every((d) => existsSync(join(CORPUS_DIR, d.file)));

// ── Offline-safe mock provider ─────────────────────────────────────────────────

/**
 * Deterministic mock provider used when ANTHROPIC_API_KEY is absent.
 * Returns one valid proposal per chunk, never fabricates LLM output.
 */
class OfflineMockProvider implements LlmProvider {
  async propose(
    chunkId: string,
    _chunkText: string,
    _sectionContext: string,
  ): Promise<CandidateProposal[]> {
    // Single deterministic proposal that resolves to the actual chunk
    return [
      {
        kind: "requirement" as const,
        fields: { text: "mock requirement (KEY-REQUIRED: no LLM output)" },
        citedChunkId: chunkId,
        confidence: 0.0, // 0 confidence signals mock
        quote: "(KEY-REQUIRED)",
      },
    ];
  }
}

// ── Real Anthropic provider factory (lazy — only imported if key present) ──────

async function makeAnthropicProvider(): Promise<LlmProvider> {
  // Dynamic import so the module graph doesn't fail when the SDK is absent
  const { AnthropicLlmProvider } = await import("../llm-provider.js");
  return new AnthropicLlmProvider();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe.skipIf(!corpusPresent)("G-C / real-run — ANGARS corpus", () => {
  for (const { file, docId } of DOCS) {
    const filePath = join(CORPUS_DIR, file);
    const hasKey = Boolean(process.env["ANTHROPIC_API_KEY"]);

    it(`${file}: deterministic pipeline — ≥1 candidate, emittedUncited=0`, async () => {
      const raw = await readFile(filePath);
      const docSha256 = createHash("sha256").update(raw).digest("hex");
      const parsed = await parsePdf(filePath);

      // Offline mock for the deterministic path
      const provider = new OfflineMockProvider();

      const result: IngestPipelineResult = await runIngestPipeline({
        text: parsed.text,
        context: {
          documentHash: docSha256,
          sectionId: "sec-root",
          sectionPath: "root",
          pageStart: 0,
          pageEnd: parsed.pages.length - 1,
          documentId: docId,
        },
        provider,
        chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
      });

      // C4 gate: no uncited candidates emitted
      expect(result.emittedUncited).toBe(0);
      // Deterministic path must find at least one candidate
      expect(result.candidates.length).toBeGreaterThanOrEqual(1);
      // C5: every chunk processed
      expect(result.processedChunks).toBe(result.totalChunks);
      // All candidate IDs have the correct format
      for (const candidate of result.candidates) {
        expect(candidate.id).toMatch(/^prose-candidate-[0-9a-f]{16}$/);
      }
    }, 120_000);

    it.skipIf(!hasKey)(
      `${file}: LLM pipeline (ANTHROPIC_API_KEY present) — processedChunks===totalChunks, emittedUncited=0`,
      async () => {
        const raw = await readFile(filePath);
        const docSha256 = createHash("sha256").update(raw).digest("hex");
        const parsed = await parsePdf(filePath);
        const provider = await makeAnthropicProvider();

        const result: IngestPipelineResult = await runIngestPipeline({
          text: parsed.text,
          context: {
            documentHash: docSha256,
            sectionId: "sec-root",
            sectionPath: "root",
            pageStart: 0,
            pageEnd: parsed.pages.length - 1,
            documentId: docId,
          },
          provider,
          chunkOptions: { chunkSize: 1500, chunkOverlap: 150 },
        });

        // C4: no uncited candidates
        expect(result.emittedUncited).toBe(0);
        // C5: every chunk submitted exactly once
        expect(result.processedChunks).toBe(result.totalChunks);
        // All candidate IDs valid format
        for (const candidate of result.candidates) {
          expect(candidate.id).toMatch(/^prose-candidate-[0-9a-f]{16}$/);
        }
      },
      300_000, // 5-min timeout for real LLM calls
    );
  }
});
