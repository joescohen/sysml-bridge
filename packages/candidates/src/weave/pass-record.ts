/**
 * pass-record.ts — the pass-record envelope + audit summary + convergence gate (W3).
 *
 * A closed gap-driven pass writes `<out>/passes/pass-NNN.json`, envelope
 * `sysml-foundry/pass-record@1`, carrying the SIX fields the spec fixes
 * (§5, §8 W3): `{auditBefore, queries, candidatesProposed, dispositionsApplied,
 * auditAfter, warningsDelta}`. Persistence mirrors `../mentions/index.ts` /
 * `../chunk-store/index.ts` EXACTLY: self-describing envelope, deterministic
 * serialization, throw-on-malformed load — a corrupt pass record must fail
 * loudly, never silently yield an empty history.
 *
 * Convergence discipline (spec §5):
 *   - HARD gate — a closed pass must end with ZERO error-severity findings and
 *     may NEVER end with more errors than it began. `evaluateConvergence`
 *     computes the verdict; the CLI turns a non-ok verdict into a non-zero exit.
 *   - SOFT signal — the completeness-warning delta per rule id is REPORTED, not
 *     gated (a legitimately growing model creates new orphans; the record makes
 *     the trend inspectable instead of pretending monotonicity).
 *
 * Pure — no I/O beyond the read/write helpers at the bottom.
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";

import type { WeaveFinding, WeaveQuery } from "./gap-queue.js";

// ── Audit summary ───────────────────────────────────────────────────────────

/** Per-rule severity counts. */
export interface RuleCounts {
  error: number;
  warning: number;
  info: number;
}

/** A compact, comparable snapshot of an audit's findings. */
export interface AuditSummary {
  errorCount: number;
  warningCount: number;
  infoCount: number;
  /** ruleId → severity counts. Sorted keys for deterministic serialization. */
  byRuleId: Record<string, RuleCounts>;
}

/** Summarize findings into an `AuditSummary` (pure, deterministic). */
export function summarizeAudit(findings: readonly WeaveFinding[]): AuditSummary {
  let errorCount = 0;
  let warningCount = 0;
  let infoCount = 0;
  const byRuleId: Record<string, RuleCounts> = {};

  for (const f of findings) {
    const rc = (byRuleId[f.ruleId] ??= { error: 0, warning: 0, info: 0 });
    if (f.severity === "error") {
      errorCount++;
      rc.error++;
    } else if (f.severity === "warning") {
      warningCount++;
      rc.warning++;
    } else {
      infoCount++;
      rc.info++;
    }
  }

  // Re-key in sorted order for byte-stable output.
  const sorted: Record<string, RuleCounts> = {};
  for (const key of Object.keys(byRuleId).sort()) sorted[key] = byRuleId[key]!;

  return { errorCount, warningCount, infoCount, byRuleId: sorted };
}

// ── Pass-record fields ────────────────────────────────────────────────────────

/** One proposed candidate, attributed to the gap(s) it targets. */
export interface ProposedCandidateSummary {
  id: string;
  relationFamily: string;
  sourceId: string;
  targetId: string;
  /** Gap element ids whose query scope this candidate falls within (≥1). */
  targetsGapElementIds: string[];
}

/** One human disposition applied between the open pass and its close. */
export interface DispositionSummary {
  candidateId: string;
  disposition: "approved" | "rejected";
  layer: string;
}

/** The soft signal: warning delta for one rule id across the pass. */
export interface WarningsDeltaEntry {
  ruleId: string;
  before: number;
  after: number;
  delta: number;
}

/** The six-field pass record. */
export interface PassRecord {
  auditBefore: AuditSummary;
  queries: WeaveQuery[];
  candidatesProposed: ProposedCandidateSummary[];
  dispositionsApplied: DispositionSummary[];
  auditAfter: AuditSummary;
  warningsDelta: WarningsDeltaEntry[];
}

/**
 * Compute the per-rule warning delta (SOFT signal). Deterministic: the union of
 * rule ids present in either summary, sorted, each with before/after/delta.
 * Only warning-severity counts participate.
 */
export function computeWarningsDelta(
  before: AuditSummary,
  after: AuditSummary,
): WarningsDeltaEntry[] {
  const ruleIds = new Set<string>([
    ...Object.keys(before.byRuleId),
    ...Object.keys(after.byRuleId),
  ]);
  const out: WarningsDeltaEntry[] = [];
  for (const ruleId of [...ruleIds].sort()) {
    const b = before.byRuleId[ruleId]?.warning ?? 0;
    const a = after.byRuleId[ruleId]?.warning ?? 0;
    if (b === 0 && a === 0) continue; // not a warning rule — skip
    out.push({ ruleId, before: b, after: a, delta: a - b });
  }
  return out;
}

// ── Convergence gate (HARD) ─────────────────────────────────────────────────

export interface ConvergenceVerdict {
  /** True iff the pass converged: zero errors AND errors did not increase. */
  ok: boolean;
  errorsBefore: number;
  errorsAfter: number;
  /** after.error > before.error — a hard violation. */
  errorsIncreased: boolean;
  /** after.error > 0 — the pass did not reach zero errors (hard target). */
  endsWithErrors: boolean;
}

/**
 * The HARD convergence gate. A closed pass converges iff it ends with zero
 * error-severity findings AND never ends with more errors than it began. A
 * non-ok verdict must become a non-zero CLI exit.
 */
export function evaluateConvergence(
  before: AuditSummary,
  after: AuditSummary,
): ConvergenceVerdict {
  const errorsIncreased = after.errorCount > before.errorCount;
  const endsWithErrors = after.errorCount > 0;
  return {
    ok: !errorsIncreased && !endsWithErrors,
    errorsBefore: before.errorCount,
    errorsAfter: after.errorCount,
    errorsIncreased,
    endsWithErrors,
  };
}

// ── Persistence — mirrors ../mentions/index.ts exactly ────────────────────────

/** Schema tag stamped into every pass-NNN.json envelope. */
export const PASS_RECORD_SCHEMA = "sysml-foundry/pass-record@1";

/** The on-disk envelope wrapping one pass record. */
export interface PassRecordFile {
  schema: typeof PASS_RECORD_SCHEMA;
  generatedAt: string;
  passNumber: number;
  record: PassRecord;
}

/** Zero-pad a pass number to the `pass-NNN.json` filename convention. */
export function passFileName(passNumber: number): string {
  return `pass-${String(passNumber).padStart(3, "0")}.json`;
}

/**
 * Serialize a pass record to the on-disk envelope JSON string. Byte-stable for
 * a fixed `generatedAt` (tests pass a fixed value).
 */
export function serializePassRecord(
  passNumber: number,
  record: PassRecord,
  generatedAt: Date = new Date(),
): string {
  const file: PassRecordFile = {
    schema: PASS_RECORD_SCHEMA,
    generatedAt: generatedAt.toISOString(),
    passNumber,
    record,
  };
  return `${JSON.stringify(file, null, 2)}\n`;
}

/** Assert every one of the six record fields is present + correctly shaped. */
function assertRecord(value: unknown): PassRecord {
  if (typeof value !== "object" || value === null) {
    throw new Error("pass-record: 'record' is not an object");
  }
  const rec = value as Record<string, unknown>;
  for (const key of [
    "auditBefore",
    "queries",
    "candidatesProposed",
    "dispositionsApplied",
    "auditAfter",
    "warningsDelta",
  ] as const) {
    if (!(key in rec)) {
      throw new Error(`pass-record: record.${key} is missing`);
    }
  }
  if (typeof rec["auditBefore"] !== "object" || rec["auditBefore"] === null) {
    throw new Error("pass-record: record.auditBefore must be an object");
  }
  if (typeof rec["auditAfter"] !== "object" || rec["auditAfter"] === null) {
    throw new Error("pass-record: record.auditAfter must be an object");
  }
  for (const key of ["queries", "candidatesProposed", "dispositionsApplied", "warningsDelta"] as const) {
    if (!Array.isArray(rec[key])) {
      throw new Error(`pass-record: record.${key} must be an array`);
    }
  }
  return rec as unknown as PassRecord;
}

/**
 * Parse a pass-NNN.json string into a validated `PassRecordFile`. Throws on a
 * malformed envelope or a record missing any of the six fields — a corrupt pass
 * record fails loudly rather than silently degrading the pass history.
 */
export function parsePassRecord(json: string): PassRecordFile {
  let decoded: unknown;
  try {
    decoded = JSON.parse(json);
  } catch (err) {
    throw new Error(
      `pass-record: file is not valid JSON: ${err instanceof Error ? err.message : String(err)}`,
    );
  }
  if (typeof decoded !== "object" || decoded === null) {
    throw new Error("pass-record: top-level value is not an object");
  }
  const env = decoded as Record<string, unknown>;
  if (env["schema"] !== PASS_RECORD_SCHEMA) {
    throw new Error(
      `pass-record: unexpected schema '${String(env["schema"])}' (want '${PASS_RECORD_SCHEMA}')`,
    );
  }
  if (typeof env["generatedAt"] !== "string") {
    throw new Error("pass-record: 'generatedAt' must be a string");
  }
  if (typeof env["passNumber"] !== "number") {
    throw new Error("pass-record: 'passNumber' must be a number");
  }
  const record = assertRecord(env["record"]);
  return {
    schema: PASS_RECORD_SCHEMA,
    generatedAt: env["generatedAt"] as string,
    passNumber: env["passNumber"] as number,
    record,
  };
}

/** Write a pass record to `filePath` (creating parent dirs). Byte-stable. */
export async function writePassRecordFile(
  filePath: string,
  passNumber: number,
  record: PassRecord,
  generatedAt?: Date,
): Promise<void> {
  await mkdir(dirname(filePath), { recursive: true });
  await writeFile(filePath, serializePassRecord(passNumber, record, generatedAt), "utf8");
}

/** Load + validate a pass-NNN.json file. */
export async function loadPassRecordFile(filePath: string): Promise<PassRecordFile> {
  const json = await readFile(filePath, "utf8");
  return parsePassRecord(json);
}
