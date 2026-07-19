# Corpus Weaver — Entity Resolution, Cross-Document Linkage, and Gap-Driven Passes

**Status:** approved design, not started. **Date:** 2026-07-14. **Owner:** Joe.
**Executors:** subagents, one phase per branch, against the done-criteria in §8.
**Baseline:** `main` @ `378f12b` (verbatim citations C6, six-format parsers, BM25 link
evidence, persisted chunk store, seeded harness 8/8).

---

## 1. Problem and goal

foundry's back half (typed store → serializer → validator → gates → renders → Cameo) is
solid. Its front half assumes structure the real world won't give it: linkage-candidate
enumeration derives pairs from structure already in the model (ANGARS N2 triples, L2/L3
owner groups). A real corpus — PowerPoints, Word docs, emails, PDFs — states no allocation
in any single document. The same component appears as "FCM", "Flight Ctrl Module", and
"flight control processing unit" in three files, and the link evidence spans all three.

Goal: make foundry able to **weave a system model out of a messy multi-document corpus over
multiple passes**, by adding the three missing layers:

- **W1 — Mentions & entity resolution (the hub).** Cluster cross-document mentions into
  canonical entities with alias sets and full citation provenance. Without unified
  endpoints, cross-document links are structurally impossible.
- **W2 — Cross-document candidate enumeration (spokes & chains).** Propose relation
  candidates from the mention graph (co-occurrence) and from transitive composition of
  already-accepted relations (chains) — feeding the *existing* premise-contract → debate →
  human-gate machinery unchanged.
- **W3 — Gap-driven passes (the loop).** Turn Gate-1 completeness warnings (`GATE02-orphan`,
  `-unsatisfied`, `-uncovered-need`) into the targeted retrieval + proposal queue for the
  next pass, with a persisted pass record and a convergence check.

Non-goals (locked): no embeddings / vector store (the BM25 seam stays; swapping it later is
a one-module change); no auto-approve of anything, anywhere (merges included); no CUI/banner
detection; no serializer or Cameo-semantics changes; ingest stays an exhaustive
every-chunk-exactly-once sweep (C5 and its enforcing grep-test are untouchable).

## 2. Design invariants (extend, never weaken)

The repo's standing invariants apply to every phase. New code must thread through them:

- **C4/C5/C6 (prose ingest):** every emitted record cites a resolvable chunk; every chunk
  hits the provider exactly once; every quote verbatim-resolves into its cited chunk
  (matcher: `packages/model/src/verbatim.ts`). Mentions (§3) get the same treatment: a
  mention with an unresolvable chunkId or non-verbatim quote is dropped and counted.
- **No auto-approve:** the only sanctioned write paths for dispositions are the existing
  writer helpers + review-ui server (`DEFINING_MODULES` in the `sourceScanRatchet` call in
  `packages/candidates/src/__tests__/no-auto-approve.test.ts`). Entity-merge writers (§3)
  are added to the *scanned token list* and their defining modules to the shrink-only
  allowlist. A fuzzy or LLM-suggested merge is a **proposal**; only a human disposition
  makes it real.
- **Premise contract:** every LLM-proposed link cites ≥1 resolvable premise id
  (`validatePremises`, `INFER-unpremised` error rule). Chain candidates (§4) cite the ids
  of their constituent relations plus evidence chunk ids.
- **Determinism:** same corpus + same approvals → byte-identical mentions, auto-clusters,
  enumeration order, and context bundles. All ids are content-addressed via
  `stableId`-style hashing. LLM calls are the only nondeterminism, and they only ever
  produce *proposals*.
- **Every gate proven to fail:** each new gate rule ships in the same PR as its planted
  defect in `examples/angars/pipeline/seeded-defects.ts` (via
  `@sysml-foundry/invariants` `seededDefectHarness`, `soleError: true` for error rules).
  No silent caps: any bounding (top-k, per-family caps) logs what was dropped.

## 3. W1 — Mentions and entity resolution

### Mention substrate

A **mention** is one naming event of a model-relevant thing in one chunk.

```ts
interface MentionRecord {
  mentionId: string;      // stableId("mention", `${docSha256}:${chunkId}:${normSurface}:${kindHint}`)
  surfaceForm: string;    // as written in the document
  kindHint: MentionKind;  // component | function | requirement | mode | interface | flow | unknown
  citation: { docId, docSha256, chunkId, sectionPath, quote };  // quote verbatim-checked (C6)
  confidence: number;
}
```

Source: extend the propose-response schema in
`packages/candidates/src/prose/llm-provider.ts` with an optional `mentions[]` array —
harvested **in the same provider call** as candidate proposals (C5: still exactly one call
per chunk). Additionally, every candidate proposal's own name field is derived into a
mention deterministically, so mentions ⊇ candidates even with a provider that returns no
explicit mentions. Persisted as `<out>/mentions.json` with a self-describing envelope
(`sysml-foundry/mention-store@1`), mirroring `chunks.json`; malformed stores throw.

### Entity resolution — three bands, one human gate

1. **Deterministic auto-cluster (no judgment, no approval needed):** mentions whose
   `normSurface` (NFKC, casefold, whitespace/punct collapse — reuse the verbatim
   normalizer's building blocks) and compatible `kindHint` match exactly are one cluster.
   Pure function; tested for determinism.
2. **Suggested merges (judgment → proposal):** acronym/expansion heuristics (initial-letter
   match: "FCM" ↔ "Flight Control Module"), token-overlap scoring (dependency-free, e.g.
   Jaccard over name tokens — reuse `nameTokens` from the relevance filter), and — for the
   mid-band — an LLM adjudication reusing the **advocate/challenger debate** verbatim
   (advocate argues same-entity, challenger argues distinct; deterministic verdict
   thresholds as in `debate.ts`). Output: `EntityMergeProposal` records with the evidence
   quotes from both sides. These enter the **existing review queue** (review-ui + a
   `/mbse-approve`-equivalent path) as a new candidate kind `entity-merge`.
3. **Human disposition:** approve → merge is recorded; reject → recorded with the
   content-addressed pair key so the same suggestion is never re-asked (mirror the
   inferred-rejection triple pattern).

### Entity records

```ts
interface EntityRecord {
  entityId: string;        // stableId("entity", `${kind}:${canonicalNormSurface}`) of the FIRST cluster seed
  kind: MentionKind;
  canonicalName: string;   // human-editable at approval time; default = most frequent surface form
  aliases: string[];       // all merged surface forms
  mentionIds: string[];    // full provenance — every citation reachable
  mergeDispositions: string[]; // ids of the approval records that built this entity
}
```

Identity is minted from the first deterministic cluster seed and is **stable under growth**
(merges append aliases/mentions; they never re-derive `entityId` from the member set).
Persisted as `<out>/entities.json` (`sysml-foundry/entity-store@1`). When an entity is
promoted into the model (existing approval flow), the model element records `entityId` in
provenance so model↔corpus identity survives.

### New gate rules (Gate 1, `packages/gates`)

- `ENT-unapproved-merge` (**error**): an entity's alias set contains a surface form not
  reachable by deterministic normalization from its seeds and not covered by a merge
  disposition record.
- `ENT-dangling-mention-ref` (**error**): an entity references a `mentionId` absent from
  the mention store (store available) — the C4 discipline applied to entities.
- `ENT-duplicate-suspect` (warning): two entities of the same kind whose canonical names
  auto-cluster-match — the resolver missed a deterministic merge (should be impossible;
  the warning is the tripwire).
- Store-unavailable degrade: like `PROSE-unverbatim-quote-unavailable` — an explicit
  warning, never a vacuous pass.

## 4. W2 — Cross-document candidate enumeration

New enumerators in `packages/candidates/src/inference/candidate-generator.ts`, alongside
(not replacing) the structural ones. Both consume the entity store, so endpoints are
canonical entities, not raw names.

- **Co-occurrence (spokes):** for each declared relation family, enumerate (entityA,
  entityB) pairs whose mentions co-occur in ≥ `minCooccur` chunks (default 1) or within the
  same `sectionPath` prefix. Typed by the existing type gate; per-family caps via
  `resolveFamilyCap`, with dropped-count logging (no silent caps). Offered facts for the
  pair include the co-occurring chunks (citable premise ids — resolvable via
  `extraResolvableIds`, already wired for BM25 chunks).
- **Chains:** for an explicit, small **composition table** (initial entries:
  `allocation ∘ containment → allocation`, `flowTyping ∘ interfaceAggregation → flowTyping`
  — extend only via the table, never inferred), compose pairs of already-accepted relations
  (corpus-backed or human-approved inferred; **never** pending proposals) sharing a middle
  entity. Candidate premises = the two constituent relation ids + their evidence chunks.
  2-hop only. A chain candidate whose family composition is not in the table must be
  rejected by the type gate (tested).

Both flows then reuse, unchanged: type gate → premise contract → BM25 evidence bundle →
advocate/challenger debate → human review queue. No new approval surface.

## 5. W3 — Gap-driven passes

CLI first (`pnpm weave`), MCP tool later (§8 Phase W5).

One pass = **audit → queue → propose → (human reviews) → recompose → re-audit → record**:

1. Run `audit()` on the composed IR. Map completeness findings to query strategies via an
   explicit table:
   `GATE02-unsatisfied(reqR)` → BM25 query from R's name+text → satisfy-family candidates
   scoped to entities retrieved; `GATE02-orphan(elemE)` → allocation-family candidates for
   E; `GATE02-uncovered-need(needN)` → derive-family. The table is data, in one module,
   unit-tested.
2. Run a **bounded** targeted inference pass over only those queries (budget caps logged).
3. Proposals land in the normal review queue. The pass then **stops for the human** —
   `weave` never waits on or writes dispositions (no-auto-approve).
4. `weave --close-pass` recomposes, re-audits, and writes
   `<out>/passes/pass-NNN.json` (`sysml-foundry/pass-record@1`):
   `{auditBefore, queries, candidatesProposed, dispositionsApplied, auditAfter, warningsDelta}`.
5. **Convergence discipline:** hard gate — a closed pass must end with **zero
   error-severity findings** and may never end with more errors than it began. Soft signal —
   the completeness-warning delta is reported per rule id, not gated (legitimately growing
   models create new orphans; the record makes the trend inspectable instead of pretending
   monotonicity).

## 6. Proof-of-recall mini-corpus (the eval)

The seeded-defect harness proves gates catch planted *defects*; the weaver needs the dual —
proof it *finds* planted *facts*. Ship `examples/weave-mini/`:

- A synthetic 3-document corpus (one `.md`, one `.docx`, one `.xlsx` — exercising the new
  parsers) describing a small system (~8 components, ~6 functions, ~10 requirements) with a
  committed **answer key** (`answer-key.json`): K entities with cross-document aliases
  (≥1 acronym pair), L cross-document links stated in no single document, ≥1 valid 2-hop
  chain, and **≥1 trap**: two distinct entities sharing a surface form (must NOT merge).
- **Deterministic layers asserted exactly in CI:** parsing, chunking, mention derivation
  from fixture provider responses, auto-clustering, co-occurrence/chain enumeration — all
  against the key, via mock providers (fixture-recorded responses), zero API key.
- **Live eval, not CI:** `pnpm weave:eval` runs the real provider and computes
  precision/recall for entity resolution and link discovery against the key, printing a
  scored table. Requires `ANTHROPIC_API_KEY`; results are a report, not a gate.

## 7. Storage & layout summary

```
packages/candidates/src/
  mentions/        # mention derivation + store (read/write/validate)   — W1
  entities/        # normalizer, auto-cluster, merge suggesters, store  — W1
  inference/       # + cooccurrence.ts, chains.ts, composition-table.ts — W2
  weave/           # gap-queue table, pass runner, pass records         — W3
packages/gates/src/entities.ts   # ENT-* rules                          — W1
examples/weave-mini/             # corpus + answer key + fixtures       — W4
scripts/weave.ts                 # CLI (pass / close-pass / eval)       — W3
```

Envelope files (all self-describing, all throw on malformation): `mentions.json`,
`entities.json`, `passes/pass-NNN.json` — siblings of the existing `chunks.json`.

## 8. Phasing with validation criteria

Each phase: own branch (`feat/weaver-w<N>`), lands independently, full repo gate green
(`pnpm build && pnpm test && ANTHROPIC_API_KEY="" pnpm demo && pnpm demo:seeded &&
pnpm check:skills && pnpm smoke:mcp`), and a `PHASE-W<N>-VERIFICATION` record commit per
repo convention. Model hints for executors: W1/W2 are design-heavy (opus); W0/W4 are
well-scoped (sonnet); W3/W5 medium (either).

### Phase W0 — Mention substrate
Deliver: `mentions/` module; provider-schema `mentions[]` extension; `mentions.json`.
- Mention derivation from proposals is pure + deterministic (same input → same
  `mentionId`s, asserted twice-run byte-identical).
- C5 untouched: the ingest grep-test still passes; provider called once per chunk (counter
  test).
- Mentions with unresolvable chunkIds / non-verbatim quotes dropped + counted
  (`droppedUnverbatimMentions`); fail-able positive control included.
- `mentions.json` round-trips (write → load → deep-equal); malformed store throws (test).

### Phase W1 — Entities: auto-cluster + merge proposals + human gate + ENT rules
Deliver: `entities/`, `entities.json`, review-ui `entity-merge` queue, gates `ENT-*` rules,
ratchet extension.
- Auto-cluster: deterministic, normalization-exact only; the answer-key trap pair (same
  surface, different kind context) does NOT auto-merge (test with a minimal inline
  fixture; full trap corpus arrives in W4).
- Merge proposals: acronym + token-overlap suggesters unit-tested with fail-able controls;
  debate adjudication behind the existing `InferenceProvider` seam (mock-tested).
- Human gate: UI approval and helper approval write field-equal merge dispositions (shared
  zod schema; equivalence test mirroring the Phase-5 review-ui test). A rejected pair is
  never re-proposed (content-addressed pair key test).
- Ratchet: `appendEntityMerge`-style writers added to the `sourceScanRatchet` token list;
  allowlist grows by exactly the defining module + review-ui server; planting a rogue call
  site in another package fails the test (prove it, then revert — the live positive
  control).
- Gates: `ENT-unapproved-merge`, `ENT-dangling-mention-ref` (errors) + degrade warnings;
  seeded harness grows to **10/10** with planted defects (i) an alias without a merge
  disposition and (j) a dangling mentionId — both `soleError: true`, clean control CLEAN.

### Phase W2 — Cross-document enumerators
Deliver: `cooccurrence.ts`, `chains.ts`, `composition-table.ts`, wired into
`runInferenceEngine` behind an `entityStore` option (absent → exactly today's behavior).
- Co-occurrence enumeration deterministic (order test); caps logged with dropped counts
  (assert the log line, not just the cap).
- Chain candidates only from accepted relations; a pending-proposal input produces zero
  chain candidates (test). Illegal composition (not in table) rejected by type gate (test
  with a fail-able control: a legal composition passes).
- Premises of both candidate kinds resolve end-to-end (mock provider → queued, not
  `dropped_unpremised`); a candidate citing a fabricated premise id IS dropped (control).
- Existing ANGARS demo unchanged: `pnpm demo` and `demo:seeded` byte-identical results.

### Phase W3 — Gap-driven pass loop
Deliver: `weave/` + `scripts/weave.ts` (`pnpm weave`, `weave --close-pass`).
- Finding→query table covers `GATE02-unsatisfied`, `-orphan`, `-uncovered-need`; unknown
  finding ids are reported, not silently skipped (test).
- A pass on a fixture with a known gap proposes ≥1 candidate targeting that gap's element
  id (mock provider), and `weave` exits WITHOUT writing any disposition (no-auto-approve
  scan still green).
- `pass-NNN.json` written with all six fields; `--close-pass` hard-fails (non-zero exit) if
  error findings increased; warnings delta reported per rule id.
- Runbook section added to README (§ Tier 2) describing the pass loop honestly.

### Phase W4 — Weave-mini corpus + proof-of-recall eval
Deliver: `examples/weave-mini/` corpus, answer key, fixture provider responses,
`pnpm weave:eval`.
- CI (no key): deterministic layers score **100%** against the answer key — every aliased
  entity auto-clusters or is proposed for merge; the trap pair is NOT merged; the planted
  2-hop chain is enumerated; assertions on specific entity/link ids, not counts.
- Live eval (key required, not CI): prints precision/recall per layer; documented in the
  example's README with the honest note that scores are provider-dependent.
- All three file formats exercised through the real parsers (no pre-extracted text
  fixtures for the corpus itself).

### Phase W5 — MCP + skill surface (last, thin)
Deliver: `weave_pass` / `close_pass` MCP tools + `/mbse-weave` skill page.
- Tools registered; `registration.test.ts` count updated; `pnpm check:skills` green (skill
  references only registered tools).
- Session lifecycle: weave maps into the existing stage machine (decide: `build`/`trace`
  stages revisited, or a new `enrich` stage — document the choice in the tool docstring);
  `validate_model` stage-advance semantics unchanged.
- MCP smoke (`pnpm smoke:mcp`) exercises one weave tool call against a fixture project.

### Parked (documented, not built)
- Embedding/hybrid retrieval behind the `Bm25Index` seam (revisit when lexical misses are
  *measured* by the W4 live eval, not before).
- Mention extraction from diagrams/images in PPT/PDF (needs OCR/vision; out of scope).
- Email ingestion (`.eml`/`.msg` parser) — same parser interface, add when a real corpus
  demands it.

## 9. Open items (deliberate, non-blocking)

- Canonical-name editing UX in review-ui at merge-approval time (default: most frequent
  surface form; a text input is enough).
- Whether `EntityRecord.kind` should admit human reclassification post-approval (leaning
  yes, as a new disposition kind — decide in W1 implementation).
- Composition-table growth policy: additions require a paired type-gate test per entry
  (make the test structure enforce this — a table entry without a test should fail a
  registry-completeness check, mirroring the query-keys pattern).
