/**
 * @sysml-bridge/invariants — the reusable core of the repo's four
 * self-enforcing-invariant mechanisms, extracted from ad-hoc re-implementations.
 *
 *   - sourceScanRatchet   — walk the live tree, grep guarded tokens, report
 *                           non-allowlisted call sites (shrink-only allowlist).
 *   - seededDefectHarness — plant known defects, prove each gate catches its
 *                           defect + a non-vacuous clean control.
 *   - pairedControl       — prove a gate can fail: good passes, bad is rejected.
 *   - findFinding/hasFinding — the canonical finding matcher shared by the
 *                           harness and the MCP coupling assertions.
 *
 * Dependency-light: node fs/path only.
 */

export {
  sourceScanRatchet,
  deriveScanRoots,
  defaultIncludeFile,
  defaultIsCallSite,
  type SourceScanRatchetOptions,
  type SourceScanRatchetResult,
} from "./source-scan-ratchet.js";

export {
  seededDefectHarness,
  type SeededDefect,
  type AuditDefect,
  type CustomDefect,
  type SeededDefectRow,
  type SeededDefectHarnessOptions,
  type SeededDefectHarnessResult,
} from "./seeded-defect-harness.js";

export {
  pairedControl,
  type PairedControlOptions,
  type PairedControlResult,
} from "./paired-control.js";

export {
  findFinding,
  hasFinding,
  findingMatches,
  type FindingLike,
  type FindingQuery,
} from "./findings.js";
