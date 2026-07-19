# sysml-foundry Phase 3 — Candidates + Seeded Defects Implementation Plan

> **For agentic workers:** executed task-by-task by dispatched executor subagents; each task ends with its own verification. Steps use checkbox (`- [ ]`) syntax.

**Goal:** the human-gated LLM layers ported as one `packages/candidates` package, the
seeded-defect eval harness in CI (each gate proven to catch its planted defect, by id), and
`docs/gate-pattern.md` — Tier 2 ships.

**Architecture:** `packages/candidates` = old `prose-ingest` + `inference` under one roof:
`src/prose/` and `src/inference/` keep their ported internals; `src/contract.ts` names the
shared shape (a Candidate carries provenance/citations; a Disposition is an explicit
approve/reject record; composeIR in `@sysml-foundry/model` is the ONLY path into the model).
The seeded-defect harness is deterministic (no API key): it plants three known defects in a
copy of the clean ANGARS build and asserts each gate reports its specific defect id — plus a
clean control run with zero findings (paired positive controls, both directions).

**Prerequisite:** Phase 2 complete. Spec: `docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md` §6, §7, §8 Phase 3.

## Global Constraints

- `$OLD` = `/home/joescohen/Engineering/projects/sysml-bridge` (read-only). `$NEW` = this repo.
- Established conventions: package configs modeled on `packages/sysml/*`; tsconfig extends
  `../../tsconfig.base.json`; vitest aliases workspace deps to `../<pkg>/src/index.ts`.
- All Phase-3 criteria run WITHOUT `ANTHROPIC_API_KEY` (live-LLM tests stay env-gated extras).
- `pnpm demo` and all 506+ existing tests stay green. jq is not installed — use node -e.
- No auto-approve path may exist anywhere; the tests prove it.

---

### Task 1: port `packages/candidates` (prose + inference under one contract)

**Files:**
- Create `packages/candidates/{package.json,tsconfig.json,vitest.config.ts}` — name
  `@sysml-foundry/candidates`; deps `@sysml-foundry/model` (+ `@anthropic-ai/sdk` and `zod`
  matching the old packages' manifests; check `$OLD/packages/{prose-ingest,inference}/package.json`
  for the full dep lists incl. PDF parsing).
- Port `$OLD/packages/prose-ingest/src/` → `packages/candidates/src/prose/` (all files + tests).
- Port `$OLD/packages/inference/src/` → `packages/candidates/src/inference/` (all files + tests).
- Create `src/contract.ts` — re-export + document the shared candidate/disposition types that
  already exist (`CandidateEntry`, `InferenceCandidate`, approval helpers live in
  `@sysml-foundry/model`); one short doc-comment naming the invariant: candidates NEVER enter
  composeIR without a disposition record. `src/index.ts` barrels prose, inference, contract.
- Import fixes per established conventions (`@sysml-bridge/ir` → `@sysml-foundry/model`).

**Verify:** `pnpm --filter @sysml-foundry/candidates test` green with NO API key (env-gated
live tests skip — record the skip count and which files); `pnpm -r test` green; `pnpm build`
+ `pnpm lint` exit 0. Commit: `feat: port candidates package (prose-ingest + inference) under one contract`

### Task 2: failure isolation + parse-failure counters (the §6 fixes)

**Files:**
- Modify `src/inference/engine.ts` — `boundedPool` gains per-task error isolation: a worker
  wraps each task in try/catch; failures are recorded (index + error message) and surfaced in
  the run stats; remaining tasks continue. Match the old signature for callers or update the
  one call site.
- Modify LLM response parsing in BOTH layers so parse/schema failures (a) log one stderr line
  with context, (b) increment a counter surfaced in run stats — prose: `llm-provider.ts`
  returns `[]` sites; inference: `inference-provider.ts` debate `{score: 0.5}` fallbacks (the
  0.5 default stays, but it is now counted and logged).
- Tests: (a) boundedPool — N tasks, 1 throws → N-1 results present, 1 failure recorded, no
  rejection of the whole pool; (b) mock provider feeding malformed JSON → counters increment
  and a log line is produced (spy on console.error).

**Verify:** package tests green; `pnpm -r test` green. Commit:
`fix: inference pool failure isolation + counted LLM parse failures`

### Task 3: no-auto-approve proof

**Files:**
- Test `src/__tests__/no-auto-approve.test.ts`: (a) composeIR (both prose and inferred
  variants from `@sysml-foundry/model`) with candidates present but NO disposition records →
  zero candidate content enters the composed IR (assert the composed output equals the
  base extraction); (b) with an explicit approval record → the approved entry appears;
  (c) source-scanning ratchet: glob `packages/candidates/src/**/*.ts` +
  `packages/model/src/*.ts` for calls to the approval-record writers (`appendApproval`,
  `appendInferredApproval`) and assert every call site is in an allowlisted file (the
  helpers' own modules + tests) — a new call site fails the test until reviewed.

**Verify:** tests green including a positive control (temporarily add a rogue
`appendApproval` call in a scratch copy → ratchet fails → remove). Commit:
`test: no-auto-approve invariant + approval-writer ratchet`

### Task 4: seeded-defect harness in CI

**Files:**
- Create `examples/angars/pipeline/seeded-defects.ts`: builds the SAME model `demo:build`
  builds (reuse its build function — export one if needed, keeping `demo:build` behavior
  identical), then plants exactly three defects with FIXED known identifiers:
  (a) element `seeded-uncited-part` with NO provenanceSourceId,
  (b) after serialization, corrupt the emitted text copy (write to
  `out/angars-poisoned.sysml`): replace one `requirement ` keyword with `requirment `,
  (c) trace edge `seeded-def-operand-satisfy`: a SatisfyRequirementUsage whose operand is a
  Definition.
  Then asserts: Gate 1 findings contain a provenance finding whose elementId is the seeded
  uncited element's id AND an `R4-def-operand` finding for the seeded satisfy's operand;
  Gate 2 (`tools/sysml-validator/run.sh`) exits NON-zero on the poisoned file and ZERO on the
  clean file (paired control); prints a summary table; exit 0 iff all assertions hold.
- Root script `"demo:seeded": "tsx examples/angars/pipeline/seeded-defects.ts"`; CI: add a
  `pnpm demo:seeded` step to the demo job (after `pnpm demo`).

**Verify:** local `pnpm demo:seeded` exit 0 with all three catches shown; deliberately
disable one assertion input (e.g. give the uncited element a provenance id) → script exits
non-zero (positive control, recorded, reverted). Commit:
`feat: seeded-defect eval harness — every gate proven to catch its planted defect`

### Task 5: gate-pattern doc + Tier-2 README row + verification record

**Files:**
- `docs/gate-pattern.md` — ≤2 pages: the four rules (grammar-as-truth, validate-before-claim,
  provenance-or-reject, positive controls), each with: the failure that motivated it, the
  mechanism in THIS repo (file paths), and how to apply it in any repo. Source: CLAUDE.md
  R1-R4, the spec §1, the 2026-06-09 fabrication story, the two red-CI proofs (runs
  28831984796 / 28833309369 — unit tests and pipeline gate catching DIFFERENT defect classes).
- README: flip the Tier-2 row to "ships now", one-paragraph Tier-2 description pointing at
  `/mbse-approve`, `pnpm demo:seeded`, and gate-pattern.md.
- Push; CI green (conclusion via `gh run view --json`, not watch exit codes); empty commit
  `chore: Phase 3 done-criteria verification record` listing every §8 Phase 3 criterion with
  observed results; push.
