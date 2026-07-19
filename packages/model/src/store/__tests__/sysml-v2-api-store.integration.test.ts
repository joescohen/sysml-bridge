import { describe, it, expect, beforeAll } from "vitest";
import { SysmlV2ApiStore } from "../sysml-v2-api-store.js";

// ---------------------------------------------------------------------------
// LIVE integration test — requires a running OMG SysML v2 API pilot server
// (Postgres-backed) at SYSML_FOUNDRY_API_ENDPOINT / http://localhost:9000.
// Run with:
//
//   INTEGRATION=1 pnpm --filter @sysml-bridge/model test
//
// Skipped by default so the regular unit suite never depends on external
// infra. Every wire shape exercised here was verified live against the
// pilot on 2026-07-12 before this store was written (see the class-level
// comment in ../sysml-v2-api-store.ts for the deviations found).
// ---------------------------------------------------------------------------

const API_ENDPOINT = process.env.SYSML_FOUNDRY_API_ENDPOINT ?? "http://localhost:9000";

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

describe.skipIf(!process.env.INTEGRATION)(
  "SysmlV2ApiStore integration (requires the live pilot server)",
  () => {
    let store: SysmlV2ApiStore;

    beforeAll(async () => {
      store = new SysmlV2ApiStore(API_ENDPOINT);
    });

    it("connects to the live pilot server", async () => {
      const connected = await store.checkConnection();
      expect(connected).toBe(true);
    });

    it("creates a project against the live server", async () => {
      const project = await store.createProject(`foundry-milestone2-${Date.now()}`);
      expect(project["@id"]).toBeTruthy();
      expect(project["@id"]).toMatch(UUID_RE);
      expect(store.projectId).toBeTruthy();
      expect(store.branchId).toBeTruthy();
    });

    it("creates a PartDefinition and a PartUsage in one commit, with server-assigned elementIds", async () => {
      const elements = await store.createElements([
        { type: "PartDefinition", name: "Engine" },
        { type: "PartUsage", name: "engine1" },
      ]);

      expect(elements).toHaveLength(2);
      for (const el of elements) {
        // Server-assigned identity: a real UUID, and id/elementId agree.
        expect(el.elementId).toMatch(UUID_RE);
        expect(el.id).toBe(el.elementId);
      }

      const partDef = elements.find((e) => e.type === "PartDefinition");
      const partUsage = elements.find((e) => e.type === "PartUsage");
      expect(partDef?.name).toBe("Engine");
      expect(partUsage?.name).toBe("engine1");
    });

    it("round-trips both created elements back out through queryElements()", async () => {
      const parts = await store.queryElements("PartDefinition");
      expect(parts.some((p) => p.name === "Engine")).toBe(true);
      for (const p of parts) {
        expect(p.elementId).toMatch(UUID_RE);
      }

      const usages = await store.queryElements("PartUsage");
      expect(usages.some((u) => u.name === "engine1")).toBe(true);
    });

    it("folds a foundry-local aliasId into aliasIds and reads it back distinct from the server elementId", async () => {
      const el = await store.createElement("PartDefinition", "Wheel", {
        aliasId: "foundry-local-wheel-1",
      });
      expect(el.elementId).toMatch(UUID_RE);
      expect(el.aliasId).toBe("foundry-local-wheel-1");
      expect(el.elementId).not.toBe(el.aliasId);

      const fetched = await store.getElement(el.elementId);
      expect(fetched.aliasId).toBe("foundry-local-wheel-1");
    });

    it("reflects the created elements' counts in getProjectState()", async () => {
      const state = await store.getProjectState();
      expect(state.projectId).toBe(store.projectId);
      expect(state.totalElements).toBeGreaterThanOrEqual(3);
      expect(state.elementCountsByType["PartDefinition"]).toBeGreaterThanOrEqual(2);
      expect(state.elementCountsByType["PartUsage"]).toBeGreaterThanOrEqual(1);
    });

    it("updates an element's declaredName while preserving its elementId and other fields", async () => {
      const created = await store.createElement("PartDefinition", "Bracket", {
        isAbstract: true,
      });

      const updated = await store.updateElement(created.elementId, { name: "Bracket v2" });
      expect(updated.elementId).toBe(created.elementId);
      expect(updated.name).toBe("Bracket v2");
      expect(updated.raw.isAbstract).toBe(true);
    });

    it("deletes an element so it no longer appears in the current commit", async () => {
      const created = await store.createElement("PartDefinition", "Throwaway");
      await store.deleteElement(created.elementId);

      await expect(store.getElement(created.elementId)).rejects.toThrow();
      const remaining = await store.queryElements("PartDefinition", "Throwaway");
      expect(remaining).toHaveLength(0);
    });
  }
);
