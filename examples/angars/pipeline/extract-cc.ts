/**
 * extract-cc.ts
 *
 * Reads the ANGARS requirements spreadsheets and emits a structured JSON
 * scoped to the Command & Control (C&C) subsystem:
 *   F1 "Manage Refueling Requests"
 *   F8 "Manage HMI"
 *
 * Output: examples/angars/out/cc-extracted.json
 *
 * Usage: pnpm tsx examples/angars/pipeline/extract-cc.ts
 *
 * Ported from sysml-bridge scripts/extract-angars-cc.ts. Reads directly from
 * the corpus xlsx workbooks (does not depend on extract.ts's output).
 */

import * as XLSX from "xlsx";
import * as fs from "fs";
import * as path from "path";
import {
  stableId,
  extractN2Triples,
} from "../../../packages/model/src/index.js";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------
const REPO_ROOT = path.resolve(__dirname, "../../..");
const CORPUS_DIR = path.join(
  REPO_ROOT,
  "examples/angars/corpus/requirements"
);

const FINAL_XLSX = path.join(CORPUS_DIR, "ANGARS Requirements FINAL.xlsx");
const FUNCTIONS_XLSX = path.join(
  CORPUS_DIR,
  "ANGARS Requirements-Functions.xlsx"
);
const N2_XLSX = path.join(CORPUS_DIR, "Interface Data N2.xlsx");

const OUTPUT_DIR = path.join(REPO_ROOT, "examples/angars/out");
const OUTPUT_FILE = path.join(OUTPUT_DIR, "cc-extracted.json");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------
interface Requirement {
  id: string;
  name: string;
  statement: string;
  needIds: string[];
  verifyMethod: string;
}

interface Need {
  id: string;
  name: string;
}

interface FunctionEntry {
  id: string;
  name: string;
  level: string;
  owner: string;
}

interface Component {
  name: string;
}

interface Satisfies {
  reqId: string;
  functionId: string;
}

/**
 * A directed C&C-internal interface flow, sliced from the ANGARS N2 matrices.
 * Same id scheme + provenance shape as extracted.json's n2 rows (extract.ts):
 *   id            = stableId("n2", "<scope>:<srcLabel>-><tgtLabel>:<flow>")
 *   sourceId/tgt  = stableId("component"|"function", label)
 * Every row carries provenance back to a corpus cell (R1/provenance discipline).
 */
interface N2Interface {
  id: string;
  kind: "n2";
  scope: "component" | "functional";
  sourceId: string;
  targetId: string;
  sourceLabel: string;
  targetLabel: string;
  flow: string;
  provenance: { workbook: string; sheet: string; row: number; cell: string };
}

interface Output {
  subsystem: string;
  needs: Need[];
  requirements: Requirement[];
  functions: FunctionEntry[];
  components: Component[];
  satisfies: Satisfies[];
  allocations: never[];
  allocationsNote: string;
  n2Interfaces: N2Interface[];
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Return true if a function ID belongs to C&C scope (F1.x or F8.x). */
function isCCFunction(id: string): boolean {
  return /^F1(\.|$)/.test(id) || /^F8(\.|$)/.test(id);
}

/** Parse the "Relevant Need(s)" cell into an array of need IDs. */
function parseNeeds(raw: string | undefined | null): string[] {
  if (!raw) return [];
  // Split on commas and/or whitespace, filter empties, keep tokens like N1, N12…
  return String(raw)
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter((t) => /^N\d+$/.test(t));
}

// ---------------------------------------------------------------------------
// Step 1 & 2: Parse Satisfied By → C&C scoped req IDs + satisfies map
// ---------------------------------------------------------------------------
const functionsWb = XLSX.readFile(FUNCTIONS_XLSX);
const satisfiedBySheet = functionsWb.Sheets["Satisfied By"];
const satisfiedByRows = XLSX.utils.sheet_to_json<unknown[]>(satisfiedBySheet, {
  header: 1,
}) as unknown[][];

// header: Req ID | Requirement Name | Requirement Statement | Activity Name | Reason
const satisfies: Satisfies[] = [];
const ccReqIds = new Set<string>();

for (const row of satisfiedByRows.slice(1)) {
  const reqId = String(row[0] ?? "").trim();
  const activityRaw = String(row[3] ?? "").trim();
  if (!reqId || !activityRaw) continue;

  // Parse function id: token before first colon, trimmed
  const colonIdx = activityRaw.indexOf(":");
  const functionId =
    colonIdx >= 0
      ? activityRaw.slice(0, colonIdx).trim()
      : activityRaw.trim();

  if (isCCFunction(functionId)) {
    ccReqIds.add(reqId);
    satisfies.push({ reqId, functionId });
  }
}

// ---------------------------------------------------------------------------
// Step 3: Build requirements[] from Final sheet
// ---------------------------------------------------------------------------
const finalWb = XLSX.readFile(FINAL_XLSX);
const finalSheet = finalWb.Sheets["Final"];
const finalRows = XLSX.utils.sheet_to_json<unknown[]>(finalSheet, {
  header: 1,
}) as unknown[][];

// header: Old ID | Req ID | Relevant Need(s) | Category | Type | Requirement Name | Requirement Statement | Verification Method
const requirements: Requirement[] = [];

for (const row of finalRows.slice(1)) {
  const id = String(row[1] ?? "").trim();
  if (!ccReqIds.has(id)) continue;

  const needIds = parseNeeds(row[2] as string | undefined);
  const name = String(row[5] ?? "").trim();
  const statement = String(row[6] ?? "").trim();
  const verifyMethod = String(row[7] ?? "").trim();

  requirements.push({ id, name, statement, needIds, verifyMethod });
}

// ---------------------------------------------------------------------------
// Step 4: needs[] = unique need IDs referenced by scoped requirements
// ---------------------------------------------------------------------------
const allNeedIds = new Set<string>();
for (const req of requirements) {
  for (const n of req.needIds) allNeedIds.add(n);
}

const needs: Need[] = Array.from(allNeedIds)
  .sort()
  .map((id) => ({ id, name: id }));

// ---------------------------------------------------------------------------
// Step 5: functions[] from All Behaviors (F1, F8, and L3 children)
// ---------------------------------------------------------------------------
const allBehaviorsSheet = functionsWb.Sheets["All Behaviors"];
const allBehaviorsRows = XLSX.utils.sheet_to_json<unknown[]>(
  allBehaviorsSheet,
  { header: 1 }
) as unknown[][];

// header: # | ID | Level | Name | Owner | Requirements Satisfied
const functions: FunctionEntry[] = [];

for (const row of allBehaviorsRows.slice(1)) {
  const id = String(row[1] ?? "").trim();
  if (!isCCFunction(id)) continue;

  const level = String(row[2] ?? "").trim();
  // Name column contains "F1.1: Receive & Authenticate Request" — strip the ID prefix
  const rawName = String(row[3] ?? "").trim();
  const colonIdx = rawName.indexOf(":");
  const name =
    colonIdx >= 0 ? rawName.slice(colonIdx + 1).trim() : rawName;
  const owner = String(row[4] ?? "").trim();

  functions.push({ id, name, level, owner });
}

// ---------------------------------------------------------------------------
// Step 6: components[] from C&C N2 sheet header row
// ---------------------------------------------------------------------------
const n2Wb = XLSX.readFile(N2_XLSX);
const ccSheet = n2Wb.Sheets["C&C"];
const n2Rows = XLSX.utils.sheet_to_json<unknown[]>(ccSheet, {
  header: 1,
}) as unknown[][];

// Row 1 = column headers; skip "Source / Destination" and "External"
const SKIP_HEADERS = new Set(["Source / Destination", "External"]);
const headerRow = n2Rows[0] as string[];
const components: Component[] = headerRow
  .map((h) => String(h ?? "").trim())
  .filter((h) => h.length > 0 && !SKIP_HEADERS.has(h))
  .map((name) => ({ name }));

// ---------------------------------------------------------------------------
// Step 7: n2Interfaces — C&C-internal interface flows sliced from the N2 matrices
//
//   (a) component scope: all directed flows on the "C&C" sheet of
//       Interface Data N2.xlsx whose BOTH endpoints are among the 6 C&C
//       components (drops rows to/from "External").
//   (b) functional scope: directed flows from N2 Functional.xlsx :: Internal N2
//       whose BOTH endpoints are among the 17 C&C functions.
//
// Ids/participant-ids/provenance mirror extract.ts exactly (byte-identical id
// scheme), so a full-corpus n2 row and its C&C-scoped counterpart share an id.
// ---------------------------------------------------------------------------
const N2_WORKBOOK = "Interface Data N2.xlsx";
const ccComponentNames = new Set(components.map((c) => c.name));

const n2Interfaces: N2Interface[] = [];
const seenN2Ids = new Set<string>();

// (a) component scope — reuse the same ccSheet rows read in Step 6.
const ccCompTriples = extractN2Triples(n2Rows);
for (const t of ccCompTriples) {
  // Keep only intra-C&C flows: both endpoints must be real C&C components.
  if (!ccComponentNames.has(t.sourceLabel)) continue;
  if (!ccComponentNames.has(t.targetLabel)) continue;

  const tripleNaturalKey = `component:${t.sourceLabel}->${t.targetLabel}:${t.flow}`;
  const tripleId = stableId("n2", tripleNaturalKey);
  if (seenN2Ids.has(tripleId)) continue;
  seenN2Ids.add(tripleId);

  const cellRef = XLSX.utils.encode_cell({ r: t.rowIndex + 1, c: t.colIndex });
  n2Interfaces.push({
    id: tripleId,
    kind: "n2",
    scope: "component",
    sourceId: stableId("component", t.sourceLabel),
    targetId: stableId("component", t.targetLabel),
    sourceLabel: t.sourceLabel,
    targetLabel: t.targetLabel,
    flow: t.flow,
    provenance: { workbook: N2_WORKBOOK, sheet: "C&C", row: t.rowIndex, cell: cellRef },
  });
}
const componentScopeCount = n2Interfaces.length;

// (b) functional scope — Internal N2 flows between C&C functions.
// The Internal N2 sender row is keyed by header POSITION (corpus anomaly 2:
// the position-9 row is mislabeled "F8: Provide Power"). We only keep rows whose
// positional sender AND column target are both C&C functions (F1, F8, or their
// leaf children in the 17-function set).
const FUNC_N2_WORKBOOK = "N2 Functional.xlsx";
const ccFunctionIds = new Set(functions.map((f) => f.id));
const funcN2Wb = XLSX.readFile(path.join(CORPUS_DIR, FUNC_N2_WORKBOOK));
const funcN2Sheet = funcN2Wb.Sheets["Internal N2"];
if (!funcN2Sheet) {
  throw new Error(`Sheet "Internal N2" not found in ${FUNC_N2_WORKBOOK}`);
}
const funcN2Rows = XLSX.utils.sheet_to_json<unknown[]>(funcN2Sheet, {
  header: 1,
  blankrows: false,
}) as unknown[][];
const funcHeader = (funcN2Rows[0] as unknown[]).map((h) => String(h ?? "").trim());
const funcTriples = extractN2Triples(funcN2Rows);
for (const t of funcTriples) {
  // Sender keyed by header position (not the row label) — matches extract.ts.
  const positionalSourceId = funcHeader[t.rowIndex + 1];
  if (!ccFunctionIds.has(positionalSourceId)) continue;
  if (!ccFunctionIds.has(t.targetLabel)) continue;

  const tripleNaturalKey = `functional:${positionalSourceId}->${t.targetLabel}:${t.flow}`;
  const tripleId = stableId("n2", tripleNaturalKey);
  if (seenN2Ids.has(tripleId)) continue;
  seenN2Ids.add(tripleId);

  const cellRef = XLSX.utils.encode_cell({ r: t.rowIndex + 1, c: t.colIndex });
  n2Interfaces.push({
    id: tripleId,
    kind: "n2",
    scope: "functional",
    sourceId: stableId("function", positionalSourceId),
    targetId: stableId("function", t.targetLabel),
    sourceLabel: positionalSourceId,
    targetLabel: t.targetLabel,
    flow: t.flow,
    provenance: { workbook: FUNC_N2_WORKBOOK, sheet: "Internal N2", row: t.rowIndex, cell: cellRef },
  });
}
const functionalScopeCount = n2Interfaces.length - componentScopeCount;

// Deterministic order: sort by id (mirrors extract.ts's final n2Interfaces sort).
n2Interfaces.sort((a, b) => a.id.localeCompare(b.id));

// ---------------------------------------------------------------------------
// Step 8: allocations = [] (no corpus source)
// ---------------------------------------------------------------------------
const allocations: never[] = [];
const allocationsNote =
  "No corpus Func→Comp source; allocations are model-asserted downstream.";

// ---------------------------------------------------------------------------
// Assemble output
// ---------------------------------------------------------------------------
const output: Output = {
  subsystem: "Command & Control",
  needs,
  requirements,
  functions,
  components,
  satisfies,
  allocations,
  allocationsNote,
  n2Interfaces,
};

// ---------------------------------------------------------------------------
// Write output
// ---------------------------------------------------------------------------
fs.mkdirSync(OUTPUT_DIR, { recursive: true });
fs.writeFileSync(OUTPUT_FILE, JSON.stringify(output, null, 2), "utf-8");

// ---------------------------------------------------------------------------
// Sanity check: print counts
// ---------------------------------------------------------------------------
console.log("=== ANGARS C&C Extraction Complete ===");
console.log(`requirements : ${requirements.length}`);
console.log(`needs        : ${needs.length}  (${needs.map((n) => n.id).join(", ")})`);
console.log(`functions    : ${functions.length}`);
console.log(`components   : ${components.length}  (${components.map((c) => c.name).join(", ")})`);
console.log(`satisfies    : ${satisfies.length}`);
console.log(
  `n2Interfaces : ${n2Interfaces.length}  (component=${componentScopeCount}, functional=${functionalScopeCount})`
);
console.log(`Output       : ${OUTPUT_FILE}`);
