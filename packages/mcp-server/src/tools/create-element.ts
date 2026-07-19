import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";
import { structuralCheck, resolveGateCorpus } from "@sysml-bridge/gates";
import type { Finding } from "@sysml-bridge/gates";

export function registerCreateElement(server: McpServer, smaps: ModelStore) {
  server.tool(
    "create_element",
    "Create a SysML v2 element. Supports all SysML v2 types: PartDefinition, RequirementDefinition, ActionDefinition, etc.",
    {
      type: z
        .string()
        .describe("SysML v2 element type (e.g. PartDefinition, RequirementDefinition, Package)"),
      name: z.string().describe("Element name"),
      attributes: z
        .record(z.unknown())
        .optional()
        .describe("Additional element attributes"),
      allow_invalid: z
        .boolean()
        .optional()
        .describe(
          "Skip structural Gate 1 checks — only for multi-step construction where intermediate states are incomplete"
        ),
    },
    async ({ type, name, attributes, allow_invalid }) => {
      try {
        // ── GATE-05 pre-add structural check (check-BEFORE-add) ──
        // Extract sourceIds/targetIds from attributes.source/attributes.target
        // arrays of {"@id"} objects when present (mirrors file-store.ts raw
        // conventions). Plain elements without source/target get empty arrays.
        const rawAttrs = attributes ?? {};
        const extractIds = (key: string): string[] => {
          const val = rawAttrs[key];
          if (!Array.isArray(val)) return [];
          return val
            .filter((v): v is { "@id": string } => v !== null && typeof v === "object" && "@id" in v)
            .map((v) => v["@id"]);
        };

        const candidate = {
          type,
          name,
          sourceIds: extractIds("source"),
          targetIds: extractIds("target"),
          provenanceSourceId: rawAttrs.provenanceSourceId,
        };

        const existingElements = await smaps.queryElements();
        const resolutionSet = await resolveGateCorpus();
        const findings: Finding[] = structuralCheck(candidate, existingElements, resolutionSet);
        const errors = findings.filter((f) => f.severity === "error");

        if (errors.length > 0 && !allow_invalid) {
          // Reject: store untouched
          return {
            isError: true,
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ rejected: true, findings }, null, 2),
              },
            ],
          };
        }

        // ── Persist ──
        const element = await smaps.createElement(type, name, rawAttrs);

        // RESPONSE SHAPE RULE (plan B2): when ANY findings exist, return
        // {element, findings} with the FULL set — including error-severity
        // findings when allow_invalid bypassed them (bypass must be auditable
        // in transcripts; never filter errors out). When zero findings, return
        // the element verbatim so existing tests parse the bare element.
        if (findings.length > 0) {
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ element, findings }, null, 2),
              },
            ],
          };
        }

        return {
          content: [{ type: "text" as const, text: JSON.stringify(element, null, 2) }],
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
