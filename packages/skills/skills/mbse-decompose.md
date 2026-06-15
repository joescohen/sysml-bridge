---
name: mbse-decompose
description: Build F->subfunction decomposition trees from examples/angars/model/extracted.json behaviorDecomp[] data with provenance backpointers. Resolves parent->child relationships via parentId (stableId), producing 8 L2 root ActionDefinitions and 54 L3 child ActionDefinitions wired with FeatureMembership edges.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Decompose

The function decomposition layer. Build F->subfunction decomposition trees from
`examples/angars/model/extracted.json` (schema_version "1.0.0") using the
file-native MCP tools. This skill covers the `behaviorDecomp[]` decomposition
hierarchy exclusively. Activity creation (F1–F9 ActionDefinitions from
`functions[]`) and N2 item flows are owned by `mbse-build`. Allocation and
traceability edges are owned by `mbse-trace`.

## Data Source

Read `examples/angars/model/extracted.json` before issuing any tool calls.
The relevant array is `behaviorDecomp[]` — **62 entries** — each entry has the
IR shape:

```
{
  id:          string        // stableId (used as parentId reference)
  kind:        "behaviorDecomp"
  naturalKey:  string        // e.g. "F1", "F1.3"
  parentId?:   string        // stableId of the parent entry; absent for L2 roots
  level:       string        // "L2" for top-level functions, "L3" for subfunctions
  name:        string
  owner?:      string
  provenance:  {
    workbook: "ANGARS Requirements-Functions.xlsx"
    sheet:    "All Behaviors"
  }
}
```

**Corpus shape:** 8 L2 roots (parentId absent) + 54 L3 children (parentId
present) = 62 entries total.

### Provenance Anomaly — L2==8 / F9-absent (REQUIRED reading)

The `behaviorDecomp[]` array contains **8 L2 roots, not 9**. F9 does not appear
as a `behaviorDecomp` entry. F9 is synthesized elsewhere in the corpus and is
NOT present in the "All Behaviors" decomposition source. This is an anomaly in
the upstream workbook data, not a processing error.

**This skill MUST build only what `behaviorDecomp[]` actually contains.**
Do NOT hand-invent an F9 decomposition node to "complete" the set to 9 roots.
Fabricating a missing root is exactly the failure this pipeline exists to
prevent. Report the L2==8 / F9-absent anomaly explicitly in the run report
(Step 4 output).

All elements created by this skill must carry `provenanceSourceId` in their
`attributes`. See the **Provenance Mechanism** section below.

## Provenance Mechanism

`create_element(type, name, attributes)` spreads all keys from `attributes`
directly into `element.raw`. Passing
`{ provenanceSourceId: "<naturalKey>" }` in `attributes` causes
`element.raw.provenanceSourceId` to equal that string.

**Convention:** `provenanceSourceId` = the `behaviorDecomp` entry's `naturalKey`.

- L2 root ActionDefinition: `provenanceSourceId = entry.naturalKey` (e.g. `"F1"`)
- L3 child ActionDefinition: `provenanceSourceId = entry.naturalKey` (e.g. `"F1.3"`)

This is the ONLY supported mechanism — no wrapper, no nesting, no intermediary.
The exact attribute key is `provenanceSourceId` (camelCase, string value).

`validate_model` flags any `ActionDefinition` missing a non-empty
`raw.provenanceSourceId`.

`export_sysml` emits `// @source: <provenanceSourceId>` as a trailing comment
on each declaration line when the field is present.

## Workflow

### Step 0: Read session state and find existing elements

```
get_project_state()
```

```
query_elements(type_filter: "ActionDefinition")
```

Build a map of `naturalKey -> elementId` from any ActionDefinitions that
`mbse-build` may already have created. `behaviorDecomp[]` entries share
`naturalKey` values with `functions[]` entries (e.g., both may have `"F1"`).
If an ActionDefinition already exists with a matching `provenanceSourceId`,
reuse its id rather than creating a duplicate.

### Step 1: Create or reuse L2 root ActionDefinitions

For each entry in `behaviorDecomp[]` where `parentId` is absent (`level == "L2"`):

1. Check the map from Step 0 for an existing ActionDefinition with
   `provenanceSourceId == entry.naturalKey`.
2. If found, record `stableId -> existingElementId` and continue.
3. If not found, create:

```
create_element(
  type: "ActionDefinition",
  name: <entry.name>,
  attributes: { provenanceSourceId: <entry.naturalKey> }
)
```

Capture the returned `id`. Build a stableId-to-elementId map:
`stableIdMap[entry.id] = returnedElementId`.

Repeat for all 8 L2 roots.

### Step 2: Create L3 child ActionDefinitions and wire FeatureMembership

For each entry in `behaviorDecomp[]` where `parentId` is present (`level == "L3"`):

1. Resolve the parent element id using `stableIdMap[entry.parentId]`. The
   `parentId` field is the stableId of the parent `behaviorDecomp` entry —
   do NOT guess the parent by string-prefix matching on `naturalKey`.
2. Check the map from Step 0 for an existing ActionDefinition with
   `provenanceSourceId == entry.naturalKey`. Reuse if found.
3. If not found, create:

```
create_element(
  type: "ActionDefinition",
  name: <entry.name>,
  attributes: {
    provenanceSourceId: <entry.naturalKey>,
    owner: <stableIdMap[entry.parentId]>
  }
)
```

Capture the returned `id` and add to `stableIdMap[entry.id] = returnedElementId`.

4. Wire decomposition containment:

```
create_relationship(
  type: "FeatureMembership",
  source_id: <stableIdMap[entry.parentId]>,
  target_id: <stableIdMap[entry.id]>
)
```

If `stableIdMap[entry.parentId]` is undefined (parentId did not resolve),
this is a dangling edge — log it, do NOT create the relationship, and
report it in the run output as an unresolved parentId.

Repeat for all 54 L3 children.

### Step 3: Validate the decomposition model

```
validate_model()
```

Confirm:
- `provenanceCoverage: 100` on all decomposition ActionDefinitions.
- No dangling FeatureMembership (every L3 child's parentId resolved to an
  element id in `stableIdMap`).
- Total ActionDefinitions created/reused equals 62 (or fewer if reusing
  existing elements from `mbse-build`).

If any parentId failed to resolve, surface the details before proceeding.

### Step 4: Export and report anomaly

```
export_sysml()
```

`export_sysml` emits `// @source: <provenanceSourceId>` on each declaration.

**Run report must include:**

1. Counts: L2 roots created/reused, L3 children created/reused.
2. FeatureMembership edge count (should equal 54 — one per L3 child).
3. Any unresolved parentId entries (should be 0).
4. **Anomaly notice:** "behaviorDecomp[] contains 8 L2 roots. F9 is absent from
   the 'All Behaviors' workbook sheet and was NOT synthesized. The decomposition
   tree built here reflects the corpus exactly."

---

## Tools Used

This skill uses the **file-native stdio MCP server** (`packages/mcp-server`).
There is no live SysON or SMAPS dependency.

Available tools:
- `init_project`
- `get_project_state`
- `query_elements`
- `query_relationships`
- `create_element`
- `create_relationship`
- `validate_model`
- `import_sysml`
- `export_sysml`

**Never** call `create_bdd_structure`, `create_ibd_connection`,
`create_diagram`, `create_connection`, `delete_element`, `update_element`,
or `create_project`. These are HTTP-server-only tools and do NOT exist in the
file-native MCP server.

---

## Output

8 L2 root ActionDefinitions + 54 L3 child ActionDefinitions (62 total from
`behaviorDecomp[]`), FeatureMembership decomposition edges resolved via
`parentId`, `provenanceCoverage` 100, L2==8 / F9-absent anomaly reported,
`// @source:` backpointers on every emitted declaration.
