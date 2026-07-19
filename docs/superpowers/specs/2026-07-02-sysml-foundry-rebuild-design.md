# sysml-foundry — Rebuild Design Spec

**Date:** 2026-07-02
**Status:** Approved in brainstorming session (all three sections user-approved)
**Supersedes:** the presentation/architecture surface of `sysml-bridge` (this repo). The old
repo is archived with a pointer README once Phase 1 of the new repo is showable.

---

## 1. Identity and goals

**What it is:** a new repository, `sysml-foundry` (name provisional — alternatives
`mbse-foundry`, `sysml-studio`; final call at scaffold time), that rebuilds sysml-bridge as
**primarily a portfolio demonstration, secondarily an open-source product**.

**Tagline:** *"Corpus-grounded SysML v2 authoring with Claude — every element cited, every
gate enforced."*

**Audiences, in priority order:**
1. Defense/MBSE employers (know Cameo/DOORS; judge SE discipline)
2. GitHub general public (clone-and-run in 5 minutes)
3. Live interview walkthroughs driven by Joe

**Explicitly NOT goals:** enterprise scale past Cameo CE's 500-element cap, SMAPS/live-Cameo
REST integration, fixing the JSON-store-vs-text architecture (parked as a documented future
spike). The architecture was audited 2026-07-02 and is sound; the rebuild targets
presentability and demo impact.

**Compass alignment (from the vault's `Compass.md`):** sysml-bridge scores strongest-in-portfolio
on Q4 (feedback loop: the R1–R4 gate discipline) but is capped at Q5 (compounding — "solo-repo
discipline that doesn't generalize"). This rebuild is engineered to move Q5 from ⚠️ to ✅ at
near-zero operating cost (the CEI lesson: a loop that costs more than it returns is overhead):
- CI runs the full demo pipeline on every push — the feedback loop runs without Joe.
- A committed seeded-defect fixture proves the gates can FAIL (paired positive controls).
- `docs/gate-pattern.md` generalizes the R1–R4 judgment beyond this repo.

## 2. The four demo tiers

The repo is organized around four reviewer experiences, in increasing effort:

| Tier | Experience | Requires | Phase |
|---|---|---|---|
| 0 — browse | README-as-gallery: rendered BDD/IBD/activity/state/traceability diagrams, fidelity numbers, Cameo screenshots, the trust story | nothing | 1 |
| 1 — run | `pnpm demo`: corpus → extract → build → Gate 1 audit → grammar validate → render, on the committed ANGARS corpus. Deterministic, no API key | Node+pnpm+Python | 1 |
| 2 — gates | Human-gated LLM layers: prose candidates with citations, inference propose/debate, approve/reject; seeded-defect run shows fabrication being CAUGHT | ANTHROPIC_API_KEY | 3 |
| 3 — drive | `/mbse` orchestrator live in Claude Code — the interview prop | Claude Code | 2 |

**ANGARS ships publicly.** User decision 2026-07-02: the gitignore on the corpus was
over-caution; the corpus, extracted IR, model, renders, and audits are all committed. ANGARS
itself is the public demo.

## 3. Repo layout

```
sysml-foundry/
├── packages/
│   ├── model/        # element/relationship types, ModelStore interface, FileStore,
│   │                 #   IR schema (zod) + composeIR   [merges old ir + mcp-server store halves]
│   ├── sysml/        # model ↔ textual notation: serializer, parser, validator-runner glue
│   ├── gates/        # Gate 1 audits (provenance/relational/fidelity) + approval-record machinery
│   ├── candidates/   # prose-ingest + inference, unified under one candidate/approval contract
│   ├── mcp-server/   # THIN: tool registration wiring the packages above
│   └── skills/       # /mbse orchestrator + verb skills (markdown)
├── tools/
│   ├── sysml-validator/   # ported as-is (committed ANTLR Python parser — proven)
│   └── viewer/            # DeciSym fork, ported from tools/decisym-viewer
├── examples/angars/       # committed corpus, extracted.json, .sysml, renders, audit reports
├── probes/                # small .sysml probe models, one per notation feature (Phase 4)
├── docs/
│   ├── sysml-v2-reference/    # vendored grammar (.g4, MIT) + cheatsheet — ported as-is
│   ├── reference/cameo-notation/  # harvested Cameo CE reference screenshots (Phase 4)
│   ├── gate-pattern.md        # the generalized R1–R4 write-up (Phase 3)
│   └── architecture.md        # ONE honest architecture doc, kept current
└── .github/workflows/ci.yml   # build + test + demo pipeline + seeded-defect assertions
```

**Ports (proven code, carried over):** serializer, grammar validator (committed ANTLR parser +
run.sh), IR schema + composeIR, FileStore, Gate-1 audit modules (provenance/relational/
fidelity/matrix/corpus), prose-ingest, inference engine, DeciSym viewer fork, vendored grammar
+ cheatsheet, the R1–R4 emission discipline (new repo's CLAUDE.md).

**Dies (not ported):** the 15 skills as-is; all ~25 one-off `scripts/*.ts` (replaced by named
demo commands); `smaps-client.ts` (the `ModelStore` interface remains the documented
portability seam — the claim survives, the untested code doesn't); `.planning/` history;
`docs/design.md` (stale pre-pivot architecture); `.env.example` ghost variables
(SYSON_ENDPOINT, VITE_SYSON_URL); `cc-presentation.ts` demo-specific projection moves next to
the demo that uses it (`examples/angars/`) rather than living in a generic package.

**New construction:** the `/mbse` orchestrator + verbs, the demo-tier commands, the gallery
README, the seeded-defect fixture, the parity tuning loop (probes + reference gallery +
rubric + matrix), later the review UI.

## 4. Surface design

### MCP tools (deterministic logic lives here — testable)

Same 11-tool surface as sysml-bridge minus SMAPS backend, with `validate_model` exposing the
full Gate-1 audit result (findings + fidelity numbers) as structured JSON: `init_project`,
`create_element`, `query_elements`, `update_element`, `delete_element`, `create_relationship`,
`query_relationships`, `validate_model`, `export_sysml`, `import_sysml`, `get_project_state`.

Rules carried into tool implementations: R3 (verify only inside verification-def objective
bodies), R4 (trace operands are usages, never defs — enforced by Gate 1 relational audit,
because the grammar validator cannot catch it).

### Skills (thin intent-translators)

- **`/mbse`** — the stateful orchestrator. Reads `.mbse/session.json` (lifecycle state
  machine: `init → ingest → build → trace → validate → render`), reports where the model is,
  routes to the next step. All heavy lifting through MCP tools.
- **Verbs:** `/mbse-query` (NL questions against the model), `/mbse-edit` (NL mutations via
  store tools only — NEVER via import_sysml round-trip, per TF-3), `/mbse-approve` (the human
  gate over candidates), `/mbse-render` (drive the viewer).

That is the entire skill table a reviewer reads. Skill→tool drift is checked by a script that
extracts backtick-quoted tool names from skill markdown and asserts each is a registered tool
name (extends old `check-skill-paths.ts`).

### Data flow

```
corpus (xlsx/PDF, committed)
  → extract (deterministic)          → extracted.json
  → candidates (LLM, optional)       → candidate records with citations
  → HUMAN APPROVAL (/mbse-approve)   → approved/rejected disposition records
  → composeIR                        → composed IR
  → build via MCP tools              → file-native model store
  → Gate 1 (validate_model)          → findings + fidelity JSON  [STOP on findings]
  → serialize (export_sysml)         → .sysml
  → Gate 2 (grammar validator)       → exit 0 required           [STOP on errors]
  → render (viewer)                  → PDF/PNG gallery
  → (manual epilogue) Cameo CE import → screenshots as evidence
```

## 5. Rendering

**DeciSym fork is the renderer of record.** Rationale (verified landscape, 2026-06-09
sessions): only free tool that reads `.sysml` directly and exports headless PDF/PNG; already
given a Cameo-style pass (initial/final nodes, decision/merge/fork/join, guards, orthogonal
routing, open arrowheads, named frames); user prefers its look. syside failed hands-on (would
not render elements — do not revisit). sysml-reactflow (MIT, near-Cameo polish, all pillars)
is parked as the interactive option for the Phase-5 review UI only; it takes model objects,
not `.sysml`, and has no headless export.

**View roadmap (priority order):** state-transition view (small: capture `transition first X
then Y` endpoints in the parser, mirror `parse_succession`; reuse the activity layout engine)
→ requirements view → traceability view (the money shot for SE reviewers).

**Legal note:** the tuning target is the OMG SysML v2 specification's normative notation
(which Cameo also implements), not Cameo's code or assets. "Cameo-fidelity" = spec-fidelity
with professional polish.

### The renderer parity tuning loop (Phase 4)

1. **Probe models:** `probes/*.sysml`, one per notation concern (def-vs-usage corners, ports,
   connectors, activity control nodes, state transitions, requirement boxes,
   satisfy/verify/allocate arrows). Each validates clean (Gate 2 exit 0).
2. **Reference harvest (once):** import each probe into Cameo CE on the Mac, create the
   matching view, screenshot → `docs/reference/cameo-notation/<probe>-<view>.png`. Cameo then
   drops out of the loop; references are refreshed only deliberately.
3. **Tune locally:** render probe with viewer → side-by-side vs reference → score against the
   per-view rubric (corner radii, stereotype text, compartment separators, port straddle,
   arrowhead style per relationship kind, label placement, frame notation — drawn from the
   OMG spec notation tables) → adjust Rust shape/config code → re-render. Claude drives
   iterations (reads both images, critiques against rubric, edits, re-renders).
4. **Parity matrix:** `docs/reference/parity-matrix.md` — view × feature × {✅/⚠️/❌}, updated
   every iteration. The matrix is a committed artifact: the receipts.

## 6. Quality baseline (fixed during the port, not carried)

From the 2026-07-02 audit of sysml-bridge — each has a concrete acceptance test in §8:

1. Atomic model-file writes (temp file + rename in `FileStore.persist()`).
2. `quoteName()` escapes quotes/backslashes per the grammar's quoted-name rules.
3. Real MCP tool tests — the `expect(true).toBe(true)` suite does not port; every tool gets
   ≥1 end-to-end test via InMemoryTransport (pattern: old `create-relationship.test.ts`).
4. Committed fixtures so gate tests run in CI (no more `describe.skipIf(corpus absent)` as
   the only path — env-gated tests may remain as *additional* real-corpus runs).
5. Error isolation in the inference `boundedPool` (one task failure ≠ whole-run failure;
   failures recorded per-task).
6. LLM parse failures logged with context and counted in run stats (never silently dropped,
   never silently defaulted to a neutral debate score without a counter increment).
7. Dependency hygiene: MCP SDK at a version whose transitive `hono` ≥ 4.12.25 (or pnpm
   override), zod/vitest at current majors at scaffold time.

## 7. Testing strategy (three layers)

1. **Unit tests** port with their packages (~171 in old repo; keep green).
2. **The Tier-1 demo IS the integration test:** CI runs `pnpm demo` (full pipeline) on every
   push and regenerates gallery renders. A broken pipeline = red CI. No separate integration
   harness to maintain (CEI lesson: zero marginal operating cost).
3. **The seeded-defect fixture IS the eval harness:** a poisoned ANGARS variant with exactly
   three planted defects — (a) one element with no `provenanceSourceId` (Gate 1 provenance
   must flag), (b) one grammar error in emitted-text fixture (Gate 2 must exit non-zero),
   (c) one satisfy relationship whose operand is a Definition (Gate 1 relational must flag —
   the class Cameo used to catch). CI asserts each gate reports its specific defect. This is
   the paired-positive-control rule made structural.

## 8. Phasing with validation criteria

Every criterion below is machine-checkable — a command and an expected result. An executor
model claims a phase done ONLY when every criterion passes, run from a fresh clone.

### Phase 0 — Scaffold + core port
Deliver: new repo; `packages/model`, `packages/sysml`, `tools/sysml-validator`,
`tools/viewer` ported; CI green.

- `pnpm install && pnpm build && pnpm test` → all exit 0 on Node 20 and 22.
- `pnpm tsc --noEmit -r` (or per-package `lint` script) → exit 0.
- `pnpm validate:sysml <known-good fixture>.sysml` → exit 0; `pnpm validate:sysml <committed
  known-bad fixture>.sysml` → exit non-zero (positive control committed in Phase 0).
- `grep -rn "expect(true).toBe(true)" packages/` → zero matches.
- Atomic-write test exists and passes: test asserts `persist()` writes via temp file + rename
  (e.g. spies fs calls or crashes mid-write in a child process and asserts the store file
  still parses as valid JSON).
- Quote-escaping test exists and passes: element named `O'Brien's "Console"` → `export_sysml`
  → output passes `pnpm validate:sysml` with exit 0.
- Viewer builds: `cargo build --release --bin export_figures` → exit 0; renders a committed
  smoke `.sysml` to PDF; output file exists and is non-empty.

### Phase 1 — ANGARS demo + gallery (first showable)
Deliver: committed corpus; `pnpm demo`; README gallery v1; CI runs demo.

- Fresh clone: `pnpm install && pnpm demo` → exit 0 with no ANTHROPIC_API_KEY set.
- `pnpm demo` produces `examples/angars/out/angars.sysml`; `pnpm validate:sysml` on it → exit
  0 errors.
- Gate 1 audit JSON emitted (`examples/angars/out/audit.json`) with `findings.length === 0`
  and a fidelity block; CI asserts fidelity ≥ the committed baseline (starting baseline:
  the proven 28/28 = 100% on the C&C slice; the number is a committed constant, not
  hardcoded in CI).
- ≥ 3 rendered views produced (General/BDD, IBD, activity — the views the ported viewer
  already supports; state/requirements/traceability arrive in Phase 4) as PDF+PNG under
  `examples/angars/out/renders/`; CI fails if any expected render is missing or zero-byte.
- README displays ≥ 3 of those renders via relative links; a CI step verifies every image
  referenced in README exists in the repo.
- CI workflow runs the full demo on push; a deliberately broken serializer edit on a branch
  turns CI red (verified once manually during the phase, recorded in the PR description).

### Phase 2 — MCP server + orchestrator (Tier 3)
Deliver: thin mcp-server; `/mbse` + verbs; session state machine.

- Every registered tool has ≥ 1 InMemoryTransport test that calls it via `client.callTool()`
  and asserts response shape; `pnpm --filter mcp-server test` → exit 0.
- Tool count in `index.ts` === tool count asserted in a registration test (listTools()).
- Skill-drift check: `pnpm check:skills` extracts tool names from all skill markdown and
  exits non-zero if any name is not a registered tool; runs in CI; passes.
- Session state machine test: driving init → ingest → build → trace → validate → render via
  tools updates `.mbse/session.json` through exactly those states; invalid transitions
  rejected with a clear error.
- Manual smoke (recorded as PR evidence): `/mbse` in Claude Code on a fresh project reports
  state and routes correctly at least init → build → validate.

### Phase 3 — Candidates + gates + seeded defects (Tier 2)
Deliver: `packages/candidates`; `/mbse-approve`; `docs/gate-pattern.md`; seeded-defect CI.

- Prose-ingest and inference unit tests green in new repo; `boundedPool` failure-isolation
  test: 1 of N tasks throws → run completes, N-1 results present, 1 failure recorded in
  stats.
- No auto-approve path: a test asserts candidates cannot enter composeIR without a
  disposition record; grep-level check that no code path writes approvals without the
  approve tool/skill.
- Seeded-defect CI job: runs pipeline on the poisoned fixture and asserts (a) Gate 1
  provenance finding for the planted uncited element ID, (b) Gate 2 exit non-zero on the
  planted grammar error, (c) Gate 1 relational finding for the planted def-operand satisfy.
  All three assertions on specific IDs, not just non-zero counts.
- LLM parse-failure counters: test feeds malformed JSON responses through a mock provider
  and asserts stats counters increment and a log line is produced.
- `docs/gate-pattern.md` exists, ≤ 2 pages, and names the four rules (grammar-as-truth,
  validate-before-claim, provenance-or-reject, positive controls).

### Phase 4 — Renderer parity loop (parallel-capable after Phase 1)
Deliver: probes, reference gallery, tuning iterations, parity matrix; state → requirements →
traceability views.

- Every `probes/*.sysml` passes `pnpm validate:sysml` with exit 0.
- Every probe has a reference screenshot in `docs/reference/cameo-notation/` AND a viewer
  render; a script asserts the pairing is complete (no probe without both).
- `docs/reference/parity-matrix.md` exists with one row per (view, feature) rubric item and
  no empty cells; regenerated/updated in the same PR as any viewer change (CI checks the
  matrix mtime/hash changes when `tools/viewer/src` changes — or simpler: a PR checklist).
- State-transition view: viewer renders a state probe with ≥ 2 states + ≥ 1 transition where
  the transition ENDPOINTS are drawn (the old parser dropped them — regression test on the
  parsed relationship count).
- Requirements + traceability views: render the ANGARS model; traceability view contains ≥
  (committed baseline) nodes and edges; both views appear in the README gallery.

### Phase 5 — Review UI
Deliver: local web UI for candidate review (approve/reject with citation + source excerpt
side-by-side). Design details deferred to its own spec when the phase starts (sysml-reactflow
embedding decided then).

- UI starts with one command; approve/reject actions write disposition records
  byte-compatible with the `/mbse-approve` schema (same zod schema validates both).
- An approval made in the UI flows through composeIR identically to one made via the skill
  (one shared test proves equivalence).

### Parked (documented, not built)
- Text-native store spike (.sysml as source of truth) — `docs/architecture.md` future-work
  section with the design sketch and the parser-risk note (syside ruled out).
- Benchmark framing (publish the fidelity eval as a "can your LLM model a system faithfully?"
  challenge) — revisit after Phase 4.

## 9. Migration mechanics

- New repo scaffolded fresh (no history import); old repo gets an ARCHIVED banner + pointer
  in its README once Phase 1 is showable, and its GitHub repo is marked archived.
- Port = copy + adapt (imports, package names, the §6 fixes), never blind copy. Each ported
  package lands with its tests in the same PR.
- The old repo remains the reference for anything not yet ported; nothing is deleted from it.

## 10. Open items (deliberate, non-blocking)

- Final repo name (default `sysml-foundry` unless Joe objects at scaffold time).
- Phase 5 review-UI stack choice (plain local web page vs sysml-reactflow embed) — decided in
  its own spec.
- Whether Phase 4 references are harvested by Joe manually or via computer-use — either
  satisfies the criteria.
