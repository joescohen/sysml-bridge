/**
 * entities.ts — ENT-* rule pack (Gate 1 audit counterpart for W1 entities).
 *
 * The W1 entity resolver clusters mentions into canonical entities. This gate is
 * the AUDIT counterpart, applying the standing invariants to the entity store:
 *
 *   - ENT-unapproved-merge (ERROR): an entity's alias set contains a surface form
 *     NOT reachable by deterministic normalization from its seed AND NOT covered
 *     by a merge disposition. Deterministic auto-cluster keeps every alias at ONE
 *     normSurface; a fuzzy/LLM merge that appended a divergent alias without a
 *     human approval record is the no-auto-approve violation this rule catches.
 *   - ENT-dangling-mention-ref (ERROR): an entity references a mentionId absent
 *     from the mention store (the C4 citation-resolution discipline, for entities).
 *   - ENT-duplicate-suspect (WARNING): two entities of the same kind whose
 *     canonical names auto-cluster-match — the resolver missed a deterministic
 *     merge (should be impossible; the warning is the tripwire).
 *   - ENT-mention-store-unavailable (WARNING): the degrade path — when the mention
 *     store is not provided, dangling-ref cannot be checked, so we warn rather
 *     than vacuously pass (mirrors PROSE-unverbatim-quote-unavailable).
 *
 * Uses the SAME canonical normalizer as the resolver (@sysml-bridge/model
 * `normSurface`) so emit-time and audit-time identity never drift. Pure — no I/O.
 */

import { normSurface } from "@sysml-bridge/model";
import type { Finding } from "./findings.js";

/**
 * Structural view of a W1 EntityRecord (defined in @sysml-bridge/candidates).
 * The gate takes the SHAPE, not the type, so packages/gates keeps no dependency
 * on packages/candidates (which already depends on gates — the arrow only points
 * one way).
 */
export interface EntityRecordLike {
  entityId: string;
  kind: string;
  canonicalName: string;
  aliases: string[];
  mentionIds: string[];
  /** Ids of the merge-approval records that grew this entity. */
  mergeDispositions: string[];
}

/**
 * Audit an entity store.
 *
 * @param entities   The resolved entity records.
 * @param mentionIds The set of every mentionId present in the mention store, or
 *                   `undefined` when the store is unavailable (degrade path).
 */
export function entityFindings(
  entities: readonly EntityRecordLike[],
  mentionIds: ReadonlySet<string> | undefined
): Finding[] {
  const findings: Finding[] = [];
  if (entities.length === 0) return findings; // nothing to audit — genuine no-op

  // ── ENT-unapproved-merge + ENT-dangling-mention-ref (per entity) ────────────
  const storeAvailable = mentionIds !== undefined;
  let entitiesReferencingMentions = 0;

  for (const e of entities) {
    // ENT-unapproved-merge: count distinct normSurfaces among the aliases. A pure
    // auto-cluster has exactly ONE (all aliases normalize equal); every additional
    // normSurface group is a merge that MUST be covered by a disposition record.
    const normGroups = new Set(e.aliases.map((a) => normSurface(a)));
    const unapprovedGroups = normGroups.size - 1 - e.mergeDispositions.length;
    if (unapprovedGroups > 0) {
      const divergent = distinctByNorm(e.aliases).slice(1); // drop the seed group rep
      findings.push({
        elementId: e.entityId,
        ruleId: "ENT-unapproved-merge",
        severity: "error",
        message:
          `Entity '${e.entityId}' (${e.kind}, "${e.canonicalName}") has ${normGroups.size} ` +
          `distinct normalized surface groups but only ${e.mergeDispositions.length} merge ` +
          `disposition(s): alias(es) ${divergent.map((s) => `"${s}"`).join(", ")} are not ` +
          `reachable by deterministic normalization from the seed and are not covered by a ` +
          `human-approved merge — a fuzzy/LLM merge with no approval record.`,
        suggestedFix:
          "Approve the corresponding entity-merge proposal in the review UI (recording a merge " +
          "disposition), or remove the un-approved alias from the entity.",
      });
    }

    // ENT-dangling-mention-ref: every referenced mentionId must resolve.
    if (e.mentionIds.length > 0) entitiesReferencingMentions++;
    if (storeAvailable) {
      for (const mid of e.mentionIds) {
        if (!mentionIds!.has(mid)) {
          findings.push({
            elementId: e.entityId,
            ruleId: "ENT-dangling-mention-ref",
            severity: "error",
            message:
              `Entity '${e.entityId}' (${e.kind}, "${e.canonicalName}") references mentionId ` +
              `'${mid}' which is absent from the mention store — a dangling provenance ` +
              `reference (the C4 citation-resolution discipline applied to entities).`,
            suggestedFix:
              "Re-run mention derivation so the referenced mention exists, or rebuild the entity " +
              "store from the current mentions so its mentionIds resolve.",
          });
        }
      }
    }
  }

  // ── ENT-mention-store-unavailable (degrade, never a vacuous pass) ────────────
  if (!storeAvailable && entitiesReferencingMentions > 0) {
    findings.push({
      elementId: "entities",
      ruleId: "ENT-mention-store-unavailable",
      severity: "warning",
      message:
        `${entitiesReferencingMentions} entit${entitiesReferencingMentions === 1 ? "y" : "ies"} ` +
        `reference mentionIds but the mention store was not provided to the audit, so ` +
        `ENT-dangling-mention-ref could not verify them.`,
      suggestedFix:
        "Attach the mention store (the set of mentionIds from mentions.json) before auditing so " +
        "ENT-dangling-mention-ref can confirm every entity's mention references resolve.",
    });
  }

  // ── ENT-duplicate-suspect (warning) ──────────────────────────────────────────
  const byKindNorm = new Map<string, EntityRecordLike[]>();
  for (const e of entities) {
    const key = `${e.kind} ${normSurface(e.canonicalName)}`;
    const bucket = byKindNorm.get(key);
    if (bucket) bucket.push(e);
    else byKindNorm.set(key, [e]);
  }
  for (const bucket of byKindNorm.values()) {
    if (bucket.length < 2) continue;
    for (let i = 1; i < bucket.length; i++) {
      const e = bucket[i]!;
      findings.push({
        elementId: e.entityId,
        ruleId: "ENT-duplicate-suspect",
        severity: "warning",
        message:
          `Entity '${e.entityId}' (${e.kind}, "${e.canonicalName}") auto-cluster-matches ` +
          `entity '${bucket[0]!.entityId}' (same kind + normalized canonical name) — the ` +
          `resolver should have merged these deterministically.`,
        suggestedFix:
          "Re-run the deterministic auto-cluster; two same-kind entities with an equal " +
          "normalized canonical name must collapse to one.",
      });
    }
  }

  return findings;
}

/** First alias per distinct normSurface, in first-seen order. */
function distinctByNorm(aliases: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const a of aliases) {
    const n = normSurface(a);
    if (!seen.has(n)) {
      seen.add(n);
      out.push(a);
    }
  }
  return out;
}
