# sysml-foundry Phase 5 — Candidate Review UI Implementation Plan

> **For agentic workers:** executed task-by-task by dispatched executor subagents; each task ends with its own verification. Steps use checkbox (`- [ ]`) syntax.

**Goal:** a local web UI for the human gate — candidate + citation + source excerpt side by
side, explicit approve/reject — whose disposition records are byte-compatible with the
`/mbse-approve` path, proven by a shared-schema + equivalence test.

**Architecture:** `packages/review-ui` — a zero-build, dependency-light Node server
(`node:http`; no framework) serving one inline-styled HTML page and three JSON endpoints:
`GET /api/state` (candidates + dispositions merged), `POST /api/approve`, `POST /api/reject`.
The write path calls the SAME `appendApproval` / `recordRejection` /
`appendInferredApproval` / `recordInferredRejection` helpers from `@sysml-foundry/model` that
the `/mbse-approve` skill uses — byte-compatibility by construction, verified by test. Demo
data: the real ANGARS candidate files from the old repo. One command: `pnpm review`.

**Spec criteria (§8 Phase 5):** one-command start; UI dispositions validate against the same
zod schemas; one shared test proves a UI approval flows through composeIR identically to a
helper-made approval.

## Global Constraints

- `$OLD` = `/home/joescohen/Engineering/projects/sysml-bridge` (read-only). `$NEW` = this repo.
- THE RATCHET IS THE POINT: `packages/candidates/src/__tests__/no-auto-approve.test.ts`
  scans for production call sites of the approval writers and WILL fail on the new server.
  Extending its scan scope + allowlist for the review-ui server module is a REQUIRED,
  CONSCIOUS step with a justifying comment ("the review UI is a human-gate surface — every
  write happens on an explicit user click; no automated path reaches these endpoints").
  The ratchet must also gain the review-ui scan root so future non-allowlisted call sites
  there still fail.
- Every POST handler writes ONLY on an explicit request; no batch/auto-approve endpoint may
  exist. GET endpoints are read-only.
- No new runtime dependencies beyond workspace packages (node:http + inline HTML/CSS/JS; the
  page needs no framework for a list + detail + two buttons).
- The server binds 0.0.0.0 (WSL → browser on another machine) and prints the URL; port via
  `--port`/env, default 4173.
- All existing gates stay green: `pnpm demo`, `pnpm demo:seeded`, `pnpm -r test`,
  `pnpm check:skills`, `pnpm check:parity`.

---

### Task 1: demo candidate data + package scaffold + server

- Copy `$OLD/examples/angars/model/prose-candidates.json` and
  `$OLD/examples/angars/model/inference-candidates.json` → `$NEW/examples/angars/candidates/`
  (they are corpus-derived, public like the corpus; verify they parse against
  `CandidateEntrySchema`/`InferenceCandidateSchema` from `@sysml-foundry/model` — if a file
  has a wrapper shape, document it and parse accordingly).
- `packages/review-ui/{package.json,tsconfig.json,vitest.config.ts}` per sibling conventions
  (dep: `@sysml-foundry/model: workspace:*`).
- `src/server.ts` — `createReviewServer(opts: {candidatesDir, dispositionsDir, port})`
  returning the `http.Server` (exported for tests; `main()` guard for CLI). Endpoints:
  - `GET /` — the page (inline HTML/CSS/JS): candidate list (kind, name/summary, pending/
    approved/rejected badge), detail pane with FULL citation (docId, sectionPath, quote) for
    prose or premises/confidence/rationale-redacted for inference, approve + reject buttons
    (POST then refresh). Keep it clean and readable; no framework.
  - `GET /api/state` — candidates from both files + disposition status per candidate
    (reading prose-approved/prose-rejections/inferred-approved/inferred-rejections in
    `dispositionsDir`).
  - `POST /api/approve` `{layer: "prose"|"inference", candidateId}` — finds the candidate,
    calls the matching append helper with `approvedBy` = `REVIEW_UI_USER` env or
    `os.userInfo().username`.
  - `POST /api/reject` `{layer, candidateId}` — matching rejection helper.
  Malformed requests → 400 with a JSON error; unknown candidate → 404.
- Root script `"review": "tsx packages/review-ui/src/server.ts --candidates examples/angars/candidates --dispositions examples/angars/out/dispositions"`.

**Verify:** `pnpm review` starts and prints the URL; `curl localhost:4173` returns the page
HTML (assert it contains a candidate name from the demo data); `curl /api/state` returns
both layers' candidates; a `curl -X POST /api/approve` writes a disposition file that
`ProseApprovedEntrySchema` (or inferred) parses; `pnpm -r test` green. Commit:
`feat: candidate review UI — human gate as a local web page`

### Task 2: ratchet extension + equivalence tests

- Extend the ratchet in `no-auto-approve.test.ts`: add `packages/review-ui/src` to the scan
  roots AND `packages/review-ui/src/server.ts` to the allowlist with the justifying comment.
  POSITIVE CONTROL: a rogue writer call in a different review-ui file must still fail
  (record both runs).
- `packages/review-ui/src/__tests__/equivalence.test.ts`:
  (a) shared-schema: start the server on an ephemeral port against tmp dirs with fixture
  candidates; POST approve + reject for both layers; parse every written file with the SAME
  zod schemas the skill path uses — all parse, statuses correct.
  (b) equivalence: approve candidate X via the HTTP endpoint and candidate X (same content,
  fresh tmp dir) via direct `appendApproval` — the two entries are identical except
  `approvedAt`/`approvedBy` (assert field-level equality on id, kind, fields, citation,
  candidateId, status; the stable id MUST be identical since it is content-addressed);
  `composeProseTwoLayer` over each yields the same composed entry modulo those two fields.
  (c) no-auto-approve at the HTTP layer: `GET /api/state` never mutates (read it twice,
  hash the dispositions dir before/after — identical); an unknown endpoint 404s.

**Verify:** package tests green; the ratchet positive control recorded; `pnpm -r test` green.
Commit: `test: review-ui equivalence with the /mbse-approve path + ratchet extension`

### Task 3: README + smoke + verification record + push

- README: add Tier-2 sentence + a short "Review UI" subsection (`pnpm review`, what it
  shows, that it writes the same disposition records as `/mbse-approve`). Screenshot the
  page if feasible (curl-rendered HTML is not a screenshot — if no browser tooling is
  available, describe honestly and skip the image rather than fake it).
- CI: add review-ui tests via `pnpm -r test` (automatic — verify the package test script
  runs in CI) — no server needed in CI beyond the ephemeral-port tests.
- Push; CI green (conclusion via `gh run view --json`); empty commit
  `chore: Phase 5 done-criteria verification record` (criteria: one-command start + URL;
  schema-shared dispositions; equivalence test; ratchet extension + control; CI run URL);
  push. Phase 5 = spec complete.
