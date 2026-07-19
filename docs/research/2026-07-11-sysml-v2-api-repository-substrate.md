# Research: SysML v2 API & Services as a versioned repository substrate

**Type:** Feasibility + Validation (substrate comparison) · **Depth:** thorough · **Saturation:** full (5 dimension agents + adversarial critic) · **Date:** 2026-07-11

Provenance tags: `[VERIFIED]` ≥2 independent HIGH/MODERATE sources · `[SUPPORTED]` 1 HIGH/MODERATE, no contradiction · `[ASSUMED]` LOW/inference · `[CONTESTED]` sources disagree. Full citations in `sources.md`.

---

## Executive summary

The OMG **Systems Modeling API & Services** is exactly the "maintain it in a database" substrate you sensed existed: an **adopted formal v1.0 spec** defining a **git-like commit graph** (Projects → Branches/Tags → Commits → immutable DataVersion deltas → Elements → Query) over a REST API. `[VERIFIED]` Your instinct — SysML v2 + a standard repository — is architecturally correct and standards-blessed.

But **no production-hardened server exists in mid-2026.** `[VERIFIED]` Both realistic self-host options are reference-quality: the OMG pilot (`SysML-v2-API-Services`, full-API-compliant but self-labeled "proof-of-concept," no auth/HA, monthly breaking changes) and Eclipse **SysON** (very actively developed, Postgres-backed, but its REST layer is only a *subset* of the standard API and its primary interface is GraphQL, not the standard REST).

**Recommendation:** Adopt the API's **commit-graph as your identity + versioning model of record**, implemented against the pilot as the reference server — but keep your **file-native/text store as the local working copy and the LLM-facing surface** (textual SysML is dramatically more token-efficient to generate than DataVersion JSON). This **hybrid** is exactly what your existing `ModelStore` seam was built for: a one-file swap adds a `SysmlV2ApiStore` alongside `FileStore`. Do **not** make Cameo round-trip the keystone — its API conformance is an untested vendor claim routed through proprietary Teamwork Cloud (lock-in), and the certification program that would validate it only launched June 2026.

**The real first task is identity, not the backend.** Your text round-trip is lossy *by construction* today (no `@id` survives serialize→parse). Fixing the identity spine gates everything downstream — both API sync and any Cameo interop depend on stable `elementId` surviving the trip.

---

## Key findings

### SQ1 — Spec status + versioning data model
**Finding:** Adopted OMG **formal v1.0** (supersedes 1.0/Beta3), a *separate* normative document ("Part 3") from KerML (Part 1) and SysML (Part 2). Data model confirmed from the official `openapi.yaml`: `/projects`, `/branches`, `/tags`, `/commits`, `/commits/{id}/elements`, `/elements/{id}/relationships`, `/queries`. A **Commit** = `{change[], previousCommit}` where `change[]` is an array of **DataVersion** — an **incremental delta, not a full snapshot**. DataVersions are immutable and never deleted (a delete is a new DataVersion with empty payload). Writes are incremental: POST a commit carrying only changed DataVersions. `[VERIFIED — primary openapi.yaml + issue #97]`
**Caveats:** `ProjectUsage` (cross-project referencing) resolution semantics were an **open unresolved spec issue** `[SUPPORTED]` — a direct risk to any multi-project platform. Adoption month reported inconsistently (June/July vs Sept 2025) — immaterial, but pin it before citing. `[CONTESTED, minor]`
**Evidence quality:** HIGH. **Convergence:** OMG spec page + official OpenAPI + release repo + maintainer issue thread.

### SQ2 — Server implementations
**Finding:** `[VERIFIED]`
- **OMG pilot `SysML-v2-API-Services`** — Java/Play/sbt, **PostgreSQL**, Docker + `sbt run` (:9000). License **changed LGPL→EPL-2.0** at the 2026-04 release (May 14 2026). ~599 commits / 90 releases, active. Self-labeled **"proof-of-concept pilot."** Only implementation targeting **full** API compliance; companion `SysML-v2-API-Cookbook` (Jupyter) exercises commits/branches/tags/CRUD/query on a spacecraft example (but is ~14 months stale vs a monthly-changing API).
- **SysON (Obeo/Eclipse)** — web modeler on Sirius Web (**Spring Boot + PostgreSQL + GraphQL**). Exposes GraphQL (primary) **plus a REST API implementing only a *subset*** of the standard. Dual EPL-2.0/LGPL-3.0. **v2026.7.0 (Jul 10 2026)**, ~1538 commits, near-weekly cadence — the most active option.
- Others: `SysML-v2-Pilot-Implementation` (Xtext editors + Jupyter kernel, *not* the REST server); Open-MBEE Flask wrapper (WIP); Intercax **Syndeia** (commercial digital-thread client, not a self-hostable server).
**Verdict:** No hardened production server. SysON = velocity + Postgres but partial/GraphQL-first API; pilot = faithful API + real versioned repo but PoC-grade. **Evidence quality:** HIGH (repos/releases/licenses).

### SQ3 — Cameo / Magic SoSA 2026x
**Finding:** 2026x ships **native** SysML v2 (KerML/SysML metamodel, not a UML profile) as a "SysML v2 Plugin" on the MagicDraw/Teamwork Cloud platform, in **paid tiers only**; the free Community Edition is metamodel-conformant but capped at **500 major elements and has no REST API.** `[SUPPORTED — vendor docs]` A REST API exists in the paid tier **via Teamwork Cloud**, claimed to "fully conform to OMG SysML v2 API v1.0," with desktop + REST clients sharing one TWC repository. `[CONTESTED / LOW — pure vendor claim; certification launched only Jun 2026 so nothing is independently certified yet]`
**Critical gap:** **No evidence of round-trip against an external/third-party API server** — Cameo's "API" is its own Teamwork Cloud. Treat foreign-server interop as **unverified**. `[ASSUMED — likely TWC-only]` Practitioner (Webel) reports real semantic gaps (default-subject auto-binding, satisfy-by feature paths, `//` comments stripped on sync) and macOS instability, though currency vs 2026x is uncertain; the SysML v2 simulation plugin is EXPERIMENTAL. `[SUPPORTED, dated]`
**Evidence quality:** MODERATE→LOW. The load-bearing "Cameo speaks the open API" claim is the weakest in the whole report.

### SQ4 — SysON maturity + API conformance
**Finding:** Real, actively-developed web SysML v2 modeler; Postgres-backed; but textual import/export is explicitly **partial** ("some concepts still under development"; dependencies must be pre-imported or relationships won't resolve), and its standard-REST coverage is a **subset never enumerated in the evidence.** `[SUPPORTED]` Which endpoints (commits/branches/queries) it actually implements is the pilot-vs-SysON tiebreaker and must be checked hands-on. **Evidence quality:** HIGH for "subset," but the *scope* of the subset is an open gap.

### SQ5 — Round-trip fidelity: text vs JSON/API `[CONTESTED — and both are right]`
**Finding:** Two camps, both talking their book, but the disagreement reflects **two valid use-cases, not one error**:
- **Pro-API/JSON** (Sodius/Willert guide; Intercax Oct-2024 live demo syncing FreeCAD/MATLAB/openLCA/TomSawyer through one API repo): JSON is more resilient because **text has no first-class slot for `elementId`**, so round-tripping through text alone loses/regenerates identity.
- **Pro-text** (Sensmetry/Syside): text + Git as source of truth for CI/CD; their REST API is still "on the roadmap."
**The substrate-neutral facts** `[VERIFIED]`: `elementId` (UUID) is the canonical stable anchor (aliasIds allow multiples; qualified names shift on rename); the reference tool has a real **unresolved** text re-serialization failure (issue #509, closed "not planned" in 2023 — stale, re-verify); and **neither** substrate carries diagram layout (OMG only has a working group "organized to define" it) — **views never survive round-trips; every tool re-lays-out from semantics.**
**Reframe for our architecture:** *identity lives in the API; text is a projection.* Author/generate/diff in text (LLM-friendly), but let the commit graph own identity and history.

### SQ6 — Codebase feasibility (grounding the recommendation)
**Finding:** `[VERIFIED — direct code inspection]` The `ModelStore` interface is genuinely backend-agnostic — `projectId/branchId/headCommitId` already on it, all CRUD/query return Promises, and serializer/parser/gates never import `FileStore`. Swapping `FileStore`→`SysmlV2ApiStore` touches **exactly one file** (`packages/mcp-server/src/index.ts`). The predecessor `sysml-bridge` has a **real, non-stub `SmapsClient` (391 lines)** implementing every method against a commit-based REST API (POST `/commits` with `DataVersion[]` per mutation, query-constraint builder, wire↔domain mappers), with a mocked-fetch unit suite and an INTEGRATION-gated live suite that was **never run green in CI** — a salvageable skeleton, unvalidated against a live server.
**The leaks:** (1) **Identity** — `FileStore` mints `randomUUID()` locally and returns synchronously; a real API assigns `elementId` server-side on commit response, and `SysmlElement` has no `aliasId`/`externalId` to hold a local-vs-server distinction. (2) **`updateElement`** does a cheap in-place merge; a commit API needs read-current-then-full-payload, and **concurrent-commit / stale-head conflict semantics are unmodeled** in the interface. (3) **Text round-trip is lossy today**: parser has no `@id`, serializer never emits one, so every import mints new UUIDs; **no test asserts round-trip equivalence.**

---

## Evidence landscape

**Convergences (trust as architectural fact):** commit-delta versioned model; no production-grade server; text loses identity while `elementId` is the anchor; no layout interchange; the `ModelStore` seam is clean and the `SmapsClient` is a real starting point.

**Contradictions:** text-vs-JSON (resolved as two use-cases → hybrid); SysON "full compliance" (marketing) vs "subset" (docs — believe the docs); Cameo "conforms" (vendor) vs zero interop evidence.

**Gaps the evidence did not close (⇒ spike must answer):**
1. Does Cameo 2026x round-trip against a **non-Teamwork-Cloud** SysML v2 API server? (Likely no.)
2. **Enumerate** SysON's REST subset — which of commits/branches/tags/queries exist.
3. Does the **spec/pilot define merge/conflict semantics** for concurrent commits? (Fatal gap for a multi-writer backend if absent.)
4. Re-derive the stale claims: DLR perf numbers (REST ~10ms/req, >10s deep traversal on ~200k-element models) and issue #509 — both old enough to be fixed *or worse* on current releases.

**No production deployment of any SysML v2 API server appears anywhere in the evidence** — every interop "success" is a staged single-repository demo. Adopt with eyes open.

---

## Recommendation

1. **Fix identity first (backend-independent).** Stamp `elementId` through the serializer in a grammar-legal way, capture it on parse, thread it through `import_sysml`, and add a deep-equality round-trip test (`store → serialize → parse → store` asserts `id/type/name/qualifiedName/owner/raw` equality). This is the true keystone: API sync *and* Cameo interop both depend on it. `[confidence: HIGH]`
2. **Adopt the OMG API commit-graph as the versioning model of record**, implemented against the **pilot server** as the reference target, resurrecting `SmapsClient` as `SysmlV2ApiStore` behind the existing `ModelStore` seam. `[HIGH]`
3. **Keep the hybrid.** File-native/text stays as local working copy + LLM generation surface; the API store is the shared, versioned, interop layer. Don't force LLMs to emit DataVersion JSON when text is far more token-efficient. `[SUPPORTED — critic insight]`
4. **Treat Cameo as a *verify-in-spike* interop target, not a design assumption.** Its open-API path is proprietary-TWC-shaped and uncertified; the reliable open interop is pilot↔SysON. `[HIGH]`
5. **Do not treat any server as production infra yet.** Budget for hardening (auth, backup/DR, migration-off-PoC), watch the EPL license flip as a governance signal, and note Teamwork Cloud lock-in + ITAR/CUI on-prem needs undercut the open-standard premise for defense users. `[SUPPORTED]`

### Spike design (hands-on, ~1–2 weeks)
- Stand up the pilot `SysML-v2-API-Services` (Docker Postgres + `sbt run`). Prove: create project → branch → commit N elements → query → read back; measure real latency on a few-hundred-element model (re-derive DLR).
- Resurrect `SmapsClient` as `SysmlV2ApiStore`; make foundry's MCP tools pass against the live pilot (flip the skipped INTEGRATION suite to green).
- Answer the 4 gap questions above empirically — especially **merge semantics** and **Cameo↔external-server**.
- Prove one **lossless text round-trip** end-to-end with identity preserved.

**Exit criterion:** a foundry model built via MCP tools, committed to a live pilot repo on a branch, read back byte-faithful by `elementId`, and one clean text round-trip — with a written finding on whether Cameo can join the loop or is TWC-only.

---

## Addendum — hands-on live standup (2026-07-12, primary evidence)

The spike's Milestone 2 stood up the **real OMG pilot server** on this box and validated the substrate end-to-end. This upgrades several report claims from inference to `[VERIFIED]`:

- **The pilot builds and runs — but is decisively PoC-grade / finicky.** Stack is 2019-era: Scala 2.12.6, sbt 1.2.8, Play 2.x, targeting JDK 11 (fails on modern JDKs without care). On aarch64 with JDK 11 it needed a workaround (`-Dlog4j2.loggerContextFactory=org.apache.logging.log4j.core.impl.Log4jContextFactory`, else sbt's own logging throws `MatchError: SimpleLoggerContext`) and a held-open stdin (Play dev-mode `run` stops the app on stdin EOF under nohup). First boot ran Hibernate `hbm2ddl` generating **~8,200 tables** for the KerML+SysML metamodel; the first request took **151s** (compile + full DDL). No auth, built-in connection pool, "not for production" warnings. → Confirms "reference-quality, not production" and adds: *even standing it up is a project.* `[VERIFIED]`
- **Commit-graph works end-to-end.** `POST /projects` auto-creates a `main` branch; `POST /projects/{id}/commits?branchId=` with a `{Commit, change:[DataVersion{payload}]}` creates elements; `GET commits/{id}/elements` reads them back with a **server-assigned `elementId` (== `@id`)** and an `aliasIds[]` array; branch `head` advances per commit. `[VERIFIED — curl + a green 8-test live integration suite]`
- **Spike Q3 (merge semantics) — ANSWERED: there is NO merge/rebase/diff endpoint.** `conf/routes` exposes projects/branches/tags/commits/changes/elements/roots/relationships/queries/query-results/projectUsage — but no merge. **Merge/conflict resolution is a client responsibility, not provided by the standard API server.** Direct consequence for the platform: multi-writer reconciliation (Milestone 3) must be built by us. `[VERIFIED — routes]`
- **Live wire deviations from the report's assumptions** (now encoded in `packages/model/src/store/sysml-v2-api-store.ts`): KerML naming is `declaredName` (not `name`); `PrimitiveConstraint.value` must be an array even for scalar equality (a bare string 500s); the `instanceOf` query operator is **unimplemented server-side** (so "list all relationships" must classify client-side); a `DataVersion` payload is a **full replace, not a patch** (update = read-then-merge-then-commit); a fresh project's branch `head` is `null` (must be guarded). `[VERIFIED]`

**Net for the recommendation:** the hybrid stands. The API commit-graph is real and usable as the versioned identity store (Milestone 2 proves it live), but the pilot is not deployable infra as-is — for a real platform, prefer SysON's actively-maintained server or budget real hardening of the pilot, and plan to implement merge/reconciliation ourselves. Spike restart: toolchain + clone live in `../.sysmlv2-spike/` (JDK 11, sbt, pilot); Postgres via `docker start sysml2-postgres`; server via `sleep infinity | sbt "run 9000"` with the log4j factory flag.

Still NOT verified live (need a Cameo 2026x install): Q1 Cameo↔external-server round-trip, Q2 SysON's exact REST subset. Q4 DLR perf: partially re-derived — the 151s first-request here is dominated by one-time compile+DDL, not steady-state; steady-state reads were sub-second.

---
*Generated by `/ei-research` (thorough): 5 parallel dimension agents (Sonnet) + adversarial critic (Fable), conductor synthesis. Addendum: live standup + Milestone-2 validation (Sonnet executor, independently re-run). The remaining unverified claims (Cameo conformance, SysON subset scope) need a Cameo/SysON install.*
