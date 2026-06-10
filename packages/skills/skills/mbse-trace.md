---
name: mbse-trace
description: Build satisfy, allocate (model-asserted), and verify traceability edges from cc-extracted.json. Does NOT create Need elements or DeriveRequirementUsage edges — those are owned by mbse-requirements.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Trace

Populate the three traceability hops that connect C&C requirements to
functions, functions to components, and requirements to verification cases.
This skill does NOT touch Need elements or `DeriveRequirementUsage` edges —
those are created by `mbse-requirements`.

## Data Source

Read `examples/angars/model/cc-extracted.json` before issuing any tool calls.
Relevant arrays:

- `requirements[]` — each entry: `{ id, name, statement, needIds, verifyMethod }`
- `functions[]` — each entry: `{ id, name, level, owner }`
- `components[]` — each entry: `{ name }` (no `id`; resolve IDs via `query_elements`)
- `satisfies[]` — each entry: `{ reqId, functionId }` (corpus ground truth)
- `allocations: []` — **empty**; see `allocationsNote`
- `allocationsNote` — confirms no Func→Comp source exists; allocations are model-asserted

## Dependency: elements must already exist

`mbse-requirements` must have run first (RequirementDefinition elements present).
`mbse-build` must have run first (ActionDefinition / function elements present)
or function elements must otherwise exist. If they are missing, stop and prompt
the user to run those skills before proceeding.

## Workflow

### Step 0 — Load existing state

```
get_project_state()
query_elements(type: "RequirementDefinition")
query_elements(type: "ActionDefinition")
query_elements(type: "PartDefinition")
```

Build two lookup maps from the results:

- `reqByProvenance`: `provenanceSourceId → element.id` for every RequirementDefinition
- `funcByProvenance`: `provenanceSourceId → element.id` for every ActionDefinition
- `compByName`: `element.name → element.id` for every PartDefinition

These maps are used in every subsequent step to resolve corpus IDs to live model IDs.

---

### Step 1 — Req → Function (satisfy)

For each entry in `satisfies[]`, create a `SatisfyRequirementUsage` edge with
**source = function element** and **target = requirement element**:

```
create_relationship(
  type: "SatisfyRequirementUsage",
  source_id: funcByProvenance[entry.functionId],   // satisfier (function)
  target_id: reqByProvenance[entry.reqId]           // requirement being satisfied
)
```

Full `satisfies[]` table from cc-extracted.json (28 entries):

| reqId       | functionId |
|-------------|------------|
| ANGARS-4    | F1.1       |
| ANGARS-10   | F1.1       |
| ANGARS-14   | F1.2       |
| ANGARS-62   | F1.6       |
| ANGARS-67   | F1.6       |
| ANGARS-103  | F8.1       |
| ANGARS-104  | F8.3       |
| ANGARS-105  | F8.4       |
| ANGARS-106  | F8.5       |
| ANGARS-107  | F8.2       |
| ANGARS-108  | F8.6       |
| ANGARS-109  | F8.7       |
| ANGARS-110  | F8.7       |
| ANGARS-111  | F8.8       |
| ANGARS-112  | F8.5       |
| ANGARS-113  | F8.6       |
| ANGARS-114  | F8.8       |
| ANGARS-115  | F8.4       |
| ANGARS-116  | F8.3       |
| ANGARS-117  | F8.9       |
| ANGARS-141  | F1.3       |
| ANGARS-147  | F1.3       |
| ANGARS-149  | F1.1       |
| ANGARS-150  | F1.3       |
| ANGARS-151  | F1.4       |
| ANGARS-152  | F1.5       |
| ANGARS-153  | F1.6       |
| ANGARS-154  | F1.6       |

After all edges are created, confirm with:

```
query_relationships(type: "SatisfyRequirementUsage")
```

Expected: count ≥ 28 (one per entry above; shared functions create multiple edges).

---

### Step 2 — Function → Component (allocate, model-asserted)

The corpus `allocations` array is empty. The `allocationsNote` in
cc-extracted.json explicitly states: "No corpus Func→Comp source; allocations
are model-asserted downstream."

Infer allocations from engineering judgment over the 6 C&C components and the
F1/F8 function trees. Apply the following model-asserted allocation table:

| functionId | component name              | rationale                               |
|------------|-----------------------------|-----------------------------------------|
| F1         | Flight Control Module       | Top-level request management runs on flight control |
| F1.1       | Flight Control Module       | Authenticate/receive — flight control comms  |
| F1.2       | Flight Control Module       | Fuel capacity check — flight control sensors |
| F1.3       | Flight Control Module       | Prioritization algorithm — flight control compute |
| F1.4       | Flight Control Module       | Schedule generation — flight control compute |
| F1.5       | Flight Control Module       | Dynamic schedule update — flight control compute |
| F1.6       | Flight Control Module       | Status/report transmit — flight control comms |
| F8         | Operator Control Plane      | HMI management — operator control top-level |
| F8.1       | HMI Panel & Displays        | Display mission data — HMI displays     |
| F8.2       | Operator Console Module     | Receive operator input — console        |
| F8.3       | Operator Control Plane      | Process manual override — operator plane |
| F8.4       | Operator Control Plane      | Execute emergency controls — operator plane |
| F8.5       | Haptic Alert Unit           | Provide alerts/feedback — haptic unit   |
| F8.6       | HMI Panel & Displays        | Update HMI displays — HMI panel         |
| F8.7       | HMI Panel & Displays        | Subsystem health/multilingual — HMI panel |
| F8.8       | Operator Console Module     | Logging & dashboard integration — console |
| F8.9       | Operator Control Plane      | Reprioritize queue — operator control   |

For each row, create an `AllocationUsage` edge. Pass `provenanceSourceId: "model-asserted"`
in the `attributes` parameter so the edge carries an explicit audit flag — this is the
ONLY provenance assertion permitted when no corpus table exists:

```
create_relationship(
  type: "AllocationUsage",
  source_id: funcByProvenance[functionId],        // function being allocated
  target_id: compByName[componentName],           // component receiving the allocation
  attributes: { provenanceSourceId: "model-asserted" }
)
```

After all edges are created, confirm with:

```
query_relationships(type: "AllocationUsage")
```

**Report the count of model-asserted allocations** to the user verbatim:

> "N AllocationUsage edges created. All are model-asserted (no corpus ground truth;
> source: allocationsNote in cc-extracted.json). Audit flag: provenanceSourceId = model-asserted."

This makes the audit honest about which hop lacks corpus backing.

---

### Step 3 — Requirement → Verify (verify)

Create one `VerificationCaseDefinition` element per DISTINCT `verifyMethod` value
in `requirements[]`. The four distinct methods in cc-extracted.json are:
**Test**, **Demonstration**, **Analysis**, **Inspection**.

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

Then for **each requirement**, create a `VerifyRequirementUsage` edge with
**source = the VerificationCaseDefinition** matching the requirement's
`verifyMethod`, and **target = the requirement element**:

```
create_relationship(
  type: "VerifyRequirementUsage",
  source_id: <verCaseElementId>,          // VerificationCaseDefinition for this method
  target_id: reqByProvenance[req.id]      // requirement being verified
)
```

Verify method distribution across the 27 C&C requirements:

| verifyMethod  | requirements                                                          |
|---------------|-----------------------------------------------------------------------|
| Demonstration | ANGARS-4, ANGARS-14, ANGARS-103, ANGARS-104, ANGARS-110, ANGARS-112, ANGARS-114, ANGARS-117, ANGARS-151, ANGARS-153, ANGARS-154 |
| Test          | ANGARS-10, ANGARS-105, ANGARS-106, ANGARS-107, ANGARS-108, ANGARS-113, ANGARS-115, ANGARS-149, ANGARS-152 |
| Analysis      | ANGARS-62, ANGARS-116, ANGARS-141, ANGARS-147, ANGARS-150            |
| Inspection    | ANGARS-67, ANGARS-109, ANGARS-111                                     |

After all edges are created, confirm with:

```
query_relationships(type: "VerifyRequirementUsage")
```

Expected: count = 27 (one per requirement).

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

| Purpose                  | `type` arg                  | source       | target        |
|--------------------------|-----------------------------|--------------|---------------|
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

- 28 SatisfyRequirementUsage edges (corpus-backed, from `satisfies[]`)
- 17 AllocationUsage edges (model-asserted, flagged with `provenanceSourceId: "model-asserted"`)
- 4 VerificationCaseDefinition elements (one per distinct verifyMethod)
- 27 VerifyRequirementUsage edges (one per requirement)
- Audit statement reporting model-asserted allocation count
