---
name: mbse-build
description: Build SysML v2 structural and behavioral artifacts — BDD, IBD, and F1 activity — from cc-extracted.json using file-native MCP tools only.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Build

The construction layer. Build SysML v2 model artifacts from the
`examples/angars/model/cc-extracted.json` data using the file-native MCP
tools. This skill covers three artifacts in the first cut: BDD, IBD, and F1
activity. The sequence diagram is deferred to Phase 2 and is explicitly out
of scope here.

## Data Source

Read `examples/angars/model/cc-extracted.json` before issuing any tool calls.
Relevant arrays:

- `subsystem` — `"Command & Control"` (the root PartDefinition name)
- `components[]` — 6 entries: `{ name }` each
- `functions[]` — F1 and F1.x entries (and F8/F8.x for future phases)
- `satisfies[]` — `{ reqId, functionId }` pairs

All elements created by this skill must carry `provenanceSourceId` in their
`attributes`. See the **Provenance Mechanism** section below.

## Provenance Mechanism

`create_element(type, name, attributes)` spreads all keys from `attributes`
directly into `element.raw`. Passing
`{ provenanceSourceId: "<source-id>" }` in `attributes` causes
`element.raw.provenanceSourceId` to equal that string. This is the ONLY
supported mechanism — no wrapper, no nesting, no intermediary. The exact
attribute key is `provenanceSourceId` (camelCase, string value).

`validate_model` flags any `PartDefinition` or `ActionDefinition` missing a
non-empty `raw.provenanceSourceId`.

`export_sysml` emits `// @source: <provenanceSourceId>` as a trailing comment
on each declaration line when the field is present.

## Common Workflow

1. **Read session state** — `get_project_state` to understand what exists.
2. **Query existing elements** — `query_elements` to find requirements and any
   previously created parts or actions.
3. **Present plan** — show what will be created, ask for approval.
4. **Create elements** — `create_element` calls, each with `provenanceSourceId`.
5. **Create relationships** — `create_relationship` for all typed edges.
6. **Validate** — `validate_model` before declaring the artifact complete.
7. **Export** — `export_sysml` to emit SysML v2 textual notation for review.

---

## Subcommand: `mbse-build bdd`

Build Block Definition Diagram — the Command & Control Subsystem PartDefinition
and its 6 component PartDefinitions, wired with FeatureMembership containment.

### Sequence

**1. Create the subsystem root PartDefinition:**

```
create_element(
  type: "PartDefinition",
  name: "Command & Control Subsystem",
  attributes: {
    provenanceSourceId: "Command & Control"
  }
)
```

Capture the returned `id` as `<subsystemId>`.

**2. Create each component PartDefinition** (6 entries from `components[]`):

```
create_element(
  type: "PartDefinition",
  name: "C&C Power Module",
  attributes: { provenanceSourceId: "component:C&C Power Module" }
)

create_element(
  type: "PartDefinition",
  name: "Operator Control Plane",
  attributes: { provenanceSourceId: "component:Operator Control Plane" }
)

create_element(
  type: "PartDefinition",
  name: "Operator Console Module",
  attributes: { provenanceSourceId: "component:Operator Console Module" }
)

create_element(
  type: "PartDefinition",
  name: "HMI Panel & Displays",
  attributes: { provenanceSourceId: "component:HMI Panel & Displays" }
)

create_element(
  type: "PartDefinition",
  name: "Haptic Alert Unit",
  attributes: { provenanceSourceId: "component:Haptic Alert Unit" }
)

create_element(
  type: "PartDefinition",
  name: "Flight Control Module",
  attributes: { provenanceSourceId: "component:Flight Control Module" }
)
```

Capture each returned `id` as `<componentId_*>`.

**3. Wire containment with FeatureMembership:**

For each component, create a FeatureMembership from the subsystem root to the
component PartDefinition:

```
create_relationship(
  type: "FeatureMembership",
  source_id: <subsystemId>,
  target_id: <componentId_*>
)
```

Repeat for all 6 components.

**4. Wire satisfy edges to requirements** (query requirements first):

```
query_elements(type_filter: "RequirementDefinition")
```

For each `(reqId, functionId)` pair in `satisfies[]` where a component
PartDefinition can be the satisfier, create:

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

Build Internal Block Diagram — PartUsages, PortUsages, and interior
ConnectionUsages for the C&C Subsystem, including boundary ports for the
6 external interfaces.

### External interfaces (boundary ports)

The C&C Subsystem has the following external interfaces that require boundary
PortUsages on the subsystem root PartDefinition:

- `AGNS_Interface` (AGNS link)
- `FuelTransfer_Interface` (Fuel Transfer link)
- `Comms_Interface` (Communications link)
- `Processing_Interface` (Processing link)
- `Power_Interface` (Power bus)
- `ExternalOperator_Interface` (External operator link)

### Sequence

**1. Create PartUsages** for each component, owned by the subsystem root:

```
create_element(
  type: "PartUsage",
  name: "ccPowerModule",
  attributes: {
    provenanceSourceId: "component:C&C Power Module",
    owner: "<subsystemId>"
  }
)
```

Repeat for all 6 components. Use camelCase short names as usage names.
Capture each returned `id` as `<partUsageId_*>`.

**2. Create boundary PortUsages** on the subsystem root:

```
create_element(
  type: "PortUsage",
  name: "AGNS_Interface",
  attributes: {
    provenanceSourceId: "external:AGNS",
    owner: "<subsystemId>"
  }
)
```

Repeat for the remaining 5 external interfaces listed above.
Capture each returned `id` as `<boundaryPortId_*>`.

**3. Create internal PortUsages** on component PartUsages that participate
in internal flows (add as needed based on the interface topology):

```
create_element(
  type: "PortUsage",
  name: "<portName>",
  attributes: {
    provenanceSourceId: "internal-port:<componentName>:<portName>",
    owner: "<partUsageId>"
  }
)
```

**4. Create interior ConnectionUsages** between component ports:

```
create_element(
  type: "ConnectionUsage",
  name: "<connectionName>",
  attributes: {
    provenanceSourceId: "connection:<srcComponent>-<tgtComponent>",
    owner: "<subsystemId>"
  }
)
```

Then wire the endpoints using FeatureMembership or by adding source/target
attributes. If the connection endpoints cannot be wired through
`create_element` attributes alone, use `import_sysml` with textual SysML v2
connection syntax:

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

## Subcommand: `mbse-build activity` (F1 ONLY — first cut)

Build ActionDefinitions for the F1 "Manage Refueling Requests" function and
its 6 child behaviors (F1.1–F1.6). Control flow is modeled with
SuccessionUsage edges. **F8 and the sequence diagram are out of scope for
this first cut (Phase 2).**

### F1 tree from `functions[]`

```
F1   Manage Refueling Requests        (L2)
F1.1 Receive & Authenticate Request   (L3, owner: F1)
F1.2 Validate Fuel Capacity           (L3, owner: F1)
F1.3 Prioritize Requests              (L3, owner: F1)
F1.4 Generate Schedule                (L3, owner: F1)
F1.5 Update Schedule Dynamically      (L3, owner: F1)
F1.6 Transmit Status & Reports        (L3, owner: F1)
```

### Sequence

**1. Create F1 root ActionDefinition:**

```
create_element(
  type: "ActionDefinition",
  name: "Manage Refueling Requests",
  attributes: { provenanceSourceId: "F1" }
)
```

Capture returned `id` as `<f1Id>`.

**2. Create child ActionDefinitions** for F1.1–F1.6, owned by F1:

```
create_element(
  type: "ActionDefinition",
  name: "Receive & Authenticate Request",
  attributes: {
    provenanceSourceId: "F1.1",
    owner: "<f1Id>"
  }
)

create_element(
  type: "ActionDefinition",
  name: "Validate Fuel Capacity",
  attributes: {
    provenanceSourceId: "F1.2",
    owner: "<f1Id>"
  }
)

create_element(
  type: "ActionDefinition",
  name: "Prioritize Requests",
  attributes: {
    provenanceSourceId: "F1.3",
    owner: "<f1Id>"
  }
)

create_element(
  type: "ActionDefinition",
  name: "Generate Schedule",
  attributes: {
    provenanceSourceId: "F1.4",
    owner: "<f1Id>"
  }
)

create_element(
  type: "ActionDefinition",
  name: "Update Schedule Dynamically",
  attributes: {
    provenanceSourceId: "F1.5",
    owner: "<f1Id>"
  }
)

create_element(
  type: "ActionDefinition",
  name: "Transmit Status & Reports",
  attributes: {
    provenanceSourceId: "F1.6",
    owner: "<f1Id>"
  }
)
```

Capture each returned `id` as `<f1_1Id>` through `<f1_6Id>`.

**3. Wire control flow with SuccessionUsage edges:**

```
create_relationship(
  type: "FeatureMembership",
  source_id: <f1Id>,
  target_id: <f1_1Id>
)
```

Then create SuccessionUsage (sequential control flow F1.1 → F1.2 → ... → F1.6):

```
create_relationship(
  type: "Connector",
  source_id: <f1_1Id>,
  target_id: <f1_2Id>
)
```

Repeat for F1.2→F1.3, F1.3→F1.4, F1.4→F1.5, F1.5→F1.6.

**4. Wire SatisfyRequirementUsage edges** from functions to requirements
using the `satisfies[]` array. For each `{ reqId, functionId }` pair where
`functionId` starts with `F1`:

```
create_relationship(
  type: "SatisfyRequirementUsage",
  source_id: <actionDefinitionId_for_functionId>,
  target_id: <requirementId_for_reqId>
)
```

Query requirements first:
```
query_elements(type_filter: "RequirementDefinition")
```

**5. Validate:**

```
validate_model()
```

Confirm: ActionDefinitions have `provenanceCoverage: 100`, all F1.x functions
are forward-traced via SatisfyRequirementUsage, no orphan design elements.

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

---

## Phase 2 deferred items

The following are explicitly out of scope for this first cut and will be
addressed in Phase 2:

- F8 "Manage HMI" ActionDefinitions and its F8.1–F8.9 children
- Sequence diagram (lifecycle / message-passing view)
- AllocationUsage edges from functions to physical components
- VerifyRequirementUsage edges to verification cases
