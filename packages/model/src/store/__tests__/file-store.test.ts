import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "../file-store.js";

describe("FileStore (file-native backend)", () => {
  let dir: string;
  let store: FileStore;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-filestore-"));
    store = new FileStore(dir);
    await store.createProject("ANGARS Test");
  });

  afterEach(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("creates and queries elements by type and by name substring", async () => {
    await store.createElement("PartDefinition", "Engine");
    await store.createElement("RequirementDefinition", "MassReq");

    const parts = await store.queryElements("PartDefinition");
    expect(parts).toHaveLength(1);
    expect(parts[0].name).toBe("Engine");

    const byName = await store.queryElements(undefined, "mass");
    expect(byName).toHaveLength(1);
    expect(byName[0].type).toBe("RequirementDefinition");
  });

  // NOTE: the "nests children under their owner so serialization is hierarchical"
  // test lives in @sysml-bridge/sysml (store-serialize.test.ts) — it exercises the
  // serializer, which this package must not depend on.

  it("classifies source/target elements as relationships and filters by direction", async () => {
    const part = await store.createElement("PartDefinition", "Pump");
    const req = await store.createElement("RequirementDefinition", "FlowReq");
    await store.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": part.id }],
      target: [{ "@id": req.id }],
    });

    const inbound = await store.queryRelationships(req.id, "in");
    expect(inbound).toHaveLength(1);
    expect(inbound[0].type).toBe("SatisfyRequirementUsage");
    expect(inbound[0].sourceIds).toContain(part.id);
    expect(inbound[0].targetIds).toContain(req.id);

    // The requirement is the target, not a source — no outbound relationships.
    const outbound = await store.queryRelationships(req.id, "out");
    expect(outbound).toHaveLength(0);
  });

  it("persists to disk and reloads in a fresh instance", async () => {
    await store.createElement("PartDefinition", "Persisted");
    const projectId = store.projectId!;

    const reopened = new FileStore(dir);
    await reopened.loadProject(projectId);

    const parts = await reopened.queryElements("PartDefinition");
    expect(parts.some((p) => p.name === "Persisted")).toBe(true);
  });

  it("reports project state with element counts by type", async () => {
    await store.createElements([
      { type: "PartDefinition", name: "A" },
      { type: "PartDefinition", name: "B" },
      { type: "RequirementDefinition", name: "R1" },
    ]);

    const state = await store.getProjectState();
    expect(state.totalElements).toBe(3);
    expect(state.elementCountsByType["PartDefinition"]).toBe(2);
    expect(state.elementCountsByType["RequirementDefinition"]).toBe(1);
  });

  it("requires initialization before mutations", async () => {
    const fresh = new FileStore(dir);
    await expect(fresh.createElement("PartDefinition", "X")).rejects.toThrow(
      /not initialized/
    );
  });
});
