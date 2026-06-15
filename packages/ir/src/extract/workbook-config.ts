/**
 * workbook-config.ts
 *
 * Declarative configuration tables for all 9 ANGARS corpus workbooks.
 * Pure data module — no xlsx import, no I/O. All counts are corpus-pinned
 * (observed 2026-06-09 against the real files; see 02-RESEARCH.md §Authoritative Sheet Map).
 *
 * ETL-01: every workbook entry here covers one of the 9 source files.
 * ETL-03: the `expected` field on each WORKBOOKS entry is the count assertion contract.
 */

// ---------------------------------------------------------------------------
// Row-table workbook config
// ---------------------------------------------------------------------------

export interface WorkbookConfig {
  /** Workbook filename (relative to CORPUS_DIR). */
  file: string;
  /** Authoritative sheet name. */
  sheet: string;
  /** 0-based row index of the header row. */
  headerRow: number;
  /** Expected number of DATA rows (header excluded). Drives ETL-03 assertCount. */
  expected: number;
  /** Column-index map for fields this workbook contributes. */
  cols: Record<string, number>;
}

/**
 * WORKBOOKS — one entry per row-table sheet in the 9 ANGARS workbooks.
 * Entries are ordered to match the canonical extraction sequence.
 *
 * NOTE on entries 8-9 (Top-Level Report workbooks):
 *   02-RESEARCH says "4 data rows" — that is WRONG. Both Report sheets are
 *   ref A1:I5 = title row + header row + 3 data rows. Pin 3, not 4. [VERIFIED live]
 */
export const WORKBOOKS = [
  // 1. Needs — 16 rows (N1..N16)
  {
    file: "ANGARS Needs FINAL.xlsx",
    sheet: "Sheet1",
    headerRow: 0,
    expected: 16,
    cols: { id: 0, name: 1, category: 2, description: 3 },
  },
  // 2. Requirements FINAL — 165 rows (authoritative req set)
  {
    file: "ANGARS Requirements FINAL.xlsx",
    sheet: "Final",
    headerRow: 0,
    expected: 165,
    cols: { id: 1, needRefs: 2, category: 3, type: 4, name: 5, statement: 6, verify: 7 },
  },
  // 3. ASpec Sheet3 — 174 rows (A-Spec requirements, normalized; overlaps FINAL)
  {
    file: "ASpec.xlsx",
    sheet: "Sheet3",
    headerRow: 0,
    expected: 174,
    cols: { id: 0, needRefs: 1, category: 2, type: 3, name: 4, statement: 5, verify: 6 },
  },
  // 4. ASpec Sheet5 — 7 rows (ASPEC-011..ASPEC-017; -ilities/HSI/Interface allocation)
  //    col8 is an UNHEADERED trailing column carrying the need allocation
  //    (corpus-verified: N7, N11, N13, N12, N12, N15, N15)
  {
    file: "ASpec.xlsx",
    sheet: "Sheet5",
    headerRow: 0,
    expected: 7,
    cols: { id: 0, category: 1, statement: 3, verify: 6, need: 8 },
  },
  // 5. Satisfied By — 154 rows (req->activity map; the "155" in the brief counts the header)
  {
    file: "ANGARS Requirements-Functions.xlsx",
    sheet: "Satisfied By",
    headerRow: 0,
    expected: 154,
    cols: { reqId: 0, activity: 3 },
  },
  // 6. All Behaviors — 65 raw data rows (L1=1, L2=8, L3=54, blank=2); filtered to 62 non-empty ID rows
  {
    file: "ANGARS Requirements-Functions.xlsx",
    sheet: "All Behaviors",
    headerRow: 0,
    expected: 65,
    cols: { id: 1, level: 2, name: 3, owner: 4 },
  },
  // 7. KPP Justifications — 10 rows (ANGARS-2,-27,-31,-43,-44,-53,-56,-62,-91,-125)
  {
    file: "KPP Justifications.xlsx",
    sheet: "Sheet3",
    headerRow: 0,
    expected: 10,
    cols: { id: 0, title: 1 },
  },
  // 8. Top-Level KPP Requirements — 3 data rows (headerRow=1; row0=title banner)
  {
    file: "Top-Level KPP Requirements.xlsx",
    sheet: "Report",
    headerRow: 1,
    expected: 3,
    cols: { name: 1 },
  },
  // 9. Top-Level Mission Requirements — 3 data rows (headerRow=1; row0=title banner)
  {
    file: "Top-Level Mission Requirements.xlsx",
    sheet: "Report",
    headerRow: 1,
    expected: 3,
    cols: { id: 1 },
  },
] as const satisfies WorkbookConfig[];

// ---------------------------------------------------------------------------
// N2 matrix sheets
// ---------------------------------------------------------------------------

export interface N2SheetConfig {
  /** Workbook filename (relative to CORPUS_DIR). Defaults to "Interface Data N2.xlsx". */
  file?: string;
  /** Sheet scope classification. */
  scope: "subsystem" | "component" | "functional";
  /**
   * Expected number of participant (roster) entries — header columns excluding
   * "Source / Destination" and "External". Used for roster-count assertions.
   * For the subsystem ANGARS SS scope this is 7 (6 subsystems + External), which
   * is tracked separately via ANGARS_SS_HEADERS.
   */
  expectedParticipants: number;
}

/**
 * N2_SHEETS — configuration for the 8 N2 matrix sheets.
 *
 * Component roster counts (External excluded) verified 2026-06-09:
 *   AGNS: 8, C&C: 6, Comms: 6, Fuel Xfer: 6, Power: 4, Processing: 4 = 34 total
 */
export const N2_SHEETS = {
  // Subsystem-level N2 (Interface Data N2.xlsx)
  "ANGARS SS": {
    scope: "subsystem",
    expectedParticipants: 7, // 6 subsystems + External
  },
  // Per-subsystem component N2 sheets (Interface Data N2.xlsx)
  "AGNS": { scope: "component", expectedParticipants: 8 },
  "C&C": { scope: "component", expectedParticipants: 6 },
  "Comms": { scope: "component", expectedParticipants: 6 },
  "Fuel Xfer": { scope: "component", expectedParticipants: 6 },
  "Power": { scope: "component", expectedParticipants: 4 },
  "Processing": { scope: "component", expectedParticipants: 4 },
  // Functional N2 (N2 Functional.xlsx)
  "Internal N2": {
    file: "N2 Functional.xlsx",
    scope: "functional",
    expectedParticipants: 9, // F1..F9
  },
} as const satisfies Record<string, N2SheetConfig>;

// ---------------------------------------------------------------------------
// Subsystem header labels (ANGARS SS cols 1..7) — corpus-pinned verbatim
// ---------------------------------------------------------------------------

/**
 * ANGARS_SS_HEADERS — the verbatim column header labels for ANGARS SS (cols 1..7).
 * The SUBSYSTEM_SHEET_MAP keys depend on exact string equality with these values.
 * Any corpus drift in the header row will cause the header-verbatim assertion to throw.
 */
export const ANGARS_SS_HEADERS = [
  "External",
  "Autonomous Guidance & Navigation Sensors Subsystem",
  "Command & Control Subsystem",
  "Fuel Transfer Subsystem",
  "Power Subsystem",
  "Comms Subsystem",
  "Processing Subsystem",
] as const;

// ---------------------------------------------------------------------------
// Subsystem header -> component N2 sheet name map
// ---------------------------------------------------------------------------

/**
 * SUBSYSTEM_SHEET_MAP — maps the verbatim ANGARS SS column header for each
 * subsystem to the per-subsystem component N2 sheet name in Interface Data N2.xlsx.
 * "External" is an environment actor and has no component N2 sheet.
 */
export const SUBSYSTEM_SHEET_MAP: Record<string, string> = {
  "Autonomous Guidance & Navigation Sensors Subsystem": "AGNS",
  "Command & Control Subsystem": "C&C",
  "Fuel Transfer Subsystem": "Fuel Xfer",
  "Power Subsystem": "Power",
  "Comms Subsystem": "Comms",
  "Processing Subsystem": "Processing",
};
