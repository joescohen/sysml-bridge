# sysml-foundry Phase 0 — Scaffold + Core Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the new `sysml-foundry` repo and port the proven core (model store + IR, serializer/parser, grammar validator, DeciSym viewer) with the audit fixes (atomic writes, quote escaping) applied, ending with green CI.

**Architecture:** pnpm TypeScript monorepo. `packages/model` = element types + `ModelStore` interface + `FileStore` + zod IR schema (merges the old repo's `packages/ir` with the store half of `packages/mcp-server`). `packages/sysml` = serializer + parser (model ↔ SysML v2 textual notation). `tools/sysml-validator` = committed Python ANTLR grammar validator, ported verbatim. `tools/viewer` = vendored Rust DeciSym fork, ported verbatim minus build artifacts.

**Tech Stack:** Node ≥ 20, pnpm 9, TypeScript 5 strict, vitest, zod, Python 3 (ANTLR runtime), Rust 1.96 (viewer only).

**Design spec:** `docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md` in the OLD repo (read §1, §3, §6, §8 Phase 0 before starting).

## Global Constraints

- OLD repo (read-only source of ports): `/home/joescohen/Engineering/projects/sysml-bridge` — referred to as `$OLD` below. NEVER modify it.
- NEW repo: `/home/joescohen/Engineering/projects/sysml-foundry` — referred to as `$NEW`.
- Package scope is `@sysml-foundry/*` (replaces `@sysml-bridge/*`).
- Do NOT port: `smaps-client.ts`, `types/smaps.ts` SMAPS-specific parts used only by it, `cc-presentation.ts` (moves in Phase 1), any `scripts/*.ts`, the 15 skills, `.planning/`.
- `expect(true).toBe(true)` must appear NOWHERE in `$NEW` (do not port `$OLD/packages/mcp-server/src/__tests__/tools.test.ts`).
- Every ported package lands WITH its tests in the same commit and `pnpm test` green.
- The SysML v2 grammar reference is vendored, not remembered: when any SysML syntax question arises, read `$NEW/docs/sysml-v2-reference/` (ported in Task 1), never guess.
- GitHub repo is created **private** initially; flipping public is Joe's call at Phase 1.
- Node engines `>=20`, `packageManager: pnpm@9.15.0` (match old repo).

---

### Task 1: Scaffold the repo

**Files:**
- Create: `$NEW/package.json`, `$NEW/pnpm-workspace.yaml`, `$NEW/tsconfig.base.json`, `$NEW/.gitignore`, `$NEW/LICENSE`, `$NEW/README.md` (stub), `$NEW/CLAUDE.md`, `$NEW/docs/sysml-v2-reference/` (ported), `$NEW/docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md` (the spec, committed here as the founding design doc)

**Interfaces:**
- Produces: workspace layout every later task installs into; `tsconfig.base.json` that package tsconfigs extend.

- [ ] **Step 1: Create repo and copy the grammar reference + spec**

```bash
mkdir -p ~/Engineering/projects/sysml-foundry && cd ~/Engineering/projects/sysml-foundry
git init -b main
cp -r /home/joescohen/Engineering/projects/sysml-bridge/docs/sysml-v2-reference docs/sysml-v2-reference
mkdir -p docs/superpowers/specs
cp /home/joescohen/Engineering/projects/sysml-bridge/docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md docs/superpowers/specs/
cp /home/joescohen/Engineering/projects/sysml-bridge/LICENSE LICENSE
```

- [ ] **Step 2: Write root config files**

`package.json`:
```json
{
  "name": "sysml-foundry",
  "version": "0.1.0",
  "private": true,
  "description": "Corpus-grounded SysML v2 authoring with Claude — every element cited, every gate enforced",
  "author": "Joe Cohen",
  "license": "MIT",
  "scripts": {
    "build": "pnpm -r build",
    "test": "pnpm -r test",
    "lint": "pnpm -r lint",
    "validate:sysml": "tools/sysml-validator/run.sh"
  },
  "engines": { "node": ">=20" },
  "packageManager": "pnpm@9.15.0"
}
```

`pnpm-workspace.yaml`:
```yaml
packages:
  - "packages/*"
```

`tsconfig.base.json`:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "declaration": true,
    "sourceMap": true
  }
}
```

`.gitignore`:
```
node_modules/
dist/
target/
.venv/
*.tmp-*
examples/*/out/
.mbse/
```

- [ ] **Step 3: Write CLAUDE.md carrying the R1–R4 discipline**

Copy the "RULES — SysML v2 emission discipline" and "Validation gate" sections verbatim from `$OLD/CLAUDE.md` into `$NEW/CLAUDE.md`, then edit path references: `packages/mcp-server/src/utils/sysml-serializer.ts` → `packages/sysml/src/sysml-serializer.ts`; the regenerate/validate example commands change in Phase 1 (leave the gate order text intact, replace the concrete `generate-cc-model` command with the placeholder line `pnpm demo  # Phase 1`). Keep R1–R4 numbering and wording otherwise identical.

- [ ] **Step 4: Stub README and commit**

`README.md` stub (replaced in Phase 1):
```markdown
# sysml-foundry

Corpus-grounded SysML v2 authoring with Claude — every element cited, every gate enforced.

**Status: under construction (Phase 0).** Design spec: docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md
```

```bash
git add -A && git commit -m "chore: scaffold sysml-foundry workspace with vendored grammar reference"
```

- [ ] **Step 5: Verify**

Run: `pnpm install`
Expected: exit 0 (empty workspace is fine).

---

### Task 2: Port the grammar validator + positive-control fixtures

**Files:**
- Create: `$NEW/tools/sysml-validator/` (ported), `$NEW/tools/sysml-validator/fixtures/smoke-good.sysml`, `$NEW/tools/sysml-validator/fixtures/smoke-bad.sysml`

**Interfaces:**
- Produces: `tools/sysml-validator/run.sh <file>.sysml` → exit 0 iff 0 grammar errors (used by every later gate); `pnpm validate:sysml <file>` alias (already wired in Task 1 package.json).

- [ ] **Step 1: Copy the validator verbatim**

```bash
cp -r /home/joescohen/Engineering/projects/sysml-bridge/tools/sysml-validator ~/Engineering/projects/sysml-foundry/tools/sysml-validator
```

- [ ] **Step 2: Create the venv it expects**

```bash
cd ~/Engineering/projects/sysml-foundry
python3 -m venv .venv
.venv/bin/pip install -r tools/sysml-validator/requirements.txt
```

- [ ] **Step 3: Write the fixtures**

`tools/sysml-validator/fixtures/smoke-good.sysml`:
```sysml
package Smoke {
    part def Vehicle;
    part vehicle : Vehicle;
}
```

`tools/sysml-validator/fixtures/smoke-bad.sysml` (deliberate grammar error — this is the committed positive control proving the gate can fail):
```sysml
package Smoke {
    part def
}
```

- [ ] **Step 4: Verify both directions**

Run: `pnpm validate:sysml tools/sysml-validator/fixtures/smoke-good.sysml; echo "exit=$?"`
Expected: reports 0 errors, `exit=0`.

Run: `pnpm validate:sysml tools/sysml-validator/fixtures/smoke-bad.sysml; echo "exit=$?"`
Expected: reports ≥1 error, `exit=1`.

If the good fixture unexpectedly fails: STOP, read `docs/sysml-v2-reference/cheatsheet.md`, fix the fixture from the grammar — never proceed with a failing positive path.

- [ ] **Step 5: Commit**

```bash
git add tools/sysml-validator && git commit -m "feat: port local ANTLR grammar validator with positive-control fixtures"
```

---

### Task 3: Port `packages/model` (IR + store) with atomic persist

**Files:**
- Create: `$NEW/packages/model/package.json`, `$NEW/packages/model/tsconfig.json`, `$NEW/packages/model/vitest.config.ts`
- Create (ported): `$NEW/packages/model/src/` ← `$OLD/packages/ir/src/` (all files incl. `__tests__/` and `extract/`)
- Create (ported): `$NEW/packages/model/src/store/store.ts` ← `$OLD/packages/mcp-server/src/store.ts`; `$NEW/packages/model/src/store/file-store.ts` ← `$OLD/packages/mcp-server/src/file-store.ts`; `$NEW/packages/model/src/store/types.ts` ← `$OLD/packages/mcp-server/src/types/sysml-elements.ts`
- Create (ported): `$NEW/packages/model/src/store/__tests__/file-store.test.ts` ← `$OLD/packages/mcp-server/src/__tests__/file-store.test.ts`
- Test: `$NEW/packages/model/src/store/__tests__/atomic-persist.test.ts` (new)

**Interfaces:**
- Consumes: nothing (first package).
- Produces: `@sysml-foundry/model` exporting — `ExtractedSchema`, `stableId`, `composeIR` and the other `packages/ir` exports (keep old `index.ts` export list); plus `ModelStore` (interface), `FileStore` (class), `SysmlElement`, `SysmlRelationship`, `ProjectState` types. Later tasks import ONLY from `@sysml-foundry/model`.

- [ ] **Step 1: Copy and rename**

```bash
cd ~/Engineering/projects/sysml-foundry
mkdir -p packages/model/src/store/__tests__
cp -r /home/joescohen/Engineering/projects/sysml-bridge/packages/ir/src/* packages/model/src/
cp /home/joescohen/Engineering/projects/sysml-bridge/packages/ir/tsconfig.json packages/model/tsconfig.json
cp /home/joescohen/Engineering/projects/sysml-bridge/packages/ir/vitest.config.ts packages/model/vitest.config.ts
cp /home/joescohen/Engineering/projects/sysml-bridge/packages/mcp-server/src/store.ts packages/model/src/store/store.ts
cp /home/joescohen/Engineering/projects/sysml-bridge/packages/mcp-server/src/file-store.ts packages/model/src/store/file-store.ts
cp /home/joescohen/Engineering/projects/sysml-bridge/packages/mcp-server/src/types/sysml-elements.ts packages/model/src/store/types.ts
cp /home/joescohen/Engineering/projects/sysml-bridge/packages/mcp-server/src/__tests__/file-store.test.ts packages/model/src/store/__tests__/file-store.test.ts
```

`packages/model/package.json` — copy `$OLD/packages/ir/package.json` then change: `"name": "@sysml-foundry/model"`, description `"Model core: IR contract (zod) + ModelStore interface + FileStore"`, repository URL to `sysml-foundry` / directory `packages/model`. Keep the zod dependency and scripts as-is.

- [ ] **Step 2: Fix imports in the store files**

In `store/store.ts` and `store/file-store.ts`: change `./types/sysml-elements.js` → `./types.js`. `store.ts` also imports `SmapsProject` from `./types/smaps.js` — the FileStore uses it only for its `loadProject()`/`toSmapsProject()` return shape. Inline a minimal replacement at the top of `store/types.ts` instead of porting `smaps.ts`:

```ts
/** Minimal project descriptor (formerly the SMAPS wire shape). */
export interface ProjectDescriptor {
  "@id": string;
  "@type": "Project";
  name: string;
  defaultBranch: { "@id": string };
}
```

Then in `store.ts`/`file-store.ts` replace `SmapsProject` with `ProjectDescriptor` (imported from `./types.js`) everywhere. If `store.ts` references any other SMAPS-only type, STOP and report — do not port `smaps-client.ts` to make it compile.

In `src/store/__tests__/file-store.test.ts`: fix relative imports to `../file-store.js` etc. Add `export * from "./store/store.js"; export * from "./store/file-store.js"; export * from "./store/types.js";` to `src/index.ts`.

- [ ] **Step 3: Run the ported tests**

Run: `pnpm install && pnpm --filter @sysml-foundry/model test`
Expected: all ported ir tests + file-store tests PASS (old repo had these green; failures here mean an import/path mistake, not a logic bug).

- [ ] **Step 4: Write the failing atomic-persist test**

`packages/model/src/store/__tests__/atomic-persist.test.ts`:
```ts
import { describe, it, expect, vi, afterEach } from "vitest";
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

    const writeSpy = vi.spyOn(fsp, "writeFile");
    const renameSpy = vi.spyOn(fsp, "rename");

    await store.createElement("PartDefinition", "Engine");

    const storeFile = path.join(dir, "atomic-demo.json");
    // No writeFile call may target the real store file...
    for (const call of writeSpy.mock.calls) {
      expect(String(call[0])).not.toBe(storeFile);
    }
    // ...and the final rename must land on it.
    const lastRename = renameSpy.mock.calls.at(-1)!;
    expect(String(lastRename[1])).toBe(storeFile);
    // The store file exists, parses, and no temp file is left behind.
    const doc = JSON.parse(await fsp.readFile(storeFile, "utf8"));
    expect(doc.elements).toHaveLength(1);
    const leftovers = (await fsp.readdir(dir)).filter((f) => f.includes(".tmp-"));
    expect(leftovers).toEqual([]);
  });
});
```

Note: `FileStore`'s constructor/`createProject` signatures must match the ported code — read `store/file-store.ts` first and adjust the two setup lines (constructor arg name, `createProject` vs `initProject`) to the real API. The assertions are the contract; the setup lines follow the ported code.

- [ ] **Step 5: Run it to verify it fails**

Run: `pnpm --filter @sysml-foundry/model test -- atomic-persist`
Expected: FAIL — current `persist()` writes the store file directly (writeFile targets `storeFile`, rename never called).

- [ ] **Step 6: Implement atomic persist**

In `store/file-store.ts`, replace the body of `persist()` (currently a direct `fs.writeFile` of the doc — old repo lines ~326-342) with:

```ts
private async persist(): Promise<void> {
  await fs.mkdir(this.modelDir, { recursive: true });
  const doc: FileModelDoc = {
    "@type": "FileModel",
    id: this.projectId!,
    name: this.projectName ?? this.projectId!,
    branchId: this.branchId!,
    headCommitId: this.headCommitId!,
    elements: this.elements,
  };
  const target = this.filePath(this.projectId!);
  const tmp = `${target}.tmp-${process.pid}`;
  await fs.writeFile(tmp, JSON.stringify(doc, null, 2), "utf8");
  await fs.rename(tmp, target);
}
```

(Keep the existing `FileModelDoc` fields exactly as ported — if the ported doc shape differs from the above, keep the ported shape and change only the write mechanics.)

- [ ] **Step 7: Run tests to verify pass**

Run: `pnpm --filter @sysml-foundry/model test`
Expected: PASS, including atomic-persist and all ported tests.

- [ ] **Step 8: Commit**

```bash
git add packages/model && git commit -m "feat: port model package (IR + ModelStore + FileStore) with atomic persist"
```

---

### Task 4: Port `packages/sysml` (serializer + parser) with quote escaping

**Files:**
- Create: `$NEW/packages/sysml/package.json`, `tsconfig.json`, `vitest.config.ts` (same pattern as Task 3; name `@sysml-foundry/sysml`, add dependency `"@sysml-foundry/model": "workspace:*"`)
- Create (ported): `src/sysml-serializer.ts` ← `$OLD/packages/mcp-server/src/utils/sysml-serializer.ts`; `src/sysml-parser.ts` ← `$OLD/.../utils/sysml-parser.ts`; `src/trace-compare.ts` ← `$OLD/.../utils/trace-compare.ts`
- Create (ported tests): `src/__tests__/` ← `$OLD/packages/mcp-server/src/__tests__/{sysml-serializer.test.ts, sysml-serializer-aspects.test.ts, sysml-serializer-metatag.test.ts, sysml-parser.test.ts, export-sysml.test.ts, traceability-compare.test.ts}` — port `export-sysml.test.ts` ONLY if it tests the serializer directly; if it drives the MCP tool, skip it (tool tests come in Phase 2). Decide by reading its imports.
- Test: `src/__tests__/quote-escaping.test.ts` (new)

**Interfaces:**
- Consumes: `SysmlElement`, `SysmlRelationship` types from `@sysml-foundry/model`.
- Produces: `serializeToSysml(elements, relationships, opts?)` and the parser's exported parse function — keep the exact exported names found in the ported files (read them; do not rename exports).

- [ ] **Step 1: Copy, rename package, fix imports**

Copy the files listed above. In each, change type imports from `../types/sysml-elements.js` to `@sysml-foundry/model`. Create `src/index.ts` re-exporting the three modules' public exports.

- [ ] **Step 2: Run ported tests**

Run: `pnpm install && pnpm --filter @sysml-foundry/sysml test`
Expected: PASS (green in old repo).

- [ ] **Step 3: Write the failing quote-escaping test**

`src/__tests__/quote-escaping.test.ts` — the serializer must emit grammar-legal quoted names per the vendored KerML lexer rule (`UNRESTRICTED_NAME`: backslash-escaped `'` and `\`, escape sequences for control chars — confirm at `docs/sysml-v2-reference/grammar/SysMLv2Lexer.g4` before implementing):

```ts
import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../sysml-serializer.js";
import type { SysmlElement } from "@sysml-foundry/model";

function el(id: string, name: string, type = "PartDefinition"): SysmlElement {
  return { id, name, type, raw: { "@type": type } } as SysmlElement;
}

describe("quoted-name escaping", () => {
  it("escapes single quotes in element names", () => {
    const out = serializeToSysml([el("1", "Operator's Console")], []);
    expect(out).toContain("'Operator\\'s Console'");
    expect(out).not.toContain("'Operator's Console'");
  });

  it("escapes backslashes and control characters", () => {
    const out = serializeToSysml([el("2", "A\\B\nC")], []);
    expect(out).toContain("'A\\\\B\\nC'");
  });
});
```

Adjust the `el()` helper to the real `SysmlElement` shape from the ported types (read `packages/model/src/store/types.ts`; include any required fields). The two `toContain` assertions are the contract.

- [ ] **Step 4: Run to verify it fails**

Run: `pnpm --filter @sysml-foundry/sysml test -- quote-escaping`
Expected: FAIL — ported `quoteName` is `` `'${name}'` `` with no escaping.

- [ ] **Step 5: Implement escaping**

In `src/sysml-serializer.ts`, replace `quoteName` (old repo lines ~842-844):

```ts
/** Render a name as a SysML reference token: bare when it is a valid
 *  identifier, otherwise single-quoted with grammar-legal escapes. */
function quoteName(name: string): string {
  if (isValidIdentifier(name)) return name;
  const escaped = name
    .replace(/\\/g, "\\\\")
    .replace(/'/g, "\\'")
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "\\r")
    .replace(/\t/g, "\\t");
  return `'${escaped}'`;
}
```

Then search the file for any OTHER site that wraps a raw name in single quotes without going through `quoteName` (`grep -n "'\${" src/sysml-serializer.ts` and inspect callers of `refNameRaw`): route each through `quoteName`. If a `refNameRaw` caller applies its own quoting that cannot simply delegate (e.g., composes a dotted path), STOP and report the site rather than improvising.

- [ ] **Step 6: Run tests, then the end-to-end grammar check**

Run: `pnpm --filter @sysml-foundry/sysml test`
Expected: PASS (all, including the two new tests).

End-to-end positive control — write the serialized output of an apostrophe-named element to a temp file and validate it:

```bash
cd ~/Engineering/projects/sysml-foundry
pnpm tsx -e "
import { serializeToSysml } from './packages/sysml/src/index.js';
import * as fs from 'node:fs';
const el = { id: '1', name: \"O'Brien's Console\", type: 'PartDefinition', raw: { '@type': 'PartDefinition' } };
fs.writeFileSync('/tmp/quote-check.sysml', serializeToSysml([el], []));
"
pnpm validate:sysml /tmp/quote-check.sysml; echo "exit=$?"
```
Expected: 0 errors, `exit=0`. (Adjust the inline element literal to the real `SysmlElement` shape, same as the test. If `tsx` needs a real script file instead of `-e`, write it to `/tmp/quote-check.ts` and run that.)

- [ ] **Step 7: Commit**

```bash
git add packages/sysml && git commit -m "feat: port sysml package (serializer/parser) with grammar-legal quoted-name escaping"
```

---

### Task 5: Port `tools/viewer` (DeciSym fork)

**Files:**
- Create: `$NEW/tools/viewer/` ← `$OLD/tools/decisym-viewer/` EXCLUDING `target/` (2 GB build artifacts)
- Create: `$NEW/tools/viewer/fixtures/render-smoke.sysml` ← `$OLD/examples/demos/structural-ibd-complex.sysml` (proven to render: it contains the `C&C Subsystem` context the default view specs target)

**Interfaces:**
- Produces: `tools/viewer/render.sh <file.sysml> <out-dir> [--png]` → PDF(s) in out-dir, exit 0 on ≥1 view rendered (used by Phase 1's demo).

- [ ] **Step 1: Copy without build artifacts**

```bash
rsync -a --exclude 'target/' /home/joescohen/Engineering/projects/sysml-bridge/tools/decisym-viewer/ ~/Engineering/projects/sysml-foundry/tools/viewer/
cp /home/joescohen/Engineering/projects/sysml-bridge/examples/demos/structural-ibd-complex.sysml ~/Engineering/projects/sysml-foundry/tools/viewer/fixtures/render-smoke.sysml
```

- [ ] **Step 2: Build**

Run: `cd ~/Engineering/projects/sysml-foundry/tools/viewer && cargo build --release --bin export_figures`
Expected: exit 0 (~2 min cold). If `cargo` is missing: `rustup toolchain install 1.96.0` per `rust-toolchain.toml`, then retry. Any compile error: STOP and report (the fork built clean in the old repo; an error means the copy is incomplete).

- [ ] **Step 3: Smoke render**

Run: `tools/viewer/render.sh tools/viewer/fixtures/render-smoke.sysml /tmp/foundry-render-smoke --png; echo "exit=$?"; ls -la /tmp/foundry-render-smoke`
Expected: `exit=0`; at least one non-empty `.pdf` (and `.png`) in the out dir. `render.sh` skips view specs whose context isn't in the file — the smoke fixture's `C&C Subsystem` matches the default specs' IBD view. If zero views are produced: STOP and report (do not edit Rust code in this phase).

Note: `render.sh` resolves the repo root via `git rev-parse` — run it from inside `$NEW`.

- [ ] **Step 4: Validate the fixture through the grammar gate too**

Run: `pnpm validate:sysml tools/viewer/fixtures/render-smoke.sysml; echo "exit=$?"`
Expected: 0 errors, `exit=0`.

- [ ] **Step 5: Commit**

```bash
cd ~/Engineering/projects/sysml-foundry
git add tools/viewer && git commit -m "feat: port DeciSym viewer fork with render smoke fixture"
```

Verify `target/` was not committed: `git ls-files tools/viewer | grep -c '^tools/viewer/target/'` → Expected: `0`.

---

### Task 6: CI

**Files:**
- Create: `$NEW/.github/workflows/ci.yml`

**Interfaces:**
- Consumes: everything above. Produces: the green-CI gate all later phases extend.

- [ ] **Step 1: Write the workflow**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  build-and-test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        node-version: [20, 22]
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: ${{ matrix.node-version }}
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      - uses: actions/setup-python@v5
        with:
          python-version: '3.x'
      - name: Set up validator venv
        run: |
          python -m venv .venv
          .venv/bin/pip install -r tools/sysml-validator/requirements.txt
      - run: pnpm build
      - run: pnpm test
      - name: Grammar gate positive controls
        run: |
          pnpm validate:sysml tools/sysml-validator/fixtures/smoke-good.sysml
          if pnpm validate:sysml tools/sysml-validator/fixtures/smoke-bad.sysml; then
            echo "POSITIVE CONTROL FAILED: bad fixture passed the validator" >&2
            exit 1
          fi

  viewer:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.96.0
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: tools/viewer
      - name: Build viewer
        run: cargo build --release --bin export_figures
        working-directory: tools/viewer
      - name: Smoke render
        run: |
          tools/viewer/render.sh tools/viewer/fixtures/render-smoke.sysml /tmp/render-smoke
          test -n "$(find /tmp/render-smoke -name '*.pdf' -size +0c)"
```

- [ ] **Step 2: Create the private GitHub repo and push**

```bash
cd ~/Engineering/projects/sysml-foundry
git add .github && git commit -m "ci: build, test, grammar positive controls, viewer smoke render"
gh repo create sysml-foundry --private --source . --push
```

(Private by design — Joe flips it public at Phase 1. If `gh` is not authenticated, STOP and report; do not configure credentials yourself.)

- [ ] **Step 3: Verify CI green**

Run: `gh run watch --exit-status` (or `gh run list --limit 1` until completed)
Expected: both jobs succeed. If the viewer job fails on toolchain/GL issues: report the log — `export_figures` is headless by design (no display needed); a failure is a real defect, not an environment quirk to suppress.

---

### Task 7: Phase 0 done-criteria verification (fresh clone)

**Files:** none (verification only; results recorded in the final commit message or PR description).

- [ ] **Step 1: Fresh clone and run every spec §8 Phase 0 criterion**

```bash
cd /tmp && rm -rf foundry-verify && git clone ~/Engineering/projects/sysml-foundry foundry-verify && cd foundry-verify
pnpm install && pnpm build && pnpm test            # expect exit 0
pnpm lint                                          # expect exit 0
python3 -m venv .venv && .venv/bin/pip install -r tools/sysml-validator/requirements.txt
pnpm validate:sysml tools/sysml-validator/fixtures/smoke-good.sysml   # expect exit 0
pnpm validate:sysml tools/sysml-validator/fixtures/smoke-bad.sysml    # expect exit NON-zero
grep -rn "expect(true).toBe(true)" packages/ | wc -l                  # expect 0
pnpm --filter @sysml-foundry/model test -- atomic-persist             # expect PASS
pnpm --filter @sysml-foundry/sysml test -- quote-escaping             # expect PASS
cd tools/viewer && cargo build --release --bin export_figures && cd ../..
tools/viewer/render.sh tools/viewer/fixtures/render-smoke.sysml /tmp/fv-render --png
find /tmp/fv-render -name '*.pdf' -size +0c | head                    # expect ≥1 file
```

- [ ] **Step 2: Record**

Every command's actual exit code/output goes into a `PHASE-0-VERIFICATION` section of the final commit message (or the PR body if working on a branch). A criterion that fails means Phase 0 is NOT done — fix and re-run; never record a failure as done.
