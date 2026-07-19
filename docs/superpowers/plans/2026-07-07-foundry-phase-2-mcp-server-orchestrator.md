# sysml-foundry Phase 2 — MCP Server + /mbse Orchestrator Implementation Plan

> **For agentic workers:** executed task-by-task by dispatched executor subagents; each task ends with its own verification. Steps use checkbox (`- [ ]`) syntax.

**Goal:** the thin `packages/mcp-server` (10 tools over `ModelStore`), the lifecycle session
state machine, and the `/mbse` orchestrator + verb skills — Tier 3 becomes usable.

**Architecture:** `mcp-server` registers 10 tools against `FileStore` (no SMAPS). Every tool
gets an InMemoryTransport end-to-end test. A `SessionTracker` records lifecycle progress
(`init → ingest → build → trace → validate → render`) into `.mbse/session.json` as a side
effect of tool calls; backward transitions are rejected. Skills are thin markdown contracts;
a drift check asserts every tool name they reference is actually registered.

**Prerequisite:** Phases 0–1 complete (they are). Spec: `docs/superpowers/specs/2026-07-02-sysml-foundry-rebuild-design.md` §4, §8 Phase 2.

## Global Constraints

- `$OLD` = `/home/joescohen/Engineering/projects/sysml-bridge` (read-only). `$NEW` = this repo.
- Established conventions (from committed Phase 0/1 plans): package configs modeled on
  `packages/sysml/{package.json,tsconfig.json,vitest.config.ts}`; tsconfig extends
  `../../tsconfig.base.json`; vitest `resolve.alias` maps workspace deps to `../<pkg>/src/index.ts`.
- No `expect(true).toBe(true)` anywhere. No SmapsClient port. `pnpm demo` must stay green.
- Type imports: `../types/sysml-elements.js` / `@sysml-bridge/ir` → `@sysml-foundry/model`;
  `../utils/sysml-serializer.js` etc. → `@sysml-foundry/sysml`; audit → `@sysml-foundry/gates`.

---

### Task 1: `packages/mcp-server` scaffold + 10 tools (validate-model relocates home)

**Files:**
- Create: `packages/mcp-server/{package.json,tsconfig.json,vitest.config.ts}` (name
  `@sysml-foundry/mcp-server`; deps: `@modelcontextprotocol/sdk ^1.0.0`, `zod`, and workspace
  `@sysml-foundry/{model,sysml,gates}`)
- Create (ported): `src/tools/*.ts` ← the 9 tool files from `$OLD/packages/mcp-server/src/tools/`
  (all except validate-model.ts)
- Move: `packages/gates/src/tools/validate-model.ts` → `packages/mcp-server/src/tools/validate-model.ts`
  (this is the owed Phase-2 relocation; afterwards `packages/gates` drops its
  `@modelcontextprotocol/sdk` dependency — verify with a grep that no gates source imports the SDK)
- Move: `packages/gates/src/__tests__/{validate-model-findings,validate-coverage}.test.ts` →
  `packages/mcp-server/src/__tests__/` (they exercise the tool, so they live with it)
- Create: `src/index.ts` ← ported from `$OLD/packages/mcp-server/src/index.ts`, FileStore-only
  (delete the SmapsClient import/branch; store = `new FileStore(modelDir)` with
  `SYSML_FOUNDRY_MODEL_DIR` env default `.sysml-foundry/models`)

**Verify:**
- `pnpm install && pnpm --filter @sysml-foundry/mcp-server test` → the two moved test files pass.
- `pnpm --filter @sysml-foundry/gates test` → still green WITHOUT the SDK dep.
- `pnpm build && pnpm lint` → exit 0. `node packages/mcp-server/dist/index.js` over stdio starts
  (spawn it, expect no stderr crash within 2s, kill it).
- Commit: `feat: port mcp-server package — 10 tools over FileStore; validate-model relocated from gates`

### Task 2: end-to-end tool tests — every tool, via callTool

**Files:**
- Port from `$OLD/packages/mcp-server/src/__tests__/`: `create-relationship.test.ts`,
  `export-sysml.test.ts`, `query-relationships.test.ts`, `edit-tools-coupling.test.ts`,
  `write-path-coupling.test.ts`, `prose-gate.test.ts`, `inferred-gate.test.ts` → adapt imports.
  Do NOT port `tools.test.ts` (assertion-free) or `integration.test.ts` (SMAPS/Docker).
- Create: `src/__tests__/registration.test.ts` — registers ALL tools on a server, connects an
  InMemoryTransport client, asserts `client.listTools()` returns exactly the 10 expected names.
- Create: `src/__tests__/tools-e2e.test.ts` — for each tool NOT already covered by a ported
  test's `callTool` (check: init_project, create_element, query_elements, update_element,
  delete_element, get_project_state, import_sysml, validate_model), one happy-path callTool test
  asserting response shape, modeled on the ported `create-relationship.test.ts` pattern.

**Verify:** every one of the 10 tool names appears in at least one `callTool(` invocation across
the test suite (grep-based check, recorded); `pnpm --filter @sysml-foundry/mcp-server test` green.
Commit: `test: end-to-end callTool coverage for all 10 MCP tools`

### Task 3: lifecycle session state machine

**Files:**
- Create: `packages/mcp-server/src/session.ts` — `LIFECYCLE = ["init","ingest","build","trace","validate","render"] as const`;
  `class SessionTracker { constructor(dir); state(); advance(to)); }` — persisted to
  `<dir>/.mbse/session.json` (atomic temp+rename, same pattern as FileStore.persist).
  `advance(to)`: forward moves (including skips) allowed, same-state idempotent, backward moves
  throw `Error("invalid lifecycle transition <from> → <to>")`.
- Modify: `src/index.ts` — wire tool handlers to record progress: init_project→init,
  import_sysml→ingest, create_element→build, create_relationship→trace, validate_model (clean
  run)→validate, export_sysml→render. Wrap so a rejected (backward) advance NEVER fails the tool
  call itself — the session simply doesn't regress (log to stderr).
- Create: `src/__tests__/session.test.ts` — (a) unit: full ordered walk hits exactly the six
  states; backward advance throws with the clear error; (b) e2e: drive
  init_project→create_element→create_relationship→validate_model→export_sysml via callTool and
  assert `.mbse/session.json` reads `render` at the end and passed through the expected states.

**Verify:** tests green; `pnpm demo` still green (demo does not use the server — confirm).
Commit: `feat: lifecycle session tracker (.mbse/session.json) wired to tool calls`

### Task 4: skills — /mbse orchestrator + verbs + drift check

**Files:**
- Create: `packages/skills/{package.json}` (private, no build; `"lint": "echo skills"` +
  `"test": "echo skills"` so `pnpm -r` passes) and `packages/skills/skills/`:
  `mbse.md` (the stateful orchestrator: reads `.mbse/session.json`, reports lifecycle position,
  routes to next step; instructs use of the MCP tools BY EXACT NAME), `mbse-query.md`,
  `mbse-edit.md` (store tools only — NEVER import_sysml round-trip), `mbse-approve.md` (the
  human gate over candidate/disposition files), `mbse-render.md` (runs `pnpm demo:build` +
  `tools/viewer/render.sh` with `--spec`). Source material: `$OLD/packages/skills/skills/*.md`
  (tone/structure) + the spec §4 — but WRITE FRESH, consolidated; each ≤ 120 lines; every MCP
  tool reference in backticks with the exact registered name.
- Create: `scripts/check-skills.ts` — extracts backticked `snake_case` tokens matching the tool
  name grammar from `packages/skills/skills/*.md`, extracts registered names from
  `packages/mcp-server/src/index.ts` (`server.tool("<name>"` occurrences or the register
  functions' literals — read the actual pattern), exits non-zero listing any referenced tool
  that is not registered. Root script `"check:skills": "tsx scripts/check-skills.ts"`.
- Modify: `.github/workflows/ci.yml` — add `pnpm check:skills` step to build-and-test.

**Verify:** `pnpm check:skills` exit 0; deliberately misspell a tool name in a scratch copy and
confirm the script fails (positive control, recorded); CI green after push (done in Task 5).
Commit: `feat: /mbse orchestrator + verb skills with tool-name drift check`

### Task 5: stdio smoke + wiring + verification record

**Files:**
- Create: `.mcp.json` (server `sysml-foundry` → `node packages/mcp-server/dist/index.js`).
- Create: `scripts/smoke-mcp.ts` — spawns the BUILT server over stdio via the SDK's
  `StdioClientTransport`, calls: init_project → create_element → create_relationship →
  validate_model → export_sysml → get_project_state, prints each result, exits non-zero on any
  error; asserts the exported text contains the created element; asserts `.mbse/session.json`
  ends at `render`. Root script `"smoke:mcp": "pnpm --filter @sysml-foundry/mcp-server build && tsx scripts/smoke-mcp.ts"`.
- CI: add `pnpm smoke:mcp` to the build-and-test job (after build).

**Verify:** `pnpm smoke:mcp` exit 0 locally; push; CI all-green; empty-commit
`chore: Phase 2 done-criteria verification record` listing every §8 Phase 2 criterion with its
observed result (tool-coverage grep, registration count, session e2e, check:skills + its
positive control, smoke:mcp, CI run URL). Note honestly: live `/mbse`-in-Claude-Code smoke is
recorded as the stdio-client equivalent; a human Claude Code session smoke remains for Joe.
