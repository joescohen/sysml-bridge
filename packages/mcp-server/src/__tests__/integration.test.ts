import { describe, it, expect, beforeAll } from "vitest";
import { SmapsClient } from "../smaps-client.js";

const SMAPS_URL = process.env.SMAPS_ENDPOINT ?? "http://localhost:9000";

describe.skipIf(!process.env.INTEGRATION)(
  "SmapsClient integration (requires Docker)",
  () => {
    let client: SmapsClient;

    beforeAll(async () => {
      client = new SmapsClient(SMAPS_URL);
    });

    it("connects to the SMAPS server", async () => {
      const connected = await client.checkConnection();
      expect(connected).toBe(true);
    });

    it("creates a project", async () => {
      const project = await client.createProject(`test-${Date.now()}`);
      expect(project["@id"]).toBeTruthy();
      expect(client.projectId).toBeTruthy();
      expect(client.branchId).toBeTruthy();
    });

    it("creates an element via commit", async () => {
      const element = await client.createElement("PartDefinition", "Engine");
      expect(element.type).toBe("PartDefinition");
      expect(element.name).toBe("Engine");
      expect(element.id).toBeTruthy();
    });

    it("queries elements by type", async () => {
      await client.createElement("PartDefinition", "Wheel");
      const results = await client.queryElements("PartDefinition");
      expect(results.length).toBeGreaterThanOrEqual(2);
    });

    it("creates a requirement and queries it", async () => {
      await client.createElement("RequirementDefinition", "MassReq");
      const reqs = await client.queryElements("RequirementDefinition");
      expect(reqs.some((r) => r.name === "MassReq")).toBe(true);
    });

    it("gets project state with element counts", async () => {
      const state = await client.getProjectState();
      expect(state.projectId).toBeTruthy();
      expect(state.totalElements).toBeGreaterThan(0);
      expect(state.elementCountsByType).toBeDefined();
    });
  }
);
