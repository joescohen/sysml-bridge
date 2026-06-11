# SPEC — F8: Inference / Extrapolation Layer

**Date:** 2026-06-11
**Status:** Ready for planning (follow-on to `2026-06-10-prose-ingestion-and-adoption-pack.md`)
**Audience:** planning/execution agents. Self-contained; read the referenced knowledge-base
docs before planning. No conversation context required.

---

## 0. Problem and objective

Documents only state part of a system model. The connective tissue a Cameo model lives on —
which component *performs* a function (allocation), which operational mode a function is
active in, which interface item a flow carries, how parallel/conditional control flow joins —
is *implied* by the corpus but rarely *stated*. Today the pipeline has exactly two provenance
classes: **corpus-stated** (extracted, chunk-cited) and **model-asserted** (tiny allowlist).
Faced with the gap, generation either omits whole dimensions (no allocations, no
mode↔function links) or silently invents structure (the linear-activity incident:
successions asserted by natural-key order the corpus never stated).

**F8 adds the third class: `inferred`.** An LLM proposes cross-pillar links by reasoning over
the composed IR graph; every proposal carries **premises** (citations to the corpus/prose
facts it reasoned from), a rationale, and a confidence; proposals pass a type gate and an
adversarial validation stage; a **human approves** every link that enters the model; and
inferred content is **metadata-tagged end-to-end**, including inside the exported SysML v2
text visible in Cameo (user decision, locked).

The invariant that makes this safe (unchanged from the whole pipeline): **nothing enters the
model unattributed.** Inference is not looser provenance — it is a different, explicitly
labeled provenance whose attribution is a premise chain instead of a quote.

## Design rationale sources
- SEPAL link pipeline (`se-process-platform` @ b39b071): `link-discovery.ts`, `reranker.ts`,
  `debate-pass.ts` — the staged candidate→validate→verdict machine this design adapts.
  See the Adopt/Adapt/Skip table (§7).
- arXiv:2508.16181 (LLM-assisted semantic alignment in SysML v2) — direct precedent:
  LLM-proposed alignment/allocation links with confidence + rationale + metadata labels +
  per-stage human confirmation.
- arXiv:2412.08742 (KG completion w/ topological context) — per-relation-type passes,
  subgraph serialization; documented failure modes (popularity bias, type violations).
- arXiv:2510.12697 (NeurIPS 2025, multi-agent debate judges) — +4–5pp precision on
  *ambiguous* cases only → debate gated to the mid-confidence band.
- W3C PROV-DM — the provenance record vocabulary (`wasDerivedFrom`/`wasAttributedTo`/
  `wasAssociatedWith`) our fields map onto.
- SysML v2 metadata grammar — validated locally 2026-06-11: `metadata def` with attributes +
  `metadata X about Y { ... }` passes `pnpm validate:sysml` 0 errors (probe in §5). Cameo
  2026x caveat: zero-feature tagging metadata does not display; **feature-bearing metadata
  does** — ours carries features, so the standard form is correct.
- Repo rules: CLAUDE.md R1–R4; Gate-1 extension precedent (n2Interfaces commit `32701a0`,
  prose-ids extension); the layered-IR + approval-queue architecture (spec F1/F2, built).

## Locked decisions
1. **Metatag inferred content in the export** (user, 2026-06-11): every inferred element in
   emitted `.sysml` carries an `InferenceProvenance` metadata annotation (§5) — visible in
   Cameo. No "clean export" mode in v1.
2. **Third layer file**, not an extension of the prose layer: `inferred-approved.json`
   (append-only, same discipline as `prose-approved.json`). Prose layer = *extractions*
   (content found in documents); inference layer = *links* (relations reasoned over the
   composed graph). Different epistemic status, different files.
3. **Premise citation is mandatory and first-class.** The literature does not formalize
   this (research gap → our contribution): a proposal without resolvable premises is dropped
   before validation, mirroring the no-uncited-candidate rule (C4) of prose ingestion.
4. **Human approval for every inferred link.** The debate stage filters and annotates; it
   never auto-approves into the model. (Auto-*reject* below the floor band is allowed — a
   rejected proposal never reaches the queue, but is logged.)
5. **Rationale is audit-only** (SEPAL DEBAT-04 adopted): premises + verdict + confidence are
   queryable; advocate/challenger prose is stored for audit and MUST NOT surface in tool
   results, grounding blocks, or exported models.

---

## 1. Provenance classes (the contract)

| class | meaning | attribution | gate treatment | export tag |
|---|---|---|---|---|
| `corpus-stated` | extracted from a document | chunk citation (quote) | GATE-03 resolves id; uncited = hard error | none (default) |
| `inferred` | reasoned over the composed IR | premise chain (ids of corpus/prose/model facts) + rationale + confidence + approver | resolves via inferred-layer ids; unapproved proposal id = hard error; premise drift → suspect | `InferenceProvenance` metadata |
| `model-asserted` | explicit human assertion w/o derivation | approver only | allowlisted (existing) | `InferenceProvenance` w/ `provenanceClass = "asserted"` (upgrade existing allocate edges in v1) |

`suspect` remains an orthogonal *lifecycle state* (DOORS-style), not a class: a prose entry
whose source doc changed, or an inferred link whose **premise** entry changed/was superseded,
flips to suspect (still composes; warning finding). Premise-drift propagation is new in F8.

## 2. The inference pipeline (stage machine)

Adapted from SEPAL's `discovered → confirmed_reranker → confirmed_debate` with IR-native
stages. All stage records live in `inference-candidates.json` (gitignored; corpus-derived).

```
[generate] → typed-candidate (per relation type, over composed IR)
   ↓ type gate (deterministic, pre-LLM)         → rejected_type (reason-coded, logged)
   ↓ LLM proposal pass (premises + confidence)  → proposal | dropped_unpremised (logged)
   ↓ confidence bands:
       conf < 0.40           → auto_rejected (logged, never queued)
       0.40 ≤ conf < 0.70    → debate stage (advocate/challenger) → verdict annotates
       conf ≥ 0.70           → straight to queue
   ↓ human approval queue (mbse-infer skill)    → approved → inferred-approved.json
                                                → rejected (recorded, never re-proposed)
```

**Stage details:**

- **Candidate generation (deterministic):** enumerate typed pairs per relation family from
  the composed IR — v1 families: `allocation` (leaf ActionUsage × component PartUsage,
  scoped to the function's owning subsystem context), `modeMembership` (leaf function ×
  approved mode), `flowTyping` (N2 flow item × interface candidate), `controlJoin`
  (activity successions/branches beyond what prose extraction stated). Enumeration is
  bounded by graph locality (1-hop neighborhoods), not all-pairs.
- **Type gate (deterministic, pre-LLM):** a static type-compatibility table keyed by
  relation family (e.g. allocation: source ∈ ActionUsage-kinds, target ∈ PartUsage-kinds —
  the R4 lesson generalized). Ill-typed candidates are rejected with a structured reason
  code (`rejected_type:<rule>`), never silently. Zero LLM cost spent on them.
- **LLM proposal pass:** per-relation-family prompts (not one global pass). Input: the two
  elements + serialized 1-hop IR neighborhoods + the relevant corpus/prose quotes for both.
  Output (zod-enforced): `{ sourceId, targetId, relationFamily, premises: [ids…],
  rationale, confidence }`. **A proposal whose premises don't all resolve in the composed
  IR is dropped + counted** (`dropped_unpremised`) — the C4 rule applied to inference.
  Default model: Haiku (PROSE_INGEST_MODEL-style override).
- **Debate stage (mid-band only):** SEPAL's mechanics — advocate makes the strongest case
  FOR (sees premises + neighborhoods), challenger argues AGAINST (sees the advocate's
  summary), deterministic verdict: advocate ≥ 0.7 ∧ challenger < 0.5 → `confirmed`;
  challenger ≥ 0.7 → `auto_rejected`; else `uncertain` (queued with the uncertainty shown).
  Failure isolation: an errored pair → `uncertain`, loop continues. Sentinel row prevents
  re-running on an unchanged composed-IR hash; `INFER_FORCE=1` overrides. Pre-flight cost
  estimate logged before the first call; configurable budget cap aborts before spend.
- **Approval queue:** skill `mbse-infer` (sibling of `mbse-ingest`): groups proposals by
  relation family → subsystem; each shows source/target, premises (with their quotes),
  confidence, debate verdict; AskUserQuestion approve/reject (batch allowed). Approve →
  append to `inferred-approved.json` with `approvedBy` + `approvedAt`. NO auto-approve path.

## 3. The approved record (schema, zod in packages/ir)

```ts
InferredApprovedEntry {
  id: string                  // stableId of (relationFamily, sourceId, targetId)
  relationFamily: "allocation" | "modeMembership" | "flowTyping" | "controlJoin"
  sourceId: string            // composed-IR id
  targetId: string            // composed-IR id
  premises: string[]          // composed-IR ids (corpus/prose entries, model facts) — ≥1, all must resolve
  rationale: string           // audit-only (DEBAT-04 discipline)
  confidence: number          // proposal confidence at approval time
  debate?: { verdict: "confirmed"|"uncertain", advocate: number, challenger: number } // audit-only prose excluded
  inferenceRunId: string      // PROV wasAssociatedWith (run = Activity; prompt template version recorded on the run manifest)
  approvedBy: string          // PROV wasAttributedTo
  approvedAt: string          // ISO
  status: "approved" | "superseded" | "suspect"
  supersedes?: string
}
```
PROV-DM mapping (recorded, not serialized as PROV-O in v1): entry =`Entity`; run =`Activity`;
LLM + approver = `Agent`s; `premises` = `wasDerivedFrom`; `approvedBy` = `wasAttributedTo`.

`composeIR` gains the third layer: spreadsheet ∪ prose-approved ∪ inferred-approved.
**Suspect propagation (new):** at composition, if any premise id resolves to an entry that is
itself `suspect`/`superseded`, or no longer exists, the inferred entry composes as
`status: suspect` and Gate 1 emits warning `INFER-suspect-premise` listing entry + premise.

## 4. Gate-1 treatment

- Resolution set gains inferred-approved ids (the third narrow, test-driven extension —
  n2Interfaces → prose-ids → inferred-ids; same pattern, RED-first, diff = kinds add only).
- A model element/relationship whose `provenanceSourceId` is an inferred-layer id passes
  GATE-03 **only if** that entry is `approved` (not candidate/rejected/superseded).
- New rules: `INFER-suspect-premise` (warning, §3) and `INFER-unpremised` (error — an
  inferred-layer entry whose premises don't all resolve; defense-in-depth behind the
  pipeline drop).
- Fidelity report gains a 4th bucket: `inferred` — counted separately from
  drops/fabrications/near-matches, never netted against them.
- RTM/requirements-table/coverage outputs gain a provenance-class column.

## 5. Export metatagging (validated form)

Emitted once per model (definition) + once per inferred element (usage). This exact shape
passed `pnpm validate:sysml` with 0 errors on 2026-06-11:

```sysml
metadata def InferenceProvenance {
    attribute provenanceClass : ScalarValues::String;   // "inferred" | "asserted"
    attribute confidenceScore : ScalarValues::Real;
    attribute premiseRefs : ScalarValues::String;        // comma-joined premise ids
    attribute inferenceRunId : ScalarValues::String;
    attribute approvedBy : ScalarValues::String;
}
// per inferred element:
metadata InferenceProvenance about <elementName> {
    provenanceClass = "inferred";
    confidenceScore = 0.82;
    premiseRefs = "n2-1234, function-abcd";
    inferenceRunId = "run-…";
    approvedBy = "…";
}
```
Notes: feature-bearing metadata displays in Cameo 2026x (the known display bug affects only
zero-feature tagging metadata — does not apply). `premiseRefs` carries ids, never corpus
text (privacy: committed/exported artifacts stay quote-free). The serializer emits the
def once and an `about` block per tagged element; rationale is NEVER exported (audit-only).
Renderer (DeciSym): out of scope to render the tag in v1; do not let metadata blocks break
parsing (parser may skip them — verify, add a skip arm if needed).

## 6. Acceptance criteria (binary)

- **A1 (type gate):** an ill-typed candidate (e.g. allocation Def→Def, or requirement→
  component allocation) is rejected pre-LLM with a structured reason code; test enumerates
  one violation per family.
- **A2 (no unpremised proposal):** with a mock provider returning proposals with
  unresolvable premise ids, emitted-unpremised == 0 and the dropped count is logged (test).
- **A3 (band routing):** proposals at conf .3/.5/.9 route to auto_rejected/debate/queue
  respectively (test with mock provider).
- **A4 (debate verdict determinism):** the SEPAL threshold logic reproduces
  confirmed/rejected/uncertain on fixture confidences (unit test).
- **A5 (approval round-trip):** candidate → approve → `inferred-approved.json` entry
  (schema-valid) → composeIR exposes it → a model element carrying its id passes Gate 1;
  the same id pre-approval fails with GATE03 (test pair).
- **A6 (suspect propagation):** superseding a premise prose entry flips the dependent
  inferred entry to suspect at composition + `INFER-suspect-premise` warning (test).
- **A7 (metatag export):** export of a model containing one approved inferred allocation
  emits the `InferenceProvenance` def + `about` block; `pnpm validate:sysml` = 0 errors;
  the rendered/committed artifact contains no rationale text and no corpus quotes (grep).
- **A8 (e2e on ANGARS):** one full run: generate → gate → propose (live or mock per key
  availability — state which honestly) → queue fixture-approve ≥3 allocation links →
  rebuild → Gate1 0 errors → Gate2 0 errors; RTM shows the provenance column; fidelity
  shows the 4th bucket.
- **A9 (no regression):** the 498-test suite + prose-ingest pipeline unaffected when
  `inferred-approved.json` is absent (backward-compat path byte-identical).

## 7. SEPAL Adopt / Adapt / Skip (from the 2026-06-11 deep-read)

| Mechanism | Decision | Note |
|---|---|---|
| Stage machine + deterministic verdict thresholds | **Adopt** | §2 bands |
| Advocate/challenger debate (challenger sees advocate summary) | **Adopt** | mid-band only |
| Audit-only rationale (DEBAT-04) | **Adopt** | never exported / agent-facing |
| Sentinel + FORCE override; failure→uncertain; pre-flight cost log + budget cap | **Adopt** | |
| Haiku for debate; heavier model only if proposals prove too weak | **Adapt** | env-overridable |
| Post-approval link-type classification | **Defer** | v1 families are explicit already |
| Embedding/cosine candidate generation; MiniLM reranker; LanceDB | **Skip** | IR-graph heuristics + type gate replace them |

## 8. Non-goals (v1)
- Auto-approval at any confidence. — Rendering the metadata tag in DeciSym views. —
  PROV-O serialization. — Inference across corpora. — Re-proposing rejected links with
  different rationale (rejections are durable). — Custom user-defined relation families.

## 9. Risks
1. **Popularity bias** (literature-documented): high-degree nodes (e.g. C&C subsystem)
   attract spurious proposals → mitigation: per-family candidate bounding + the queue
   groups by target so over-proposal is visible; track approve/reject ratios per family.
2. **Debate same-model bias**: advocate and challenger share Haiku's blind spots →
   acceptable v1 (human is the final gate); revisit if approval quality is poor.
3. **Premise quality**: the LLM may cite premises that resolve but don't actually support
   the link — the human queue shows premise quotes for exactly this reason; spot-audit
   approved links during A8.
4. **Volume**: 54 leaf functions × 34 components is bounded by subsystem scoping; if the
   queue exceeds ~100 proposals/family, raise the auto-reject floor before widening bands.

## 10. Sequencing
F8 builds on F1/F2 (shipped 2026-06-11). Order: schema+composeIR third layer → type gate +
candidate generation → proposal pass → debate stage → `mbse-infer` queue → serializer
metatag emission → Gate-1 extension + suspect propagation → A8 e2e. Hard invariant (the
standing doctrine): the Gate-1 inferred-id extension and the approval path must be real and
tested before any inferred content can reach the model. The control-flow extraction fix
(prior commitment, separate from F8) proceeds independently; `controlJoin` inference only
covers what prose extraction cannot state.
