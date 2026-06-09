/**
 * traceability-compare.ts
 *
 * IEEE 15288 §6.3.3 — Traceability Fidelity Comparator
 *
 * Verifies that the generated SysML v2 model faithfully carries every
 * Satisfied By trace link recorded in the authoritative human artefact
 * (ANGARS Requirements-Functions.xlsx, "Satisfied By" sheet).
 *
 * This is a pipeline defect detector — it catches:
 *   • DROPPED links  (authoritative → not in model)   = "missing"
 *   • FABRICATED links (in model → not in authoritative) = "unsupported"
 *
 * Pure diff logic lives in packages/mcp-server/src/utils/trace-compare.ts
 * (testable without file I/O or xlsx reads).
 *
 * Output: examples/angars/audits/cc-trace-fidelity.md
 *
 * Usage: pnpm tsx scripts/traceability-compare.ts
 */

import * as XLSX from "xlsx";
import * as fs from "fs";
import * as path from "path";

// Import pure diff logic from the testable package module
import {
  compareTrace,
  normalizeFunctionId,
  type TracePair,
  type CompareResult,
} from "../packages/mcp-server/src/utils/trace-compare.js";

// Re-export for any consumers that import directly from this script
export { compareTrace, normalizeFunctionId, type TracePair, type CompareResult };

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const REPO_ROOT = path.resolve(__dirname, "..");
const FUNCTIONS_XLSX = path.join(
  REPO_ROOT,
  "examples/angars/corpus/requirements/ANGARS Requirements-Functions.xlsx"
);
const STORE_DIR = path.join(REPO_ROOT, "examples/angars/model/.store");
const AUDIT_DIR = path.join(REPO_ROOT, "examples/angars/audits");
const REPORT_FILE = path.join(AUDIT_DIR, "cc-trace-fidelity.md");

// ---------------------------------------------------------------------------
// Authoritative reader — "Satisfied By" sheet, scoped to C&C (F1.* / F8.*)
// ---------------------------------------------------------------------------

function isCCFunction(id: string): boolean {
  return id.startsWith("F1") || id.startsWith("F8");
}

export function readAuthoritative(): TracePair[] {
  const wb = XLSX.readFile(FUNCTIONS_XLSX);
  const sheet = wb.Sheets["Satisfied By"];
  if (!sheet) {
    throw new Error(
      `Sheet "Satisfied By" not found in ${FUNCTIONS_XLSX}`
    );
  }

  const rows = XLSX.utils.sheet_to_json<unknown[]>(sheet, {
    header: 1,
  }) as unknown[][];

  // header: Req ID | Requirement Name | Requirement Statement | Activity Name | Reason
  const pairs: TracePair[] = [];

  for (const row of rows.slice(1)) {
    const reqId = String((row as unknown[])[0] ?? "").trim();
    const activityRaw = String((row as unknown[])[3] ?? "").trim();
    if (!reqId || !activityRaw) continue;

    const functionId = normalizeFunctionId(activityRaw);
    if (!isCCFunction(functionId)) continue;

    pairs.push({ reqId, functionId });
  }

  return pairs;
}

// ---------------------------------------------------------------------------
// Generated model reader — FileStore JSON, SatisfyRequirementUsage elements
// ---------------------------------------------------------------------------

interface FileModelDoc {
  "@type": string;
  id: string;
  name: string;
  elements: Array<{
    id: string;
    type: string;
    name: string | null;
    raw: Record<string, unknown>;
  }>;
}

function idsFrom(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    if (typeof item === "string") {
      out.push(item);
    } else if (item && typeof item === "object" && "@id" in (item as object)) {
      const id = (item as { "@id"?: unknown })["@id"];
      if (typeof id === "string") out.push(id);
    }
  }
  return out;
}

export function readGenerated(): TracePair[] {
  // Graceful failure when the model hasn't been generated yet (Task 11)
  if (!fs.existsSync(STORE_DIR)) {
    console.error(
      `generated model not found at ${STORE_DIR} — run the generation pipeline (Task 11) first`
    );
    process.exit(1);
  }

  const storeFiles = fs
    .readdirSync(STORE_DIR)
    .filter((f) => f.endsWith(".json"));

  if (storeFiles.length === 0) {
    console.error(
      `generated model not found at ${STORE_DIR} — run the generation pipeline (Task 11) first`
    );
    process.exit(1);
  }

  // Aggregate all elements across all project files in the store
  const allElements: FileModelDoc["elements"][number][] = [];
  for (const file of storeFiles) {
    const raw = fs.readFileSync(path.join(STORE_DIR, file), "utf8");
    const doc = JSON.parse(raw) as FileModelDoc;
    if (doc["@type"] !== "FileModel") continue;
    allElements.push(...(doc.elements ?? []));
  }

  if (allElements.length === 0) {
    console.error(
      `generated model not found at ${STORE_DIR} — run the generation pipeline (Task 11) first`
    );
    process.exit(1);
  }

  // Build id → provenanceSourceId map for quick lookups
  const provenanceById = new Map<string, string>();
  for (const el of allElements) {
    const prov = el.raw?.provenanceSourceId;
    if (typeof prov === "string" && prov) {
      provenanceById.set(el.id, prov);
    }
  }

  // Extract SatisfyRequirementUsage relationships
  const pairs: TracePair[] = [];

  for (const el of allElements) {
    if (el.type !== "SatisfyRequirementUsage") continue;

    const sourceIds = idsFrom(el.raw.source);
    const targetIds = idsFrom(el.raw.target);

    for (const srcId of sourceIds) {
      for (const tgtId of targetIds) {
        const functionId = provenanceById.get(srcId);
        const reqId = provenanceById.get(tgtId);
        if (!functionId || !reqId) continue;

        const normalizedFn = normalizeFunctionId(functionId);
        if (!isCCFunction(normalizedFn)) continue;

        pairs.push({ reqId, functionId: normalizedFn });
      }
    }
  }

  return pairs;
}

// ---------------------------------------------------------------------------
// Report writer
// ---------------------------------------------------------------------------

function renderTable(pairs: TracePair[]): string {
  if (pairs.length === 0) return "_None_\n";
  const header = "| Req ID | Function ID |\n|--------|-------------|";
  const rows = pairs
    .map((p) => `| ${p.reqId} | ${p.functionId} |`)
    .join("\n");
  return `${header}\n${rows}\n`;
}

function writeReport(result: CompareResult): void {
  const now = new Date().toISOString();
  const lines: string[] = [
    "# C&C Traceability Fidelity Audit",
    "",
    `**Standard:** IEEE 15288 §6.3.3 — Requirements Traceability Completeness and Consistency  `,
    `**Scope:** Command & Control subsystem (F1.\\*, F8.\\* functions)  `,
    `**Authoritative source:** \`examples/angars/corpus/requirements/ANGARS Requirements-Functions.xlsx\` → sheet \`Satisfied By\`  `,
    `**Generated model:** \`examples/angars/model/.store\`  `,
    `**Generated:** ${now}`,
    "",
    "---",
    "",
    "## Summary",
    "",
    `| Metric | Value |`,
    `|--------|-------|`,
    `| **Fidelity** | **${result.fidelityPct}%** |`,
    `| Faithful (present) | ${result.present.length} |`,
    `| Dropped by pipeline (missing) | ${result.missing.length} |`,
    `| Fabricated (unsupported) | ${result.unsupported.length} |`,
    `| Authoritative total | ${result.present.length + result.missing.length} |`,
    `| Generated total | ${result.present.length + result.unsupported.length} |`,
    "",
    "---",
    "",
    "## Interpretation",
    "",
    "- **Faithful (present):** Links that appear in both the authoritative `Satisfied By` sheet and the generated model. These are correctly propagated.",
    "- **Dropped by pipeline (missing):** Links present in the authoritative source but absent from the generated model. These are **pipeline defects** — the generation step silently dropped a mandated trace.",
    "- **Fabricated (unsupported):** Links present in the generated model but absent from the authoritative source. These are **pipeline defects** — the generation step invented a trace with no authoritative backing.",
    "",
    "---",
    "",
    "## Faithful Links (present in both)",
    "",
    renderTable(result.present),
    "",
    "## Dropped by Pipeline — DEFECTS (missing from generated model)",
    "",
    result.missing.length === 0
      ? "_None — all authoritative links are faithfully represented._\n"
      : renderTable(result.missing),
    "",
    "## Fabricated Links — DEFECTS (in generated model, not in authoritative source)",
    "",
    result.unsupported.length === 0
      ? "_None — no unsupported links found._\n"
      : renderTable(result.unsupported),
  ];

  fs.mkdirSync(AUDIT_DIR, { recursive: true });
  fs.writeFileSync(REPORT_FILE, lines.join("\n"), "utf8");
  console.log(`Report written to ${REPORT_FILE}`);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const authoritative = readAuthoritative();
const generated = readGenerated();

const result = compareTrace(authoritative, generated);
writeReport(result);

console.log(
  `Fidelity: ${result.fidelityPct}% | present=${result.present.length} missing=${result.missing.length} unsupported=${result.unsupported.length}`
);
