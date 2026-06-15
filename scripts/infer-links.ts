#!/usr/bin/env tsx
/**
 * infer-links.ts — CLI driver for the F8 inference / extrapolation engine.
 *
 * Composes the three-layer IR, runs the inference engine (type gate → propose →
 * band route → debate), and writes inference-candidates.json.
 *
 * Usage:
 *   pnpm tsx scripts/infer-links.ts [--dry-run] [--out <path>] [--help]
 *
 * Options:
 *   --dry-run         Generate + type gate only; skip LLM calls (no API key required).
 *                     Prints candidate counts per family + type-gate rejection counts.
 *   --out <path>      Output path for inference-candidates.json
 *                     Default: examples/angars/model/inference-candidates.json
 *   --force           Equivalent to INFER_FORCE=1 — override sentinel check
 *   --help            Show this message
 *
 * Environment:
 *   ANTHROPIC_API_KEY  Required for live LLM runs (loaded from .env if present)
 *   INFER_MODEL        Override default model (claude-haiku-4-5-20251001)
 *   INFER_FORCE        Set to "1" to override sentinel and re-run even if IR unchanged
 *   INFER_BUDGET_USD   Abort before spend if estimated cost exceeds this value
 *
 * Outputs:
 *   inference-candidates.json — stage-annotated candidate records (gitignored, corpus-derived)
 *     includes: typed, rejected_type, dropped_unpremised, auto_rejected, debate, queued
 *
 * Idempotent: candidate ids are keyed on stableId(family+source+target);
 * candidates already approved/rejected in inferred-approved.json are skipped.
 *
 * DO NOT run a live LLM pass without an API key — the dry-run path is the
 * default when no key is present, per conductor instructions.
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { join, dirname } from "node:path";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { composeIR } from "../packages/ir/src/inferred-approved.js";
import {
  runInferenceEngine,
} from "../packages/inference/src/engine.js";
import { AnthropicInferenceProvider } from "../packages/inference/src/inference-provider.js";
import type { InferenceProvider } from "../packages/inference/src/inference-provider.js";
import type { ProposeResult } from "../packages/inference/src/types.js";

// ── Constants ─────────────────────────────────────────────────────────────────

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = join(__dirname, "..");

const EXTRACTED_PATH = join(REPO_ROOT, "examples/angars/model/extracted.json");
const PROSE_APPROVED_PATH = join(REPO_ROOT, "examples/angars/model/prose-approved-modes.json");
const INFERRED_APPROVED_PATH = join(REPO_ROOT, "examples/angars/model/inferred-approved.json");
const DEFAULT_OUT = join(REPO_ROOT, "examples/angars/model/inference-candidates.json");

// ── Zero-dependency .env loader (identical to ingest-prose.ts) ───────────────

function loadDotEnv(): void {
  const envPath = join(REPO_ROOT, ".env");
  if (!existsSync(envPath)) return;
  for (const raw of readFileSync(envPath, "utf8").split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq === -1) continue;
    const key = line.slice(0, eq).trim();
    if (!key || process.env[key] !== undefined) continue; // shell export wins
    let value = line.slice(eq + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    process.env[key] = value;
  }
}

// ── No-op provider for dry-run (no API key needed) ───────────────────────────

class NoOpInferenceProvider implements InferenceProvider {
  async propose(): Promise<ProposeResult> {
    return { kind: "declined" }; // never proposes — dry run only
  }
  async advocate(): Promise<{ score: number; summary: string }> {
    return { score: 0.5, summary: "dry-run" };
  }
  async challenge(): Promise<{ score: number; summary: string }> {
    return { score: 0.5, summary: "dry-run" };
  }
}

// ── CLI arg parsing ───────────────────────────────────────────────────────────

function parseArgs(argv: string[]): { outPath: string; dryRun: boolean; force: boolean } {
  let outPath = DEFAULT_OUT;
  let dryRun = false;
  let force = false;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!;
    if (arg === "--out" && argv[i + 1]) {
      outPath = argv[++i]!;
    } else if (arg === "--dry-run") {
      dryRun = true;
    } else if (arg === "--force") {
      force = true;
    } else if (arg === "--help") {
      console.log(`Usage: pnpm tsx scripts/infer-links.ts [--dry-run] [--out <path>] [--force] [--help]`);
      console.log(`  --dry-run     Generate + type gate only; skip all LLM calls`);
      console.log(`  --out <path>  Output path (default: examples/angars/model/inference-candidates.json)`);
      console.log(`  --force       Override sentinel check (re-run even if IR unchanged)`);
      process.exit(0);
    }
  }

  return { outPath, dryRun, force };
}

// ── Candidate file format ────────────────────────────────────────────────────

interface CandidatesFile {
  generatedAt: string;
  irHash: string;
  stats: Array<{
    family: string;
    generated: number;
    rejectedType: number;
    proposed: number;
    droppedUnpremised: number;
    autoRejected: number;
    debate: number;
    queued: number;
  }>;
  counts: {
    total: number;
    byStage: Record<string, number>;
    byFamily: Record<string, number>;
  };
  records: unknown[];
}

// ── Main ──────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  loadDotEnv(); // load ANTHROPIC_API_KEY + INFER_MODEL from .env if present

  const { outPath, dryRun, force } = parseArgs(process.argv.slice(2));

  if (force) {
    process.env["INFER_FORCE"] = "1";
  }

  // ── Verify required files ─────────────────────────────────────────────────
  if (!existsSync(EXTRACTED_PATH)) {
    console.error(`ERROR: extracted.json not found at ${EXTRACTED_PATH}`);
    console.error("Run pnpm tsx scripts/extract-angars.ts first.");
    process.exit(1);
  }

  // ── Compose the three-layer IR ────────────────────────────────────────────
  const proseApprovedPath = existsSync(PROSE_APPROVED_PATH) ? PROSE_APPROVED_PATH : undefined;
  const inferredApprovedPath = existsSync(INFERRED_APPROVED_PATH) ? INFERRED_APPROVED_PATH : undefined;

  process.stderr.write(`[infer-links] Composing IR...\n`);
  process.stderr.write(`  extracted:        ${EXTRACTED_PATH}\n`);
  process.stderr.write(`  prose-approved:   ${proseApprovedPath ?? "(none)"}\n`);
  process.stderr.write(`  inferred-approved:${inferredApprovedPath ?? "(none)"}\n`);

  const ir = await composeIR(EXTRACTED_PATH, proseApprovedPath, undefined, inferredApprovedPath);

  process.stderr.write(`[infer-links] Composed IR: ${ir.extracted.functions?.length ?? 0} functions, ${ir.extracted.components?.length ?? 0} components, ${ir.extracted.n2Interfaces?.length ?? 0} N2 flows, ${ir.proseEntries.length} prose entries\n`);

  // ── Select provider ────────────────────────────────────────────────────────
  const hasApiKey = Boolean(process.env["ANTHROPIC_API_KEY"]);
  const effectiveDryRun = dryRun || !hasApiKey;

  if (!hasApiKey && !dryRun) {
    process.stderr.write(`[infer-links] WARNING: No ANTHROPIC_API_KEY found — running in dry-run mode (type gate + candidate generation only, no LLM calls).\n`);
    process.stderr.write(`[infer-links] Set ANTHROPIC_API_KEY in .env or environment to run the full pipeline.\n`);
  }

  const provider: InferenceProvider = effectiveDryRun
    ? new NoOpInferenceProvider()
    : new AnthropicInferenceProvider();

  process.stderr.write(`[infer-links] Mode: ${effectiveDryRun ? "DRY RUN (type gate + generation only)" : "LIVE (LLM propose + debate)"}\n`);

  // ── Run the engine ─────────────────────────────────────────────────────────
  const result = await runInferenceEngine(ir, provider, {
    dryRun: effectiveDryRun,
    log: (msg) => process.stderr.write(msg + "\n"),
  });

  if (result.skippedSentinel) {
    process.stderr.write(`[infer-links] Sentinel match — no run needed. Use --force to override.\n`);
    process.exit(0);
  }

  // ── Print summary ──────────────────────────────────────────────────────────
  process.stderr.write(`\n[infer-links] ── RESULTS ──\n`);
  process.stderr.write(`IR hash: ${result.irHash}\n`);

  // Counts per family and stage
  const byFamily: Record<string, number> = {};
  const byStage: Record<string, number> = {};
  for (const record of result.records) {
    const r = record as { stage: string; relationFamily?: string };
    byStage[r.stage] = (byStage[r.stage] ?? 0) + 1;
    if (r.relationFamily) {
      byFamily[r.relationFamily] = (byFamily[r.relationFamily] ?? 0) + 1;
    }
  }

  // Per-family stats table
  process.stderr.write(`\nFamily          │ Generated │ RejUnbnd │ RejCap │ RejType │ Proposed │ Declined │ ParseErr │ Repaired │ DrpUnprem │ AutoRej │ Debate │ Queued\n`);
  process.stderr.write(`────────────────┼───────────┼──────────┼────────┼─────────┼──────────┼──────────┼──────────┼──────────┼───────────┼─────────┼────────┼───────\n`);
  for (const st of result.stats) {
    const f = st.family.padEnd(15);
    process.stderr.write(`${f} │ ${String(st.generated).padStart(9)} │ ${String(st.rejectedUnbounded).padStart(8)} │ ${String(st.rejectedCapped).padStart(6)} │ ${String(st.rejectedType).padStart(7)} │ ${String(st.proposed).padStart(8)} │ ${String(st.proposalDeclined).padStart(8)} │ ${String(st.proposalParseError).padStart(8)} │ ${String(st.premiseRepaired).padStart(8)} │ ${String(st.droppedUnpremised).padStart(9)} │ ${String(st.autoRejected).padStart(7)} │ ${String(st.debate).padStart(6)} │ ${String(st.queued).padStart(6)}\n`);
  }
  process.stderr.write(`\n`);
  process.stderr.write(`Total records:     ${result.records.length}\n`);
  process.stderr.write(`Dropped unpremised: ${result.droppedUnpremised} (emittedUnpremised=${result.emittedUnpremised})\n`);
  if (!effectiveDryRun) {
    process.stderr.write(`Estimated cost:    $${result.estimatedCostUsd.toFixed(4)} USD\n`);
  }

  // ── Write output file ──────────────────────────────────────────────────────
  const outDir = dirname(outPath);
  await mkdir(outDir, { recursive: true });

  const outputFile: CandidatesFile = {
    generatedAt: new Date().toISOString(),
    irHash: result.irHash,
    stats: result.stats,
    counts: {
      total: result.records.length,
      byStage,
      byFamily,
    },
    records: result.records,
  };

  await writeFile(outPath, JSON.stringify(outputFile, null, 2) + "\n", "utf8");
  process.stderr.write(`[infer-links] Written: ${outPath}\n`);

  // Dry-run final count summary (for conductor verification)
  if (effectiveDryRun) {
    process.stdout.write(`\nDRY-RUN CANDIDATE COUNTS (relevance filter + cap + type gate applied):\n`);
    for (const st of result.stats) {
      const passed = st.generated - st.rejectedUnbounded - st.rejectedCapped - st.rejectedType;
      process.stdout.write(`  ${st.family.padEnd(16)}: generated=${st.generated}, rejectedUnbounded=${st.rejectedUnbounded}, rejectedCapped=${st.rejectedCapped}, rejectedType=${st.rejectedType}, passed=${passed}\n`);
    }
    process.stdout.write(`\n`);
  }
}

main().catch((err) => {
  console.error(`[infer-links] FATAL:`, err);
  process.exit(1);
});
