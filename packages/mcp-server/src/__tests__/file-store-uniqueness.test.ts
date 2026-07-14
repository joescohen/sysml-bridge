/**
 * file-store-uniqueness.test.ts
 *
 * Regression coverage for two data-integrity gaps in FileStore (see the
 * "Known limitations" validation pass):
 *
 *   Gap 1 — slug collision: two DISTINCT project names that slugify to the same
 *           on-disk id (e.g. "My Model" vs "my-model") used to overwrite each
 *           other silently, so listProjects() showed only one. createProject now
 *           disambiguates a colliding slug while preserving the reset-on-same-
 *           name behavior that generate-cc-model.ts / e2e-proof.ts depend on.
 *
 *   Gap 2 — duplicate @id: a caller-supplied attributes["@id"] (forwarded by the
 *           create_element tool) was inserted unconditionally, producing an
 *           unreachable zombie row and letting a single deleteElement() wipe both
 *           halves of the pair. insert() now rejects a colliding id, and
 *           deleteElement() removes exactly one matched row by position.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "../file-store.js";

describe("FileStore — project slug uniqueness (Gap 1)", () => {
  let dir: string;
  let store: FileStore;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-slug-"));
    store = new FileStore(dir);
  });

  afterEach(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("disambiguates distinct names that slugify to the same id instead of overwriting", async () => {
    const p1 = await store.createProject("My Model");
    await store.createElement("PartDefinition", "EngineA");

    // Distinct name, identical slug ("my-model"): must NOT clobber the first.
    const p2 = await store.createProject("my-model");
    await store.createElement("PartDefinition", "EngineB");

    expect(p1["@id"]).toBe("my-model");
    expect(p2["@id"]).toBe("my-model-2");
    expect(p2["@id"]).not.toBe(p1["@id"]);

    // Both projects are visible — the bug reported only one.
    const list = await store.listProjects();
    expect(list).toHaveLength(2);
    expect(list.map((p) => p.name).sort()).toEqual(["My Model", "my-model"]);
  });

  it("preserves the FIRST project's model when a slug-colliding project is created", async () => {
    const p1 = await store.createProject("ANGARS Test");
    await store.createElement("PartDefinition", "OriginalPart");

    // Different name, same slug ("angars-test").
    await store.createProject("angars test");
    await store.createElement("PartDefinition", "OtherPart");

    // The first project's element must still be intact on disk.
    const reopened = new FileStore(dir);
    await reopened.loadProject(p1["@id"]);
    const parts = await reopened.queryElements("PartDefinition");
    expect(parts.map((e) => e.name)).toEqual(["OriginalPart"]);
  });

  it("preserves reset-on-same-name: re-creating the SAME name reuses the id and empties it", async () => {
    const first = await store.createProject("Reset Me");
    await store.createElement("PartDefinition", "Temp");
    expect(await store.queryElements()).toHaveLength(1);

    // Same name again → same id, model reset to empty (relied on by
    // generate-cc-model.ts and e2e-proof.ts).
    const again = await store.createProject("Reset Me");
    expect(again["@id"]).toBe(first["@id"]);
    expect(await store.queryElements()).toHaveLength(0);

    // Reset must NOT spawn a suffixed duplicate file.
    const sameName = (await store.listProjects()).filter((p) => p.name === "Reset Me");
    expect(sameName).toHaveLength(1);
  });
});

describe("FileStore — element @id uniqueness (Gap 2)", () => {
  let dir: string;
  let store: FileStore;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-elemid-"));
    store = new FileStore(dir);
    await store.createProject("Uniqueness Test");
  });

  afterEach(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("rejects a createElement with a colliding caller-supplied @id (no zombie)", async () => {
    await store.createElement("PartDefinition", "A", { "@id": "x" });

    await expect(
      store.createElement("PartDefinition", "B", { "@id": "x" })
    ).rejects.toThrow(/Duplicate element id: x/);

    // Exactly one element carries id "x" and it is the original — the rejected
    // insert left the store untouched (no unreachable second row).
    const all = await store.queryElements();
    expect(all.filter((e) => e.id === "x")).toHaveLength(1);
    expect((await store.getElement("x")).name).toBe("A");
  });

  it("rejects a createElements batch with an internal duplicate and inserts nothing", async () => {
    await expect(
      store.createElements([
        { type: "PartDefinition", name: "P1", attributes: { "@id": "dup" } },
        { type: "PartDefinition", name: "P2", attributes: { "@id": "dup" } },
      ])
    ).rejects.toThrow(/Duplicate element id: dup/);

    // No partial mutation: the whole batch is rejected before any insert.
    expect(await store.queryElements()).toHaveLength(0);
  });

  it("rejects a createElements batch whose @id collides with an existing element", async () => {
    await store.createElement("PartDefinition", "Existing", { "@id": "e1" });

    await expect(
      store.createElements([
        { type: "PartDefinition", name: "New1" },
        { type: "PartDefinition", name: "Collide", attributes: { "@id": "e1" } },
      ])
    ).rejects.toThrow(/Duplicate element id: e1/);

    // Only the pre-existing element remains; the batch's "New1" was not inserted.
    const all = await store.queryElements();
    expect(all).toHaveLength(1);
    expect(all[0].name).toBe("Existing");
  });

  it("still creates normal auto-UUID elements with unique ids", async () => {
    const a = await store.createElement("PartDefinition", "Alpha");
    const b = await store.createElement("PartDefinition", "Beta");
    expect(a.id).not.toBe(b.id);
    expect(await store.queryElements()).toHaveLength(2);
  });

  it("deleteElement removes exactly ONE matched row when a duplicate exists in a loaded doc", async () => {
    await store.createElement("PartDefinition", "Keep");
    await store.createElement("PartDefinition", "DupA", { "@id": "dup" });

    // A duplicate can no longer be created through the store, so simulate one
    // arriving via a hand-edited / legacy persisted doc: append a second row
    // sharing "dup" directly to the on-disk model and reload.
    const docPath = path.join(dir, `${store.projectId}.json`);
    const doc = JSON.parse(await fs.readFile(docPath, "utf8")) as {
      elements: Array<{ id: string; name: string | null; [k: string]: unknown }>;
    };
    const dupA = doc.elements.find((e) => e.id === "dup")!;
    doc.elements.push({ ...dupA, name: "DupB" });
    await fs.writeFile(docPath, JSON.stringify(doc, null, 2), "utf8");
    await store.loadProject(store.projectId!);

    // Sanity: the loaded doc genuinely holds two rows with id "dup".
    expect((await store.queryElements()).filter((e) => e.id === "dup")).toHaveLength(2);

    // One delete removes ONE row — the old id-equality filter wiped BOTH.
    await store.deleteElement("dup");
    expect((await store.queryElements()).filter((e) => e.id === "dup")).toHaveLength(1);

    // The surviving duplicate is reachable and can be deleted on its own.
    await store.deleteElement("dup");
    expect((await store.queryElements()).filter((e) => e.id === "dup")).toHaveLength(0);

    // The unrelated element is never touched.
    expect((await store.queryElements()).some((e) => e.name === "Keep")).toBe(true);
  });

  it("deleteElement still removes a normal unique element", async () => {
    const solo = await store.createElement("PartDefinition", "Solo");
    await store.deleteElement(solo.id);
    expect(await store.queryElements()).toHaveLength(0);
  });
});
