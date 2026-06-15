---
name: mbse-requirements
description: Create RequirementDefinition and Need elements from extracted.json (schema_version 1.0.0), with provenance backpointers, and derive Need→Requirement edges.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Requirements

Populate the model with RequirementDefinition and Need elements sourced from
`examples/angars/model/extracted.json`. Every element carries a
`provenanceSourceId` attribute so that `validate_model` reports 100%
provenance coverage and the SysML export emits `// @source:` backpointers.

## Data Source

Read `examples/angars/model/extracted.json` (schema_version `"1.0.0"`) before
issuing any tool calls. The relevant arrays are:

- `needs[]` — each entry: `{ id, kind:"need", naturalKey, name, category?, description? }`
  - `.id` is the stable namespaced-hash ID (e.g. `need-b513c395606113c8`)
  - `.naturalKey` is the human-readable key (e.g. `N1`)
  - The ANGARS corpus has **16** needs.

- `requirements[]` — each entry: `{ id, kind:"requirement", naturalKey, name, statement, needIds:[], verifyMethod?, category?, reqType? }`
  - `.id` is the stable namespaced-hash ID (e.g. `req-a4f2b0c1...`)
  - `.naturalKey` is the human-readable key (e.g. `ANGARS-1`)
  - `needIds[]` holds **stableIds** (the `.id` form, not the `.naturalKey` strings) — derive edges resolve a need's stableId back to the Need model element created in Step 2.
  - The ANGARS corpus has **182** requirements.

- `satisfies[]` — each entry: `{ reqId, functionId }` (consumed by `mbse-build`, not this skill)

## Provenance Mechanism

`create_element(type, name, attributes)` spreads all keys from `attributes`
directly into `element.raw`. Therefore passing
`{ provenanceSourceId: "<naturalKey>" }` in the `attributes` argument causes
`element.raw.provenanceSourceId` to equal that string. This is the ONLY
supported mechanism — no wrapper, no nesting. The exact attribute key is
`provenanceSourceId` (camelCase, string value).

The `provenanceSourceId` MUST be the IR entity's `naturalKey` (e.g. `"ANGARS-10"`,
`"N1"`) — NOT the stableId hash. Gate 1 (Phase 5) resolves `provenanceSourceId`
by looking up the `naturalKey` in `extracted.json`. Using a stableId hash instead
of the naturalKey will fail Gate 1 validation.

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
  name: "<need.name>",
  attributes: {
    provenanceSourceId: "<need.naturalKey>"   // e.g. "N1"
  }
)
```

Capture the returned model `id` for each Need element, keyed by `need.id`
(the stableId). This map is used in Step 4 to resolve derive-edge sources.

Iterate all entries in `needs[]` — do not hardcode the list. The ANGARS
corpus has 16 needs.

### Step 3 — Create Requirement elements

For **each entry in `requirements[]`**, create a RequirementDefinition
using the requirement's `name` field as the element name. Pass the
requirement's `naturalKey` as `provenanceSourceId`, `statement` as `doc`,
and `verifyMethod` when present.

```
create_element(
  type: "RequirementDefinition",
  name: "<req.name>",
  attributes: {
    provenanceSourceId: "<req.naturalKey>",   // e.g. "ANGARS-4"
    doc: "<req.statement>",
    verifyMethod: "<req.verifyMethod>"        // omit if absent
  }
)
```

Capture the returned model `id` for each Requirement element, keyed by
`req.id` (the stableId). This map is used in Step 4 to resolve
derive-edge targets.

Iterate all entries in `requirements[]` — do not hardcode the list. The
ANGARS corpus has 182 requirements.

### Step 4 — Derive Need → Requirement edges

For **each requirement**, for **each stableId in `req.needIds[]`**, create a
`DeriveRequirementUsage` relationship from the Need element to the Requirement
element.

`needIds[]` contains stableIds (the `.id` form). Resolve each stableId against
the Step 2 map (stableId → model element id) to obtain the Need's model id.
Do NOT attempt to resolve by naturalKey string matching.

```
create_relationship(
  type: "DeriveRequirementUsage",
  source_id: <needModelId>,          // the Need's model id from Step 2 map
  target_id: <requirementModelId>    // the Requirement's model id from Step 3 map
)
```

This encodes "this requirement is derived from that operational need."

### Step 5 — Validate

```
validate_model()
```

Confirm:
- `provenanceCoverage` = 100 (no elements missing `provenanceSourceId`)
- No dangling relationships (all `needIds` stableIds resolved to model elements)
- `backwardPercent` reflects the `DeriveRequirementUsage` edges created

All `needs[]` and `requirements[]` entries must be represented.

### Step 6 — Export

```
export_sysml()
```

Each `requirement def` declaration should carry `// @source: <naturalKey>`
as a trailing comment (e.g. `// @source: ANGARS-10` or `// @source: N1`).
Report the provenance coverage percentage and the backward-trace percentage
to the user.

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

16 Need RequirementDefinitions + 182 requirement RequirementDefinitions for the
ANGARS corpus (generic: one per `needs[]`/`requirements[]` entry),
DeriveRequirementUsage edges from `needIds`, provenanceCoverage 100,
`// @source:` backpointers on every element.
