# weave-mini — proof-of-recall corpus (W4)

A synthetic 3-document corpus proving the weaver's deterministic layers
*find* planted facts — the dual of `examples/angars/pipeline/seeded-defects.ts`,
which proves the gates *catch* planted defects. See
`docs/superpowers/specs/2026-07-14-corpus-weaver-design.md` §6 and §8 (Phase W4).

## The corpus

A small "Cargo Handling System" spec, split across three real file formats so
all three prose-ingest parsers are exercised end to end (no pre-extracted
text fixtures for the corpus itself):

| File | Format | Content |
|---|---|---|
| `corpus/system-overview.md` | Markdown | 4 components, 3 functions, 1 mode, 3 requirements |
| `corpus/subsystem-spec.docx` | Word (OOXML zip) | 4 components (+1 acronym alias), 1 mode, 3 functions, 4 requirements |
| `corpus/requirements-matrix.xlsx` | Excel (2 sheets) | 2 components (cross-doc aliases), 3 requirements, 1 need |

`subsystem-spec.docx` / `requirements-matrix.xlsx` are committed binaries,
(re)generated from `packages/candidates/fixtures/gen-weave-mini-corpus.ts`
(`tsx fixtures/gen-weave-mini-corpus.ts` from `packages/candidates/`) — the
same technique `packages/candidates/fixtures/gen-fixtures.ts` uses for the
parser test fixtures.

Each document is deliberately small enough (given the eval's chunk-size
setting) to parse+chunk into exactly **one** chunk, keyed to a nested
`sectionPath` ("Cargo Handling", "Cargo Handling/Safety", "Cargo
Handling/Requirements") that shares a common prefix — this is what gives
cross-document co-occurrence (§4, W2) a real signal to key off, entirely
independent of any shared chunk.

## `answer-key.json`

Pinned, human-reviewed ids (computed once via
`packages/candidates/fixtures/wm-compute-answer-key.ts`, never re-derived by
the test it feeds — see that script's own docstring to regenerate):

- **`entities`** — K=4 cross-document entity groups:
  - 3 **exact-spelling** cross-document aliases (`Cargo Handling Controller`,
    `Position Sensor Array`, `Fault Logger`) — identical spelling in two
    documents, so `autoCluster` merges them into ONE entity with no approval
    needed.
  - 1 **acronym pair** (`CHC` ↔ `Cargo Handling Controller`) — different
    spelling, so `autoCluster` keeps them as two entities, and the band-2
    acronym suggester (`suggestMerges`) is required to propose the merge.
- **`trap`** — two entities sharing the literal surface form `"Interlock"`,
  one a `mode` (system-overview.md) and one a `component`
  (subsystem-spec.docx). Must stay separate under `autoCluster` (different
  `kindHint`) and must NEVER be proposed for merge (`suggestMerges` requires
  kind-compatible endpoints).
- **`crossDocumentLinks`** — L=3 co-occurrence candidates whose *only* signal
  is a cross-document `sectionPath` prefix match (`cooccurKind === "section"`)
  — never a shared chunk, since chunk ids are document-local. No single
  document states either link on its own.
- **`chain`** — one valid 2-hop `allocation ∘ containment → allocation` chain,
  composed from two hand-authored, already-*accepted* relations (representing
  corpus-backed facts for this eval).

## Running the eval

**CI (zero API key, the gate):**

```
pnpm --filter @sysml-bridge/candidates test -- weave-mini-eval
```

(also runs as part of `pnpm test` / `pnpm -r test`). This is
`packages/candidates/src/__tests__/weave-mini-eval.test.ts` — it runs the
REAL parsers + chunker + `runIngestPipeline` (C4/C5/C6 gates all enforced)
against a **recorded fixture LLM provider**
(`fixture-responses.json` — every `quote` is verbatim-checked against the
real parsed corpus text, never pre-extracted or hand-waved), then the real
`autoCluster` / `suggestMerges` / `enumerateCooccurrence` / `enumerateChains`,
and asserts against `answer-key.json` by **specific pinned ids** — never
counts.

**Live eval (requires a provider key, a report, not a gate):**

Put the key in a project-local `.env` (copy `.env.example` → `.env`; it's gitignored
and auto-loaded by `pnpm weave:eval`):

```
cp .env.example .env    # then edit: set OPENROUTER_API_KEY=sk-or-...
pnpm weave:eval
```

…or pass it inline (OpenRouter/GLM preferred, model defaults to z-ai/glm-5.2):

```
OPENROUTER_API_KEY=sk-or-... pnpm weave:eval
OPENROUTER_API_KEY=sk-or-... OPENROUTER_MODEL=z-ai/glm-5.2 pnpm weave:eval
ANTHROPIC_API_KEY=sk-ant-... pnpm weave:eval   # or Anthropic
```

Runs the same corpus and downstream pipeline against a REAL provider —
`OpenRouterLlmProvider` (GLM via OpenRouter) when `OPENROUTER_API_KEY` is set,
else `AnthropicLlmProvider` — then fuzzy-matches (not exact-id — a live model
will not reproduce the fixture's exact wording) the result against
`answer-key.json` and prints a scored recall/precision table for entity
resolution, link discovery, and merge-suggestion precision.

**Honest note:** live scores are provider-dependent — model version, prompt
phrasing, and sampling all move the numbers run to run. `pnpm weave:eval` is
a report to sanity-check the pipeline against a live model, not a
pass/fail gate; the CI gate is the deterministic-layer test above, which is
100% reproducible because it runs against a recorded, verbatim-verified
fixture provider instead of a live model.

Without either key, `pnpm weave:eval` prints a clean guard message pointing at
the CI test and exits non-zero — it never fabricates a score.
