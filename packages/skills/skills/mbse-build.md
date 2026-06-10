---
name: mbse-build
description: Build SysML v2 structural and behavioral artifacts from examples/angars/model/extracted.json using file-native MCP tools only. Covers BDD and IBD (subsystem/component rosters + N2 connections) and F1-F9 activities (functions + functional N2 item flows) for any corpus.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Build

The construction layer. Build SysML v2 model artifacts from
`examples/angars/model/extracted.json` (schema_version "1.0.0") using the
file-native MCP tools. This skill covers structural artifacts (BDD, IBD) and
behavioral artifacts (F1–F9 activities with N2-sourced item flows). Allocation
and traceability edges are owned by `mbse-trace`. Detailed function
decomposition trees are owned by `mbse-decompose`. Sequence diagrams are out
of scope.

## Data Source

Read `examples/angars/model/extracted.json` before issuing any tool calls.
Relevant arrays and their IR shapes:

- `subsystems[]` — 6 entries, each:
  `{ id, kind:"subsystem", naturalKey, name, componentIds[], provenance }`.
  `componentIds[]` holds stableIds that resolve into `components[]`.
  These are the BDD roots.

- `components[]` — 34 entries, each:
  `{ id, kind:"component", naturalKey, name }`.
  Resolve a subsystem's componentIds by matching `component.id` (stableId).

- `n2Interfaces[]` — 177 entries, each:
  `{ id, kind:"n2", scope, sourceId, targetId, sourceLabel, targetLabel, flow, provenance }`.
  `scope` is one of `"subsystem"`, `"component"`, `"functional"`, or `"external"`.
  `sourceId`/`targetId` are stableIds; `sourceLabel`/`targetLabel` are human-readable
  endpoint names; `flow` is the item label.
  Count by scope: `component`=98, `subsystem`=57, `functional`=22.

- `functions[]` — 63 entries, each:
  `{ id, kind:"function", naturalKey, name, level, owner }`.
  `level` is `"L2"` or `"L3"`; 9 L2 (F1–F9) + 54 L3.
  L3 naturalKeys use the pattern `F<parent>.<n>` (e.g., `F1.3` — parent is `F1`).

All elements created by this skill must carry `provenanceSourceId` in their
`attributes`. See the **Provenance Mechanism** section below.

## Provenance Mechanism

`create_element(type, name, attributes)` spreads all keys from `attributes`
directly into `element.raw`. Passing
`{ provenanceSourceId: "<naturalKey>" }` in `attributes` causes
`element.raw.provenanceSourceId` to equal that string.

**Convention:** `provenanceSourceId` = the IR entity's `naturalKey`.

- Subsystem PartDefinition: `provenanceSourceId = subsystem.naturalKey`
- Component PartDefinition: `provenanceSourceId = component.naturalKey`
- Function ActionDefinition: `provenanceSourceId = function.naturalKey` (e.g., `"F1"`, `"F1.3"`)

This is the ONLY supported mechanism — no wrapper, no nesting, no intermediary.
The exact attribute key is `provenanceSourceId` (camelCase, string value).

`validate_model` flags any `PartDefinition` or `ActionDefinition` missing a
non-empty `raw.provenanceSourceId`.

`export_sysml` emits `// @source: <provenanceSourceId>` as a trailing comment
on each declaration line when the field is present.

## TF-10: Scalar-Attribute Cameo Import Caveat

`import ScalarValues::*;` does NOT resolve `Real`/`Integer`/`Boolean` in
Cameo CE's scratch editor (serializer lines ~443–447). For Cameo-bound models,
avoid scalar-typed component or part attributes, or explicitly note the
limitation in the export — scalar attribute types are a carried Cameo-import
risk (Phase 7). This skill does not emit scalar-typed attributes; this note
applies if future corpus data or serializer extensions introduce them.

## Common Workflow

1. **Read session state** — `get_project_state` to understand what exists.
2. **Read extracted.json** — parse the arrays listed in Data Source above.
3. **Query existing elements** — `query_elements` to find requirements and any
   previously created parts or actions.
4. **Present plan** — show what will be created, ask for approval.
5. **Create elements** — `create_element` calls, each with `provenanceSourceId`.
6. **Create relationships** — `create_relationship` for all typed edges.
7. **Validate** — `validate_model` before declaring the artifact complete.
8. **Export** — `export_sysml` to emit SysML v2 textual notation for review.

---

## Subcommand: `mbse-build bdd`

Build Block Definition Diagram — one PartDefinition per subsystem in
`subsystems[]`, with child component PartDefinitions resolved from
`components[]` via `subsystem.componentIds[]`, wired with
FeatureMembership containment. Generic over all 6 subsystems and 34
components — no fixed names.

### Sequence

**1. For each subsystem in `subsystems[]`, create a root PartDefinition:**

```
create_element(
  type: "PartDefinition",
  name: <subsystem.name>,
  attributes: {
    provenanceSourceId: <subsystem.naturalKey>
  }
)
```

Capture the returned `id` as `<subsystemId>`. Repeat for all 6 subsystems.

**2. For each stableId in `subsystem.componentIds[]`, resolve it to the
matching `components[]` entry (match on `component.id`) and create a
component PartDefinition:**

```
create_element(
  type: "PartDefinition",
  name: <component.name>,
  attributes: {
    provenanceSourceId: <component.naturalKey>
  }
)
```

Capture each returned `id` as `<componentId>`. Repeat for all 34 components.

**3. Wire containment with FeatureMembership:**

For each component belonging to a subsystem (as given by
`subsystem.componentIds[]`), create a FeatureMembership from the subsystem
PartDefinition to the component PartDefinition:

```
create_relationship(
  type: "FeatureMembership",
  source_id: <subsystemId>,
  target_id: <componentId>
)
```

**4. Wire satisfy edges to requirements** (query requirements first):

```
query_elements(type_filter: "RequirementDefinition")
```

For each `(reqId, functionId)` pair in `satisfies[]` where a component
PartDefinition is the appropriate satisfier, create:

```
create_relationship(
  type: "SatisfyRequirementUsage",
  source_id: <componentId>,
  target_id: <requirementId>
)
```

**5. Validate:**

```
validate_model()
```

Confirm `provenanceCoverage: 100` for PartDefinitions and no dangling edges.

---

## Subcommand: `mbse-build ibd`

Build Internal Block Diagram — PartUsages and PortUsages for each subsystem's
components, plus interior ConnectionUsages from `n2Interfaces[]` where scope
is `"subsystem"` (57 triples) or `"component"` (98 triples). Ports are derived
from the distinct endpoints in the relevant-scope triples — do not hardcode any
external interface list.

### Sequence

**1. Create PartUsages** for each component within its owning subsystem:

For each subsystem, and for each component in `subsystem.componentIds[]`:

```
create_element(
  type: "PartUsage",
  name: <camelCase short name derived from component.name>,
  attributes: {
    provenanceSourceId: <component.naturalKey>,
    owner: <subsystemId>
  }
)
```

Capture each returned `id` as `<partUsageId>`.

**2. Derive boundary PortUsages** on each subsystem from the endpoints
appearing in `n2Interfaces[]` where `scope == "subsystem"`:

For each distinct `sourceId` or `targetId` in the subsystem-scope triples that
resolves to a subsystem boundary endpoint (identified by `sourceLabel` or
`targetLabel`), create a PortUsage owned by the subsystem root:

```
create_element(
  type: "PortUsage",
  name: <sourceLabel or targetLabel>,
  attributes: {
    provenanceSourceId: <n2.id>,
    owner: <subsystemId>
  }
)
```

Do not hardcode port names. Use the label fields from `n2Interfaces[]` directly.

**3. Create internal PortUsages** on component PartUsages that participate in
component-scope flows (`scope == "component"`), derived from the endpoints in
those triples:

```
create_element(
  type: "PortUsage",
  name: <sourceLabel or targetLabel>,
  attributes: {
    provenanceSourceId: <n2.id + ":port">,
    owner: <partUsageId>
  }
)
```

**4. Create ConnectionUsages** from `n2Interfaces[]` where scope is
`"subsystem"` or `"component"`:

For each triple:

```
create_element(
  type: "ConnectionUsage",
  name: <flow>,
  attributes: {
    provenanceSourceId: <n2.id>,
    owner: <subsystemId>
  }
)
```

Wire the endpoints using the created PartUsage and PortUsage ids corresponding
to the triple's `sourceId` and `targetId`. If the connection endpoints cannot
be wired through `create_element` attributes alone, use `import_sysml` with
textual SysML v2 connection syntax:

```
connection <name> connect <partA>::<portA> to <partB>::<portB>;
```

If either path fails, invoke the **Fallback-Path Protocol** before continuing.

**5. Validate:**

```
validate_model()
```

Confirm: no orphaned ports, no dangling connector references, and
`provenanceCoverage: 100` for all PartUsage and PortUsage elements.

---

## Subcommand: `mbse-build activity`

Build ActionDefinitions for all F1–F9 function activities and their child
subfunctions. All 9 L2 functions and 54 L3 subfunctions in `functions[]` are
in scope. Wire N2 item flows from `n2Interfaces[]` where
`scope == "functional"` (22 triples). This subcommand is generic over
`functions[]` — there is no F1-only restriction.

Note: detailed function decomposition trees from `behaviorDecomp[]` are owned
by the `mbse-decompose` skill. This subcommand builds the function activities
and N2 item flows only.

### Sequence

**1. Create L2 root ActionDefinitions** (one per L2 function):

For each function in `functions[]` where `level == "L2"` (F1–F9):

```
create_element(
  type: "ActionDefinition",
  name: <function.name>,
  attributes: { provenanceSourceId: <function.naturalKey> }
)
```

Capture each returned `id` keyed by `function.naturalKey`.

**2. Create L3 child ActionDefinitions** (one per L3 function, owned by
their L2 parent):

For each function in `functions[]` where `level == "L3"`:

Identify the parent L2 naturalKey by the F-number prefix of the L3 naturalKey
(e.g., `F1.3`'s parent is `F1`; `F2.1`'s parent is `F2`). Retrieve the
parent's `id` from step 1.

```
create_element(
  type: "ActionDefinition",
  name: <function.name>,
  attributes: {
    provenanceSourceId: <function.naturalKey>,
    owner: <parentActionDefinitionId>
  }
)
```

**3. Wire FeatureMembership parent → child** for all L3 functions:

```
create_relationship(
  type: "FeatureMembership",
  source_id: <parentActionDefinitionId>,
  target_id: <childActionDefinitionId>
)
```

**4. Wire SuccessionUsage control flow** across sibling L3 children of
each L2 parent:

Order siblings by `naturalKey` ascending (lexicographic on the numeric suffix).
For each consecutive pair (F<x>.n, F<x>.n+1):

```
create_relationship(
  type: "Connector",
  source_id: <siblingActionDefinitionId_n>,
  target_id: <siblingActionDefinitionId_n+1>
)
```

**5. Build item flows from `n2Interfaces[]` where `scope == "functional"`
(22 triples):**

For each triple, resolve `sourceId` and `targetId` (stableIds) to the
ActionDefinition ids created in steps 1–2.

```
create_relationship(
  type: "ItemFlow",
  source_id: <resolvedSourceActionId>,
  target_id: <resolvedTargetActionId>
)
```

Label the flow with `n2.flow`.

**6. Wire SatisfyRequirementUsage edges** from functions to requirements
using the `satisfies[]` array:

```
query_elements(type_filter: "RequirementDefinition")
```

For each `{ reqId, functionId }` pair in `satisfies[]`:

```
create_relationship(
  type: "SatisfyRequirementUsage",
  source_id: <actionDefinitionId_for_functionId>,
  target_id: <requirementId_for_reqId>
)
```

**7. Validate:**

```
validate_model()
```

Confirm: ActionDefinitions have `provenanceCoverage: 100`, all L3 functions
are owned by their L2 parent, item flows resolve valid endpoints, no orphan
design elements.

---

## Fallback-Path Protocol

Whenever a step cannot be completed using the verified MCP tools below, the
pipeline MUST do the following before continuing:

1. **Stop and surface the gap** — tell the user:
   - Which operation could not be performed.
   - What was done instead (if anything), described precisely.
   - Whether any files or external state were modified outside the MCP tool surface.

2. **Emit a reproducible procedure** — provide explicit, step-by-step
   instructions the user can follow. Reference only tools or interfaces the
   user has access to, in imperative steps, not descriptions.

3. **Document in export** — when `export_sysml` is called, include a comment
   block listing every operation performed outside the MCP tool surface and
   the reproducible procedure for each.

4. **Do not complete the step silently** — a step that required a workaround
   is NOT done until the fallback procedure has been surfaced and the user has
   acknowledged it.

Operations that always trigger this protocol:
- Any file edited directly (topology.json, config files, seed data).
- Any GraphQL or REST mutation issued outside the MCP server.
- Any element ID or value looked up manually and hard-coded into a response.

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
