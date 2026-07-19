# sysml-foundry — Repository Substrate Design (hybrid: text-as-projection, API-as-record)

**Date:** 2026-07-11
**Status:** Approved (Joe, this session) — execute to completion
**Grounded in:** `docs/research/2026-07-11-sysml-v2-api-repository-substrate.md` (+ `.sources.md`)
**Supersedes non-goal:** the rebuild spec (§1) parked "SMAPS/live-Cameo REST integration" and "JSON-store-vs-text architecture" as future spikes. This spec un-parks them deliberately, as the evolution from *portfolio authoring demo* to *AI-native model+maintain platform*.

---

## 1. Goal & thesis

Evolve foundry from batch corpus→model authoring into infrastructure that **models systems architectures and maintains them in a versioned database**, interoperable with the SysML v2 ecosystem (Cameo/SysON/pilot).

**Thesis (from the research):** *Identity lives in the API; text is a projection.*
- The **OMG SysML v2 API & Services** commit graph (Projects→Branches/Tags→Commits→immutable DataVersion deltas→Elements→Query) is the versioning + identity + interop record of truth.
- **Textual SysML** stays the local working copy and the **LLM generation/diff surface** (far more token-efficient than DataVersion JSON — decisive for an AI-native platform).
- foundry's existing `ModelStore` seam already anticipates this: swapping `FileStore` → `SysmlV2ApiStore` is a one-file change; both implement the same interface.

**Non-negotiable discipline (carried from CLAUDE.md):** R1 (grammar is source of truth — never guess syntax), R2 (validate locally before claiming import), R3/R4 (verify placement, usage-vs-def operands), and the anti-fabrication provenance/gate contract. Nothing here weakens the gates.

---

## 2. Architecture (target)

```
              ┌───────────────────────────── LLM / human authoring ──────────────────────────────┐
              │  corpus → candidates (propose, never write) → /mbse-approve → composeIR            │
              └───────────────────────────────────┬───────────────────────────────────────────────┘
                                                  │  MCP tools (backend-agnostic, ModelStore)
                        ┌─────────────────────────┴──────────────────────────┐
                        │                                                     │
                 FileStore (local)                                   SysmlV2ApiStore (record)
             text/JSON working copy,                          OMG SysML v2 API commit graph:
             LLM generation surface                           projects/branches/tags/commits,
                        │                                     server-assigned elementId, query
                        │  serialize ↕ parse (LOSSLESS on identity+structure)  │  commit ↕ query
                        └──────────────────────┬──────────────────────────────┘
                                               │  (interop verify — spike)
                                        Cameo 2026x / SysON
```

**Identity contract:** every element has a stable `elementId` (UUID). It survives: (a) text serialize→parse→import, and (b) FileStore↔API sync. `elementId` is the join key for diff/merge/reconciliation. Text carries identity + structure; the API carries full fidelity (provenance, all `raw` properties, history).

---

## 3. Milestones (each has machine-checkable acceptance criteria)

### Milestone 1 — Lossless identity + structure round-trip through text  ⟵ FIRST, backend-independent

**Why first:** today round-trip is lossy *by construction* — the parser has no id field, the serializer never emits one, so every import mints a fresh `randomUUID()`. Both API sync and Cameo interop depend on identity surviving the trip. This is the keystone and needs no external infra.

**Design:**
1. **Emit `elementId` in a grammar-legal, validator-passing form.** The executor MUST choose the mechanism by reading `docs/sysml-v2-reference/` (R1), not from memory. Preference order:
   a. A **metadata annotation** carrying `elementId` (follow the existing `InferenceProvenance` metatag precedent in `packages/sysml/src/` — a real model element, survives tool sync, not a strippable comment).
   b. Fallback: a documented comment convention (e.g. `// @id: <uuid>`), *only if* (a) proves disproportionate — with an explicit note that some tools (Cameo textual sync) strip comments.
   The chosen form MUST pass `pnpm validate:sysml` (R2, Gate 2) on ANGARS output.
2. **Recover it on parse.** Extend `ParsedElement` (`packages/sysml/src/sysml-parser.ts`) with an optional `elementId?` and capture it wherever (1) emits it.
3. **Reuse it on import.** Thread the recovered id through `packages/mcp-server/src/tools/import-sysml.ts` into `attributes["@id"]` so `FileStore.buildElement` reuses it instead of minting a new UUID.
4. **Scope honesty:** text carries **identity + structure** (id, type, name, shortName, qualifiedName, ownership/containment, typedBy/specializes/redefines, and relationship endpoints). Arbitrary `raw` properties that the textual notation does not represent (e.g. provenance) are **NOT** expected to survive text — they are the API/JSON substrate's job (Milestone 2). The round-trip test asserts equality only on the text-carried set, and a short doc note lists text-carried vs API-only properties.

**Acceptance (all must pass, from a fresh `pnpm install`):**
- **New test** `packages/sysml/src/__tests__/roundtrip.test.ts` (or mcp-server equivalent): builds a representative element set in a store (parts, actions, requirements, a def+usage, and ≥3 relationship kinds incl. satisfy/verify with usage operands per R4) → `serializeToSysml` → `parseSysml` → import into a fresh `FileStore` → asserts deep-equality of `{id, elementId, type, name, shortName, qualifiedName, ownerId, ownedElementIds}` and relationship endpoints for every element. FAILS on `main` (id regeneration), PASSES after the change. This is the paired positive control — commit a note showing it red before the fix.
- `pnpm --filter @sysml-foundry/sysml --filter @sysml-foundry/model --filter @sysml-foundry/mcp-server test` → all green (baseline: 412 tests; new count ≥ 413).
- `pnpm demo` → exit 0, and `pnpm validate:sysml examples/angars/out/angars.sysml` → **0 errors** (the id-carrying form is grammar-legal).
- Fidelity baseline unchanged: `examples/angars/out/audit.json` findings length 0, trace fidelity still 28/28.

### Milestone 2 — `SysmlV2ApiStore` behind the ModelStore seam (code + unit tests; live-gated)

**Design:** Resurrect `sysml-bridge/packages/mcp-server/src/smaps-client.ts` (real 391-line commit-based `ModelStore` impl) as `packages/model/src/store/sysml-v2-api-store.ts` in foundry, reconciled with foundry's current `SysmlElement`/`ProjectDescriptor`/`SYSML_RELATIONSHIP_TYPES` types. Add backend selection (`SYSML_FOUNDRY_BACKEND=file|api`, default `file`) in `packages/mcp-server/src/index.ts` — the only file that instantiates a store. Add an `aliasId`/`localId` field to `SysmlElement` to distinguish a locally-proposed id from a server-assigned `elementId` for offline-created elements (per the identity-leak finding).

**Acceptance (deterministic):**
- `SysmlV2ApiStore` structurally satisfies `ModelStore` (tsc `--noEmit` clean; a type-level assertion test).
- Mocked-`fetch` unit suite (port the bridge's queue-based fixtures) covers every method incl. the commit/DataVersion POST body shape and query-constraint builder → green.
- Live integration suite `describe.skipIf(!process.env.INTEGRATION)` exists, targets the pilot server, and is **documented as unverified until the spike** (§4). Do NOT claim it passes without a live run.
- Existing 678-test suite + `pnpm demo` stay green with default `file` backend (zero behavior change when the flag is unset).

### Milestone 3 — parked (documented, not built this pass)
Semantic diff/merge on `elementId`; corpus **reconciliation** (upsert by stable id, never clobber human edits); the Cameo/SysON interop verification spike (§4); multi-user/auth. Each gets its own spec when started.

---

## 4. The spike must re-derive (do NOT trust the research report as current fact)
1. Does Cameo 2026x round-trip against a **non-Teamwork-Cloud** SysML v2 API server, or is its API TWC-only?
2. Enumerate SysON's REST **subset** — which of commits/branches/tags/queries it actually implements.
3. Does the spec/pilot define **merge/conflict semantics** for concurrent commits? (Fatal gap for a multi-writer backend if absent.)
4. Re-measure the DLR perf numbers (~10ms/req, >10s deep traversal) and pilot issue #509 on current releases.

Milestone 2's live-integration run (stand up the pilot via Docker+Postgres+sbt, flip the skipped suite green, one lossless commit round-trip by `elementId`) is the vehicle that answers #2–#4; #1 needs a Cameo 2026x install.

---

## 5. Execution rules
- Executors: **Sonnet/Opus** only. Validation/judgment (blind verifier): **Fable**.
- Work on branch `feat/repository-substrate`; atomic commits per milestone; do not push without ask.
- TDD: write the failing acceptance test first (paired positive control), then make it pass.
- Obey R1/R2/R3/R4 and the provenance gates at every step. Never claim "imports"/"round-trips" without the deterministic evidence in §3.
