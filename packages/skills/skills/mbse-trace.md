---
name: mbse-trace
description: Build satisfy, allocate (model-asserted), and verify traceability edges from examples/angars/model/extracted.json (schema_version 1.0.0). Does NOT create Need elements or DeriveRequirementUsage edges — those are owned by mbse-requirements.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Trace

Populate the three traceability hops that connect requirements to functions,
functions to components, and requirements to verification cases. This skill
does NOT touch Need elements or `DeriveRequirementUsage` edges — those are
created by `mbse-requirements`.

## Data Source

Read `examples/angars/model/extracted.json` (schema_version "1.0.0") before
issuing any tool calls. Relevant arrays:

- `requirements[]` (182 entries) — each entry: `{ id, naturalKey, name, statement, needIds, verifyMethod? }`
  - `verifyMethod` is a free-text field. The corpus contains many distinct values
    including compound and abbreviated forms (e.g. "T, D", "Analysis/Test", "I, T").
    Do NOT assume a fixed four-method set — enumerate the distinct values at runtime.
- `functions[]` (63 entries) — each entry: `{ id, naturalKey, name, level, owner }`
- `satisfies[]` (154 entries) — each entry: `{ reqId, functionId }` where BOTH fields
  are **stableIds** (the `.id` form, e.g. `requirement-013ea2859817aaf0` /
  `function-23e5d0ef20b3f35d`), NOT naturalKeys. Resolve via lookup maps (Step 0).
- `allocations[]` — **EMPTY** in this corpus; allocations are MODEL-ASSERTED only.

## Dependency: elements must already exist

`mbse-requirements` must have run first (RequirementDefinition elements present).
`mbse-build` must have run first (ActionDefinition / function elements present)
or function elements must otherwise exist. If they are missing, stop and prompt
the user to run those skills before proceeding.

## Workflow

### Step 0 — Load existing state and build lookup maps

```
get_project_state()
query_elements(type: "RequirementDefinition")
query_elements(type: "ActionDefinition")
query_elements(type: "PartDefinition")
```

Build three lookup maps from the live-model query results:

- `reqByProvenance`: `provenanceSourceId → element.id` for every RequirementDefinition
- `funcByProvenance`: `provenanceSourceId → element.id` for every ActionDefinition
- `compByName`: `element.name → element.id` for every PartDefinition

Build two corpus-side index maps from `extracted.json`:

- `reqStableToNaturalKey`: `requirement.id (stableId) → requirement.naturalKey`
  — for every entry in `requirements[]`
- `funcStableToNaturalKey`: `function.id (stableId) → function.naturalKey`
  — for every entry in `functions[]`

These maps are used to resolve `satisfies[]` stableIds to naturalKeys, which
are the provenanceSourceId keys for live model lookup.

---

### Step 1 — Req → Function (satisfy)

For each entry in `satisfies[]` (154 entries):

1. Resolve `entry.functionId` (stableId) → naturalKey via `funcStableToNaturalKey`
2. Resolve `entry.reqId` (stableId) → naturalKey via `reqStableToNaturalKey`
3. Look up live element ids via `funcByProvenance[naturalKey]` and `reqByProvenance[naturalKey]`
4. Create a `SatisfyRequirementUsage` edge:

```
create_relationship(
  type: "SatisfyRequirementUsage",
  source_id: funcByProvenance[funcNaturalKey],   // satisfier (function)
  target_id: reqByProvenance[reqNaturalKey]      // requirement being satisfied
)
```

After all edges are created, confirm with:

```
query_relationships(type: "SatisfyRequirementUsage")
```

Expected: count >= 154 (one per entry in `satisfies[]`; shared functions create
multiple edges). If the count is below 154, diagnose which stableId lookups failed
before proceeding.

---

### Step 2 — Function → Component (allocate, model-asserted)

`allocations[]` is **empty** in this corpus. The extracted.json carries NO
Func→Comp allocation source data.

**Any AllocationUsage edge created here is MODEL-ASSERTED.** It is an explicit
engineering-judgment overlay, NOT corpus-backed. Before creating any allocation
edges, confirm with the user which function-to-component allocations they want
to assert. Each asserted allocation MUST carry the audit flag:

```
create_relationship(
  type: "AllocationUsage",
  source_id: funcByProvenance[functionNaturalKey],   // function being allocated
  target_id: compByName[componentName],              // component receiving the allocation
  attributes: { provenanceSourceId: "model-asserted" }
)
```

`provenanceSourceId: "model-asserted"` is the ONLY provenance assertion
permitted when no corpus table exists.

After creating any allocation edges, confirm with:

```
query_relationships(type: "AllocationUsage")
```

**Report the count of model-asserted allocations** to the user verbatim:

> "N AllocationUsage edges created. All are model-asserted (no corpus ground
> truth; `allocations[]` is empty in `examples/angars/model/extracted.json`).
> Audit flag: provenanceSourceId = model-asserted."

This makes the audit honest about which hop lacks corpus backing.

---

### Step 3 — Requirement → Verify (verify)

Enumerate the DISTINCT `verifyMethod` values across all entries in
`requirements[]`. Do not assume a fixed set — collect them at runtime from the
corpus data.

Create one `VerificationCaseDefinition` element per distinct verifyMethod value:

```
create_element(
  type: "VerificationCaseDefinition",
  name: "<verifyMethod> Verification",    // e.g. "Test Verification"
  attributes: {
    provenanceSourceId: "verifyMethod:<verifyMethod>"   // e.g. "verifyMethod:Test"
  }
)
```

Capture the returned `id` for each VerificationCaseDefinition.

Then for **each requirement that carries a `verifyMethod`**, create a
`VerifyRequirementUsage` edge with **source = the VerificationCaseDefinition**
matching the requirement's `verifyMethod`, and **target = the requirement element**:

```
create_relationship(
  type: "VerifyRequirementUsage",
  source_id: <verCaseElementId>,               // VerificationCaseDefinition for this method
  target_id: reqByProvenance[req.naturalKey]   // requirement being verified
)
```

Expected count = number of requirements in `requirements[]` that carry a
`verifyMethod` field. Confirm with:

```
query_relationships(type: "VerifyRequirementUsage")
```

If the count is unexpectedly low, diagnose which `create_relationship` calls
returned errors before proceeding.

---

### Step 4 — Confirm all edges persisted

Run one confirm query per relationship type after all three steps complete:

```
query_relationships(type: "SatisfyRequirementUsage")
query_relationships(type: "AllocationUsage")
query_relationships(type: "VerifyRequirementUsage")
```

Report the three counts to the user. If any count is 0 or unexpectedly low,
do not proceed — diagnose which `create_relationship` calls failed (check for
error responses) before claiming the trace chain is complete.

---

## Valid Relationship Types (reference)

Only types in `SYSML_RELATIONSHIP_TYPES` are accepted by `create_relationship`.
Passing an unknown type returns an error response — the call does NOT silently
succeed. The full list is in
`packages/mcp-server/src/types/sysml-elements.ts`.

Types used by this skill:

| Purpose                   | `type` arg                 | source       | target        |
|---------------------------|----------------------------|--------------|---------------|
| Req→Function satisfaction | `SatisfyRequirementUsage`  | function     | requirement   |
| Func→Component allocation | `AllocationUsage`          | function     | component     |
| Req→Verify case           | `VerifyRequirementUsage`   | ver. case    | requirement   |

**NOTE:** `"Allocation"` is NOT a valid type — it is not in `SYSML_RELATIONSHIP_TYPES`
and will be rejected. Always use `"AllocationUsage"`.

**NOTE:** `query_relationships` takes a `type` parameter (not `type_filter`).
Example: `query_relationships(type: "SatisfyRequirementUsage")`.

---

## MCP Tools Used

- `get_project_state`
- `query_elements`
- `query_relationships`
- `create_element`
- `create_relationship`

**Never** call `create_bdd_structure`, `create_diagram`, `create_connection`,
`create_ibd_connection`, or any HTTP-server-only tool. They do not exist in the
file-native MCP server.

## Output

- ~154 SatisfyRequirementUsage edges (corpus-backed from `satisfies[]`, 154 entries)
- N AllocationUsage edges, ALL model-asserted (`allocations[]` is empty),
  flagged `provenanceSourceId: "model-asserted"`
- One VerificationCaseDefinition per distinct `verifyMethod` value found in
  `requirements[]` at runtime (not a fixed count — enumerate from the corpus)
- One VerifyRequirementUsage per requirement carrying a `verifyMethod`
- Audit statement reporting model-asserted allocation count and that all
  allocations are model-asserted (not corpus-sourced)
