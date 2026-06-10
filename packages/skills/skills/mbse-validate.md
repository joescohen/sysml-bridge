---
name: mbse-validate
description: Binary traceability gate — every C&C requirement must have satisfy AND verify edges; zero orphans, zero missing provenance, zero dangling relationships.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Validate

Run the traceability completeness gate against the live model. This is a
BINARY gate: the model either PASS or FAIL. There are no partial passes and
no coverage-percentage thresholds — every condition in the PASS criteria must
hold simultaneously.

## PASS Criteria (all must be true)

Read these fields from the `coverage` object returned by `validate_model`:

| Field                        | PASS value          | FAIL condition                              |
|------------------------------|---------------------|---------------------------------------------|
| `forwardPercent`             | `=== 100`           | Any requirement has no `SatisfyRequirementUsage` or `AllocationUsage` edge |
| `verifyPercent`              | `=== 100`           | Any requirement has no `VerifyRequirementUsage` or `RequirementVerificationMembership` edge |
| `orphanElements`             | `length === 0`      | Any `PartDefinition` or `ActionDefinition` participates in no trace edge |
| `elementsMissingBackpointer` | `length === 0`      | Any `RequirementDefinition`, `PartDefinition`, or `ActionDefinition` lacks a non-empty `provenanceSourceId` |
| `danglingRelationships`      | `length === 0`      | Any relationship's `sourceIds` or `targetIds` contains an ID that resolves to no existing element |

If every field meets its PASS value: report **PASS**.
If any field fails: report **FAIL**, list each failing field by name, and for
array fields print each offending element (`id`, `name`, `type`).

**Do not report a custom "partial pass" or a percentage summary as the final verdict.**
The verdict is PASS or FAIL — nothing in between.

## Expected PASS Output (C&C model after mbse-requirements + mbse-trace)

After `mbse-requirements` and `mbse-trace` have both completed successfully,
the expected `validate_model` response is:

```json
{
  "issues": [],
  "coverage": {
    "forwardPercent": 100,
    "verifyPercent": 100,
    "backwardPercent": 100,
    "orphanElements": [],
    "provenanceCoverage": 100,
    "elementsMissingBackpointer": [],
    "danglingRelationships": []
  }
}
```

Any deviation from this shape is a FAIL. The `issues[]` array must be empty
for PASS (each issue entry is a human-readable string describing a specific
violation — if it is non-empty, print every issue string).

## Workflow

### Step 1 — Get model snapshot

```
get_project_state()
```

Confirm the element counts look plausible before running validation (a count
of 0 RequirementDefinitions means mbse-requirements has not run — stop and
tell the user).

Minimum expected counts for the C&C model:

| type                        | minimum |
|-----------------------------|---------|
| RequirementDefinition       | 33      |  (27 ANGARS-* + 6 Needs)
| ActionDefinition            | 17      |  (F1, F1.1–F1.6, F8, F8.1–F8.9)
| PartDefinition              | 6       |  (6 C&C components)
| VerificationCaseDefinition  | 4       |  (Test/Demo/Analysis/Inspection)

If any count is below its minimum, report which elements are missing and stop.
Do not proceed to validation when required elements are absent.

### Step 2 — Run the gate

```
validate_model()
```

The tool returns a JSON object with `summary`, `issues`, and `coverage`.
Read `coverage` only — `summary` is for informational display.

### Step 3 — Evaluate each PASS condition

Check the five conditions in the PASS Criteria table above in order.
For each failing condition:

1. Name the field.
2. Print the actual value (`forwardPercent: 87`, or array contents for list fields).
3. State the PASS value required.
4. Name the corrective skill: `mbse-trace` (for satisfy/verify gaps), `mbse-requirements`
   (for provenance gaps on RequirementDefinitions), `mbse-build` (for orphan functions/parts).

### Step 4 — Emit verdict

```
VERDICT: PASS
```
or
```
VERDICT: FAIL
Failing conditions:
  - forwardPercent: 87 (required: 100) — 4 requirements missing SatisfyRequirementUsage/AllocationUsage
  - orphanElements: [{ id: "...", name: "F1.2", type: "ActionDefinition" }]
Run mbse-trace to fix satisfy/allocate gaps.
```

### Step 5 — If PASS: export SysML

On PASS, run:

```
export_sysml()
```

Confirm the export contains `// @source:` backpointers on RequirementDefinition
declarations. Report the provenance coverage percentage from
`coverage.provenanceCoverage` alongside the export.

## What validate_model checks (reference)

The tool inspects:

- **Forward trace** (`forwardPercent`): each `RequirementDefinition` must have ≥1 edge of
  type `SatisfyRequirementUsage` or `AllocationUsage` in either direction.
- **Verify trace** (`verifyPercent`): each `RequirementDefinition` must have ≥1 edge of
  type `VerifyRequirementUsage` or `RequirementVerificationMembership` in either direction.
- **Backward trace** (`backwardPercent`): each `RequirementDefinition` must have ≥1
  `DeriveRequirementUsage` edge (Need→Req, created by `mbse-requirements`).
- **Orphan elements** (`orphanElements`): `PartDefinition` and `ActionDefinition` elements
  that have no `SatisfyRequirementUsage`, `AllocationUsage`, or `DeriveRequirementUsage`
  edge in either direction.
- **Provenance** (`elementsMissingBackpointer`): `RequirementDefinition`, `PartDefinition`,
  and `ActionDefinition` elements with a missing or empty `raw.provenanceSourceId`.
- **Dangling relationships** (`danglingRelationships`): any relationship whose `sourceIds`
  or `targetIds` contains an ID not present in the element set.

## MCP Tools Used

- `get_project_state`
- `validate_model`
- `export_sysml`

**Never** call `create_bdd_structure`, `create_diagram`, `create_connection`,
`create_ibd_connection`, or any HTTP-server-only tool. They do not exist in the
file-native MCP server.

## Output

- A single PASS or FAIL verdict.
- On FAIL: each failing field named, its actual value, the required value, and the
  corrective skill to run.
- On PASS: `export_sysml` output with provenance coverage percentage.
