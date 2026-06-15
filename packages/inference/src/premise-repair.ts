/**
 * premise-repair.ts — Deterministic name→id premise repair (safe, bounded).
 *
 * The live run showed Haiku citing premises by NAME ("Operator Console Module",
 * "F1.4") instead of composed-IR id, despite the ids being present in the context.
 * This module mechanically resolves such citations — but ONLY against facts that
 * were actually offered in the candidate's context bundle:
 *
 *   - A cited premise string that exactly matches (case- and whitespace-
 *     insensitive) the NAME or an ALIAS (naturalKey, "key: name" label) of an
 *     offered fact is substituted with that fact's composed-IR id.
 *   - Ambiguous matches (two offered facts share the label) are NOT repaired —
 *     the premise stays as-is and drops in validation.
 *   - Anything not offered stays unresolvable and drops (dropped_unpremised).
 *
 * This is mechanical resolution within the offered evidence, not fabrication:
 * the model saw the fact, named it, and we map the name back to the id it was
 * shown next to.
 */

import type { OfferedFact } from "./types.js";

/** Normalize a label for matching: lowercase, collapse runs of whitespace, trim. */
export function normalizeLabel(s: string): string {
  return s.toLowerCase().replace(/\s+/g, " ").trim();
}

export interface RepairResult {
  /** The premise list with repairable name-citations substituted by ids. */
  premises: string[];
  /** How many premises were repaired (name → id substitutions). */
  repairedCount: number;
}

/**
 * Repair name-cited premises against the offered fact bundle.
 *
 * For each premise string:
 *   1. If it is already an offered fact's id (exact) → keep unchanged.
 *   2. If its normalized form matches exactly ONE offered fact's name/alias
 *      → substitute that fact's id (counted).
 *   3. Otherwise → keep unchanged (validation will drop it if unresolvable).
 */
export function repairPremises(
  premises: string[],
  offeredFacts: OfferedFact[]
): RepairResult {
  // Offered ids (exact match → no repair needed)
  const offeredIds = new Set<string>(offeredFacts.map((f) => f.id));

  // normalized label → set of fact ids carrying that label
  const labelMap = new Map<string, Set<string>>();
  const addLabel = (label: string, id: string) => {
    const norm = normalizeLabel(label);
    if (!norm) return;
    if (!labelMap.has(norm)) labelMap.set(norm, new Set());
    labelMap.get(norm)!.add(id);
  };
  for (const fact of offeredFacts) {
    addLabel(fact.name, fact.id);
    for (const alias of fact.aliases ?? []) {
      addLabel(alias, fact.id);
    }
  }

  let repairedCount = 0;
  const repaired = premises.map((p) => {
    // Already a correct offered id — leave it
    if (offeredIds.has(p)) return p;

    const ids = labelMap.get(normalizeLabel(p));
    if (ids !== undefined && ids.size === 1) {
      repairedCount++;
      return [...ids][0]!;
    }
    // No match or ambiguous → unchanged (drops in validation if unresolvable)
    return p;
  });

  return { premises: repaired, repairedCount };
}
