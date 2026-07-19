# sysml-foundry Phase 1 — ANGARS Demo + Gallery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Commit the ANGARS corpus publicly, port the Gate-1 audit package and the deterministic pipeline, and ship `pnpm demo` (corpus → model → gates → validate → render) plus the README gallery — the first showable tier.

**Architecture:** `packages/gates` = Gate-1 audit modules ported from the old repo's `mcp-server/src/audit/`. `examples/angars/` holds the committed corpus, the pipeline scripts (extractor + model builder, ported from old `scripts/`), and generated outputs under `examples/angars/out/` (gitignored except the committed baseline `extracted.json` and gallery copies under `docs/gallery/`). `pnpm demo` chains extract → build+Gate 1 → Gate 2 (grammar) → render and fails hard on any gate.

**Tech Stack:** as Phase 0, plus the vendored SheetJS `xlsx` tarball (corpus parsing) and poppler-utils (`pdftoppm`) for PNG rasterization.

**Design spec:** `docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md` (§2, §4 data flow, §8 Phase 1). **Prerequisite: Phase 0 complete and CI green.**

## Global Constraints

- `$OLD` = `/home/joescohen/Engineering/projects/sysml-bridge` (read-only). `$NEW` = `~/Engineering/projects/sysml-foundry`.
- ANGARS corpus is public by user decision 2026-07-02 — commit it; do NOT add ignore rules for it.
- `pnpm demo` must run with NO `ANTHROPIC_API_KEY` — the Tier-1 path is fully deterministic.
- A missing validator venv is a HARD failure in the demo (exit non-zero, clear setup message) — the old repo's advisory-skip behavior does not port (spec §8 note; design decision: validate-before-claim).
- Gate order is law (CLAUDE.md): build → Gate 1 (audit) → serialize → Gate 2 (grammar validator) → render. A failing gate stops the pipeline with non-zero exit.
- The fidelity baseline is a committed constant read by CI — never hardcoded in the workflow.

---

### Task 1: Vendor xlsx + commit the ANGARS corpus and IR baselines

**Files:**
- Create: `$NEW/vendor/xlsx-0.20.3.tgz` + `.sha512` ← `$OLD/vendor/`
- Modify: `$NEW/package.json` (add devDependency)
- Create: `$NEW/examples/angars/corpus/requirements/*.xlsx` (9 workbooks) and `corpus/specs/*.pdf` (4 PDFs) ← `$OLD/examples/angars/corpus/`
- Create: `$NEW/examples/angars/extracted.json` ← `$OLD/examples/angars/model/extracted.json`; `$NEW/examples/angars/cc-extracted.json` ← `$OLD/examples/angars/model/cc-extracted.json`

**Interfaces:**
- Produces: corpus files at `examples/angars/corpus/{requirements,specs}/`; committed IR baselines the extractor's output is compared against.

- [ ] **Step 1: Copy everything**

```bash
cd ~/Engineering/projects/sysml-foundry
mkdir -p vendor examples/angars/corpus
cp /home/joescohen/Engineering/projects/sysml-bridge/vendor/xlsx-0.20.3.tgz vendor/
cp /home/joescohen/Engineering/projects/sysml-bridge/vendor/xlsx-0.20.3.tgz.sha512 vendor/
cp -r /home/joescohen/Engineering/projects/sysml-bridge/examples/angars/corpus/requirements examples/angars/corpus/requirements
cp -r /home/joescohen/Engineering/projects/sysml-bridge/examples/angars/corpus/specs examples/angars/corpus/specs
cp /home/joescohen/Engineering/projects/sysml-bridge/examples/angars/model/extracted.json examples/angars/extracted.json
cp /home/joescohen/Engineering/projects/sysml-bridge/examples/angars/model/cc-extracted.json examples/angars/cc-extracted.json
```

- [ ] **Step 2: Wire the dependency and verify integrity**

Add to root `package.json` devDependencies: `"xlsx": "file:vendor/xlsx-0.20.3.tgz"` (the tarball is vendored deliberately — SheetJS does not publish 0.20.x to npm; never "fix" this by pointing at the registry).

Run: `cd vendor && sha512sum -c xlsx-0.20.3.tgz.sha512 && cd .. && pnpm install`
Expected: `OK` + install exit 0. (If the sha file format differs, verify with `openssl dgst -sha512` against the recorded digest instead.)

- [ ] **Step 3: Verify commit contents and commit**

Run: `git add vendor examples/angars package.json pnpm-lock.yaml && git status --short | wc -l` — expect ≥ 16 files (9 xlsx + 4 pdf + 2 json + vendor + manifests).

```bash
git commit -m "feat: commit ANGARS corpus, IR baselines, vendored xlsx"
```

---

### Task 2: Port `packages/gates` (Gate-1 audit) with fixtures always-on

**Files:**
- Create: `$NEW/packages/gates/{package.json,tsconfig.json,vitest.config.ts}` (pattern from Phase 0 Task 3; name `@sysml-foundry/gates`; deps: `@sysml-foundry/model: workspace:*`, zod)
- Create (ported): `src/` ← `$OLD/packages/mcp-server/src/audit/` (all ten files: `corpus.ts fidelity.ts findings.ts fuzzy.ts index.ts matrix.ts provenance.ts relational.ts report.ts structural.ts`)
- Create (ported tests): `src/__tests__/` ← `$OLD/packages/mcp-server/src/__tests__/{audit-corpus,audit-fidelity,audit-matrix,audit-provenance,audit-relational,fuzzy-calibration,validate-model-findings,validate-coverage}.test.ts`

**Interfaces:**
- Consumes: `SysmlElement`/`SysmlRelationship` from `@sysml-foundry/model`.
- Produces: the audit entry point as exported by the ported `index.ts` (keep exported names unchanged — the Phase-1 pipeline and Phase-2 `validate_model` tool both call it). Findings shape: the ported `findings.ts` types.

- [ ] **Step 1: Copy, rename, fix imports** (same mechanics as Phase 0 Task 3/4: type imports → `@sysml-foundry/model`; test relative paths).

- [ ] **Step 2: Make the corpus-dependent tests unconditional**

`audit-corpus.test.ts` in the old repo gates on file existence: `const describeRealCorpus = existsSync(EXTRACTED_PATH) ? describe : describe.skip;` (old line ~261). The IR baseline is now committed, so: point `EXTRACTED_PATH` at `examples/angars/extracted.json` (repo-root-relative) and replace the conditional with plain `describe`. Do the same for any other `skipIf`/conditional-skip in the ported gate tests EXCEPT ones gated on `ANTHROPIC_API_KEY` or `INTEGRATION` env vars (those stay env-gated — they are additional live runs, not the CI path).

- [ ] **Step 3: Run**

Run: `pnpm install && pnpm --filter @sysml-foundry/gates test 2>&1 | tail -5`
Expected: PASS with **0 skipped** among the corpus/fidelity/matrix/provenance/relational suites. If a ported test fails because it expected old-repo paths, fix the path constant, not the assertion.

- [ ] **Step 4: Commit**

```bash
git add packages/gates && git commit -m "feat: port Gate-1 audit package; corpus-dependent tests now unconditional in CI"
```

---

### Task 3: Port the extractor as `demo:extract`

**Files:**
- Create: `$NEW/examples/angars/pipeline/extract.ts` ← `$OLD/scripts/extract-angars.ts`; `$NEW/examples/angars/pipeline/extract-cc.ts` ← `$OLD/scripts/extract-angars-cc.ts`
- Modify: root `package.json` scripts (add `"demo:extract": "tsx examples/angars/pipeline/extract.ts"`); root devDeps add `"tsx": "^4.0.0"` and `"@sysml-foundry/model": "workspace:*"` if not present

**Interfaces:**
- Consumes: `ExtractedSchema`, `stableId`, parse helpers, `WORKBOOKS`/`N2_SHEETS` config from `@sysml-foundry/model` (they ported inside old `packages/ir`).
- Produces: `examples/angars/out/extracted.json` (generated; `out/` is gitignored) matching the committed `examples/angars/extracted.json`.

- [ ] **Step 1: Copy and re-path**

Copy both scripts. Update: imports `@sysml-bridge/ir` (or relative `../packages/ir/...`) → `@sysml-foundry/model`; corpus input dir → `examples/angars/corpus/requirements`; output path → `examples/angars/out/extracted.json` (create dir). The scripts assert pinned counts internally — leave every assertion intact.

- [ ] **Step 2: Run and diff against the committed baseline**

Run: `pnpm demo:extract && diff <(jq -S . examples/angars/out/extracted.json) <(jq -S . examples/angars/extracted.json) && echo IDENTICAL`
Expected: exit 0, `IDENTICAL`. The extractor is deterministic; any diff means a port error (paths, workbook config) — STOP and fix; never update the committed baseline to match a divergent output in this task.

- [ ] **Step 3: Commit**

```bash
git add examples/angars/pipeline package.json pnpm-lock.yaml && git commit -m "feat: port deterministic ANGARS extractor as demo:extract"
```

---

### Task 4: Port the model builder as `demo:build`

**Files:**
- Create: `$NEW/examples/angars/pipeline/build-model.ts` ← `$OLD/scripts/generate-cc-model.ts`; `$NEW/examples/angars/pipeline/cc-presentation.ts` ← `$OLD/packages/mcp-server/src/utils/cc-presentation.ts` (demo-specific projection lives WITH the demo, per spec §3)
- Modify: root `package.json` scripts: `"demo:build": "tsx examples/angars/pipeline/build-model.ts"`

**Interfaces:**
- Consumes: `FileStore` + types from `@sysml-foundry/model`; `serializeToSysml`, `compareTrace`/`TracePair` from `@sysml-foundry/sysml`; audit entry point from `@sysml-foundry/gates`; `cc-extracted.json`.
- Produces: `examples/angars/out/angars.sysml`, `examples/angars/out/audit.json` (Gate-1 findings + fidelity JSON), store dir `examples/angars/out/.store/`.

- [ ] **Step 1: Copy and re-path**

Update imports to the workspace packages; `EXTRACTED_JSON` → `examples/angars/cc-extracted.json`; `OUTPUT_SYSML` → `examples/angars/out/angars.sysml`; model dir default → `examples/angars/out/.store`; `VALIDATOR_SH` → `tools/sysml-validator/run.sh`; `cc-presentation` import → `./cc-presentation.js`.

- [ ] **Step 2: Change the venv soft-fail to hard-fail**

The old script downgrades a missing validator venv (validator exit 2) to a loud warning with `process.exitCode = 3` (old repo `generate-cc-model.ts:687-705`). Replace that branch: print the same setup instructions, then `process.exit(2)`. Rationale line for the code comment: `// Gate 2 cannot be skipped: no venv means no validation, means no claim.`

- [ ] **Step 3: Emit audit.json**

The ported script prints its gate report to stdout. Add a structured emission right after the Gate-1 audit runs (exact insertion point: where the script has the audit/trace-compare results in hand — read the ported code; it already aggregates findings for its console report):

```ts
fs.writeFileSync(
  path.join(REPO_ROOT, "examples/angars/out/audit.json"),
  JSON.stringify(
    {
      findings,                                   // the Gate-1 findings array the report prints
      fidelity: { tracePairs: tracePairsTotal, matched: tracePairsMatched },
      generatedBy: "demo:build",
    },
    null,
    2
  )
);
```

Bind `findings` / `tracePairsTotal` / `tracePairsMatched` to the variables the ported report already computes (they exist under names like the compareTrace result — match the real names when editing; the contract is the JSON shape above).

- [ ] **Step 4: Run the full build**

Run: `pnpm demo:build; echo "exit=$?"`
Expected: `exit=0`; console shows the gate report ending in the grammar gate PASS; `out/angars.sysml` exists; `out/audit.json` exists with `findings: []` (or only severity levels the old gate treated as pass — match old behavior) and `fidelity.matched === fidelity.tracePairs`.

Run the grammar gate independently: `pnpm validate:sysml examples/angars/out/angars.sysml; echo "exit=$?"` → Expected `exit=0`, 0 errors.

- [ ] **Step 5: Commit the pipeline + the fidelity baseline constant**

Create `examples/angars/fidelity-baseline.json`:
```json
{ "tracePairs": 28, "matched": 28 }
```
(28/28 is the proven C&C baseline. If Step 4's audit.json reports a different-but-100% pair count — e.g. the port composes more pairs — set the baseline to THAT observed value, and note it in the commit message. `matched` must equal `tracePairs`; a <100% fidelity is a failure to investigate, not a baseline to enshrine.)

```bash
git add examples/angars/pipeline examples/angars/fidelity-baseline.json package.json
git commit -m "feat: port ANGARS model builder as demo:build (hard-fail Gate 2, audit.json emission)"
```

---

### Task 5: The `pnpm demo` orchestrator + output assertions

**Files:**
- Create: `$NEW/examples/angars/pipeline/demo.ts`
- Create: `$NEW/examples/angars/pipeline/assert-demo.ts`
- Modify: root `package.json` scripts: `"demo": "tsx examples/angars/pipeline/demo.ts"`, `"demo:assert": "tsx examples/angars/pipeline/assert-demo.ts"`

**Interfaces:**
- Consumes: `demo:extract`, `demo:build`, `tools/viewer/render.sh`.
- Produces: `examples/angars/out/renders/*.{pdf,png}`; a one-command Tier-1 demo; `demo:assert` = the machine-checkable done-criteria for CI.

- [ ] **Step 1: Write demo.ts**

```ts
/**
 * demo.ts — Tier-1 demo: corpus → IR → model → Gate 1 → Gate 2 → renders.
 * Deterministic; requires no API key. Any gate failure exits non-zero.
 */
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "../../..");
const OUT = path.join(ROOT, "examples/angars/out");

function run(title: string, cmd: string, args: string[]) {
  console.log(`\n=== ${title} ===`);
  execFileSync(cmd, args, { cwd: ROOT, stdio: "inherit" });
}

run("1/4 extract corpus → IR", "pnpm", ["demo:extract"]);
run("2/4 build model + Gate 1 + serialize + Gate 2", "pnpm", ["demo:build"]);
fs.mkdirSync(path.join(OUT, "renders"), { recursive: true });
run("3/4 render views", path.join(ROOT, "tools/viewer/render.sh"), [
  path.join(OUT, "angars.sysml"),
  path.join(OUT, "renders"),
  "--png",
]);
run("4/4 assert outputs", "pnpm", ["demo:assert"]);
console.log("\nDemo complete. Outputs in examples/angars/out/");
```

- [ ] **Step 2: Write assert-demo.ts (the executable done-criteria)**

```ts
/** assert-demo.ts — machine checks for spec §8 Phase 1. Exit non-zero on any failure. */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const out = (...p: string[]) => path.join(ROOT, "examples/angars/out", ...p);
const fail = (msg: string): never => { console.error(`DEMO ASSERT FAIL: ${msg}`); process.exit(1); };

if (!fs.existsSync(out("angars.sysml"))) fail("angars.sysml missing");

const audit = JSON.parse(fs.readFileSync(out("audit.json"), "utf8"));
if (audit.findings.length !== 0) fail(`Gate 1 findings present: ${audit.findings.length}`);

const baseline = JSON.parse(
  fs.readFileSync(path.join(ROOT, "examples/angars/fidelity-baseline.json"), "utf8")
);
if (audit.fidelity.matched < baseline.matched || audit.fidelity.tracePairs < baseline.tracePairs)
  fail(`fidelity ${audit.fidelity.matched}/${audit.fidelity.tracePairs} below baseline ${baseline.matched}/${baseline.tracePairs}`);

const renders = fs.existsSync(out("renders"))
  ? fs.readdirSync(out("renders")).filter((f) => f.endsWith(".pdf"))
  : [];
if (renders.length < 3) fail(`expected ≥3 rendered PDFs, got ${renders.length}`);
for (const f of renders)
  if (fs.statSync(out("renders", f)).size === 0) fail(`zero-byte render: ${f}`);

console.log(`demo:assert PASS — findings 0, fidelity ${audit.fidelity.matched}/${audit.fidelity.tracePairs}, renders ${renders.length}`);
```

- [ ] **Step 3: Run end-to-end without an API key**

Run: `env -u ANTHROPIC_API_KEY pnpm demo; echo "exit=$?"`
Expected: `exit=0`, ends with `demo:assert PASS`. If the renderer produces < 3 PDFs on the full ANGARS model: check which default view specs matched (the renderer skips non-matching contexts); if the model genuinely exposes < 3 renderable views with the ported view specs, STOP and report — the fix is a view-spec adjustment decision, not a lowered assertion.

- [ ] **Step 4: Commit**

```bash
git add examples/angars/pipeline package.json && git commit -m "feat: pnpm demo — one-command Tier-1 pipeline with executable done-criteria"
```

---

### Task 6: README gallery v1

**Files:**
- Create: `$NEW/docs/gallery/` (committed PNG copies), `$NEW/examples/angars/pipeline/update-gallery.ts`
- Modify: `$NEW/README.md` (full rewrite of the stub), root `package.json` scripts: `"demo:gallery": "tsx examples/angars/pipeline/update-gallery.ts"`

**Interfaces:**
- Consumes: `examples/angars/out/renders/*.png` from `pnpm demo`.
- Produces: committed `docs/gallery/*.png`; README that references only committed images.

- [ ] **Step 1: Write update-gallery.ts**

```ts
/** Copies the current demo renders into docs/gallery/ (the committed copies). */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const SRC = path.join(ROOT, "examples/angars/out/renders");
const DST = path.join(ROOT, "docs/gallery");
fs.mkdirSync(DST, { recursive: true });
const pngs = fs.readdirSync(SRC).filter((f) => f.endsWith(".png"));
if (pngs.length === 0) { console.error("no PNGs — run pnpm demo first"); process.exit(1); }
for (const f of pngs) fs.copyFileSync(path.join(SRC, f), path.join(DST, f));
console.log(`gallery updated: ${pngs.length} images`);
```

- [ ] **Step 2: Write the README**

Structure (keep this order; write real prose, not lorem):
1. **Hero:** name, tagline, badges (CI), one IBD gallery image immediately visible.
2. **The trust story** (3 short paragraphs): models are corpus-grounded (`provenanceSourceId` on every element), gate-validated (Gate 1 audit → Gate 2 grammar → Cameo as manual semantic epilogue), human-approved (LLM layers propose, never write). Source the wording from the spec §1 and old README's validation-gate section.
3. **Gallery:** the ≥3 PNGs from `docs/gallery/` with view-name captions.
4. **Fidelity:** the numbers from `fidelity-baseline.json`, stated as "N/N trace pairs (100%) verified against the reference model, re-checked in CI on every push".
5. **Quick start:** clone → `pnpm install` → venv two-liner → `pnpm demo` (mirror the exact commands from the CI workflow so they never drift).
6. **Tiers table** from spec §2 (Tier 2/3 rows marked "coming — Phase 2/3").
7. **Layout + license.**

- [ ] **Step 3: Generate gallery and verify README image integrity**

```bash
pnpm demo && pnpm demo:gallery
grep -oE '\]\(([^)]+\.(png|pdf|svg))\)' README.md | sed 's/](//;s/)//' | while read -r img; do
  test -f "$img" || { echo "MISSING: $img"; exit 1; }
done && echo "README images OK"
```
Expected: `README images OK`.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/gallery examples/angars/pipeline/update-gallery.ts package.json
git commit -m "docs: README gallery v1 — trust story, rendered views, fidelity numbers, quick start"
```

---

### Task 7: CI demo job + red-CI proof

**Files:**
- Modify: `$NEW/.github/workflows/ci.yml` (add job)

**Interfaces:**
- Consumes: everything above. Produces: the self-running feedback loop (spec §7 layer 2 — the demo IS the integration test).

- [ ] **Step 1: Add the demo job**

Append to `ci.yml`:

```yaml
  demo:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
      - uses: actions/setup-python@v5
        with:
          python-version: '3.x'
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.96.0
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: tools/viewer
      - name: Install
        run: |
          pnpm install --frozen-lockfile
          python -m venv .venv
          .venv/bin/pip install -r tools/sysml-validator/requirements.txt
          sudo apt-get update && sudo apt-get install -y poppler-utils
      - name: Build viewer
        run: cargo build --release --bin export_figures
        working-directory: tools/viewer
      - name: Run the Tier-1 demo (no API key)
        run: pnpm demo
        env:
          ANTHROPIC_API_KEY: ""
      - name: README image integrity
        run: |
          grep -oE '\]\(([^)]+\.(png|pdf|svg))\)' README.md | sed 's/](//;s/)//' | while read -r img; do
            test -f "$img" || { echo "MISSING: $img"; exit 1; }
          done
      - uses: actions/upload-artifact@v4
        with:
          name: angars-renders
          path: examples/angars/out/renders/
```

- [ ] **Step 2: Push and verify green**

Run: `git add .github && git commit -m "ci: run the full Tier-1 demo on every push" && git push && gh run watch --exit-status`
Expected: all three jobs green.

- [ ] **Step 3: Prove the loop can fail (one-time red-CI check)**

```bash
git checkout -b ci-red-proof
# Introduce a deliberate serializer defect: in packages/sysml/src/sysml-serializer.ts,
# find the line emitting the package header (grep -n "package " src, the emit site)
# and break the emitted keyword, e.g. change the emitted string "package " to "packge ".
git commit -am "test: deliberate serializer break (red-CI proof — DO NOT MERGE)"
git push -u origin ci-red-proof && gh run watch --exit-status; echo "exit=$?"
```
Expected: NON-zero — the demo job fails at Gate 2. Record the failing run URL, then:

```bash
git checkout main && git push origin --delete ci-red-proof && git branch -D ci-red-proof
```

- [ ] **Step 4: Phase 1 done-criteria sweep (fresh clone)**

From `/tmp`: clone, install, venv, `env -u ANTHROPIC_API_KEY pnpm demo` → exit 0; `pnpm validate:sysml examples/angars/out/angars.sysml` → 0 errors; `jq '.findings | length' examples/angars/out/audit.json` → `0`; `ls examples/angars/out/renders/*.pdf | wc -l` → ≥ 3; README image check → OK. Record all outputs plus the red-CI run URL in the final commit/PR body. Phase 1 is showable when every line passes — at that point Joe decides on flipping the repo public and archiving `sysml-bridge`.
