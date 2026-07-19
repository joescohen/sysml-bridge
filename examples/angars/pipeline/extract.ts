/**
 * extract.ts
 *
 * Full-corpus extractor: reads all 9 ANGARS workbooks through the plan 02-02
 * helpers and the declarative workbook-config table, asserts every pinned count,
 * parses all three N2 scopes as (source, target, flow) triples with direction
 * spot-checks, encodes corpus anomalies as deliberate assertions, and writes
 * examples/angars/out/extracted.json through ExtractedSchema.parse with
 * stableId for every entity id (ETL-01/02/03; IR-01/02).
 *
 * Usage: pnpm demo:extract
 *        (or: pnpm tsx examples/angars/pipeline/extract.ts)
 *
 * Ported from sysml-bridge scripts/extract-angars.ts. Imports the workspace
 * model package via its RELATIVE SOURCE path (not the package name) so tsx
 * can run this without a prior `pnpm build` of @sysml-bridge/model.
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
} from "../../../packages/model/src/index.js";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const REPO_ROOT = path.resolve(__dirname, "../../..");
const CORPUS_DIR = path.join(REPO_ROOT, "examples/angars/corpus/requirements");
const OUTPUT_DIR = path.join(REPO_ROOT, "examples/angars/out");
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
// SUBSYSTEM + COMPONENT SCOPE (Interface Data N2.xlsx)
// ---------------------------------------------------------------------------

const N2_XLSX_FILE = "Interface Data N2.xlsx";
const N2_SKIP_HEADERS = new Set(["Source / Destination", "External"]);

// Read ANGARS SS sheet with header:1
const ssRows = XLSX.utils.sheet_to_json<unknown[]>(
  XLSX.readFile(path.join(CORPUS_DIR, N2_XLSX_FILE)).Sheets["ANGARS SS"],
  { header: 1, blankrows: false }
) as unknown[][];

// Assert header columns 1..7 deep-equal ANGARS_SS_HEADERS verbatim
// This assertion is the guard against corpus label drift (SUBSYSTEM_SHEET_MAP keys depend on these)
const ssHeader = (ssRows[0] as unknown[]).map((h) => String(h ?? "").trim());
const expectedHeaderSlice = Array.from(ANGARS_SS_HEADERS);
for (let i = 0; i < expectedHeaderSlice.length; i++) {
  if (ssHeader[i + 1] !== expectedHeaderSlice[i]) {
    throw new Error(
      `[ETL-03] ANGARS SS header col ${i + 1}: expected "${expectedHeaderSlice[i]}", got "${ssHeader[i + 1]}" — corpus label drifted, update SUBSYSTEM_SHEET_MAP`
    );
  }
}

// subsystems (6 entities): one per non-External header
// External is an environment actor with NO subsystems[] entry, by design
// Participant id rule (pinned): sourceId/targetId = stableId("subsystem", label) for the
// six subsystem labels, stableId("external", "External") for the External row/column.
const subsystemOrder = expectedHeaderSlice.filter((h) => h !== "External");
const subsystemComponentIds: Map<string, string[]> = new Map(
  subsystemOrder.map((h) => [h, []])
);

// We will fill componentIds after processing component N2 sheets below.
// Build placeholder subsystem objects now; componentIds arrays will be mutated.

type SubsystemEntity = {
  id: string;
  kind: "subsystem";
  naturalKey: string;
  name: string;
  componentIds: string[];
  provenance: { workbook: string; sheet: string };
};

const subsystems: SubsystemEntity[] = subsystemOrder.map((header) => ({
  id: stableId("subsystem", header),
  kind: "subsystem",
  naturalKey: header,
  name: header,
  componentIds: subsystemComponentIds.get(header)!, // reference to the same array — mutable
  provenance: { workbook: N2_XLSX_FILE, sheet: "ANGARS SS" },
}));

if (subsystems.length !== 6) {
  throw new Error(`[ETL-03] subsystems: expected 6, got ${subsystems.length}`);
}

// Extract subsystem-scope N2 triples from ANGARS SS
const ssTriples = extractN2Triples(ssRows);

// ETL-02 DIRECTION SPOT-CHECKS (both cells verified against live corpus 2026-06-09):
// Cell "Power Subsystem" row -> "Command & Control Subsystem" col = "28VDC, Telemetry, ..."
// Cell "External" row -> "Command & Control Subsystem" col = "..., Operator Commands, ..."
assertSpotCheck(ssTriples, "Power Subsystem", "Command & Control Subsystem", "28VDC");
assertSpotCheck(ssTriples, "External", "Command & Control Subsystem", "Operator Commands");

// Build n2Interfaces for subsystem scope
// Participant id rule: stableId("subsystem", label) for subsystem labels;
//                      stableId("external", "External") for External
function resolveParticipantId(label: string, scope: "subsystem" | "component" | "functional"): string {
  if (label === "External") return stableId("external", "External");
  if (scope === "functional") return stableId("function", label);
  if (scope === "subsystem") return stableId("subsystem", label);
  return stableId("component", label);
}

const n2TripleMap = new Map<string, {
  id: string;
  kind: "n2";
  scope: "subsystem" | "component" | "functional";
  sourceId: string;
  targetId: string;
  sourceLabel: string;
  targetLabel: string;
  flow: string;
  provenance: { workbook: string; sheet: string; row: number; cell: string };
}>();

for (const t of ssTriples) {
  const sourceId = resolveParticipantId(t.sourceLabel, "subsystem");
  const targetId = resolveParticipantId(t.targetLabel, "subsystem");
  const tripleNaturalKey = `subsystem:${t.sourceLabel}->${t.targetLabel}:${t.flow}`;
  const tripleId = stableId("n2", tripleNaturalKey);
  const absRow = t.rowIndex + 1 + 1; // +1 for header row (rowIndex is 0-based data), +1 for 1-based
  const cellRef = XLSX.utils.encode_cell({ r: t.rowIndex + 1, c: t.colIndex });
  if (!n2TripleMap.has(tripleId)) {
    n2TripleMap.set(tripleId, {
      id: tripleId,
      kind: "n2",
      scope: "subsystem",
      sourceId,
      targetId,
      sourceLabel: t.sourceLabel,
      targetLabel: t.targetLabel,
      flow: t.flow,
      provenance: { workbook: N2_XLSX_FILE, sheet: "ANGARS SS", row: t.rowIndex, cell: cellRef },
    });
  }
}
console.log(`ANGARS SS (subsystem scope): ${ssTriples.length} triples`);

// COMPONENT SCOPE — read Interface Data N2.xlsx for the 6 component sheets
const n2Wb = XLSX.readFile(path.join(CORPUS_DIR, N2_XLSX_FILE));
let totalComponents = 0;

const componentSheets = Object.keys(SUBSYSTEM_SHEET_MAP).map((subsysHeader) => ({
  subsysHeader,
  sheet: SUBSYSTEM_SHEET_MAP[subsysHeader],
}));

const componentEntities = new Map<string, {
  id: string;
  kind: "component";
  naturalKey: string;
  name: string;
}>();

for (const { subsysHeader, sheet } of componentSheets) {
  const sheetData = n2Wb.Sheets[sheet];
  if (!sheetData) {
    throw new Error(`[ETL-03] Component N2 sheet "${sheet}" not found in ${N2_XLSX_FILE}`);
  }
  const compRows = XLSX.utils.sheet_to_json<unknown[]>(sheetData, {
    header: 1,
    blankrows: false,
  }) as unknown[][];

  // Roster = header cells minus "Source / Destination" and "External"
  const compHeader = (compRows[0] as unknown[]).map((h) => String(h ?? "").trim());
  const roster = compHeader.filter((h) => h.length > 0 && !N2_SKIP_HEADERS.has(h));

  const expectedRosterCount = (N2_SHEETS as Record<string, { expectedParticipants: number }>)[sheet]?.expectedParticipants;
  if (expectedRosterCount === undefined) {
    throw new Error(`[ETL-03] No expectedParticipants in N2_SHEETS for sheet "${sheet}"`);
  }
  if (roster.length !== expectedRosterCount) {
    throw new Error(
      `[ETL-03] Component N2 sheet "${sheet}" roster: expected ${expectedRosterCount} components, got ${roster.length}`
    );
  }
  totalComponents += roster.length;

  // Fill subsystem's componentIds (the array is shared via reference)
  const subsysEntry = subsystems.find((s) => s.naturalKey === subsysHeader);
  if (!subsysEntry) {
    throw new Error(`[ETL-03] No subsystem entity found for header "${subsysHeader}"`);
  }

  for (const compName of roster) {
    // Component naturalKey is the VERBATIM cell — "Transciever"/"Reciever" misspellings included
    const compEntity = {
      id: stableId("component", compName),
      kind: "component" as const,
      naturalKey: compName,
      name: compName,
    };
    if (!componentEntities.has(compName)) {
      componentEntities.set(compName, compEntity);
    }
    subsysEntry.componentIds.push(compEntity.id);
  }

  // Extract component-scope N2 triples
  const compTriples = extractN2Triples(compRows);
  for (const t of compTriples) {
    const sourceId = t.sourceLabel === "External"
      ? stableId("external", "External")
      : stableId("component", t.sourceLabel);
    const targetId = t.targetLabel === "External"
      ? stableId("external", "External")
      : stableId("component", t.targetLabel);
    const tripleNaturalKey = `component:${t.sourceLabel}->${t.targetLabel}:${t.flow}`;
    const tripleId = stableId("n2", tripleNaturalKey);
    const cellRef = XLSX.utils.encode_cell({ r: t.rowIndex + 1, c: t.colIndex });
    if (!n2TripleMap.has(tripleId)) {
      n2TripleMap.set(tripleId, {
        id: tripleId,
        kind: "n2",
        scope: "component",
        sourceId,
        targetId,
        sourceLabel: t.sourceLabel,
        targetLabel: t.targetLabel,
        flow: t.flow,
        provenance: { workbook: N2_XLSX_FILE, sheet, row: t.rowIndex, cell: cellRef },
      });
    }
  }
  console.log(`${sheet} (component scope): ${compTriples.length} triples`);
}

if (totalComponents !== 34) {
  throw new Error(`[ETL-03] components total: expected 34, got ${totalComponents}`);
}

// Sorted component array (order stable for determinism)
const components = Array.from(componentEntities.values()).sort((a, b) =>
  a.naturalKey.localeCompare(b.naturalKey)
);

if (components.length !== 34) {
  throw new Error(`[ETL-03] components unique: expected 34, got ${components.length}`);
}

// ---------------------------------------------------------------------------
// FUNCTIONAL SCOPE — N2 Functional.xlsx :: Internal N2
// ---------------------------------------------------------------------------

const FUNC_N2_FILE = "N2 Functional.xlsx";
const funcN2Wb = XLSX.readFile(path.join(CORPUS_DIR, FUNC_N2_FILE));
const funcN2Rows = XLSX.utils.sheet_to_json<unknown[]>(funcN2Wb.Sheets["Internal N2"], {
  header: 1,
  blankrows: false,
}) as unknown[][];

// Assert header == ["Sender / Receiver", "F1".."F9"] (9 function columns)
const funcHeader = (funcN2Rows[0] as unknown[]).map((h) => String(h ?? "").trim());
const expectedFuncHeader = ["Sender / Receiver", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9"];
if (funcHeader.length !== expectedFuncHeader.length) {
  throw new Error(
    `[ETL-03] Internal N2 header: expected ${expectedFuncHeader.length} columns, got ${funcHeader.length}`
  );
}
for (let i = 0; i < expectedFuncHeader.length; i++) {
  if (funcHeader[i] !== expectedFuncHeader[i]) {
    throw new Error(
      `[ETL-03] Internal N2 header col ${i}: expected "${expectedFuncHeader[i]}", got "${funcHeader[i]}"`
    );
  }
}

// Assert exactly 9 data rows
const funcDataRows = funcN2Rows.slice(1);
if (funcDataRows.length !== 9) {
  throw new Error(`[ETL-03] Internal N2 data rows: expected 9, got ${funcDataRows.length}`);
}

// CORPUS ANOMALY 2: the position-9 sender row is mislabeled "F8: Provide Power" (duplicate F8).
// Key sender by POSITION (header[i+1]), NEVER by parsing the row label.
// Assertion: exactly ONE row has parseActivityId(rowLabel) != its positional column id
let mislabeledRowCount = 0;
for (let i = 0; i < funcDataRows.length; i++) {
  const rowLabel = String((funcDataRows[i] as unknown[])[0] ?? "").trim();
  const positionalId = funcHeader[i + 1]; // header[1..9] = F1..F9
  const parsedId = parseActivityId(rowLabel);
  if (parsedId !== positionalId) {
    mislabeledRowCount++;
  }
}
// Throw if 0 (corpus fixed — position-keying comment needs revisiting) or >1 (new anomaly)
if (mislabeledRowCount === 0) {
  throw new Error(
    `[ETL-03] Internal N2 mislabeled-row assertion: expected exactly 1 mislabeled row ` +
    `(CORPUS ANOMALY 2 — "F8: Provide Power" row should be F9). Got 0 — ` +
    `if the corpus was fixed, remove the position-keying workaround and repin.`
  );
}
if (mislabeledRowCount > 1) {
  throw new Error(
    `[ETL-03] Internal N2 mislabeled-row assertion: expected exactly 1, got ${mislabeledRowCount}. ` +
    `New corpus anomaly — review and update the extractor.`
  );
}

// Extract functional triples — sender keyed by header position, not row label
const funcRawTriples = extractN2Triples(funcN2Rows);
// Map to functional scope triples: sourceId = stableId("function", header[rowIndex+1])
for (const t of funcRawTriples) {
  // rowIndex is 0-based data index; positional function id = header[rowIndex+1]
  const positionalSourceId = funcHeader[t.rowIndex + 1];
  const sourceId = stableId("function", positionalSourceId);
  // targetId resolves to Task-2 function entities (including synthesized F9)
  const targetId = stableId("function", t.targetLabel);
  const tripleNaturalKey = `functional:${positionalSourceId}->${t.targetLabel}:${t.flow}`;
  const tripleId = stableId("n2", tripleNaturalKey);
  const cellRef = XLSX.utils.encode_cell({ r: t.rowIndex + 1, c: t.colIndex });
  if (!n2TripleMap.has(tripleId)) {
    n2TripleMap.set(tripleId, {
      id: tripleId,
      kind: "n2",
      scope: "functional",
      sourceId,
      targetId,
      sourceLabel: positionalSourceId, // clean positional id, not the mislabeled row label
      targetLabel: t.targetLabel,
      flow: t.flow,
      provenance: { workbook: FUNC_N2_FILE, sheet: "Internal N2", row: t.rowIndex, cell: cellRef },
    });
  }
}
console.log(`Internal N2 (functional scope): ${funcRawTriples.length} triples`);

// External N2: deferred (A4) — actor<->function flows are not in ETL-01 enumeration
// and cells use a different Send:/Receive: verb convention
console.log("External N2: deferred (A4)");

// Assert each scope yielded > 0 triples
const ssCount = Array.from(n2TripleMap.values()).filter((t) => t.scope === "subsystem").length;
const compCount = Array.from(n2TripleMap.values()).filter((t) => t.scope === "component").length;
const funcCount = Array.from(n2TripleMap.values()).filter((t) => t.scope === "functional").length;
const extCount = Array.from(n2TripleMap.values()).filter((t) => t.scope === "external").length;

if (ssCount === 0) throw new Error("[ETL-02] n2Interfaces: subsystem scope yielded 0 triples");
if (compCount === 0) throw new Error("[ETL-02] n2Interfaces: component scope yielded 0 triples");
if (funcCount === 0) throw new Error("[ETL-02] n2Interfaces: functional scope yielded 0 triples");
if (extCount !== 0) {
  throw new Error(`[ETL-03] n2Interfaces: expected 0 external-scope triples, got ${extCount}`);
}

console.log(`n2Interfaces scope totals: subsystem=${ssCount}, component=${compCount}, functional=${funcCount}, external=${extCount}`);

// Sort n2Interfaces by id (for determinism)
const n2Interfaces = Array.from(n2TripleMap.values()).sort((a, b) => a.id.localeCompare(b.id));

// ---------------------------------------------------------------------------
// WRITE BOUNDARY (IR-01/IR-02)
// ---------------------------------------------------------------------------

const assembled = {
  schema_version: SCHEMA_VERSION,
  // "ANGARS" scopes the whole system (the legacy field now covers all subsystems)
  subsystem: "ANGARS",
  needs,
  requirements,
  functions,
  components,
  satisfies,
  // allocations: [] — no corpus Func->Comp source; allocations are model-asserted
  // downstream in Phase 4 and flagged as such (same note as legacy cc extractor)
  allocations: [] as never[],
  subsystems,
  n2Interfaces,
  kpps,
  behaviorDecomp,
};

// IR-01: ExtractedSchema.parse validates the full document BEFORE any file write
const out = ExtractedSchema.parse(assembled);

fs.mkdirSync(OUTPUT_DIR, { recursive: true });
fs.writeFileSync(OUTPUT_FILE, JSON.stringify(out, null, 2), "utf-8");

// ---------------------------------------------------------------------------
// Summary (full extraction complete)
// ---------------------------------------------------------------------------

console.log("=== ANGARS Full-Corpus Extraction Complete ===");
console.log(`needs         : ${out.needs.length}`);
console.log(`requirements  : ${out.requirements.length}`);
console.log(`functions     : ${out.functions.length}`);
console.log(`components    : ${out.components.length}`);
console.log(`satisfies     : ${out.satisfies.length}`);
console.log(`allocations   : ${out.allocations.length}`);
console.log(`subsystems    : ${out.subsystems?.length ?? 0}`);
console.log(`n2Interfaces  : ${out.n2Interfaces?.length ?? 0}`);
console.log(`kpps          : ${out.kpps?.length ?? 0}`);
console.log(`behaviorDecomp: ${out.behaviorDecomp?.length ?? 0}`);
