/**
 * check-parity-pairs.ts
 *
 * Probe ↔ reference + render pairing completeness for the notation-parity loop.
 *
 * The parity receipts are a triangle: a probe (`probes/*.sysml`), the viewer's
 * render of it, and a Cameo (or OMG-spec) reference the render is scored against.
 * If any corner is missing the matrix silently lies — a probe with no render, a
 * render row with no reference, a reference file that was deleted. This script
 * asserts the triangle is closed for EVERY probe, and exits non-zero listing the
 * gaps so CI (and Tasks 2-5) can't drift the matrix out from under the probes.
 *
 * For each `probes/<name>.sysml` it asserts:
 *   1. a view spec exists at `probes/views/<name>.json` (non-empty, valid JSON,
 *      every entry has a `file_stem`);
 *   2. every spec `file_stem` has a render row in `docs/reference/parity-matrix.md`
 *      — i.e. `<file_stem>-1.png` appears in some table's `render file` column;
 *   3. every such render row names a reference in its `reference` column that is
 *      EITHER an existing `cameo-notation/*.png` file OR an explicit
 *      `OMG-spec`-table citation (the two forms the plan allows).
 *
 * No probe may be unpaired. Exit 0 only when every probe closes the triangle.
 *
 * Usage: pnpm check:parity   (or: tsx scripts/check-parity-pairs.ts)
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PROBES_DIR = path.join(REPO_ROOT, "probes");
const VIEWS_DIR = path.join(PROBES_DIR, "views");
const MATRIX_FILE = path.join(REPO_ROOT, "docs", "reference", "parity-matrix.md");
const CAMEO_DIR = path.join(REPO_ROOT, "docs", "reference", "cameo-notation");

/** A render file cell is considered "present" when this token appears in the matrix. */
function renderToken(fileStem: string): string {
  return `${fileStem}-1.png`;
}

/** True if `cell` names an existing cameo-notation png or an OMG-spec citation. */
function referenceIsNamed(cell: string): boolean {
  // OMG-spec table citation (e.g. "OMG-spec-table ...", "OMG-spec (...)").
  if (/OMG-spec/i.test(cell)) return true;
  // A cameo-notation/*.png that actually exists on disk.
  const m = cell.match(/cameo-notation\/([A-Za-z0-9._-]+\.png)/);
  if (m) return fs.existsSync(path.join(CAMEO_DIR, m[1]));
  return false;
}

interface SpecEntry {
  file_stem?: unknown;
}

function readProbes(): string[] {
  if (!fs.existsSync(PROBES_DIR)) return [];
  return fs
    .readdirSync(PROBES_DIR, { withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith(".sysml"))
    .map((e) => e.name.replace(/\.sysml$/, ""))
    .sort();
}

// ── main ─────────────────────────────────────────────────────────────────────

const problems: string[] = [];

const probes = readProbes();
if (probes.length === 0) {
  console.error(`ERROR: no probes found under ${path.relative(REPO_ROOT, PROBES_DIR)}`);
  process.exit(1);
}

if (!fs.existsSync(MATRIX_FILE)) {
  console.error(`ERROR: parity matrix not found at ${path.relative(REPO_ROOT, MATRIX_FILE)}`);
  process.exit(1);
}
const matrix = fs.readFileSync(MATRIX_FILE, "utf8");

// Split matrix into table rows so a render token and its reference are checked
// in the SAME row (a render paired with a reference that lives in another row
// would be a false pass).
const matrixRows = matrix.split("\n").filter((l) => l.trimStart().startsWith("|"));

for (const probe of probes) {
  const specPath = path.join(VIEWS_DIR, `${probe}.json`);
  const specRel = path.relative(REPO_ROOT, specPath);

  // (1) spec exists and is well-formed.
  if (!fs.existsSync(specPath)) {
    problems.push(`${probe}: missing view spec ${specRel}`);
    continue;
  }
  let entries: SpecEntry[];
  try {
    entries = JSON.parse(fs.readFileSync(specPath, "utf8"));
  } catch (e) {
    problems.push(`${probe}: view spec ${specRel} is not valid JSON (${(e as Error).message})`);
    continue;
  }
  if (!Array.isArray(entries) || entries.length === 0) {
    problems.push(`${probe}: view spec ${specRel} has no view entries`);
    continue;
  }

  for (const entry of entries) {
    const stem = entry.file_stem;
    if (typeof stem !== "string" || stem.length === 0) {
      problems.push(`${probe}: view spec ${specRel} has an entry with no file_stem`);
      continue;
    }

    // (2) the render is cited somewhere in the matrix.
    const token = renderToken(stem);
    const rowsWithRender = matrixRows.filter((r) => r.includes(token));
    if (rowsWithRender.length === 0) {
      problems.push(
        `${probe}: render '${token}' (from ${specRel}) has no row in docs/reference/parity-matrix.md`,
      );
      continue;
    }

    // (3) at least one render row names a valid reference in the SAME row.
    const paired = rowsWithRender.some((r) => referenceIsNamed(r));
    if (!paired) {
      problems.push(
        `${probe}: render '${token}' is cited but no scored row names a valid reference ` +
          `(need a cameo-notation/*.png that exists, or an OMG-spec citation)`,
      );
    }
  }
}

if (problems.length > 0) {
  console.error(`check:parity — ${problems.length} pairing gap(s):`);
  for (const p of problems) console.error(`  GAP: ${p}`);
  process.exit(1);
}

console.log(
  `check:parity — ${probes.length} probe(s), all paired to a render + reference in parity-matrix.md`,
);
