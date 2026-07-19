import { defineConfig } from "vitest/config";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      // Resolve workspace deps to source in tests so vitest never depends on
      // a stale dist/ build.
      "@sysml-bridge/model": path.resolve(here, "../model/src/index.ts"),
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});
