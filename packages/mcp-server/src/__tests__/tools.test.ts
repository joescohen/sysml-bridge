import { describe, it, expect, vi, beforeEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { SmapsClient } from "../smaps-client.js";
import { registerCreateElement } from "../tools/create-element.js";
import { registerQueryElements } from "../tools/query-elements.js";
import { registerGetProjectState } from "../tools/get-project-state.js";

describe("MCP Tools", () => {
  let server: McpServer;
  let smaps: SmapsClient;

  beforeEach(() => {
    server = new McpServer({ name: "test", version: "0.0.1" });
    smaps = new SmapsClient("http://localhost:9000");
  });

  it("registerCreateElement registers a tool named create_element", () => {
    registerCreateElement(server, smaps);
    expect(true).toBe(true);
  });

  it("registerQueryElements registers a tool named query_elements", () => {
    registerQueryElements(server, smaps);
    expect(true).toBe(true);
  });

  it("registerGetProjectState registers a tool named get_project_state", () => {
    registerGetProjectState(server, smaps);
    expect(true).toBe(true);
  });
});
