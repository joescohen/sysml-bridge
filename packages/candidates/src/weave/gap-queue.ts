/**
 * gap-queue.ts — the finding→query table (W3).
 *
 * A gap-driven pass runs `audit()` on the composed model, then maps each
 * completeness finding to a targeted retrieval query via THIS table. The table
 * is DATA (one object, `GAP_QUERY_TABLE`), unit-tested, and the mapping function
 * is pure — no I/O, no engine coupling.
 *
 * The three mapped completeness findings (spec §5):
 *   - GATE02-unsatisfied(reqR)     → satisfy-family query from R's name+text,
 *                                     scoped to the entities that query retrieves.
 *   - GATE02-orphan(elemE)         → allocation-family query for E.
 *   - GATE02-uncovered-need(needN) → derive-family query for N.
 *
 * A finding whose ruleId is NOT in the table is NEVER silently skipped: it is
 * collected into `unmappedFindings` and REPORTED by the caller (spec §8 W3 done-
 * criterion, "unknown finding ids are reported, not silently skipped"). New
 * completeness rules therefore surface loudly until a query strategy is added,
 * instead of being quietly ignored.
 *
 * This module lives OUTSIDE `prose/` and `scripts/` (like chunk-store /
 * mentions / entities) so the C5 no-retrieval grep-test stays green, and it
 * takes only the STRUCTURAL shape of a finding (`WeaveFinding`) so `packages/
 * candidates` keeps no runtime dependency on `packages/gates`.
 */

// Type-only import: no runtime coupling, but binds QueryFamily→RelationFamily so
// a drift between the weave gap families and the engine families fails to compile.
import type { RelationFamily } from "../inference/types.js";

/** Structural view of a gates `Finding` — shape only, no gates dependency. */
export interface WeaveFinding {
  elementId: string;
  ruleId: string;
  message: string;
  severity: "error" | "warning" | "info";
  suggestedFix: string;
}

/**
 * The retrieval family a query targets. NOT the engine's `RelationFamily` (the
 * engine enumerates `allocation|modeMembership|flowTyping|controlJoin`); this is
 * the CONCEPTUAL trace family the completeness gap is missing, used to label the
 * query and to scope which downstream candidates count as closing the gap.
 */
export type QueryFamily = "satisfy" | "allocation" | "derive";

/** One targeted query derived from a completeness finding. */
export interface WeaveQuery {
  /** The finding ruleId this query was derived from (e.g. "GATE02-unsatisfied"). */
  findingRuleId: string;
  /** The gap element's id — the thing the proposed candidates must target. */
  gapElementId: string;
  /** The gap element's human name (for the query text + logs), or null. */
  gapElementName: string | null;
  /** Conceptual trace family the gap is missing. */
  family: QueryFamily;
  /** BM25 query text assembled from the gap element's name + text. */
  bm25Query: string;
}

/** A finding with no query strategy — reported, never silently skipped. */
export interface UnmappedFinding {
  ruleId: string;
  elementId: string;
}

export interface QueuePlan {
  queries: WeaveQuery[];
  unmappedFindings: UnmappedFinding[];
}

/** Text context for a gap element, resolved by the caller from the model store. */
export interface GapContext {
  name: string | null;
  /** The element's statement / description / free text (may be empty). */
  text: string;
}

/**
 * The finding→query table. DATA. Each known completeness ruleId maps to the
 * conceptual trace family whose absence the finding reports. Adding a row here
 * (with a test) is how a new completeness rule gets a query strategy.
 */
export const GAP_QUERY_TABLE: Readonly<Record<string, { family: QueryFamily }>> = {
  "GATE02-unsatisfied": { family: "satisfy" },
  "GATE02-orphan": { family: "allocation" },
  "GATE02-uncovered-need": { family: "derive" },
};

/** The completeness ruleIds this table knows how to turn into queries. */
export const MAPPED_FINDING_RULE_IDS: readonly string[] = Object.keys(GAP_QUERY_TABLE);

/**
 * QueryFamily (the conceptual trace gap) → the engine `RelationFamily` whose
 * enumeration proposes candidates that CLOSE that gap. Each value is a member of
 * the inference-engine `RelationFamily` union, so scoping a targeted pass to
 * these families makes a `satisfy` query actually yield satisfy candidates (and
 * a `derive` query derive candidates), instead of dead-ending on the four
 * structural families. Typed as `RelationFamily` (import type only — no runtime
 * coupling) so a drift between the two unions fails the type-check.
 */
export const QUERY_FAMILY_TO_RELATION_FAMILY: Readonly<Record<QueryFamily, RelationFamily>> = {
  satisfy: "satisfy",
  allocation: "allocation",
  derive: "derive",
};

/**
 * Collect the DISTINCT engine relation families a set of planned queries targets,
 * in first-seen order (deterministic). Feed the result to the targeted inference
 * pass as its `families` so enumeration is scoped to exactly the families whose
 * absence the findings reported.
 */
export function queryFamiliesToRelationFamilies(
  queries: readonly WeaveQuery[],
): RelationFamily[] {
  const out: RelationFamily[] = [];
  const seen = new Set<RelationFamily>();
  for (const q of queries) {
    const rf = QUERY_FAMILY_TO_RELATION_FAMILY[q.family];
    if (!seen.has(rf)) {
      seen.add(rf);
      out.push(rf);
    }
  }
  return out;
}

/**
 * Map completeness findings to targeted queries.
 *
 * PURE + deterministic: findings are processed in input order, so the emitted
 * query order is a pure function of the input. A finding whose ruleId is not in
 * `GAP_QUERY_TABLE` is appended to `unmappedFindings` (reported), never dropped.
 *
 * @param findings The completeness findings to map (caller decides which subset
 *                 to feed — typically the GATE02-* family).
 * @param resolve  Resolves a gap element's name + text from the model store.
 */
export function planQueries(
  findings: readonly WeaveFinding[],
  resolve: (elementId: string) => GapContext,
): QueuePlan {
  const queries: WeaveQuery[] = [];
  const unmappedFindings: UnmappedFinding[] = [];

  for (const finding of findings) {
    const row = GAP_QUERY_TABLE[finding.ruleId];
    if (row === undefined) {
      // REPORTED, never silently skipped.
      unmappedFindings.push({ ruleId: finding.ruleId, elementId: finding.elementId });
      continue;
    }
    const ctx = resolve(finding.elementId);
    const bm25Query = [ctx.name ?? "", ctx.text ?? ""].join(" ").trim();
    queries.push({
      findingRuleId: finding.ruleId,
      gapElementId: finding.elementId,
      gapElementName: ctx.name,
      family: row.family,
      bm25Query,
    });
  }

  return { queries, unmappedFindings };
}
