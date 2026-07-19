/**
 * seeded-defect-harness.ts — plant KNOWN defects, prove each gate catches ITS
 * defect, and prove the clean control is non-vacuously clean.
 *
 * A gate you never watch catch a defect is a gate you cannot trust. This harness:
 *   1. audits the CLEAN base model and asserts zero error-severity findings
 *      (the non-vacuous clean control — the catches below are real, not because
 *      the auditor flags everything);
 *   2. for each planted defect, transforms the base model and asserts a finding
 *      with the expected rule id (and, when given, the expected element id) —
 *      and, when `soleError` is set, that it is the ONLY error-severity finding,
 *      so a cross-triggering seed fails loudly instead of passing coincidentally;
 *   3. for non-audit gates (e.g. a grammar validator paired control), runs a
 *      caller-supplied `check` that returns whether the defect was caught;
 *   4. reports a summary and an `ok` flag that is true iff EVERY row — clean
 *      control included — was caught. The caller maps `ok` to an exit code.
 *
 * Dependency-light: node only (via the findings matcher). The model, the audit
 * function, and the severity predicate are all caller-supplied, so the harness
 * never depends on the gates package.
 */

import { findFinding, type FindingLike } from "./findings.js";

export interface SeededDefectRow {
  defect: string;
  gate: string;
  ruleOrCheck: string;
  caught: boolean;
  detail: string;
}

/** A defect caught by auditing a planted model for a specific rule/element. */
export interface AuditDefect<Model> {
  defect: string;
  gate?: string;
  ruleOrCheck?: string;
  /** Transform the clean base model into the seeded (defective) model. */
  plant: (base: Model) => Model | Promise<Model>;
  /** The rule id the gate must report for this defect. */
  expectRule: string;
  /** When given, the finding's elementId must equal this. */
  expectElementId?: string;
  /**
   * When true, the expected finding must be the SOLE error-severity finding in
   * the seeded audit: it is present AND no other error finding cross-triggered.
   * An isolated seed then proves "rule X flagged element Y" unambiguously — a
   * seed that also trips a second error rule is reported NOT caught (with the
   * offending extra findings named) rather than passing on a coincidental catch.
   * Only meaningful for error-severity defects; a warning/info defect leaves this
   * unset (the seeded audit yields zero error findings by design).
   */
  soleError?: boolean;
}

/** A defect caught by a caller-supplied check (non-audit gate). */
export interface CustomDefect {
  defect: string;
  gate?: string;
  ruleOrCheck?: string;
  /** Returns true iff the defect was caught. */
  check: () => boolean | Promise<boolean>;
  /** Optional detail-line renderer for the summary. */
  detail?: (caught: boolean) => string;
}

export type SeededDefect<Model> = AuditDefect<Model> | CustomDefect;

export interface SeededDefectHarnessOptions<Model, F extends FindingLike> {
  /** The clean base model (or an async/sync factory for it). */
  base: Model | (() => Model | Promise<Model>);
  /** Audit a model → findings. Used for the clean control and every audit defect. */
  audit: (model: Model) => F[] | Promise<F[]>;
  /** True iff a finding is error-severity (the clean control must yield zero of these). */
  isError: (finding: F) => boolean;
  /** Labels for the clean-control summary row. */
  cleanControl?: { defect?: string; gate?: string; ruleOrCheck?: string };
  defects: SeededDefect<Model>[];
  /** Summary line sink. Default: console.log. */
  logger?: (line: string) => void;
  /** Emit the summary table. Default: true. */
  printSummary?: boolean;
}

export interface SeededDefectHarnessResult {
  rows: SeededDefectRow[];
  cleanControlOk: boolean;
  caughtCount: number;
  defectCount: number;
  /** True iff every row (clean control + all defects) was caught. */
  ok: boolean;
}

function isCustomDefect<Model>(d: SeededDefect<Model>): d is CustomDefect {
  return typeof (d as CustomDefect).check === "function";
}

export async function seededDefectHarness<Model, F extends FindingLike>(
  opts: SeededDefectHarnessOptions<Model, F>
): Promise<SeededDefectHarnessResult> {
  const log = opts.logger ?? ((l: string) => console.log(l));
  const rows: SeededDefectRow[] = [];

  const base =
    typeof opts.base === "function"
      ? await (opts.base as () => Model | Promise<Model>)()
      : opts.base;

  // --- Non-vacuous clean control: audit the clean base → zero error findings. --
  const cleanFindings = await opts.audit(base);
  const cleanErrors = cleanFindings.filter((f) => opts.isError(f));
  const cleanControlOk = cleanErrors.length === 0;
  rows.push({
    defect: opts.cleanControl?.defect ?? "(clean control — no defects)",
    gate: opts.cleanControl?.gate ?? "audit()",
    ruleOrCheck: opts.cleanControl?.ruleOrCheck ?? "zero error-severity findings",
    caught: cleanControlOk,
    detail: cleanControlOk
      ? "0 error findings"
      : `UNEXPECTED ${cleanErrors.length} error finding(s): ` +
        cleanErrors.map((f) => `${f.ruleId}@${f.elementId}`).join(", "),
  });

  // --- Each planted defect: the gate must catch ITS defect. --------------------
  for (const d of opts.defects) {
    if (isCustomDefect(d)) {
      const caught = await d.check();
      rows.push({
        defect: d.defect,
        gate: d.gate ?? "",
        ruleOrCheck: d.ruleOrCheck ?? "",
        caught,
        detail: d.detail ? d.detail(caught) : caught ? "caught" : "MISSED",
      });
    } else {
      const seeded = await d.plant(base);
      const findings = await opts.audit(seeded);
      let caught: boolean;
      let detail: string;
      if (d.soleError) {
        // The defect must be the SOLE error-severity finding: the expected
        // finding is present AND no other error finding cross-triggered.
        const errors = findings.filter((f) => opts.isError(f));
        const hit = findFinding(errors, {
          ruleId: d.expectRule,
          elementId: d.expectElementId,
        });
        const others = errors.filter((f) => f !== hit);
        if (hit && others.length === 0) {
          caught = true;
          detail = `finding elementId=${hit.elementId} severity=${hit.severity} (sole error)`;
        } else if (hit && others.length > 0) {
          caught = false;
          detail =
            `${d.expectRule}` +
            (d.expectElementId ? `@${d.expectElementId}` : "") +
            ` present but CROSS-TRIGGERED ${others.length} other error(s): ` +
            others.map((f) => `${f.ruleId}@${f.elementId}`).join(", ");
        } else {
          caught = false;
          detail =
            `NO ${d.expectRule} finding` +
            (d.expectElementId ? ` for elementId=${d.expectElementId}` : "") +
            (errors.length
              ? ` (errors present: ${errors
                  .map((f) => `${f.ruleId}@${f.elementId}`)
                  .join(", ")})`
              : "");
        }
      } else {
        const hit = findFinding(findings, {
          ruleId: d.expectRule,
          elementId: d.expectElementId,
        });
        caught = hit !== undefined;
        detail = hit
          ? `finding elementId=${hit.elementId} severity=${hit.severity}`
          : `NO ${d.expectRule} finding` +
            (d.expectElementId ? ` for elementId=${d.expectElementId}` : "");
      }
      rows.push({
        defect: d.defect,
        gate: d.gate ?? "audit()",
        ruleOrCheck: d.ruleOrCheck ?? d.expectRule,
        caught,
        detail,
      });
    }
  }

  const defectRows = rows.slice(1); // everything after the clean control
  const caughtCount = defectRows.filter((r) => r.caught).length;
  const ok = rows.every((r) => r.caught);

  if (opts.printSummary !== false) {
    emitSummary(rows, log);
    log(
      `\nDefects caught: ${caughtCount}/${defectRows.length}   ` +
        `Clean control: ${cleanControlOk ? "CLEAN" : "DIRTY"}`
    );
    log(
      ok
        ? "\nSEEDED-DEFECT HARNESS PASS — every gate caught its planted defect; clean control clean."
        : "\nSEEDED-DEFECT HARNESS FAIL — a gate did not catch its planted defect " +
            "OR the clean control produced an unexpected finding. See the table above."
    );
  }

  return {
    rows,
    cleanControlOk,
    caughtCount,
    defectCount: defectRows.length,
    ok,
  };
}

function emitSummary(rows: SeededDefectRow[], log: (line: string) => void): void {
  log("\n============================================================================");
  log("  SEEDED-DEFECT SUMMARY");
  log("============================================================================");
  const header = ["defect", "gate", "rule / check"];
  log(`  ${header[0].padEnd(56)} ${header[1].padEnd(16)} ${header[2].padEnd(38)} caught?`);
  log("  " + "-".repeat(120));
  for (const r of rows) {
    log(
      `  ${r.defect.padEnd(56)} ${r.gate.padEnd(16)} ${r.ruleOrCheck.padEnd(38)} ${
        r.caught ? "YES" : "NO"
      }`
    );
    log(`      ↳ ${r.detail}`);
  }
  log("============================================================================");
}
