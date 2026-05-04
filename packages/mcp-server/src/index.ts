import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { SmapsClient } from "./smaps-client.js";
import { registerCreateElement } from "./tools/create-element.js";
import { registerQueryElements } from "./tools/query-elements.js";
import { registerCreateRelationship } from "./tools/create-relationship.js";
import { registerQueryRelationships } from "./tools/query-relationships.js";
import { registerValidateModel } from "./tools/validate-model.js";
import { registerExportSysml } from "./tools/export-sysml.js";
import { registerImportSysml } from "./tools/import-sysml.js";
import { registerGetProjectState } from "./tools/get-project-state.js";

const SMAPS_ENDPOINT = process.env.SMAPS_ENDPOINT ?? "http://localhost:9000";
const PROJECT_ID = process.env.PROJECT_ID ?? "default";

const server = new McpServer({
  name: "sysml-bridge",
  version: "0.1.0",
});

const smaps = new SmapsClient(SMAPS_ENDPOINT, PROJECT_ID);

registerCreateElement(server, smaps);
registerQueryElements(server, smaps);
registerCreateRelationship(server, smaps);
registerQueryRelationships(server, smaps);
registerValidateModel(server, smaps);
registerExportSysml(server, smaps);
registerImportSysml(server, smaps);
registerGetProjectState(server, smaps);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch(console.error);
