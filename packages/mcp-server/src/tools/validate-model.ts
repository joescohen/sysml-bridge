import { z } from "zod";
import * as path from "node:path";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "../store.js";
import { audit } from "../audit/index.js";
import { loadCorpusCached } from "../audit/corpus.js";
import { writeReports } from "../audit/report.js";
import { FORWARD_TYPES, VERIFY_TYPES, BACKWARD_TYPES } from "../audit/relational.js";

export function registerValidateModel(server: McpServer, smaps: ModelStore) {
  server.tool(
    "validate_model",
    "Run completeness and consistency checks — unsatisfied requirements, orphaned elements, missing connections",
    {
      scope: z.string().optional().describe("Element ID to scope validation to, or omit for full model"),
      corpus_path: z
        .string()
        .optional()
        .describe(
          "Path to extracted.json; default $SYSML_BRIDGE_CORPUS_PATH or examples/angars/model/extracted.json"
        ),
      write_report: z
        .boolean()
        .optional()
        .describe(
          "Write coverage-matrix.md and fidelity-report.md to the audits dir; default false"
        ),
    },
    async ({ corpus_path, write_report }) => {
      try {
        const state = await smaps.getProjectState();
        const requirements = await smaps.queryElements("RequirementDefinition");
        const parts = await smaps.queryElements("PartDefinition");
        const actions = await smaps.queryElements("ActionDefinition");

        // Collect all relationships once for dangling endpoint check
        const allRels = await smaps.queryRelationships();
        const allElementIds = new Set<string>(
          [
            ...requirements,
            ...parts,
            ...actions,
            // Also include any other element types that may exist as endpoints
            ...(await smaps.queryElements()),
          ].map((e) => e.id)
        );

        // ── Separate stakeholder Needs from system Requirements ──
        // A Need = RequirementDefinition with raw.stakeholderNeed === true.
        // Needs are covered by an incoming DeriveRequirementUsage (the need is the TARGET).
        // Needs are EXEMPT from forward-satisfy and verify checks.
        // System Requirements = RequirementDefinition WITHOUT raw.stakeholderNeed.
        const needs = requirements.filter((r) => r.raw.stakeholderNeed === true);
        const systemReqs = requirements.filter((r) => r.raw.stakeholderNeed !== true);

        // ── System Requirement traceability ──
        const forwardTracedIds = new Set<string>();
        const verifiedIds = new Set<string>();
        const backwardTracedIds = new Set<string>();

        for (const req of systemReqs) {
          const rels = await smaps.queryRelationships(req.id, "both");

          // Forward: any edge of forward type touching this requirement
          const hasForward = rels.some((r) => FORWARD_TYPES.has(r.type));
          if (hasForward) forwardTracedIds.add(req.id);

          // Verify: any edge of verify type, either direction
          const hasVerify = rels.some((r) => VERIFY_TYPES.has(r.type));
          if (hasVerify) verifiedIds.add(req.id);

          // Backward: outgoing DeriveRequirementUsage (req → need)
          // CR-02 fix: req must be the SOURCE of the derive edge. A req that is
          // only the TARGET of a DeriveRequirementUsage (chained derivation from
          // a peer) has NOT backtraced to a stakeholder Need.
          const hasBackward = rels.some(
            (r) => BACKWARD_TYPES.has(r.type) && r.sourceIds.includes(req.id)
          );
          if (hasBackward) backwardTracedIds.add(req.id);
        }

        const totalSystemReqs = systemReqs.length;
        const forwardPercent =
          totalSystemReqs > 0 ? Math.round((forwardTracedIds.size / totalSystemReqs) * 100) : 0;
        const verifyPercent =
          totalSystemReqs > 0 ? Math.round((verifiedIds.size / totalSystemReqs) * 100) : 0;
        const backwardPercent =
          totalSystemReqs > 0 ? Math.round((backwardTracedIds.size / totalSystemReqs) * 100) : 0;

        // ── Need coverage: a Need is covered iff it is the TARGET of >=1 DeriveRequirementUsage ──
        const coveredNeedIds = new Set<string>();
        for (const need of needs) {
          const rels = await smaps.queryRelationships(need.id, "in");
          const hasDeriveInbound = rels.some((r) => BACKWARD_TYPES.has(r.type));
          if (hasDeriveInbound) coveredNeedIds.add(need.id);
        }
        const totalNeeds = needs.length;
        const needCoveragePercent =
          totalNeeds > 0 ? Math.round((coveredNeedIds.size / totalNeeds) * 100) : 100;
        const uncoveredNeeds = needs
          .filter((n) => !coveredNeedIds.has(n.id))
          .map((n) => ({ id: n.id, name: n.name }));

        // ── 4. Orphan design elements (PartDefinition, ActionDefinition) ──
        // A design element is an orphan ONLY if:
        //   (a) it participates in NO traceability edge (SatisfyRequirementUsage, AllocationUsage,
        //       DeriveRequirementUsage) in either direction, AND
        //   (b) it is a LEAF element — it owns NO children via FeatureMembership.
        //       A container (subsystem, parent function) traces through its children; it is not
        //       an orphan even if it lacks a direct satisfy/allocate/derive edge.
        const ORPHAN_TRACE_TYPES = new Set([
          "SatisfyRequirementUsage",
          "AllocationUsage",
          "DeriveRequirementUsage",
        ]);
        const designElements = [...parts, ...actions];
        const orphanElements: Array<{ id: string; name: string | null; type: string }> = [];

        for (const el of designElements) {
          const rels = await smaps.queryRelationships(el.id, "both");
          const hasTraceEdge = rels.some((r) => ORPHAN_TRACE_TYPES.has(r.type));
          if (hasTraceEdge) continue; // has a direct trace edge — not an orphan

          // Check if it is a container: owns >=1 child via FeatureMembership (it is SOURCE)
          const isContainer = rels.some(
            (r) => r.type === "FeatureMembership" && r.sourceIds.includes(el.id)
          );
          if (!isContainer) {
            orphanElements.push({ id: el.id, name: el.name, type: el.type });
          }
        }

        // ── 5. Provenance coverage ──
        // RequirementDefinition + ActionDefinition + PartDefinition missing raw.provenanceSourceId
        const provenanceElements = [...requirements, ...parts, ...actions];
        const elementsMissingBackpointer: Array<{ id: string; name: string | null; type: string }> = [];

        for (const el of provenanceElements) {
          const prov = el.raw.provenanceSourceId;
          if (!prov || (typeof prov === "string" && prov.trim() === "")) {
            elementsMissingBackpointer.push({ id: el.id, name: el.name, type: el.type });
          }
        }

        const provenanceCoverage =
          provenanceElements.length > 0
            ? Math.round(
                ((provenanceElements.length - elementsMissingBackpointer.length) /
                  provenanceElements.length) *
                  100
              )
            : 100;

        // ── 6. Dangling relationships (source or target resolves to no existing element) ──
        const danglingRelationships: Array<{ id: string; type: string; danglingIds: string[] }> = [];

        for (const rel of allRels) {
          const danglingIds: string[] = [];
          for (const sid of rel.sourceIds) {
            if (!allElementIds.has(sid)) danglingIds.push(sid);
          }
          for (const tid of rel.targetIds) {
            if (!allElementIds.has(tid)) danglingIds.push(tid);
          }
          if (danglingIds.length > 0) {
            danglingRelationships.push({ id: rel.id, type: rel.type, danglingIds });
          }
        }

        // ── Build issues list ──
        const issues: string[] = [];

        const unsatisfied = systemReqs.filter((r) => !forwardTracedIds.has(r.id));
        if (unsatisfied.length > 0) {
          issues.push(
            `${unsatisfied.length} system requirements not forward-traced (no SatisfyRequirementUsage or AllocationUsage): ${unsatisfied.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        const unverified = systemReqs.filter((r) => !verifiedIds.has(r.id));
        if (unverified.length > 0) {
          issues.push(
            `${unverified.length} system requirements not verified (no VerifyRequirementUsage or RequirementVerificationMembership): ${unverified.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        const unbacktraced = systemReqs.filter((r) => !backwardTracedIds.has(r.id));
        if (unbacktraced.length > 0) {
          issues.push(
            `${unbacktraced.length} system requirements missing backward trace (no DeriveRequirementUsage to a Need): ${unbacktraced.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        if (uncoveredNeeds.length > 0) {
          issues.push(
            `${uncoveredNeeds.length} stakeholder needs not covered (no incoming DeriveRequirementUsage): ${uncoveredNeeds.map((n) => n.name ?? n.id).join(", ")}`
          );
        }

        if (orphanElements.length > 0) {
          issues.push(
            `${orphanElements.length} leaf design elements have no satisfy/allocate/derive edge in either direction (orphans): ${orphanElements.map((e) => e.name ?? e.id).join(", ")}`
          );
        }

        if (elementsMissingBackpointer.length > 0) {
          issues.push(
            `${elementsMissingBackpointer.length} elements missing provenanceSourceId: ${elementsMissingBackpointer.map((e) => e.name ?? e.id).join(", ")}`
          );
        }

        if (danglingRelationships.length > 0) {
          issues.push(
            `${danglingRelationships.length} relationships have dangling endpoint(s): ${danglingRelationships.map((r) => r.id).join(", ")}`
          );
        }

        if (parts.length > 0) {
          const connections = await smaps.queryElements("ConnectionUsage");
          if (connections.length === 0) {
            issues.push("Part definitions exist but no connections — IBD may be incomplete");
          }
        }

        // ── GATE-01 audit: findings / fidelity / matrix (additive) ──────────
        // Legacy envelope above stays verbatim; these are appended as new keys.
        const corpusPath =
          corpus_path ??
          process.env.SYSML_BRIDGE_CORPUS_PATH ??
          path.join(process.cwd(), "examples/angars/model/extracted.json");
        const corpus = await loadCorpusCached(corpusPath);

        // Gather all elements for the audit (allRels already fetched above).
        const allElements = await smaps.queryElements();
        const auditResult = audit(allElements, allRels, corpus);

        // Optional report write (default false — keeps existing tests clean).
        let reportPaths: { matrixPath: string; fidelityPath: string } | undefined;
        if (write_report === true) {
          const auditsDir =
            process.env.SYSML_BRIDGE_AUDITS_DIR ??
            path.join(process.cwd(), "examples/angars/audits");
          reportPaths = await writeReports(auditsDir, auditResult.matrix, auditResult.fidelity);
        }

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  summary: state,
                  issues,
                  coverage: {
                    forwardPercent,
                    verifyPercent,
                    backwardPercent,
                    needCoverage: {
                      percent: needCoveragePercent,
                      total: totalNeeds,
                      covered: coveredNeedIds.size,
                      uncovered: uncoveredNeeds,
                    },
                    orphanElements,
                    provenanceCoverage,
                    elementsMissingBackpointer,
                    danglingRelationships,
                  },
                  // ── GATE-01 additive keys ──
                  findings: auditResult.findings,
                  fidelity: auditResult.fidelity,
                  matrix: auditResult.matrix,
                  ...(reportPaths !== undefined ? { reportPaths } : {}),
                },
                null,
                2
              ),
            },
          ],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
