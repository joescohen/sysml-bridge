/**
 * Markdown report renderers and filesystem writer for Gate 1 audit artifacts.
 *
 * Exports:
 *   renderMatrixMarkdown(matrix)   — "# Traceability Coverage Matrix" table
 *   renderFidelityMarkdown(fidelity) — "# Corpus-Model Fidelity Report" with three sections
 *   writeReports(dir, matrix, fidelity) — mkdir + write both files, return paths
 *
 * The audits dir is gitignored (.gitignore:19) — overwrite-per-run is intentional.
 * No security validation needed on dir path (T-05-07 accepted: local-only write,
 * caller-supplied from a controlled env var default).
 */

import { promises as fs } from "node:fs";
import * as path from "node:path";
import type { MatrixRow, AuditResult } from "./findings.js";

// ---------------------------------------------------------------------------
// renderMatrixMarkdown
// ---------------------------------------------------------------------------

/**
 * Render the GATE-06 requirement coverage matrix as a markdown document.
 *
 * Format:
 *   # Traceability Coverage Matrix
 *   Generated: <ISO date>
 *   N requirements: X satisfied / Y verified / Z derived
 *   | Requirement | Satisfied | Verified | Derived |
 *   | ----------- | --------- | -------- | ------- |
 *   | <name>      | yes/no    | yes/no   | yes/no  |
 */
export function renderMatrixMarkdown(matrix: MatrixRow[]): string {
  const now = new Date().toISOString();
  const total = matrix.length;
  const satisfied = matrix.filter((r) => r.satisfied).length;
  const verified = matrix.filter((r) => r.verified).length;
  const derived = matrix.filter((r) => r.derived).length;

  const lines: string[] = [
    "# Traceability Coverage Matrix",
    "",
    `Generated: ${now}`,
    "",
    `${total} requirements: ${satisfied} satisfied / ${verified} verified / ${derived} derived`,
    "",
    "| Requirement | Satisfied | Verified | Derived |",
    "| ----------- | --------- | -------- | ------- |",
  ];

  for (const row of matrix) {
    const name = row.reqName ?? row.reqId;
    lines.push(
      `| ${name} | ${row.satisfied ? "yes" : "no"} | ${row.verified ? "yes" : "no"} | ${row.derived ? "yes" : "no"} |`
    );
  }

  return lines.join("\n") + "\n";
}

// ---------------------------------------------------------------------------
// renderFidelityMarkdown
// ---------------------------------------------------------------------------

/**
 * Render the GATE-04 fidelity three-bucket report as a markdown document.
 *
 * Format:
 *   # Corpus-Model Fidelity Report
 *   Generated: <ISO date>
 *
 *   ## Drops
 *   ## Fabrications
 *   ## Near-Matches
 *   (near-match rows show similarity to 3 decimals and band)
 */
export function renderFidelityMarkdown(
  fidelity: AuditResult["fidelity"]
): string {
  const now = new Date().toISOString();

  const lines: string[] = [
    "# Corpus-Model Fidelity Report",
    "",
    `Generated: ${now}`,
    "",
  ];

  // ── Drops section ────────────────────────────────────────────────────────
  lines.push("## Drops");
  lines.push("");
  if (fidelity.drops.length === 0) {
    lines.push("_No corpus entities missing from the model._");
  } else {
    lines.push("| Corpus ID | Name | Kind |");
    lines.push("| --------- | ---- | ---- |");
    for (const d of fidelity.drops) {
      lines.push(`| ${d.corpusId} | ${d.corpusName} | ${d.kind} |`);
    }
  }
  lines.push("");

  // ── Fabrications section ─────────────────────────────────────────────────
  lines.push("## Fabrications");
  lines.push("");
  if (fidelity.fabrications.length === 0) {
    lines.push("_No model elements with unresolvable provenance._");
  } else {
    lines.push("| Provenance ID | Element Name | Element Type |");
    lines.push("| ------------- | ------------ | ------------ |");
    for (const f of fidelity.fabrications) {
      lines.push(`| ${f.corpusId} | ${f.corpusName} | ${f.kind} |`);
    }
  }
  lines.push("");

  // ── Near-Matches section ─────────────────────────────────────────────────
  lines.push("## Near-Matches");
  lines.push("");
  if (fidelity.nearMatches.length === 0) {
    lines.push("_No fuzzy near-matches found._");
  } else {
    lines.push(
      "| Corpus ID | Corpus Name | Model Element | Model Name | Similarity | Band |"
    );
    lines.push(
      "| --------- | ----------- | ------------- | ---------- | ---------- | ---- |"
    );
    for (const m of fidelity.nearMatches) {
      lines.push(
        `| ${m.corpusId} | ${m.corpusName} | ${m.modelElementId} | ${m.modelName} | ${m.similarity.toFixed(3)} | ${m.band} |`
      );
    }
  }
  lines.push("");

  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// writeReports
// ---------------------------------------------------------------------------

/**
 * Write coverage-matrix.md and fidelity-report.md into dir.
 *
 * Creates dir recursively if it does not exist. Overwrites on each run
 * (the audits dir is gitignored — overwrite-per-run is intentional per
 * the plan spec, T-05-07 accepted).
 *
 * @param dir     Target directory path (e.g. examples/angars/audits)
 * @param matrix  Coverage matrix rows
 * @param fidelity Fidelity three-bucket result
 * @returns       Absolute paths to the two written files
 */
export async function writeReports(
  dir: string,
  matrix: MatrixRow[],
  fidelity: AuditResult["fidelity"]
): Promise<{ matrixPath: string; fidelityPath: string }> {
  await fs.mkdir(dir, { recursive: true });

  const matrixPath = path.join(dir, "coverage-matrix.md");
  const fidelityPath = path.join(dir, "fidelity-report.md");

  await fs.writeFile(matrixPath, renderMatrixMarkdown(matrix), "utf8");
  await fs.writeFile(fidelityPath, renderFidelityMarkdown(fidelity), "utf8");

  return { matrixPath, fidelityPath };
}
