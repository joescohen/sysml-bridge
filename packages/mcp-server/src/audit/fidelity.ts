/**
 * GATE-04 fidelity three-bucket reconciliation.
 *
 * Produces:
 *   drops        — corpus entities with no resolving model element
 *   fabrications — model elements whose provenanceSourceId is not in the resolution set
 *   nearMatches  — fuzzy candidates from the unmatched residual (human review only)
 *
 * Algorithm (exact-ID-first, locked):
 *   1. Collect model provenance values from non-relationship elements
 *      (elements whose type is in SYSML_RELATIONSHIP_TYPES are skipped)
 *   2. EXACT pass: a corpus entity is MATCHED iff any model provenance value
 *      equals its id, naturalKey, or name.
 *   3. drops = unmatched corpus entities
 *   4. fabrications = model elements whose provenanceSourceId is a non-empty
 *      string NOT in resolutionSet
 *   5. FUZZY pass on the RESIDUAL ONLY (drops × residual model elements):
 *      compute similarity; push NearMatch when band is "confident" or "review"
 *
 * Anti-laundering control (locked — THREAT T-05-05):
 *   A fuzzy near-match NEVER removes the drop or fabrication entry.
 *   Near-matches are surfaced for human review only — never auto-merged.
 *
 * THREAT T-05-04 (Repudiation mitigation):
 *   Every unresolvable element is named in fabrications.
 *   Every silently-missing corpus entity is named in drops.
 */

import type { SysmlElement } from "../types/sysml-elements.js";
import type { Extracted } from "@sysml-bridge/ir";
import type { FidelityRow, NearMatch } from "./findings.js";
import { SYSML_RELATIONSHIP_TYPES } from "../types/sysml-elements.js";
import { similarity, band } from "./fuzzy.js";

const RELATIONSHIP_TYPE_SET = new Set<string>(SYSML_RELATIONSHIP_TYPES);

// ---------------------------------------------------------------------------
// Entity kind iterator — walks all supported Extracted entity arrays
// ---------------------------------------------------------------------------

interface CorpusEntity {
  id: string;
  naturalKey?: string;
  name: string;
  kind: string;
}

const ENTITY_KINDS = [
  "needs",
  "requirements",
  "functions",
  "components",
  "subsystems",
  "kpps",
  "behaviorDecomp",
] as const;

function* iterCorpusEntities(corpus: Extracted): Generator<CorpusEntity> {
  for (const k of ENTITY_KINDS) {
    const arr = (corpus as Record<string, unknown>)[k];
    if (!Array.isArray(arr)) continue;
    for (const e of arr as Array<Record<string, unknown>>) {
      // All entities have id and name; naturalKey and name are the lookup keys
      const id = typeof e.id === "string" ? e.id : "";
      const name = typeof e.name === "string" ? e.name : "";
      const naturalKey = typeof e.naturalKey === "string" ? e.naturalKey : undefined;
      if (!id) continue;
      // Use the entity's own "kind" field if present, otherwise use the collection key
      const kind = typeof e.kind === "string" ? e.kind : k;
      yield { id, naturalKey, name, kind };
    }
  }
}

// ---------------------------------------------------------------------------
// fidelityReport
// ---------------------------------------------------------------------------

export interface FidelityResult {
  drops: FidelityRow[];
  fabrications: FidelityRow[];
  nearMatches: NearMatch[];
}

/**
 * Compute the GATE-04 three-bucket fidelity report.
 *
 * @param elements      All SysML elements in the model
 * @param corpus        Zod-validated Extracted corpus
 * @param resolutionSet Resolution set built from the corpus (id + naturalKey + name + ALLOWLIST)
 */
export function fidelityReport(
  elements: SysmlElement[],
  corpus: Extracted,
  resolutionSet: Set<string>
): FidelityResult {
  // ─── Step 1: Collect non-relationship model elements ───────────────────
  const modelElements = elements.filter(
    (el) => !RELATIONSHIP_TYPE_SET.has(el.type)
  );

  // Build a set of all provenance values present in the model
  // (used for exact matching against corpus entities)
  const allProvenanceValues = new Set<string>();
  for (const el of modelElements) {
    const prov = el.raw.provenanceSourceId;
    if (typeof prov === "string" && prov.trim() !== "") {
      allProvenanceValues.add(prov);
    }
  }

  // ─── Step 2: EXACT pass ─────────────────────────────────────────────────
  // A corpus entity is MATCHED iff any provenance value equals its id, naturalKey, or name
  const matchedCorpusIds = new Set<string>();
  const corpusEntities: CorpusEntity[] = [];

  for (const entity of iterCorpusEntities(corpus)) {
    corpusEntities.push(entity);
    const isMatched =
      allProvenanceValues.has(entity.id) ||
      (entity.naturalKey !== undefined && allProvenanceValues.has(entity.naturalKey)) ||
      allProvenanceValues.has(entity.name);
    if (isMatched) {
      matchedCorpusIds.add(entity.id);
    }
  }

  // ─── Step 3: drops — unmatched corpus entities ──────────────────────────
  const drops: FidelityRow[] = corpusEntities
    .filter((e) => !matchedCorpusIds.has(e.id))
    .map((e) => ({ corpusId: e.id, corpusName: e.name, kind: e.kind }));

  // ─── Step 4: fabrications — model elements with unresolvable provenance ─
  const fabrications: FidelityRow[] = [];
  const residualModelElements: SysmlElement[] = []; // elements that are fabrications or have no provenance + have a name

  for (const el of modelElements) {
    const prov = el.raw.provenanceSourceId;
    if (typeof prov === "string" && prov.trim() !== "") {
      if (!resolutionSet.has(prov)) {
        fabrications.push({
          corpusId: prov, // the offending provenance value
          corpusName: el.name ?? el.id,
          kind: el.type,
        });
        // This element is in the residual — include in fuzzy pass
        residualModelElements.push(el);
      }
      // else: provenance resolved → not a fabrication, not in residual
    } else {
      // Missing / empty provenance — include in residual if element has a name
      if (el.name) {
        residualModelElements.push(el);
      }
    }
  }

  // ─── Step 5: FUZZY pass on the RESIDUAL ONLY ────────────────────────────
  // For each drop × each residual model element, compute similarity.
  // Push NearMatch when band is "confident" or "review".
  // NEVER remove the drop or fabrication (anti-laundering control, locked).
  const nearMatches: NearMatch[] = [];

  for (const drop of drops) {
    for (const el of residualModelElements) {
      if (!el.name) continue;
      const sim = similarity(drop.corpusName, el.name);
      const b = band(sim);
      if (b === "confident" || b === "review") {
        nearMatches.push({
          corpusId: drop.corpusId,
          corpusName: drop.corpusName,
          kind: drop.kind,
          modelElementId: el.id,
          modelName: el.name,
          similarity: sim,
          band: b,
        });
      }
    }
  }

  return { drops, fabrications, nearMatches };
}
