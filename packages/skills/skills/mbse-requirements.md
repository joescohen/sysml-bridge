---
name: mbse-requirements
description: Create RequirementDefinition and Need elements from cc-extracted.json, with provenance backpointers, and derive Need→Requirement edges.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Requirements

Populate the model with RequirementDefinition and Need elements sourced from
`examples/angars/model/cc-extracted.json`. Every element carries a
`provenanceSourceId` attribute so that `validate_model` reports 100%
provenance coverage and the SysML export emits `// @source:` backpointers.

## Data Source

Read `examples/angars/model/cc-extracted.json` before issuing any tool calls.
The relevant arrays are:

- `needs[]` — each entry: `{ id, name }` (e.g. `{ "id": "N1", "name": "N1" }`)
- `requirements[]` — each entry: `{ id, name, statement, needIds:[], verifyMethod }`
- `satisfies[]` — each entry: `{ reqId, functionId }` (used by `mbse-build`, not this skill)

## Provenance Mechanism

`create_element(type, name, attributes)` spreads all keys from `attributes`
directly into `element.raw`. Therefore passing
`{ provenanceSourceId: "<id>" }` in the `attributes` argument causes
`element.raw.provenanceSourceId` to equal that string. This is the ONLY
supported mechanism — no wrapper, no nesting. The exact attribute key is
`provenanceSourceId` (camelCase, string value).

`validate_model` flags any `RequirementDefinition` missing a non-empty
`raw.provenanceSourceId` as a provenance gap.

`export_sysml` emits `// @source: <provenanceSourceId>` as a trailing comment
on each declaration line when the field is present.

## Workflow

### Step 1 — Initialize and check existing state

```
get_project_state()
query_elements(type_filter: "RequirementDefinition")
```

If RequirementDefinitions already exist, show them to the user and confirm
before adding more (avoid duplicates).

### Step 2 — Create Need elements

For **each entry in `needs[]`**, create a RequirementDefinition so the Need
exists as a real model element. Without this, any downstream
`DeriveRequirementUsage` edge from a Need to a Requirement has a dangling
source and is silently dropped by `validate_model`.

```
create_element(
  type: "RequirementDefinition",
  name: "<need.name>",           // e.g. "N1"
  attributes: {
    provenanceSourceId: "<need.id>"   // e.g. "N1"
  }
)
```

Capture the returned `id` for each Need element.

Full Need list from cc-extracted.json:
`N1`, `N12`, `N15`, `N16`, `N2`, `N7`

### Step 3 — Create Requirement elements

For **each entry in `requirements[]`**, create a RequirementDefinition
using the requirement's `name` field as the element name. Pass the
requirement `id` as `provenanceSourceId` and `statement` as `doc`.

```
create_element(
  type: "RequirementDefinition",
  name: "<req.name>",            // e.g. "Aircraft ID Verification"
  attributes: {
    provenanceSourceId: "<req.id>",   // e.g. "ANGARS-4"
    doc: "<req.statement>",
    verifyMethod: "<req.verifyMethod>"
  }
)
```

Capture the returned `id` for each Requirement element.

Full requirement list (27 entries):
`ANGARS-4`, `ANGARS-10`, `ANGARS-14`, `ANGARS-62`, `ANGARS-67`,
`ANGARS-103`, `ANGARS-104`, `ANGARS-105`, `ANGARS-106`, `ANGARS-107`,
`ANGARS-108`, `ANGARS-109`, `ANGARS-110`, `ANGARS-111`, `ANGARS-112`,
`ANGARS-113`, `ANGARS-114`, `ANGARS-115`, `ANGARS-116`, `ANGARS-117`,
`ANGARS-141`, `ANGARS-147`, `ANGARS-149`, `ANGARS-150`, `ANGARS-151`,
`ANGARS-152`, `ANGARS-153`, `ANGARS-154`

### Step 4 — Derive Need → Requirement edges

For each requirement, for each `needId` in `req.needIds[]`, create a
`DeriveRequirementUsage` relationship from the Need element to the
Requirement element. This encodes "this requirement is derived from that
operational need."

```
create_relationship(
  type: "DeriveRequirementUsage",
  source_id: <needElementId>,     // the Need's model id (from Step 2)
  target_id: <requirementElementId>  // the Requirement's model id (from Step 3)
)
```

### Step 5 — Validate

```
validate_model()
```

Confirm:
- `provenanceCoverage` = 100 (no elements missing `provenanceSourceId`)
- No dangling relationships (all Need element IDs resolve)
- `backwardPercent` reflects the `DeriveRequirementUsage` edges created

### Step 6 — Export

```
export_sysml()
```

Each `requirement def` declaration should carry `// @source: ANGARS-*` or
`// @source: N*` as a trailing comment. Report the provenance coverage
percentage and the backward-trace percentage to the user.

## MCP Tools Used

- `get_project_state`
- `query_elements`
- `create_element`
- `create_relationship`
- `validate_model`
- `export_sysml`

**Never** call `create_bdd_structure`, `create_diagram`, `create_connection`,
`create_ibd_connection`, or any HTTP-server-only tool. They do not exist in
the file-native MCP server.

## Output

- 6 Need RequirementDefinitions (N1, N2, N7, N12, N15, N16)
- 27 ANGARS-* RequirementDefinitions
- DeriveRequirementUsage edges wiring each Need to its derived Requirements
- `validate_model` showing `provenanceCoverage: 100` and no dangling
  relationships
- SysML export with `// @source:` backpointers on every element
