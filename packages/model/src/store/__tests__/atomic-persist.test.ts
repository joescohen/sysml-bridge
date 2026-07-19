import { describe, it, expect, vi, afterEach } from "vitest";
import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { FileStore } from "../file-store.js";

describe("FileStore.persist atomicity", () => {
  afterEach(() => vi.restoreAllMocks());

  it("writes to a temp file then renames — never writes the store file directly", async () => {
    const dir = await fsp.mkdtemp(path.join(os.tmpdir(), "foundry-atomic-"));
    const store = new FileStore(dir);
    await store.createProject("Atomic Demo");

    // file-store.ts uses `import { promises as fs } from "node:fs"` — spy on that object.
    const writeSpy = vi.spyOn(fs.promises, "writeFile");
    const renameSpy = vi.spyOn(fs.promises, "rename");

    await store.createElement("PartDefinition", "Engine");

    const storeFile = path.join(dir, "atomic-demo.json");
    // No writeFile call may target the real store file...
    for (const call of writeSpy.mock.calls) {
      expect(String(call[0])).not.toBe(storeFile);
    }
    // ...and the final rename must land on it.
    expect(renameSpy).toHaveBeenCalled();
    const lastRename = renameSpy.mock.calls.at(-1)!;
    expect(String(lastRename[1])).toBe(storeFile);
    // The store file exists, parses, and no temp file is left behind.
    const doc = JSON.parse(await fsp.readFile(storeFile, "utf8"));
    expect(doc.elements).toHaveLength(1);
    const leftovers = (await fsp.readdir(dir)).filter((f) => f.includes(".tmp-"));
    expect(leftovers).toEqual([]);
  });
});
