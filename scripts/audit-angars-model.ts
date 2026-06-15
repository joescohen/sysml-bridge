/**
 * audit-angars-model.ts
 *
 * Real-model Gate 1 audit runner.
 *
 * Loads the ANGARS C&C model from the FileStore and runs the production
 * audit() pipeline against the real extracted.json corpus, writes Gate 1
 * artifacts to examples/angars/audits/, and prints a structured console
 * summary with per-ruleId finding counts and matrix/fidelity bucket totals.
 *
 * NOTE: The model was generated with a legacy generator that uses Definition
 * operands for trace relationships. R4-def-operand WILL fire as errors — that
 * is the correct, expected keystone behavior (decision A3, 05-RESEARCH.md
 * Pattern 2). Do not treat these as tool failures.
 *
 * Usage:
 *   pnpm tsx scripts/audit-angars-model.ts
 *
 * Env overrides:
 *   SYSML_BRIDGE_MODEL_DIR  — path to .store directory (default: examples/angars/model/.store)
 */

import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { FileStore } from "../packages/mcp-server/src/file-store.js";
import {
  audit,
  type Finding,
} from "../packages/mcp-server/src/audit/index.js";
import { loadCorpus } from "../packages/mcp-server/src/audit/corpus.js";
import { writeReports } from "../packages/mcp-server/src/audit/report.js";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..");

const MODEL_DIR =
  process.env.SYSML_BRIDGE_MODEL_DIR ??
  path.join(REPO_ROOT, "examples/angars/model/.store");

const CORPUS = path.join(REPO_ROOT, "examples/angars/model/extracted.json");
const AUDITS_DIR = path.join(REPO_ROOT, "examples/angars/audits");

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  // ── Load the real ANGARS C&C store ────────────────────────────────────────
  const store = new FileStore(MODEL_DIR);
  const projects = await store.listProjects();

  if (projects.length === 0) {
    console.error(
      "ERROR: No projects found in store. Run pnpm tsx scripts/generate-cc-model.ts first."
    );
    process.exit(1);
  }

  await store.loadProject(projects[0]["@id"]);

  const elements = await store.queryElements();
  const relationships = await store.queryRelationships();

  // ── Load corpus — fail loudly on malformed input (not cached) ────────────
  const corpus = await loadCorpus(CORPUS);

  // ── Run the full audit pipeline ───────────────────────────────────────────
  const result = audit(elements, relationships, corpus);

  // ── Write artifact reports ────────────────────────────────────────────────
  const { matrixPath, fidelityPath } = await writeReports(
    AUDITS_DIR,
    result.matrix,
    result.fidelity
  );

  // ── Console summary ───────────────────────────────────────────────────────
  printSummary(result.findings, result.matrix, result.fidelity, matrixPath, fidelityPath);
}

// ---------------------------------------------------------------------------
// printSummary — greppable, per-ruleId counts
// ---------------------------------------------------------------------------

function printSummary(
  findings: Finding[],
  matrix: ReturnType<typeof audit>["matrix"],
  fidelity: ReturnType<typeof audit>["fidelity"],
  matrixPath: string,
  fidelityPath: string
): void {
  // ── Per-ruleId counts ─────────────────────────────────────────────────────
  const ruleCounts = new Map<string, { severity: string; count: number }>();
  for (const f of findings) {
    const existing = ruleCounts.get(f.ruleId);
    if (existing) {
      existing.count++;
    } else {
      ruleCounts.set(f.ruleId, { severity: f.severity, count: 1 });
    }
  }

  console.log("\n=== Gate 1 Audit — ANGARS C&C Model ===\n");

  if (ruleCounts.size === 0) {
    console.log("  (no findings)");
  } else {
    // Sort by severity: error > warning > info
    const severityOrder = { error: 0, warning: 1, info: 2 };
    const sorted = [...ruleCounts.entries()].sort(([, a], [, b]) => {
      const sev =
        (severityOrder[a.severity as keyof typeof severityOrder] ?? 99) -
        (severityOrder[b.severity as keyof typeof severityOrder] ?? 99);
      if (sev !== 0) return sev;
      return a.count - b.count;
    });
    for (const [ruleId, { severity, count }] of sorted) {
      console.log(`  ${severity.padEnd(7)}  ${ruleId}  x${count}`);
    }
  }

  // ── Fidelity bucket sizes ─────────────────────────────────────────────────
  console.log(
    `\ndrops=${fidelity.drops.length} fabrications=${fidelity.fabrications.length} nearMatches=${fidelity.nearMatches.length}`
  );

  // ── Matrix totals ─────────────────────────────────────────────────────────
  const total = matrix.length;
  const satisfied = matrix.filter((r) => r.satisfied).length;
  const verified = matrix.filter((r) => r.verified).length;
  const derived = matrix.filter((r) => r.derived).length;
  console.log(
    `matrix: ${total} reqs, satisfied=${satisfied} verified=${verified} derived=${derived}`
  );

  // ── Report paths ──────────────────────────────────────────────────────────
  console.log(`\nReports written:`);
  console.log(`  ${matrixPath}`);
  console.log(`  ${fidelityPath}`);

  // ── Keystone note (decision A3) ───────────────────────────────────────────
  console.log(
    "\nR4 errors above are expected on the legacy-generated model until the generator emits usage operands (see 05-RESEARCH.md Pattern 2)."
  );
}

main().catch((err) => {
  console.error("FATAL:", err);
  process.exit(1);
});
