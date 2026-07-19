import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";
import {
  parseSysml,
  parsedElementsToStoreInputs,
  parsedRelationshipsToStoreInputs,
  buildNameIndex,
} from "@sysml-bridge/sysml";
import { checkBatch, resolveGateCorpus } from "@sysml-bridge/gates";
import type { Candidate } from "@sysml-bridge/gates";
import type { Finding } from "@sysml-bridge/gates";

export function registerImportSysml(server: McpServer, smaps: ModelStore) {
  server.tool(
    "import_sysml",
    "Parse SysML v2 textual notation and import elements into the model",
    {
      sysml_text: z.string().describe("SysML v2 textual notation to parse and import"),
      allow_invalid: z
        .boolean()
        .optional()
        .describe(
          "Skip structural Gate 1 checks — only for multi-step construction where intermediate states are incomplete"
        ),
    },
    async ({ sysml_text, allow_invalid }) => {
      try {
        const parsed = parseSysml(sysml_text);

        if (parsed.errors.length > 0) {
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ success: false, errors: parsed.errors }, null, 2),
              },
            ],
            isError: true,
          };
        }

        // Milestone 1 (identity + structure round trip): recursively flatten
        // the parsed element TREE (parsed.elements only holds the top-level
        // array — nested children live in ParsedElement.children) and thread
        // the recovered `// @id: <uuid>` elementId into attributes["@id"] so
        // FileStore.buildElement REUSES it instead of minting a fresh id.
        const elementsToCreate = parsedElementsToStoreInputs(parsed.elements);

        // ── GATE-05 pre-add structural check (check-BEFORE-add, all-or-nothing) ──
        // Map parsed elements to Candidates. Since parsed elements have no
        // source/target arrays (they are plain element defs from the SysML text),
        // sourceIds/targetIds are empty arrays. The provenance check still applies.
        const candidates: Candidate[] = elementsToCreate.map((e) => ({
          type: e.type,
          name: e.name,
          sourceIds: [],
          targetIds: [],
          provenanceSourceId: e.attributes?.provenanceSourceId,
        }));

        const existingElements = await smaps.queryElements();
        const resolutionSet = await resolveGateCorpus();
        const findings: Finding[] = checkBatch(candidates, existingElements, resolutionSet);
        const errors = findings.filter((f) => f.severity === "error");

        if (errors.length > 0 && !allow_invalid) {
          // All-or-nothing: reject the whole batch, store untouched
          return {
            isError: true,
            content: [
              {
                type: "text" as const,
                text: JSON.stringify(
                  {
                    rejected: true,
                    findings,
                    elementsRejected: candidates.length,
                  },
                  null,
                  2
                ),
              },
            ],
          };
        }

        // ── Persist ──
        const created = await smaps.createElements(elementsToCreate);

        // Milestone 1: reconstruct satisfy/verify/allocate/dependency
        // relationships parsed from the text (parsed.relationships carries
        // NAME-based endpoint references, not ids — resolve them against the
        // elements just created). Additive/best-effort: an unresolvable
        // endpoint name is skipped rather than failing the whole import (see
        // parsedRelationshipsToStoreInputs's doc comment for scope/rationale).
        const createdByName = buildNameIndex(created);
        const relationshipsToCreate = parsedRelationshipsToStoreInputs(
          parsed.relationships,
          createdByName
        );
        const createdRelationships =
          relationshipsToCreate.length > 0
            ? await smaps.createElements(relationshipsToCreate)
            : [];

        // RESPONSE SHAPE RULE (plan B2): keep existing {success, elementsImported,
        // elements} shape; append findings whenever ANY findings exist — including
        // error-severity findings bypassed via allow_invalid:true (bypass must be
        // auditable in transcripts; never filter errors out). relationshipsImported/
        // relationships are ADDITIVE new fields (Milestone 1) — existing consumers
        // that only read elementsImported/elements are unaffected.
        const response: Record<string, unknown> = {
          success: true,
          elementsImported: created.length,
          elements: created,
        };
        if (createdRelationships.length > 0) {
          response.relationshipsImported = createdRelationships.length;
          response.relationships = createdRelationships;
        }
        if (findings.length > 0) {
          response.findings = findings;
        }

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(response, null, 2),
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
