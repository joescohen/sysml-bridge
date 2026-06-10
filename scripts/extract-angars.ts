/**
 * extract-angars.ts
 *
 * Full-corpus extractor: reads all 9 ANGARS workbooks through the plan 02-02
 * helpers and the declarative workbook-config table, asserts every pinned count,
 * parses all three N2 scopes as (source, target, flow) triples with direction
 * spot-checks, encodes corpus anomalies as deliberate assertions, and writes
 * examples/angars/model/extracted.json through ExtractedSchema.parse with
 * stableId for every entity id (ETL-01/02/03; IR-01/02).
 *
 * Usage: pnpm extract:angars
 *        (or: pnpm tsx scripts/extract-angars.ts)
 */

import * as XLSX from "xlsx";
import * as fs from "fs";
import * as path from "path";

import {
  ExtractedSchema,
  SCHEMA_VERSION,
  stableId,
  parseNeeds,
  parseActivityId,
  stripIdPrefix,
  assertCount,
  extractN2Triples,
  assertSpotCheck,
  WORKBOOKS,
  N2_SHEETS,
  ANGARS_SS_HEADERS,
  SUBSYSTEM_SHEET_MAP,
} from "@sysml-bridge/ir";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const REPO_ROOT = path.resolve(__dirname, "..");
const CORPUS_DIR = path.join(REPO_ROOT, "examples/angars/corpus/requirements");
const OUTPUT_DIR = path.join(REPO_ROOT, "examples/angars/model");
const OUTPUT_FILE = path.join(OUTPUT_DIR, "extracted.json");

// ---------------------------------------------------------------------------
// readSheet helper
// ---------------------------------------------------------------------------

/**
 * Read a sheet as header:1 rows and split into header + data.
 * Throws (never warns) if the sheet is missing — ETL-03 ordering: assert first.
 */
function readSheet(
  file: string,
  sheet: string,
  headerRow: number
): { header: unknown[]; data: unknown[][] } {
  const absPath = path.join(CORPUS_DIR, file);
  const wb = XLSX.readFile(absPath);
  if (!wb.Sheets[sheet]) {
    throw new Error(
      `[ETL-03] Sheet "${sheet}" not found in "${file}". Available: ${Object.keys(wb.Sheets).join(", ")}`
    );
  }
  const rows = XLSX.utils.sheet_to_json<unknown[]>(wb.Sheets[sheet], {
    header: 1,
    blankrows: false,
  }) as unknown[][];
  const header = rows[headerRow] ?? [];
  const data = rows.slice(headerRow + 1);
  return { header, data };
}

// ---------------------------------------------------------------------------
// NEEDS (16 entities)
// ---------------------------------------------------------------------------

const needsCfg = WORKBOOKS[0]; // "ANGARS Needs FINAL.xlsx" / "Sheet1" / expected 16
const { data: needsData } = readSheet(needsCfg.file, needsCfg.sheet, needsCfg.headerRow);
// ETL-03: assert BEFORE building entities
assertCount(`${needsCfg.file}::${needsCfg.sheet}`, needsData.length, needsCfg.expected);

const needs = needsData.map((row, rowIdx) => {
  const naturalKey = String((row as unknown[])[needsCfg.cols.id] ?? "").trim();
  const name = String((row as unknown[])[needsCfg.cols.name] ?? "").trim();
  const category = String((row as unknown[])[needsCfg.cols.category] ?? "").trim();
  const description = String((row as unknown[])[needsCfg.cols.description] ?? "").trim();
  return {
    id: stableId("need", naturalKey),
    kind: "need" as const,
    naturalKey,
    name,
    category: category || undefined,
    description: description || undefined,
  };
});

if (needs.length !== 16) {
  throw new Error(`[ETL-03] needs: expected 16, got ${needs.length}`);
}

// ---------------------------------------------------------------------------
// REQUIREMENTS (182 merged from three sources)
// ---------------------------------------------------------------------------

// Build a Map keyed by verbatim req-id naturalKey for dedup and cross-checks.
const reqMap = new Map<
  string,
  {
    id: string;
    kind: "requirement";
    naturalKey: string;
    name: string;
    statement: string;
    needIds: string[];
    verifyMethod?: string;
    category?: string;
    reqType?: string;
  }
>();

// (a) ANGARS Requirements FINAL — 165 rows
const finalCfg = WORKBOOKS[1];
const { data: finalData } = readSheet(finalCfg.file, finalCfg.sheet, finalCfg.headerRow);
assertCount(`${finalCfg.file}::${finalCfg.sheet}`, finalData.length, finalCfg.expected);

for (const row of finalData) {
  const r = row as unknown[];
  const reqId = String(r[finalCfg.cols.id] ?? "").trim();
  if (!reqId) continue;
  const needRefs = String(r[finalCfg.cols.needRefs] ?? "");
  const needIds = parseNeeds(needRefs).map((n) => stableId("need", n));
  const name = String(r[finalCfg.cols.name] ?? "").trim();
  const statement = String(r[finalCfg.cols.statement] ?? "").trim();
  const verifyMethod = String(r[finalCfg.cols.verify] ?? "").trim() || undefined;
  const category = String(r[finalCfg.cols.category] ?? "").trim() || undefined;
  const reqType = String(r[finalCfg.cols.type] ?? "").trim() || undefined;

  reqMap.set(reqId, {
    id: stableId("requirement", reqId),
    kind: "requirement",
    naturalKey: reqId,
    name,
    statement,
    needIds,
    verifyMethod,
    category,
    reqType,
  });
}

if (reqMap.size !== 165) {
  throw new Error(`[ETL-03] requirements after Final: expected 165 unique ids, got ${reqMap.size}`);
}

// (b) ASpec Sheet3 — 174 rows; add only NEW ids (Final row is authoritative for overlaps)
const aspecSheet3Cfg = WORKBOOKS[2];
const { data: aspecSheet3Data } = readSheet(aspecSheet3Cfg.file, aspecSheet3Cfg.sheet, aspecSheet3Cfg.headerRow);
assertCount(`${aspecSheet3Cfg.file}::${aspecSheet3Cfg.sheet}`, aspecSheet3Data.length, aspecSheet3Cfg.expected);

let sheet3NewCount = 0;
for (const row of aspecSheet3Data) {
  const r = row as unknown[];
  const reqId = String(r[aspecSheet3Cfg.cols.id] ?? "").trim();
  if (!reqId) continue;
  if (reqMap.has(reqId)) continue; // Final is authoritative — skip duplicates

  const needRefs = String(r[aspecSheet3Cfg.cols.needRefs] ?? "");
  const needIds = parseNeeds(needRefs).map((n) => stableId("need", n));
  const name = String(r[aspecSheet3Cfg.cols.name] ?? "").trim();
  const statement = String(r[aspecSheet3Cfg.cols.statement] ?? "").trim();
  const verifyMethod = String(r[aspecSheet3Cfg.cols.verify] ?? "").trim() || undefined;
  const category = String(r[aspecSheet3Cfg.cols.category] ?? "").trim() || undefined;
  const reqType = String(r[aspecSheet3Cfg.cols.type] ?? "").trim() || undefined;

  reqMap.set(reqId, {
    id: stableId("requirement", reqId),
    kind: "requirement",
    naturalKey: reqId,
    name,
    statement,
    needIds,
    verifyMethod,
    category,
    reqType,
  });
  sheet3NewCount++;
}

// Corpus-pinned merge assertions (must both be exactly right):
// - ASpec Sheet3 adds exactly 10 new ids (ASPEC-001..ASPEC-010)
// - Single Final-only id is ANGARS-79 (not present in ASpec Sheet3)
if (sheet3NewCount !== 10) {
  throw new Error(
    `[ETL-03] ASpec Sheet3 merge: expected exactly 10 new ids, got ${sheet3NewCount}`
  );
}
if (reqMap.size !== 175) {
  throw new Error(
    `[ETL-03] requirements after ASpec Sheet3: expected merged size 175, got ${reqMap.size}`
  );
}

// (c) ASpec Sheet5 — 7 rows of -ilities/HSI/Interface allocation (ASPEC-011..ASPEC-017)
const aspecSheet5Cfg = WORKBOOKS[3];
const { data: aspecSheet5Data } = readSheet(aspecSheet5Cfg.file, aspecSheet5Cfg.sheet, aspecSheet5Cfg.headerRow);
assertCount(`${aspecSheet5Cfg.file}::${aspecSheet5Cfg.sheet}`, aspecSheet5Data.length, aspecSheet5Cfg.expected);

for (const row of aspecSheet5Data) {
  const r = row as unknown[];
  const reqId = String(r[aspecSheet5Cfg.cols.id] ?? "").trim();
  if (!reqId) continue;
  // col1 = Category (e.g. "-ilities (Rel)") — this sheet has no name column; use category as name
  const category = String(r[aspecSheet5Cfg.cols.category] ?? "").trim();
  const statement = String(r[aspecSheet5Cfg.cols.statement] ?? "").trim();
  const verifyMethod = String(r[aspecSheet5Cfg.cols.verify] ?? "").trim() || undefined;
  // col8 is an unheadered trailing column carrying the need allocation
  const needToken = String(r[aspecSheet5Cfg.cols.need] ?? "").trim();
  const needIds = needToken ? [stableId("need", needToken)] : [];

  reqMap.set(reqId, {
    id: stableId("requirement", reqId),
    kind: "requirement",
    naturalKey: reqId,
    name: category, // no name column — use category as the name
    statement,
    needIds,
    verifyMethod,
    category: category || undefined,
    reqType: undefined,
  });
}

if (reqMap.size !== 182) {
  throw new Error(
    `[ETL-03] requirements after ASpec Sheet5: expected merged size 182, got ${reqMap.size}`
  );
}

// Emit requirements sorted by naturalKey (locale-compare, numeric option)
const requirements = Array.from(reqMap.values()).sort((a, b) =>
  a.naturalKey.localeCompare(b.naturalKey, undefined, { numeric: true })
);

if (requirements.length !== 182) {
  throw new Error(`[ETL-03] requirements final sort: expected 182, got ${requirements.length}`);
}

// ---------------------------------------------------------------------------
// SATISFIES (154 entries)
// ---------------------------------------------------------------------------

const satisfiesCfg = WORKBOOKS[4];
const { data: satisfiesData } = readSheet(satisfiesCfg.file, satisfiesCfg.sheet, satisfiesCfg.headerRow);
// ETL-03: assert BEFORE building entities
assertCount(`${satisfiesCfg.file}::${satisfiesCfg.sheet}`, satisfiesData.length, satisfiesCfg.expected);

// Keep ALL rows — the isCCFunction filter is deliberately dropped (ETL-01 scope = full corpus)
const satisfies = satisfiesData
  .map((row) => {
    const r = row as unknown[];
    const reqId = String(r[satisfiesCfg.cols.reqId] ?? "").trim();
    const activityRaw = String(r[satisfiesCfg.cols.activity] ?? "").trim();
    if (!reqId || !activityRaw) return null;
    const functionId = parseActivityId(activityRaw);
    return {
      reqId: stableId("requirement", reqId),
      functionId: stableId("function", functionId),
    };
  })
  .filter(Boolean) as Array<{ reqId: string; functionId: string }>;

if (satisfies.length !== 154) {
  throw new Error(`[ETL-03] satisfies: expected 154, got ${satisfies.length}`);
}

// Sort by (reqId, functionId) for determinism
satisfies.sort((a, b) => {
  if (a.reqId !== b.reqId) return a.reqId.localeCompare(b.reqId);
  return a.functionId.localeCompare(b.functionId);
});

// ---------------------------------------------------------------------------
// ALL BEHAVIORS -> functions + behaviorDecomp
// ---------------------------------------------------------------------------

const allBehaviorsCfg = WORKBOOKS[5];
const { data: allBehaviorsData } = readSheet(
  allBehaviorsCfg.file,
  allBehaviorsCfg.sheet,
  allBehaviorsCfg.headerRow
);
// ETL-03: assert BEFORE building entities
// 65 raw data rows — includes one L1 row with empty ID, two near-blank rows; blankrows:false filtered
assertCount(`${allBehaviorsCfg.file}::${allBehaviorsCfg.sheet}`, allBehaviorsData.length, allBehaviorsCfg.expected);

// Filter to rows with a non-empty trimmed ID (col1) -> 62 rows
const filteredBehaviors = allBehaviorsData.filter((row) => {
  const id = String((row as unknown[])[allBehaviorsCfg.cols.id] ?? "").trim();
  return id.length > 0;
});

// Assert level breakdown on filtered set
// CORPUS ANOMALY 1: there is NO F9 row in All Behaviors — assert L2 == 8, never 9
const l2Count = filteredBehaviors.filter(
  (row) => String((row as unknown[])[allBehaviorsCfg.cols.level] ?? "").trim() === "L2"
).length;
const l3Count = filteredBehaviors.filter(
  (row) => String((row as unknown[])[allBehaviorsCfg.cols.level] ?? "").trim() === "L3"
).length;

if (l2Count !== 8) {
  // CORPUS ANOMALY 1: All Behaviors contains F1-F8 only at L2 (no F9 row exists in this sheet).
  // F9 "Provide Power" is synthesized from N2 Functional Internal N2 column header below.
  throw new Error(
    `[ETL-03] All Behaviors L2 assertion failed: expected 8 (no F9 row — corpus anomaly), got ${l2Count}`
  );
}
if (l3Count !== 54) {
  throw new Error(
    `[ETL-03] All Behaviors L3 assertion failed: expected 54, got ${l3Count}`
  );
}

// behaviorDecomp (62 entries)
const behaviorDecomp = filteredBehaviors.map((row, dataIdx) => {
  const r = row as unknown[];
  const fnId = String(r[allBehaviorsCfg.cols.id] ?? "").trim();
  const level = String(r[allBehaviorsCfg.cols.level] ?? "").trim();
  const rawName = String(r[allBehaviorsCfg.cols.name] ?? "").trim();
  const owner = String(r[allBehaviorsCfg.cols.owner] ?? "").trim() || undefined;
  const name = stripIdPrefix(rawName);

  // For L3 rows, parentId = stableId("behaviorDecomp", top-level function id)
  // Derived by splitting the corpus ID on the first dot: "F1.1" -> "F1"
  // No hand-rostered mapping — pure string split on corpus id
  const parentId =
    level === "L3"
      ? stableId("behaviorDecomp", fnId.split(".")[0])
      : undefined;

  return {
    id: stableId("behaviorDecomp", fnId),
    kind: "behaviorDecomp" as const,
    naturalKey: fnId,
    level,
    name,
    owner,
    parentId,
    provenance: {
      workbook: allBehaviorsCfg.file,
      sheet: allBehaviorsCfg.sheet,
      row: dataIdx, // 0-based data-row index (header excluded)
    },
  };
});

if (behaviorDecomp.length !== 62) {
  throw new Error(`[ETL-03] behaviorDecomp: expected 62, got ${behaviorDecomp.length}`);
}

// functions (63 entries) — same 62 rows as function entities PLUS synthesized F9
// CORPUS ANOMALY 1: F9 "Provide Power" exists only as the Internal N2 column F9.
// Its sender row in N2 Functional is mislabeled "F8: Provide Power" (duplicate-F8 typo).
// Synthesize F9 from the N2 column header — provenance is N2 Functional :: Internal N2.
const functions = [
  ...filteredBehaviors.map((row) => {
    const r = row as unknown[];
    const fnId = String(r[allBehaviorsCfg.cols.id] ?? "").trim();
    const level = String(r[allBehaviorsCfg.cols.level] ?? "").trim();
    const rawName = String(r[allBehaviorsCfg.cols.name] ?? "").trim();
    const owner = String(r[allBehaviorsCfg.cols.owner] ?? "").trim();
    const name = stripIdPrefix(rawName);
    return {
      id: stableId("function", fnId),
      kind: "function" as const,
      naturalKey: fnId,
      name,
      level,
      owner,
    };
  }),
  // Synthesized F9 — CORPUS ANOMALY 1:
  // Source: N2 Functional.xlsx :: Internal N2 column header "F9" (position 9).
  // The All Behaviors sheet has NO F9 row. The position-9 sender row in Internal N2
  // is mislabeled "F8: Provide Power" — it IS F9, evidenced by the clean column header.
  // provenance: N2 Functional :: Internal N2 (column header F9 + position-9 row).
  {
    id: stableId("function", "F9"),
    kind: "function" as const,
    naturalKey: "F9",
    name: "Provide Power",
    level: "L2",
    owner: "",
  },
];

if (functions.length !== 63) {
  throw new Error(`[ETL-03] functions: expected 63, got ${functions.length}`);
}
const f9Count = functions.filter((f) => f.naturalKey === "F9").length;
if (f9Count !== 1) {
  throw new Error(`[ETL-03] F9 uniqueness: expected exactly 1 F9 function, got ${f9Count}`);
}

// Sort functions and behaviorDecomp by naturalKey (localeCompare, numeric)
functions.sort((a, b) =>
  a.naturalKey.localeCompare(b.naturalKey, undefined, { numeric: true })
);
behaviorDecomp.sort((a, b) =>
  a.naturalKey.localeCompare(b.naturalKey, undefined, { numeric: true })
);

// ---------------------------------------------------------------------------
// KPPS (10 entries)
// ---------------------------------------------------------------------------

const kppCfg = WORKBOOKS[6];
const { data: kppData } = readSheet(kppCfg.file, kppCfg.sheet, kppCfg.headerRow);
assertCount(`${kppCfg.file}::${kppCfg.sheet}`, kppData.length, kppCfg.expected);

const kpps = kppData.map((row, rowIdx) => {
  const r = row as unknown[];
  const kppId = String(r[kppCfg.cols.id] ?? "").trim();
  const title = String(r[kppCfg.cols.title] ?? "").trim();
  return {
    id: stableId("kpp", kppId),
    kind: "kpp" as const,
    naturalKey: kppId,
    title,
    reqId: stableId("requirement", kppId),
    provenance: {
      workbook: kppCfg.file,
      sheet: kppCfg.sheet,
      row: rowIdx,
    },
  };
});

if (kpps.length !== 10) {
  throw new Error(`[ETL-03] kpps: expected 10, got ${kpps.length}`);
}

// Cross-link assertion: every KPP id must exist as a key in the requirements Map
// (all 10 KPP ids ARE requirement ids — throw listing any that do not resolve)
const missingKppReqs = kpps.filter((k) => !reqMap.has(k.naturalKey));
if (missingKppReqs.length > 0) {
  throw new Error(
    `[ETL-03] KPP cross-link: the following KPP ids do not resolve to requirements: ${missingKppReqs.map((k) => k.naturalKey).join(", ")}`
  );
}

// ---------------------------------------------------------------------------
// TOP-LEVEL CROSS-CHECKS (Report workbooks — no new entities; assertions only)
// ---------------------------------------------------------------------------

// Top-Level Mission Requirements — headerRow=1, 3 data rows
const missionCfg = WORKBOOKS[8];
const { data: missionData } = readSheet(missionCfg.file, missionCfg.sheet, missionCfg.headerRow);
assertCount(`${missionCfg.file}::${missionCfg.sheet}`, missionData.length, missionCfg.expected);

const missingMissionReqs: string[] = [];
for (const row of missionData) {
  const r = row as unknown[];
  const idToken = String(r[missionCfg.cols.id] ?? "").trim();
  if (!reqMap.has(idToken)) {
    missingMissionReqs.push(idToken);
  }
}
if (missingMissionReqs.length > 0) {
  throw new Error(
    `[ETL-03] Top-Level Mission Requirements cross-check: ids not in requirements Map: ${missingMissionReqs.join(", ")}`
  );
}
console.log(
  `Top-Level Mission Requirements (${missionCfg.file}::${missionCfg.sheet}): cross-checked, no new entities`
);

// Top-Level KPP Requirements — headerRow=1, 3 data rows
const kppTopCfg = WORKBOOKS[7];
const { data: kppTopData } = readSheet(kppTopCfg.file, kppTopCfg.sheet, kppTopCfg.headerRow);
assertCount(`${kppTopCfg.file}::${kppTopCfg.sheet}`, kppTopData.length, kppTopCfg.expected);

const kppNaturalKeySet = new Set(kpps.map((k) => k.naturalKey));
const missingKppTopReqs: string[] = [];
for (const row of kppTopData) {
  const r = row as unknown[];
  // Name column (col1) leads with req id token: "ANGARS-2 Refueling Time"
  const nameCell = String(r[kppTopCfg.cols.name] ?? "").trim();
  const match = nameCell.match(/^(ANGARS-\d+)/);
  if (!match) {
    throw new Error(
      `[ETL-03] Top-Level KPP Requirements: could not extract ANGARS-NNN token from "${nameCell}"`
    );
  }
  const token = match[1];
  if (!reqMap.has(token)) {
    missingKppTopReqs.push(`${token} (not in reqMap)`);
  }
  if (!kppNaturalKeySet.has(token)) {
    missingKppTopReqs.push(`${token} (not in kpps)`);
  }
}
if (missingKppTopReqs.length > 0) {
  throw new Error(
    `[ETL-03] Top-Level KPP Requirements cross-check failed: ${missingKppTopReqs.join(", ")}`
  );
}
console.log(
  `Top-Level KPP Requirements (${kppTopCfg.file}::${kppTopCfg.sheet}): cross-checked, no new entities`
);

// ---------------------------------------------------------------------------
// TODO stubs for Task 3 (subsystems, components, n2Interfaces)
// ---------------------------------------------------------------------------

const subsystems: never[] = [];
const components: never[] = [];
const n2Interfaces: never[] = [];

// ---------------------------------------------------------------------------
// Summary (Tasks 1-2 partial; write happens in Task 3)
// ---------------------------------------------------------------------------

console.log("=== ANGARS Full-Corpus Extraction (partial — Tasks 1-2) ===");
console.log(`needs         : ${needs.length}`);
console.log(`requirements  : ${requirements.length}`);
console.log(`satisfies     : ${satisfies.length}`);
console.log(`functions     : ${functions.length}`);
console.log(`behaviorDecomp: ${behaviorDecomp.length}`);
console.log(`kpps          : ${kpps.length}`);
console.log("(subsystems/components/n2Interfaces: Task 3 stubs)");
