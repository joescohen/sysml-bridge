import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";
import { parseSysml } from "../utils/sysml-parser.js";

export function registerImportSysml(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "import_sysml",
    "Parse SysML v2 textual notation and import elements into the model store",
    {
      sysml_text: z.string().describe("SysML v2 textual notation to parse and import"),
    },
    async ({ sysml_text }) => {
      const parsed = parseSysml(sysml_text);

      if (parsed.errors.length > 0) {
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  success: false,
                  errors: parsed.errors,
                },
                null,
                2
              ),
            },
          ],
        };
      }

      const created = [];
      for (const element of parsed.elements) {
        const result = await smaps.createElement(
          element.type,
          element.name,
          element.attributes
        );
        created.push(result);
      }

      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(
              {
                success: true,
                elementsImported: created.length,
                elements: created,
              },
              null,
              2
            ),
          },
        ],
      };
    }
  );
}
