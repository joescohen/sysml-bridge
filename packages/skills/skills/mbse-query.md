---
name: mbse-query
description: Answer natural-language questions grounded in actual model elements
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Query

Answer natural-language questions about the system by grounding every answer in TWO sources:
the corpus IR (`examples/angars/model/extracted.json`) and the live file-native model store
(queried via `query_elements` and `query_relationships`). Every answer declares WHICH side
produced it, and flags divergence when the two sides disagree.

---

## Two sides of the pipe

| Side | Source | How to read |
|------|--------|-------------|
| **CORPUS** | `examples/angars/model/extracted.json` | Read as a data file by path — NEVER imported as a code module (Pillar D). Contains the ground-truth needs, requirements, functions, components, satisfies links, and allocations as extracted from the workbooks. |
| **MODEL** | Live file-native store | Query via `query_elements` / `query_relationships` / `get_project_state` / `validate_model`. Reflects the current state of the built SysML v2 model. |

**Answer provenance labels — REQUIRED on every factual claim:**

- `[corpus]` — fact comes from `examples/angars/model/extracted.json` only.
- `[model]` — fact comes from the live store only.
- `[both]` — confirmed in both sources and they agree.
- `[divergent]` — sources disagree; describe the gap explicitly.

A model-only element — present in the live store but absent from the corpus — is a
corpus-grounding concern (Pillar C). Label it `[divergent]` and surface it; never hide it.

---

## Routing a question

Apply the following decision procedure before issuing any tool call:

1. **Corpus-grounding questions** — "Does the corpus contain a need for X?",
   "How many requirements were extracted?", "What does the corpus say F1 decomposes into?"
   → Read `examples/angars/model/extracted.json` by path. Answer labeled `[corpus]`.

2. **Model-state questions** — "What RequirementDefinitions exist in the model?",
   "Is REQ-001 satisfied in the model?", "Are there any dangling relationships?"
   → Call `query_elements` / `query_relationships`. Answer labeled `[model]`.

3. **Reconciliation questions** — "Is corpus requirement R-12 actually in the built model?",
   "Are there model elements with no corpus source?", "Do corpus and model agree on X?"
   → Read BOTH sides. Join on `provenanceSourceId` ↔ corpus entity `id`.
   Answer labeled `[both]` (when they agree) or `[divergent]` (when they disagree).

When the question is ambiguous, answer from both sides and compare.

---

## Common question patterns

| Question | Routing | Tool sequence |
|----------|---------|---------------|
| "What requirements exist?" | Model | `query_elements("RequirementDefinition")` → `[model]` |
| "Is REQ-001 satisfied?" | Model | `query_relationships(reqId, "SatisfyRequirementUsage")` → `[model]` |
| "What connects to port X?" | Model | `query_relationships(portId, "ConnectionUsage")` → `[model]` |
| "What parts exist?" | Model | `query_elements("PartDefinition")` → `[model]` |
| "Show me the full model" | Model | `get_project_state` → `[model]` |
| "Are there validity issues?" | Model | `validate_model` → `[model]` |
| "What is allocated to subsystem Y?" | Model | `query_relationships(subsystemId, "AllocationUsage")` → `[model]` |
| "How many needs does the corpus define?" | Corpus | Read `examples/angars/model/extracted.json`, count `needs[]` → `[corpus]` |
| "What functions does the corpus list?" | Corpus | Read `examples/angars/model/extracted.json`, inspect `functions[]` → `[corpus]` |
| "Is corpus need N-03 in the model?" | Reconciliation | Read corpus + `query_elements` + join on `provenanceSourceId` → `[both]` or `[divergent]` |
| "Are there model elements with no corpus source?" | Reconciliation | `query_elements`, read corpus, find elements where `provenanceSourceId` absent from corpus ids → `[divergent]` |

---

## Corpus-vs-model question patterns

| Pattern | Corpus action | Model action | Expected answer |
|---------|--------------|--------------|-----------------|
| **Pure corpus** — "What requirements are in the corpus?" | Read `examples/angars/model/extracted.json`, return `requirements[]` array | — | `[corpus]`: list each entry's `naturalKey` and `name` |
| **Pure model** — "What RequirementDefinitions are in the model?" | — | `query_elements("RequirementDefinition")` | `[model]`: list each element `@id` and `name` |
| **Reconciliation** — "Is corpus requirement R-12 in the model?" | Find entity with matching `naturalKey` or `id` in `requirements[]`; note its `id` | `query_elements("RequirementDefinition")`, filter by `provenanceSourceId == corpus.id` | `[both]` if found; `[divergent]` if corpus has it but model does not (a **drop**), or model has an element with no matching corpus id (an **addition**) |

**Join key:** `provenanceSourceId` stored on model elements equals the `id` field of the
corresponding corpus entity in `examples/angars/model/extracted.json`. This is the canonical
join key confirmed in `05-RESEARCH.md`.

**Drop** = a corpus entity whose `id` matches no model element's `provenanceSourceId`. It was
extracted but never built into the model.

**Addition** = a model element whose `provenanceSourceId` is absent from the corpus id set.
This violates Pillar C and must be surfaced as a corpus-grounding concern.

---

## Divergence detection

To answer "do corpus and model agree on X":

1. **Load the corpus entity set** — read `examples/angars/model/extracted.json` by path,
   collect `id` values for the relevant kind (e.g., all `requirements[]` ids, all `needs[]` ids).
2. **Load the model element set** — call `query_elements(type_filter)` for the matching
   SysML type (e.g., `"RequirementDefinition"` for requirements). Collect each element's
   `provenanceSourceId` from its attributes / raw store.
3. **Join on `provenanceSourceId` ↔ corpus `id`** and classify:

| Class | Definition | Label |
|-------|-----------|-------|
| **Matched** | corpus entity id == model element's `provenanceSourceId` | `[both]` |
| **Corpus-only (drop)** | corpus entity id has no matching model element | `[divergent]` — report as a drop |
| **Model-only (addition)** | model element's `provenanceSourceId` absent from corpus | `[divergent]` — report as a corpus-grounding concern (Pillar C) |

4. **Summarise** matched count, drop count, addition count.
   - All drops and additions must be listed by `naturalKey` / element id — never silently
     aggregated.
   - A non-zero addition count is always a corpus-grounding violation; flag it prominently.

---

## Traceability queries

There is no `get_traceability` tool. Build traceability answers from:

1. `query_relationships(type_filter: "SatisfyRequirementUsage")` — all satisfy links `[model]`
2. `query_relationships(type_filter: "VerifyRequirementUsage")` — all verify links `[model]`
3. `query_relationships(type_filter: "AllocationUsage")` — all allocation links `[model]`
4. Cross-reference `source` and `target` IDs against `query_elements` results to produce
   a human-readable traceability matrix.

For a corpus-grounded traceability view, additionally read the `satisfies[]` and
`allocations[]` arrays from `examples/angars/model/extracted.json` and compare against the
live model trace edges.

---

## Worked example — reconciliation question

**User asks:** "Is the cooling need in both the corpus and the model?"

### Step 1 — Corpus side `[corpus]`

Read `examples/angars/model/extracted.json` by path. Search `needs[]` for an entry whose
`name` or `naturalKey` matches "cooling":

```
// hypothetical match
{ id: "need-007", kind: "need", naturalKey: "N-07", name: "Thermal/Cooling Management" }
```

Result: corpus id = `"need-007"`, naturalKey = `"N-07"`. `[corpus]` — found.

### Step 2 — Model side `[model]`

```
query_elements("RequirementDefinition")
```

Filter results for an element whose `provenanceSourceId == "need-007"` (or naturalKey
`"N-07"` if id is stored that way). Suppose one matches:

```
{ @id: "elem-4b2", name: "N-07 Thermal/Cooling Management", provenanceSourceId: "need-007" }
```

Result: model element `elem-4b2` found. `[model]` — found.

### Step 3 — Verdict

Both sides have the entity and they agree on provenance. Emit:

```
[both] Corpus entry N-07 ("Thermal/Cooling Management", id=need-007) is present in the model
as RequirementDefinition elem-4b2 with provenanceSourceId=need-007. No divergence.
```

**If Step 2 found no matching element:**

```
[divergent] Corpus entry N-07 ("Thermal/Cooling Management", id=need-007) exists in the corpus
but has NO matching RequirementDefinition in the model (no element with provenanceSourceId=need-007).
This is a DROP — the need was extracted but never built into the model.
```

---

## Tools Used

**Corpus side** — no MCP tool. The corpus is read as a data file by path:
`examples/angars/model/extracted.json`. This is a direct file read (Pillar D). Do not
import or require it as a code module. No MCP call is needed for corpus reads.

**Model side** — file-native stdio MCP server (`packages/mcp-server`):

- `query_elements`
- `query_relationships`
- `get_project_state`
- `validate_model`

This skill is **read-only**. Do NOT call `create_element`, `create_relationship`,
`update_element`, `delete_element`, `import_sysml`, or any write tool.
