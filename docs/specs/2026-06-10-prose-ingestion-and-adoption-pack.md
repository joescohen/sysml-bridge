# SPEC — Prose Ingestion + Adoption Pack (v1.1 milestone)

**Date:** 2026-06-10
**Status:** Ready for planning
**Audience:** planning/execution agents. This spec is self-contained; read the referenced
knowledge-base docs before planning. Nothing here requires conversation context.

---

## 0. Context and objective

`sysml-bridge` is a corpus-grounded MBSE pipeline: spreadsheet corpus → `extracted.json`
(canonical IR) → `/mbse-*` skills → MCP model store → Gate 1 (write-path-coupled audit:
provenance / relational / R4 / fidelity) → `export_sysml` → Gate 2 (ANTLR grammar) →
`.sysml` → DeciSym renders + Cameo import slices. All of that exists and is verified
(347 tests; see `.planning/ROADMAP.md` phases 1–7, all complete).

This milestone adds the **highest-criticality items** from the 2026-06-10 strategic review:

| ID | Item | Criticality |
|----|------|-------------|
| F0 | Gate-3 closure: Cameo import spot-check (precondition) | HIGH |
| F1 | Approved-prose layer (the keystone design) | HIGH |
| F2 | PDF ingestion (SEPAL module port + deterministic-first extraction + approval queue) | HIGH |
| F3 | Corpus-grounded state machine from CONOPS modes (falls out of F2) | HIGH |
| F4 | Multi-corpus mini-proof (corpus-generic demonstrated, not asserted) | MEDIUM-TOP |
| F5 | RTM-to-Excel export (CDRL-style deliverable artifact) | MEDIUM-TOP |
| F6 | Requirement-quality linting (INCOSE-style, the adoption wedge) | MEDIUM-TOP |
| F7 | DEMO.md — 10-minute reproducible walkthrough + gate-guarantees one-pager | MEDIUM-TOP |

**Explicit non-goals for this milestone** (recorded so planners do not scope-creep):
re-baseline diff as a full feature (only its *primitive* lands via F1 suspect flags);
ReqIF export (company confirmed on Cameo 2026x / SysML v2 — native `.sysml` is correct);
web UI; autonomous (non-human-approved) editing; embeddings/RAG/vector store (a one-shot
extraction pass needs none of it); incremental Gate-1 audit (known scaling cliff — trigger
is corpus > ~5k entities, record, don't build); TF-3 parser round-trip parity (becomes
mandatory only when maintainers edit in Cameo and models must be read *back*; record, don't
build).

**Required reading for planners:**
- `docs/reference/sepal-corpus-engine.md` — the SEPAL port verdict + mechanisms (authoritative)
- `docs/reference/saic-devt.md` — relational discipline Gate 1 enforces
- `CLAUDE.md` rules R1–R4 + validation gate (the emission spine; non-negotiable)
- `packages/ir/` — the IR schema this milestone extends
- `packages/mcp-server/src/audit/` — Gate 1 (note the `n2Interfaces` resolution-set
  extension pattern from commit 32701a0; F1 mirrors it)
- `examples/angars/CAMEO-HANDOFF.md` — the F0 procedure

**Standing constraints (apply to every feature):**
- Corpus privacy: anything containing corpus text (requirement statements, need text,
  extracted prose) stays in gitignored dirs (`examples/angars/model/**`). Committed docs
  are procedural only — the `grep -c "shall" == 0` acceptance check applies to every
  committed artifact.
- No fabrication: no model element or IR entry without resolvable provenance. The LLM
  never *finds* content alone; it validates/structures what deterministic passes found,
  or its proposals carry mandatory chunk citations and die in the queue without approval.
- Gates are never loosened to make a feature pass. Extensions to Gate 1's resolution set
  must be test-driven and narrowly scoped (the n2Interfaces precedent).
- Commits end with the project's standing co-author trailer. No pushes.

---

## F0 — Gate-3 closure (precondition, human-in-the-loop)

**What:** The manual Cameo CE import spot-check per `examples/angars/CAMEO-HANDOFF.md`
(6 slices, paste → Alt+S, expect zero notification errors) executed by the user.

**Why it gates this milestone:** local gates have a documented blind-spot class (R4 was
caught only in Cameo before Gate 1 learned it). Everything in this milestone stacks more
work on the `.sysml` emission path; if Gate 3 surfaces a new error class, rework compounds.

**Agent-executable part:** prepare a results-capture section in CAMEO-HANDOFF.md
(per-slice: imported clean / errors verbatim). **Doctrine:** every Cameo-rejected
construct becomes a new Gate-1 rule (test-driven) before any other milestone work that
touches emission. If the user cannot run the import promptly, F1/F2 work may proceed
in parallel BUT no milestone-completion claim may be made until F0 is recorded.

**Acceptance:** CAMEO-HANDOFF.md contains a filled results section; any failures have
corresponding Gate-1 rule issues filed (or fixed) with the failing construct as a test fixture.

---

## F1 — Approved-prose layer (keystone design; build BEFORE F2's queue)

**Problem:** `extracted.json` is currently a pure function of the spreadsheet corpus —
deterministic, byte-stable, re-runnable. Human-approved prose extractions must NOT turn it
into mutable state, or re-extraction wipes approvals and the fidelity audit loses clean
ground truth.

**Locked design:**
1. **Two layers, composed at read time.** The extractor keeps producing `extracted.json`
   (spreadsheet layer) untouched. A new append-only file
   `examples/angars/model/prose-approved.json` (gitignored) holds approved prose entries.
   A composition function in `packages/ir` — `composeIR(spreadsheet, prose)` — returns the
   merged IR all consumers use. Consumers (e2e driver, Gate-1 corpus resolution,
   requirements-table, RTM export) switch from reading `extracted.json` directly to
   `composeIR`. `schema_version` bumps; zod-validate both layers at the boundary.
2. **Approval record schema** (every entry, all fields mandatory):
   `{ id (stable hash of natural key), kind (requirement|need|mode|modeTransition|interface|component|function), fields (kind-specific payload mirroring existing IR entity shapes), citation: { docId, docSha256, chunkId, sectionPath, quote (≤300 chars) }, approvedBy, approvedAt (ISO), candidateId, status: approved|superseded|suspect }`.
3. **Append-only:** entries are never edited in place; corrections append a superseding
   entry referencing the old `id` (`supersedes` field) — this IS the change journal for prose.
4. **Suspect flagging (the re-baseline primitive):** at composition time, if an approved
   entry's `citation.docSha256` no longer matches the current document hash in the ingest
   manifest, the entry composes with `status: suspect` and Gate 1 emits a *warning-severity*
   finding (`PROSE-suspect-source`) listing it. Suspect entries still compose (the model
   keeps working) but are visibly flagged until re-approved.
5. **Gate-1 extension (GATE-03 for prose):** the corpus resolution set
   (`packages/mcp-server/src/audit/corpus.ts`, `buildResolutionSet`) gains approved-prose
   ids — mirroring the `n2Interfaces` extension: test-driven (RED first), `git diff` shows
   only the kinds addition, fabrication protections untouched. A `provenanceSourceId`
   pointing at a *candidate* (unapproved) or unknown prose id remains a hard error.

**Acceptance (binary, runnable):**
- Re-running the extractor with an existing `prose-approved.json` leaves the prose layer
  byte-identical and `composeIR` output stable (test: run twice, diff).
- A model element citing an approved prose id passes Gate 1; citing an unapproved
  candidate id fails with `GATE03-unresolvable-provenance` (tests for both).
- Tampering the source doc hash in the manifest flips composed entries to `suspect` and
  produces the `PROSE-suspect-source` warning (test).
- Appending a superseding entry hides the old one from composition but keeps it in the file.

---

## F2 — PDF ingestion (SEPAL port + extraction + approval queue)

### F2a. Port the SEPAL modules (do NOT depend on the package)
Port from `~/Engineering/projects/se-process-platform/packages/engine/src/corpus/` into
`packages/ir/src/prose/` (or a new `packages/prose-ingest/` if cleaner — planner's call,
one package, no new workspace sprawl). Modules + why:
- `parsers/pdf.ts` (134 lines; deps `unpdf` primary + `mupdf` fallback) — page-text extraction
- `banner.ts` (210 lines, dep-free) — **CUI/FOUO/ITAR/PROPRIETARY refusal BEFORE any LLM
  call or chunking**. This is a security boundary, port verbatim, do not weaken patterns.
- chunk-ID core from `chunker.ts` — IDs hash (docSha256, position, normalized context),
  **never the chunk text** — citations survive document edits. Keep this property; it is
  load-bearing for F1.
- `requirement-chunker.ts` — deterministic prose candidate detection: alphanumeric req-ID
  pattern, section-numbered shall pattern, `Requirement N:` label pattern, plus
  `Traces to:` / `Verified by:` extraction.
- `section-map.ts` — heading tree with deterministic SHA section ids (citation display).

Every ported file gets a header: source path + SEPAL commit SHA at port time (run
`git -C ~/Engineering/projects/se-process-platform rev-parse HEAD`) so drift is detectable.
Add `unpdf` + `mupdf` as dependencies of the owning package only. Port the corresponding
SEPAL tests where they exist (`packages/engine/tests/corpus/`).

### F2b. Extraction pipeline (deterministic-first, LLM-second)
New script `scripts/ingest-prose.ts`:
1. For each PDF in `examples/angars/corpus/specs/` (4 files): parse → **banner check
   (refuse on hit)** → doc hash → section map → chunks.
2. **Deterministic pass:** requirement-chunker emits candidates for anything matching its
   patterns (requirements with ids, shall statements).
3. **LLM pass (Claude API, model: claude-fable-5 or current-best per `claude-api`
   skill guidance):** receives chunks + section context; two jobs only:
   (a) *structure* deterministic candidates into IR-shaped fields (name, statement,
   parent-need hints, verify-method hints); (b) *propose* candidates the regexes cannot
   catch — **operational modes + mode transitions** (for F3), interfaces, actors — each
   with mandatory `chunkId` citations. A proposal without a resolvable chunkId is dropped
   at emission with a logged count (no silent drops).
4. Output: `examples/angars/model/prose-candidates.json` (gitignored) — same record shape
   as F1's approval schema minus approver fields, plus
   `source: deterministic|llm` and `confidence`.
5. Idempotent: re-running against unchanged docs produces identical candidate ids; already
   approved/rejected candidateIds are skipped (the queue file tracks rejections too).

### F2c. Approval queue (human on the pipe)
New skill `packages/skills/skills/mbse-ingest.md`: presents pending candidates grouped by
document/section — each showing the **quote + [Section: …] + doc/page** — and the user
approves/rejects (batch by group allowed; AskUserQuestion-driven). Approve → append to
`prose-approved.json` (F1 schema, `approvedBy` from git user). Reject → recorded in the
candidates file (`status: rejected`, never re-proposed). The skill carries the standing
knowledge preamble (`packages/skills/skills/_shared/`). **No auto-approval path exists** —
this is the same human-gate doctrine as `mbse-edit`.

**Acceptance:**
- A doctored fixture PDF containing `CUI` in its header is refused before any chunk/LLM
  call (test with a generated fixture; do not commit anything resembling real markings
  beyond the marker word).
- Deterministic candidates from a synthetic fixture PDF match expected counts (fixture with
  known 5 shall-statements → exactly 5 deterministic candidates; count-assert).
- Every emitted candidate has a chunkId that resolves into the chunk store; the
  dropped-for-no-citation count is printed.
- End-to-end on ANGARS: `pnpm tsx scripts/ingest-prose.ts` runs over the 4 corpus PDFs and
  produces a non-empty candidates file (counts logged); after a human approval session,
  approved entries flow through `composeIR` → a model element built on one passes Gate 1.

## F3 — Corpus-grounded state machine (CONOPS modes)
Once F2 lands mode/modeTransition candidates and the user approves them:
- IR gains `modes[]` + `modeTransitions[]` (via the prose layer; spreadsheet layer unchanged).
- `mbse-build` (skill text) + the e2e driver gain a state pillar: `state def` per mode
  context with transitions (`transition first A then B` + `accept`/`if` where the approved
  entries carry triggers/guards), R3-correct, provenance-stamped to approved-prose ids.
- The e2e view spec gains a `state` entry; the DeciSym renderer already supports state
  views (entry/do/exit, trigger/[guard] labels, cyclic back-edges) — no renderer work
  expected.
**Acceptance:** the state view renders from corpus-approved modes ONLY (zero hand-invented
states — every state/transition's provenanceSourceId resolves to an approved prose entry);
Gate 1 clean; Gate 2 zero errors; PNG visually verified (named frame, ≥3 states, labeled
transitions, initial node).

## F4 — Multi-corpus mini-proof
A second, tiny, **original fictional** corpus (different domain — e.g., a building HVAC
controller or campus shuttle; NOT aerospace, NOT derived from any real program) at
`examples/mini-corpus/`: one workbook (≈10 requirements, 4 needs, 4 components, 6 functions,
small N2 sheet) + one 2-page PDF with 3 shall-statements and 2 operational modes. Because it
is authored as *the corpus itself*, hand-writing it does not violate the no-fabrication rule
— it IS ground truth. It may be committed (fictional, no proprietary echo).
**Acceptance:** the UNMODIFIED pipeline (extractor config aside — corpus paths/expected
counts may be parameterized, parsing logic may not) runs corpus → IR → model → Gate 1 clean
→ Gate 2 zero errors → ≥3 views rendered. Any code change required beyond
paths/expected-count config is a corpus-generic bug — fix the pipeline, document the fix.

## F5 — RTM-to-Excel export
`scripts/rtm-export.ts`: composeIR + model store → one `.xlsx` (SheetJS write API, the
vendored 0.20.3): sheet 1 RTM (Req ID | Name | Statement | Parent Need(s) | Satisfied By |
Verify Method | Verified By | Status flags), sheet 2 coverage matrix (from the Gate-1
coverage artifact), sheet 3 gaps (unsatisfied / unverified / orphans / suspect-prose).
Output to `examples/angars/model/reports/` (gitignored). Runs against the mini-corpus too
(committed output allowed there).
**Acceptance:** generated workbook re-opens via SheetJS with expected sheet names + row
counts matching IR counts (round-trip test); gaps sheet row count matches Gate-1 findings.

## F6 — Requirement-quality linting (the wedge)
`scripts/lint-requirements.ts` + skill `mbse-lint.md`: over composed-IR requirement
statements, two passes:
- **Deterministic:** weak-word list (INCOSE Guide for Writing Requirements–style: e.g.
  "appropriate", "adequate", "as required", "minimize", "user-friendly", "etc.", "and/or",
  "support", "TBD/TBR"), compound-requirement heuristic (multiple "shall"), missing
  tolerance on numeric claims, passive-voice heuristic. Each rule has an id (`LINT-xx`),
  severity, and a findings count test against a fixture set.
- **LLM (optional flag `--llm`):** verifiability/ambiguity judgment per statement, output
  constrained to {ruleId, reqId, finding, suggestedRewrite}; suggestions are REPORT-ONLY —
  the linter never mutates anything.
Findings emit through the same severity-graded report shape as Gate 1 (reuse the type).
Output: `reports/requirements-lint.md` (gitignored for ANGARS; committed for mini-corpus).
**Acceptance:** a fixture file with 6 known defects yields exactly the 6 expected
deterministic findings (ids asserted); ANGARS run completes and reports counts.

## F7 — DEMO.md + guarantees one-pager
`docs/DEMO.md` (committed; procedural only — zero corpus text, the shall-grep check):
- 10-minute walkthrough, copy-paste commands: extract → ingest-prose (mini-corpus, so the
  audience sees the approval queue live) → build/e2e → gates → renders → table → RTM →
  lint → Cameo handoff pointer. Uses the **mini-corpus** so the demo is fully committable
  and reproducible by anyone cloning the repo.
- `docs/GUARANTEES.md`: one page, plain language: what Gate 1 mechanically prevents
  (fabrication, dangling trace, def-operands, orphans), what Gate 2 proves, what Gate 3 is,
  what the human approves (prose candidates, edits), what the tool will NOT do (autonomous
  edits, CUI ingestion, model→corpus write-back). This is the leadership-pitch artifact.
**Acceptance:** a fresh-clone dry run of DEMO.md's commands (scripted check where possible)
succeeds end-to-end on the mini-corpus; both docs pass the no-corpus-text grep.

---

## Sequencing & dependencies (planner: derive phases from this)

```
F0 (user; parallel-safe, blocks milestone CLOSURE not work-start)
F1 → F2a → F2b → F2c → F3
F4 needs F1+F2 (mini-corpus exercises prose path) — but its corpus authoring can start anytime
F5, F6 need F1 (composeIR); independent of F2 otherwise; parallel-safe with each other
F7 last (documents everything; needs F2c queue + F4 mini-corpus live)
```
Hard invariant (same doctrine as the last milestone): **the approval queue (F2c) must not
exist before the approved-prose layer (F1) and its Gate-1 extension are real and tested** —
an approval path into a gate that doesn't validate it reproduces the original fabrication
failure shape.

## Risks the planner must carry into plans
1. **SEPAL requirement-chunker prose depth unverified** — confirmed for patterns, not
   exercised on these PDFs. Mitigation: fixture-test the three patterns first; if ANGARS
   PDFs yield zero deterministic candidates, the LLM pass carries more weight — flag, don't
   silently accept.
2. **unpdf/mupdf on these specific PDFs** — extraction quality unknown (scanned pages would
   come back empty; OCR is OUT of scope). Early task: dump page text of all 4 PDFs, assert
   non-empty, eyeball one page per doc.
3. **LLM extraction cost/latency** — 4 PDFs is small; full-context per section, no RAG.
   If a doc exceeds context, chunk by section-map top-level sections. Never summarize-then-
   extract (lossy).
4. **Mode candidates may be sparse/ambiguous in the CONOPS** — if the user rejects most
   mode candidates, F3 may yield a thin state machine. That is a *correct* outcome (honest);
   do not pad. Record what the corpus supports.
5. **F0 may surface emission bugs** — if so, Gate-1 rule additions take priority over all
   F2+ work (doctrine).

## Traceability skeleton (for ROADMAP mapping)
PI-01 F1 layer+composition · PI-02 F1 suspect flag · PI-03 F1 GATE-03 extension ·
PI-04 F2a port+banner · PI-05 F2b deterministic pass · PI-06 F2b LLM pass+citations ·
PI-07 F2c queue skill · PI-08 F3 state pillar · PI-09 F4 mini-corpus ·
PI-10 F5 RTM export · PI-11 F6 linting · PI-12 F7 demo docs · PI-00 F0 Gate-3 record.
