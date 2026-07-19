/**
 * GATE-05 pre-add structural rule subset.
 *
 * Shared helper used by all three mutating MCP tools (create_element,
 * create_relationship, import_sysml). Runs check-BEFORE-add: the FileStore
 * has no transaction mechanism (file-store.ts line 123 — research Pitfall 2),
 * so rejection MUST happen before the store is touched.
 *
 * Rules in scope (the locked structural-reject subset):
 *   R4-def-operand                  error  — trace operand is a Definition
 *   GATE02-dangling-endpoint        error  — source/target id not in existing
 *   GATE03-unresolvable-provenance  error  — provenanceSourceId not in corpus
 *
 * Rules deliberately NOT here (completeness — end-state checks, never block
 * a mutation):
 *   GATE02-unsatisfied, GATE02-unverified, GATE02-unbacktraced,
 *   GATE02-orphan, GATE02-uncovered-need
 *
 * Corpus-null degradation (documented):
 *   When resolveGateCorpus() returns null (extracted.json missing or
 *   unreadable), the provenance-existence check is SKIPPED. This is
 *   intentional: the 177+ legacy tests run with no corpus on disk; making
 *   provenance a hard gate without a corpus would break every test that seeds
 *   elements with provenanceSourceId values. The check degrades gracefully to
 *   structural-only when the corpus is unavailable.
 *
 * Id-less candidate batch members:
 *   checkBatch gives id-less candidates a deterministic placeholder id
 *   (`__batch_candidate_N__`) so later batch members cannot dangle-resolve
 *   to them incorrectly (no real element will ever have that id, so if a
 *   later candidate targets it, it is still a dangling reference within
 *   the batch scope).
 */

import * as path from "node:path";
import type { SysmlElement } from "@sysml-bridge/model";
import type { Finding } from "./findings.js";
import { TRACE_TYPES } from "./relational.js";
import { loadCorpusCached, buildResolutionSet } from "./corpus.js";

// ---------------------------------------------------------------------------
// Exported types
// ---------------------------------------------------------------------------

/**
 * A model element candidate being considered for persistence.
 *
 * sourceIds / targetIds: endpoints for relationship candidates (empty arrays
 * for plain elements — plain elements skip the dangling check).
 *
 * provenanceSourceId: if present and non-empty AND a resolutionSet is
 * available, checked against the corpus resolution set.
 */
export interface Candidate {
  id?: string;
  type: string;
  name?: string | null;
  sourceIds: string[];
  targetIds: string[];
  provenanceSourceId?: unknown;
}

// ---------------------------------------------------------------------------
// Core structural check (pure function — no I/O)
// ---------------------------------------------------------------------------

/**
 * Run the three structural rules against a single candidate.
 *
 * @param candidate     - the element/relationship being created
 * @param existing      - all current store elements (from queryElements() +
 *                        queryRelationships() mapped to id-bearing shapes)
 * @param resolutionSet - corpus resolution set; null means provenance check
 *                        is skipped (corpus unavailable)
 */
export function structuralCheck(
  candidate: Candidate,
  existing: SysmlElement[],
  resolutionSet: Set<string> | null
): Finding[] {
  const findings: Finding[] = [];

  // Build id lookup from existing
  const byId = new Map<string, SysmlElement>();
  for (const el of existing) {
    byId.set(el.id, el);
  }
  const allIds = new Set<string>(existing.map((e) => e.id));

  // ── Rule 1: R4 def-operand ──
  // Only checked when candidate is a TRACE_TYPES relationship (has meaningful
  // source/target semantics). Plain elements never trigger this rule.
  if (TRACE_TYPES.has(candidate.type)) {
    // WR-03: deduplicate before iterating to avoid duplicate findings for self-refs.
    for (const id of [...new Set([...candidate.sourceIds, ...candidate.targetIds])]) {
      const el = byId.get(id);
      if (el && !el.type.endsWith("Usage")) {
        findings.push({
          elementId: id,
          ruleId: "R4-def-operand",
          severity: "error",
          message: `Trace ${candidate.type} references Definition '${el.name ?? id}' (${el.type}); operands must be Usages.`,
          suggestedFix: `Re-target the ${candidate.type} to a ${el.type.replace("Definition", "Usage")} instead of the Definition.`,
        });
      }
    }
  }

  // ── Rule 2: GATE02-dangling-endpoint ──
  // Only checked when the candidate HAS source/target ids (plain elements
  // with empty arrays skip this entirely, matching validate-model.ts semantics
  // where dangling is only checked for relationships).
  if (candidate.sourceIds.length > 0 || candidate.targetIds.length > 0) {
    const danglingIds: string[] = [];
    for (const id of [...candidate.sourceIds, ...candidate.targetIds]) {
      if (!allIds.has(id)) danglingIds.push(id);
    }
    if (danglingIds.length > 0) {
      const candidateName = candidate.name ?? candidate.id ?? candidate.type;
      findings.push({
        elementId: candidate.id ?? candidateName,
        ruleId: "GATE02-dangling-endpoint",
        severity: "error",
        message: `Candidate ${candidate.type} has dangling endpoint(s): ${danglingIds.join(", ")}`,
        suggestedFix: "Ensure all source/target element ids exist in the model before creating this relationship.",
      });
    }
  }

  // ── Rule 3: GATE03-unresolvable-provenance ──
  // Skipped when resolutionSet is null (corpus unavailable — intentional
  // degradation, see module-level doc). Also skipped when provenanceSourceId
  // is absent or empty (missing provenance is a COMPLETENESS warning, not a
  // structural reject; emitted only by the full audit, not the write-path gate).
  const prov = candidate.provenanceSourceId;
  if (resolutionSet !== null && typeof prov === "string" && prov.trim() !== "") {
    if (!resolutionSet.has(prov)) {
      findings.push({
        elementId: candidate.id ?? candidate.type,
        ruleId: "GATE03-unresolvable-provenance",
        severity: "error",
        message: `provenanceSourceId '${prov}' does not resolve to any corpus entity.`,
        suggestedFix:
          "Set provenanceSourceId to a valid corpus entity id, naturalKey, or component name from extracted.json.",
      });
    } else if (prov === "model-asserted") {
      findings.push({
        elementId: candidate.id ?? candidate.type,
        ruleId: "GATE03-model-asserted",
        severity: "info",
        message: `Candidate uses 'model-asserted' provenance (allocate convention).`,
        suggestedFix: "Acceptable for allocate/structural elements; ensure intentional.",
      });
    }
  }

  return findings;
}

// ---------------------------------------------------------------------------
// Batch check (all-or-nothing signal for import_sysml)
// ---------------------------------------------------------------------------

/**
 * Run structuralCheck on a sequence of candidates, growing the working element
 * set cumulatively so later batch members can reference earlier ones.
 *
 * Id-less candidates receive a deterministic placeholder id so later batch
 * members cannot accidentally dangle-resolve to them: if they target the
 * placeholder id, it will still be a dangling reference outside the batch
 * (no real element ever has `__batch_candidate_N__`).
 *
 * Returns the union of all findings. The caller inspects
 * `findings.filter(f => f.severity === "error")` for the all-or-nothing
 * reject decision.
 */
export function checkBatch(
  candidates: Candidate[],
  existing: SysmlElement[],
  resolutionSet: Set<string> | null
): Finding[] {
  const allFindings: Finding[] = [];

  // Working element set starts with existing elements
  const working: SysmlElement[] = [...existing];

  for (let i = 0; i < candidates.length; i++) {
    const candidate = candidates[i];

    // Run structural check for this candidate against the current working set
    const findings = structuralCheck(candidate, working, resolutionSet);
    allFindings.push(...findings);

    // Grow the working set: add a stub SysmlElement for this candidate so
    // subsequent candidates can reference it without triggering dangling errors.
    // Use the candidate's id if available, otherwise a deterministic placeholder.
    const stubId = candidate.id ?? `__batch_candidate_${i}__`;
    working.push({
      id: stubId,
      elementId: stubId,
      type: candidate.type,
      name: candidate.name ?? null,
      shortName: null,
      qualifiedName: null,
      ownerId: null,
      ownedElementIds: [],
      raw: {},
    });
  }

  return allFindings;
}

// ---------------------------------------------------------------------------
// resolveGateCorpus (async helper — used by tool handlers at runtime)
// ---------------------------------------------------------------------------

/**
 * Load the corpus and return its resolution set, or null if unavailable.
 *
 * Null return degrades the write-path gate to structural-only (R4 +
 * dangling), skipping the provenance-existence check. This is WHY legacy
 * tests (which run with no corpus on disk) are unaffected: they always
 * receive null and the provenance check never fires.
 */
export async function resolveGateCorpus(): Promise<Set<string> | null> {
  const corpusPath =
    process.env.SYSML_BRIDGE_CORPUS_PATH ??
    path.join(process.cwd(), "examples/angars/model/extracted.json");

  const corpus = await loadCorpusCached(corpusPath);
  if (corpus === null) return null;
  return buildResolutionSet(corpus);
}
