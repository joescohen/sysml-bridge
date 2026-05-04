import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";
import { parseSysml } from "../utils/sysml-parser.js";

export function registerImportSysml(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "import_sysml",
    "Parse SysML v2 textual notation and import elements into the model via commits",
    {
      sysml_text: z.string().describe("SysML v2 textual notation to parse and import"),
    },
    async ({ sysml_text }) => {
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

        const elementsToCreate = parsed.elements.map((e) => ({
          type: e.type,
          name: e.name,
          attributes: e.attributes,
        }));

        const created = await smaps.createElements(elementsToCreate);

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                { success: true, elementsImported: created.length, elements: created },
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
