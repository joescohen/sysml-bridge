import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { promises as fsp } from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import {
  sourceScanRatchet,
  deriveScanRoots,
  defaultIsCallSite,
} from "../source-scan-ratchet.js";

// The guarded token used in these fixtures — deliberately NOT one of the repo's
// real writers, so nothing here is a live call site the real ratchet would scan.
const TOKEN = "dangerousWrite";

async function writeFile(root: string, rel: string, contents: string): Promise<void> {
  const abs = path.join(root, rel);
  await fsp.mkdir(path.dirname(abs), { recursive: true });
  await fsp.writeFile(abs, contents);
}

describe("sourceScanRatchet", () => {
  let repo: string;

  beforeEach(async () => {
    repo = await fsp.mkdtemp(path.join(os.tmpdir(), "ratchet-fixture-"));
  });
  afterEach(async () => {
    await fsp.rm(repo, { recursive: true, force: true });
  });

  it("derives one scan root per package/<pkg>/src that exists", async () => {
    await writeFile(repo, "packages/alpha/src/index.ts", "export const a = 1;\n");
    await writeFile(repo, "packages/beta/src/index.ts", "export const b = 2;\n");
    // A package with no src/ is NOT a scan root.
    await writeFile(repo, "packages/gamma/README.md", "no source here\n");

    const roots = deriveScanRoots(repo);
    expect(roots).toHaveLength(2);
    expect(roots.map((r) => path.relative(repo, r))).toEqual([
      path.join("packages", "alpha", "src"),
      path.join("packages", "beta", "src"),
    ]);
  });

  it("no offenders when the only call site is an allowlisted defining module", async () => {
    await writeFile(
      repo,
      "packages/model/src/writer.ts",
      `export function ${TOKEN}(x: string) { return x; }\n`
    );
    await writeFile(
      repo,
      "packages/model/src/gate-surface.ts",
      `import { ${TOKEN} } from "./writer.js";\nexport function onClick() {\n  ${TOKEN}("ok");\n}\n`
    );

    const result = sourceScanRatchet({
      repoRoot: repo,
      tokens: [TOKEN],
      allowlist: [
        path.join("packages/model/src", "writer.ts"),
        path.join("packages/model/src", "gate-surface.ts"),
      ],
    });

    expect(result.scanRoots.length).toBeGreaterThanOrEqual(1);
    expect(result.offenders).toEqual([]);
  });

  it("POSITIVE CONTROL: a planted call site in a non-allowlisted module is an offender", async () => {
    await writeFile(
      repo,
      "packages/model/src/writer.ts",
      `export function ${TOKEN}(x: string) { return x; }\n`
    );
    // A rogue call site in a module NOT on the allowlist — must be caught.
    await writeFile(
      repo,
      "packages/rogue/src/leak.ts",
      `import { ${TOKEN} } from "../../model/src/writer.js";\nexport function sneaky() {\n  ${TOKEN}("escaped");\n}\n`
    );

    const result = sourceScanRatchet({
      repoRoot: repo,
      tokens: [TOKEN],
      allowlist: [path.join("packages/model/src", "writer.ts")],
    });

    expect(result.offenders).toHaveLength(1);
    expect(result.offenders[0]).toContain("packages/rogue/src/leak.ts");
    expect(result.offenders[0]).toContain(`${TOKEN}("escaped")`);
  });

  it("POSITIVE CONTROL: dropping a defining module from the allowlist surfaces it as an offender", async () => {
    await writeFile(
      repo,
      "packages/model/src/writer.ts",
      `export function ${TOKEN}(x: string) { return x; }\n`
    );
    await writeFile(
      repo,
      "packages/model/src/surface.ts",
      `import { ${TOKEN} } from "./writer.js";\nexport function onClick() {\n  ${TOKEN}("ok");\n}\n`
    );

    // surface.ts is NO LONGER allowlisted → its call site is now an offender.
    const result = sourceScanRatchet({
      repoRoot: repo,
      tokens: [TOKEN],
      allowlist: [path.join("packages/model/src", "writer.ts")],
    });
    expect(result.offenders.some((o) => o.includes("packages/model/src/surface.ts"))).toBe(true);
  });

  it("skips __tests__, node_modules, dist, .d.ts and .test.ts (no false offenders)", async () => {
    await writeFile(repo, "packages/model/src/__tests__/x.test.ts", `${TOKEN}("in test dir");\n`);
    await writeFile(repo, "packages/model/src/y.test.ts", `${TOKEN}("in test file");\n`);
    await writeFile(repo, "packages/model/src/z.d.ts", `declare function ${TOKEN}(): void;\n`);
    await writeFile(repo, "packages/model/src/dist/bundled.ts", `${TOKEN}("in dist");\n`);
    await writeFile(repo, "packages/model/src/node_modules/dep.ts", `${TOKEN}("in nm");\n`);

    const result = sourceScanRatchet({ repoRoot: repo, tokens: [TOKEN], allowlist: [] });
    expect(result.offenders).toEqual([]);
  });

  it("default call-site predicate ignores declarations, imports, and comments", () => {
    expect(defaultIsCallSite(`  ${TOKEN}("go");`, TOKEN)).toBe(true);
    expect(defaultIsCallSite(`export function ${TOKEN}() {}`, TOKEN)).toBe(false);
    expect(defaultIsCallSite(`import { ${TOKEN} } from "./w.js";`, TOKEN)).toBe(false);
    expect(defaultIsCallSite(`export { ${TOKEN} } from "./w.js";`, TOKEN)).toBe(false);
    expect(defaultIsCallSite(` * calls ${TOKEN}() somewhere`, TOKEN)).toBe(false);
    expect(defaultIsCallSite(`// ${TOKEN}("commented out")`, TOKEN)).toBe(false);
  });

  it("honours an explicit roots list over derivation", async () => {
    await writeFile(repo, "custom/place/leak.ts", `${TOKEN}("here");\n`);
    const result = sourceScanRatchet({
      repoRoot: repo,
      roots: [path.join(repo, "custom/place")],
      tokens: [TOKEN],
      allowlist: [],
    });
    expect(result.offenders).toHaveLength(1);
    expect(result.offenders[0]).toContain("custom/place/leak.ts");
  });
});
