# sysml-foundry Phase 4 — Renderer Parity Loop Implementation Plan

> **For agentic workers:** executed task-by-task by dispatched executor subagents; each task ends with its own verification. Steps use checkbox (`- [ ]`) syntax.

**Goal:** notation probes + a Cameo reference gallery + the parity matrix (the receipts), the
state-transition / requirements / traceability views added to the viewer, and a tuning pass
that scores every view against its reference — the diagrams reach near-Cameo fidelity with
evidence.

**Architecture:** `probes/*.sysml` (one per notation concern, all validator-clean) pair with
reference images in `docs/reference/cameo-notation/` (provenance: the old repo's REAL Cameo CE
captures from the 2026-06 live sessions at
`$OLD/examples/angars/diagrams/*.{png,pdf}`, rasterized where needed; where no capture covers
a probe, the OMG spec notation table is the named reference and the matrix says so). The
viewer (`tools/viewer`, Rust) gains `StateTransition`, `Requirements`, and `Traceability`
view kinds, reusing the existing layout/orthogonal-routing machinery. `docs/reference/parity-matrix.md`
scores view × rubric-feature with ✅/⚠️/❌ and is updated in the same commit as any viewer
change.

**Prerequisite:** Phase 1 (viewer + demo). Independent of Phases 2-3 code, but runs after
them here. Spec: design spec §5 + §8 Phase 4.

## Global Constraints

- `$OLD` = `/home/joescohen/Engineering/projects/sysml-bridge` (read-only). `$NEW` = this repo.
- Rust work stays inside `tools/viewer/`; every probe and the ANGARS model must render
  headless (`cargo build --release --bin export_figures`; render via `tools/viewer/render.sh`).
- Every probe passes `pnpm validate:sysml` with exit 0 (R2 applies to fixtures too).
- Emitted-notation questions are answered from `docs/sysml-v2-reference/` (R1), never memory.
- The viewer's existing views (General, Interconnection, ActionFlow) must not regress: the
  Phase-1 demo renders and `tools/viewer/fixtures/render-smoke.sysml` stay working — CI's demo
  and viewer jobs are the regression net.
- `pnpm demo` + `pnpm demo:seeded` + all package tests stay green throughout.

---

### Task 1: probes + reference gallery + rubric + parity matrix v1

**Files:**
- `probes/*.sysml` — copy the proven demo fixtures from `$OLD/examples/demos/`:
  `structural-pillar.sysml`, `structural-ibd-complex.sysml`, `behavioral-pillar.sysml`,
  `state-machine-control.sysml`, `activity-control-flow.sysml`, `requirements-pillar.sysml`,
  `requirements-trace.sysml`, `traceability-demo.sysml`, `bdd-structure.sysml` — each
  validated (`pnpm validate:sysml` exit 0; fix nothing — they validated in the old repo; if
  one fails here, STOP and report).
- `docs/reference/cameo-notation/` — copy from `$OLD/examples/angars/diagrams/`: all `.png`
  plus `pdftoppm -png -r 150` rasterizations of the `.pdf` captures (`angars-bdd`,
  `angars-ibd-subsystem`, `angars-activity-operations`, `state-machine`,
  `requirements-focused`, `angars-traceability`, `traceability-focused`, ...). These are real
  Cameo CE outputs (2026-06 sessions) — name them `<concern>-cameo.png`. Write
  `docs/reference/cameo-notation/README.md` stating exactly this provenance per file.
- `docs/reference/rubric.md` — the per-view rubric feature list (from the OMG spec notation
  tables + the Cameo captures): per view kind, 6-10 checkable features (e.g. BDD: sharp-corner
  def blocks vs rounded usage blocks, `<<part def>>` stereotype text, name compartment rule,
  parts-compartment separators; IBD: ports straddling boundaries, orthogonal routing, frame +
  heading; Activity: initial/final nodes, action stereotypes, guarded successions,
  decision/merge diamonds, fork/join bars; State: rounded state boxes, transition arrows w/
  triggers, initial pseudo-state; Requirements: `<<requirement>>` boxes with id+text
  compartments, derive edges w/ open arrowheads + «deriveReqt»-style labels, containment;
  Traceability: node kinds distinguishable, edge kinds labeled (satisfy/verify/allocate/derive),
  readable layered layout).
- `scripts/check-parity-pairs.ts` — walks `probes/` and asserts each probe has (a) a render
  produced by a `render.sh` invocation recorded in `docs/reference/parity-matrix.md`'s render
  column, and (b) a named reference (a `cameo-notation/*.png` file OR an explicit
  `OMG-spec-table §x.y` citation) — no probe unpaired; exits non-zero listing gaps. Root
  script `check:parity`.
- `docs/reference/parity-matrix.md` v1 — rows = (view kind × rubric feature); columns:
  status (✅/⚠️/❌), reference, render file, note. Fill by RENDERING the probes with the
  CURRENT viewer (`render.sh <probe> /tmp/parity-<n> --png` + a views spec where the default
  specs don't match probe contexts — write `probes/views/<probe>.json` as needed) and
  visually comparing against the references (the executor reads both images). Unsupported
  views (state/requirements/traceability) are ❌ rows with note "view lands in Task 2/3/4".

**Verify:** all probes validate; `pnpm check:parity` exit 0; matrix has zero empty cells;
General/IBD/ActionFlow rows scored from actual side-by-side comparisons (say which images).
Commit: `feat: notation probes, Cameo reference gallery, rubric, parity matrix v1`

### REVISION (post-Task-1 finding, 2026-07-07)

Task 1 discovered the vendored fork ALREADY renders State, Requirements, and Traceability
views at near-Cameo fidelity (matrix v1: 49 pass / 3 warn / 0 fail — score from live renders,
commit 15e26c3). The original Tasks 2-4 (from-scratch view builds) are RESCOPED to what
reality still needs:

### Task 2 (revised): ANGARS-scale requirements + traceability views

The lean ANGARS model lacks the container contexts the requirements/traceability view specs
target. Corpus-grounded fix in the pipeline (NOT the viewer): extend
`examples/angars/pipeline/build-model.ts`/`cc-presentation.ts` to emit organizational
containers — a `'C&C Requirements'` container owning the requirement usages and a
`'C&C Trace'` context for the cross-pillar web (provenance `"C&C"`, same convention as the
existing `'C&C Subsystem'`/`'C&C Operations'` containers from Phase 1). Extend
`examples/angars/views.json` with a requirements view and a traceability view of those
contexts. Update `pnpm demo`'s expected-render assertions (assert-demo.ts ≥ 5 renders) and
the gallery (`pnpm demo:gallery`) + README gallery section with the two new ANGARS views.

**Verify:** `pnpm demo` exit 0 with 5 renders; ANGARS requirements view shows ≥ 34
requirement nodes and traceability view ≥ 100 nodes+edges (from the exporter's printed
counts, or by asserting on the parsed model as a proxy the executor documents); audit stays
findings 0 / 28-28; gallery + README image check green; `pnpm -r test` green.
Commit: `feat: ANGARS-scale requirements and traceability views in the demo`

### Task 3 (revised): tuning pass on the three warn rows + state-endpoint regression guard

- Fix `T5` (allocate edge label clipped) and `A10` (object-flow item labels not emitted) in
  `tools/viewer/src` — both are label emission/placement fixes, not layout rewrites. Attempt
  `I8` (IBD tall-column layout vs Cameo compact); if it needs a real layout algorithm change,
  document the honest deferral in the matrix note instead of a cosmetic hack.
- Add a cheap state-endpoint regression guard: a script (or Rust test if a harness exists)
  asserting the state probe render reports both endpoints for every transition (the
  exporter's log/counts), wired into `check:parity` or the viewer CI job.
- Re-render affected probes, RE-SCORE the matrix in the same commit as the Rust changes.

**Verify:** matrix warn count strictly decreases (3 → ≤1, with any remaining warn honestly
annotated); regression net green (demo renders, render-smoke, probes, `pnpm -r test`).
Commit: `fix(viewer): tuning pass — edge-label placement + flow item labels; parity matrix re-scored`

### Task 4 (revised): push + CI + verification record

Push; CI green (conclusion via `gh run view --json`); empty commit
`chore: Phase 4 done-criteria verification record` (criteria: probes validate; pairing check
+ positive control; matrix complete + updated-with-viewer-commits; state-endpoint guard;
ANGARS requirements ≥ 34 nodes; traceability ≥ 100 nodes+edges; README gallery grown; the
plan-revision note — views pre-existed, Tasks 2-4 rescoped; CI URL + conclusion); push.
