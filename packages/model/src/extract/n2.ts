// N2 convention: ROW = source, COLUMN = target. Verified against the ANGARS corpus
// 2026-06-09 — Power Subsystem ROW emits "28VDC" across COLUMNS; External ROW sends
// "Operator Commands" into the C&C COLUMN. Diagonals are empty (no self-flow).
export const N2_ROW_IS_SOURCE = true as const;

/** A single directed flow triple extracted from an N2 matrix. */
export interface N2RawTriple {
  /** The row header label — the flow source. */
  sourceLabel: string;
  /** The column header label — the flow target. */
  targetLabel: string;
  /** One flow item (comma-split cells produce multiple triples). */
  flow: string;
  /** 0-based data-row index (header row excluded). */
  rowIndex: number;
  /** 0-based column index in the header row. */
  colIndex: number;
}

/**
 * Extract N2 triples from raw sheet_to_json(header:1) output.
 *
 * `rows` MUST include the header row as rows[0]. Each cell with content other
 * than empty/"−"/"NA"/bare-whitespace is split on "," and emits one triple per
 * item. Diagonal cells are skipped positionally (data row i → skip header col
 * i+1) regardless of label equality, so the Internal N2 mislabeled-F8 row does
 * not cause a false diagonal match.
 */
export function extractN2Triples(rows: unknown[][]): N2RawTriple[] {
  if (rows.length < 2) return [];

  const header = (rows[0] as unknown[]).map((h) => String(h ?? "").trim());
  const triples: N2RawTriple[] = [];

  for (let r = 1; r < rows.length; r++) {
    const sourceLabel = String((rows[r] as unknown[])[0] ?? "").trim();
    if (!sourceLabel) continue; // skip rows with empty source label

    for (let c = 1; c < header.length; c++) {
      const targetLabel = header[c];

      // Skip diagonal positionally — data row r maps to header col r (0-based
      // data index = r-1, so positional diagonal is c === r). This is required
      // because Internal N2 row labels ("F8: Provide Power") never string-equal
      // their column headers ("F8"), so label equality alone is insufficient.
      if (c === r) continue;

      // Defense-in-depth: also skip when labels happen to match (clean symmetric N2s)
      if (sourceLabel === targetLabel) continue;

      const raw = String((rows[r] as unknown[])[c] ?? "")
        .replace(/[\r\n]+/g, " ")
        .trim();

      // Skip empty markers
      if (!raw || raw === "-" || raw === "NA") continue;

      // Split on comma — one triple per flow item (ETL plan locked decision)
      for (const flowItem of raw.split(",").map((s) => s.trim()).filter(Boolean)) {
        triples.push({
          sourceLabel,
          targetLabel,
          flow: flowItem,
          rowIndex: r - 1,   // 0-based data-row index
          colIndex: c,       // 0-based column index in the header
        });
      }
    }
  }

  return triples;
}

/**
 * Ingest guard: asserts that at least one triple matching all three fields
 * exactly exists. Throws an [ETL-02]-tagged Error if not found.
 *
 * Call this after parsing each N2 sheet with the two corpus-verified cells:
 *   assertSpotCheck(triples, "Power Subsystem", "Command & Control Subsystem", "28VDC")
 *   assertSpotCheck(triples, "External", "Command & Control Subsystem", "Operator Commands")
 *
 * A throw indicates the corpus was transposed or the ROW=source convention
 * has regressed — fix the issue before proceeding.
 */
export function assertSpotCheck(
  triples: N2RawTriple[],
  sourceLabel: string,
  targetLabel: string,
  flow: string
): void {
  const found = triples.some(
    (t) =>
      t.sourceLabel === sourceLabel &&
      t.targetLabel === targetLabel &&
      t.flow === flow
  );
  if (!found) {
    throw new Error(
      `[ETL-02] N2 direction spot-check failed: no triple ${sourceLabel} -> ${targetLabel} ` +
        `carrying "${flow}" — corpus transposed or convention regressed`
    );
  }
}
