import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "@sysml-bridge/model";
import { serializeToSysml } from "../sysml-serializer.js";

// Moved from @sysml-bridge/model's file-store tests: it exercises the
// serializer against store output, so it lives with the serializer.
describe("FileStore → serializer integration", () => {
  let dir: string;
  let store: FileStore;

  beforeEach(async () => {
    dir = await fs.mkdtemp(path.join(os.tmpdir(), "sysml-store-serialize-"));
    store = new FileStore(dir);
    await store.createProject("Store Serialize");
  });

  afterEach(async () => {
    await fs.rm(dir, { recursive: true, force: true });
  });

  it("nests children under their owner so serialization is hierarchical", async () => {
    const sys = await store.createElement("PartDefinition", "System");
    await store.createElement("PartUsage", "subPart", {
      owner: { "@id": sys.id },
    });

    const elements = await store.queryElements();
    const sysml = serializeToSysml(elements, []);

    expect(sysml).toContain("part def System {");
    expect(sysml).toContain("part subPart");
    // the child must render *inside* the parent block
    expect(sysml.indexOf("part subPart")).toBeGreaterThan(
      sysml.indexOf("part def System {")
    );
  });
});
