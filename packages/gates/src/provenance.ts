/**
 * GATE-03 provenance-existence rule pack.
 *
 * Pure function over (SysmlElement[], Set<string>) — no ModelStore, no I/O.
 * The caller builds the resolution set via corpus.ts `buildResolutionSet`.
 *
 * Rules:
 *   GATE03-missing-provenance       warning — legacy element type with no provenanceSourceId
 *                                             (presence check; scoped to RequirementDefinition,
 *                                             PartDefinition, ActionDefinition only)
 *   GATE03-unresolvable-provenance  error   — provenanceSourceId is non-empty but NOT in
 *                                             the resolution set (existence check, any element)
 *   GATE03-model-asserted           info    — element uses "model-asserted" convention
 *
 * Decision A4: allowlist values (model-asserted, C&C, Demonstration, Test, Analysis,
 * Inspection) are in the resolution set via ALLOWLIST in corpus.ts — they resolve
 * successfully and produce zero error findings. model-asserted additionally gets
 * the info finding to make the convention visible.
 *
 * Security note (T-05-02): The existence check is the 2026-06-09 fabrication-failure
 * control. A provenanceSourceId that looks well-formed but doesn't resolve to any
 * corpus entity or allowlist value is a laundered fake id → error.
 */

import type { SysmlElement } from "@sysml-bridge/model";
import type { Finding } from "./findings.js";

// Legacy element types that participate in the presence check.
// Do NOT widen — adding types adds noise (validate-model.ts lines 116–126 scope).
const LEGACY_PROVENANCE_TYPES = new Set([
  "RequirementDefinition",
  "PartDefinition",
  "ActionDefinition",
]);

/**
 * Produce GATE-03 findings for a snapshot of model elements.
 *
 * @param elements     - model elements (from store.queryElements() or plain literals)
 * @param resolutionSet - built by corpus.ts buildResolutionSet from the loaded corpus
 */
export function provenanceFindings(
  elements: SysmlElement[],
  resolutionSet: Set<string>
): Finding[] {
  const findings: Finding[] = [];

  for (const el of elements) {
    const prov = el.raw.provenanceSourceId;

    // ── Presence check (legacy scope, warning) ──
    if (LEGACY_PROVENANCE_TYPES.has(el.type)) {
      if (!prov || (typeof prov === "string" && prov.trim() === "")) {
        findings.push({
          elementId: el.id,
          ruleId: "GATE03-missing-provenance",
          severity: "warning",
          message: `Element '${el.name ?? el.id}' (${el.type}) has no provenanceSourceId.`,
          suggestedFix:
            "Set provenanceSourceId to a valid corpus entity id, naturalKey, or component name from extracted.json.",
        });
        continue; // nothing more to check — no prov value to resolve
      }
    }

    // ── Existence check (any element, any non-empty prov) ──
    if (typeof prov === "string" && prov.trim() !== "") {
      if (!resolutionSet.has(prov)) {
        // Not in resolution set → laundered/forged id → error (T-05-02 control)
        findings.push({
          elementId: el.id,
          ruleId: "GATE03-unresolvable-provenance",
          severity: "error",
          message: `provenanceSourceId '${prov}' on '${el.name ?? el.id}' does not resolve to any corpus entity.`,
          suggestedFix:
            "Set provenanceSourceId to a valid corpus entity id, naturalKey, or component name from extracted.json.",
        });
      } else if (prov === "model-asserted") {
        // In the resolution set (via ALLOWLIST) — emit info to make convention visible
        findings.push({
          elementId: el.id,
          ruleId: "GATE03-model-asserted",
          severity: "info",
          message: `Element '${el.name ?? el.id}' uses 'model-asserted' provenance (allocate convention).`,
          suggestedFix:
            "Acceptable for allocate/structural elements; ensure intentional.",
        });
      }
      // All other resolution set hits (corpus ids, naturalKeys, names, other ALLOWLIST
      // values) resolve cleanly — no finding emitted.
    }
  }

  return findings;
}
