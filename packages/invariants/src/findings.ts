/**
 * findings.ts — a tiny, canonical matcher for "does this finding set contain a
 * finding flagging rule X (optionally against element Y)?"
 *
 * Both the seeded-defect harness (find by ruleId + elementId) and the MCP
 * write-path coupling tests (some by ruleId) re-implemented this predicate by
 * hand. One chokepoint keeps the semantics identical everywhere.
 *
 * Structural (not nominal) typing: any object with the relevant fields matches,
 * so this stays dependency-light and never imports the gates `Finding` type.
 */

export interface FindingLike {
  ruleId?: string;
  elementId?: string | null;
  severity?: string;
}

export interface FindingQuery {
  ruleId?: string;
  /** When provided, matches against `finding.elementId` exactly. */
  elementId?: string;
  severity?: string;
}

/** True iff `finding` matches every field present in `query`. */
export function findingMatches(finding: FindingLike, query: FindingQuery): boolean {
  if (query.ruleId !== undefined && finding.ruleId !== query.ruleId) return false;
  if (query.elementId !== undefined && finding.elementId !== query.elementId) return false;
  if (query.severity !== undefined && finding.severity !== query.severity) return false;
  return true;
}

/** The first finding matching `query`, or undefined. */
export function findFinding<F extends FindingLike>(
  findings: readonly F[],
  query: FindingQuery
): F | undefined {
  return findings.find((f) => findingMatches(f, query));
}

/** Whether any finding matches `query`. */
export function hasFinding(findings: readonly FindingLike[], query: FindingQuery): boolean {
  return findings.some((f) => findingMatches(f, query));
}
