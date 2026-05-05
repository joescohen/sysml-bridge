---
name: mbse-build
description: Build SysML v2 artifacts from requirements — BDD, IBD, activity, sequence, state, parametric
---

# MBSE Build

The construction layer. Build specific SysML v2 model artifacts from requirements via subcommands.

## Subcommands

### `mbse-build bdd`
Build Block Definition Diagram — part definitions, generalizations, compositions.

1. Query requirements to understand what blocks are needed.
2. Use `create_bdd_structure(system_name, subsystems)` to create the system PartDefinition,
   owned PartUsages, and a SysON General View representation in one step.
3. For each PartUsage that represents an independent subsystem, type it with
   `create_relationship(FeatureTyping, partUsageId, partDefId)` so the model is SysML v2 valid.
4. Use `create_relationship(SatisfyRequirementUsage, partDefId, reqId)` to link blocks to requirements.

**Never** create a BDD by dropping unowned peer PartDefinitions onto a blank General View.

### `mbse-build ibd`
Build Internal Block Diagram — ports, connections, flows.

**Mandatory sequence for SysML v2 validity:**

1. **Find or create ports** — `create_element("Port", portName, partDefId)` for each interface
   port on each PartDefinition that participates in connections.
2. **Create connections** — `create_connection(sourcePortId, targetPortId, connectionName)` for
   each port-to-port interface.
   - This creates a `ConnectionUsage` with two `ConnectorEnd` elements in SysON.
   - `ConnectorEnd.connectedFeature` references the `PortUsage` on each side.
   - This is the **only** SysML v2-valid path. Do NOT use `create_element("Connection")` alone
     or the deprecated `create_ibd_connection`.
3. **Validate** — `validate_model()` must show no broken `connectorEnd` references and no
   orphaned ports before the IBD is considered complete.
4. **Create SysON view** — `create_diagram(partDefId, "Interconnection View", name)`.

### `mbse-build activity`
Build activity/action diagrams — action definitions, control and data flows.

1. Query use cases and operational scenarios.
2. `create_element("Action Definition", name, packageId)` for each step.
3. `create_element("Action", name, actionDefId)` for usages.
4. `create_relationship(SuccessionUsage, actionA_id, actionB_id)` for control flow ordering.
5. `create_relationship(FlowConnectionUsage, outPort_id, inPort_id)` for data flows.

### `mbse-build state`
Build state machine diagrams — state definitions, transitions.

1. Query blocks that have behavioral modes.
2. `create_element("State Definition", name, packageId)` and
   `create_element("State", name, stateDefId)` for each state.
3. `create_relationship(TransitionUsage, fromStateId, toStateId)` for transitions.
4. `create_diagram(stateDefId, "State Transition View", name)` to create the SysON view.

### `mbse-build parametric`
Build parametric/constraint diagrams — constraint definitions, analysis bindings.

1. Query KPPs and quantitative requirements.
2. `create_element("Constraint Definition", name, packageId)` for analysis equations.
3. `create_element("Attribute", name, constraintDefId)` for parameters.
4. `create_relationship(BindingConnector, paramId, attributeId)` to bind parameters to block attributes.

### SysML v2 Keyword Reference

| Subcommand  | SysML v2 Keywords                                      |
|-------------|--------------------------------------------------------|
| bdd         | `part def`, `part : Type`, `:>` specialization        |
| ibd         | `port`, `connection connect X to Y`, `flow from X to Y` |
| activity    | `action def`, `action`, `first ... then`, `flow`      |
| state       | `state def`, `state`, `transition then`               |
| parametric  | `constraint def`, `constraint`, `attribute`, `bind`   |

## Common Workflow

1. **Read session state** — call `get_project_state` to understand what exists.
2. **Query existing elements** — `query_elements` to find requirements and blocks.
3. **Present plan** — show what will be created, ask for approval.
4. **Create elements** — via `create_element` calls.
5. **Create connections** — port-to-port: `create_connection`. Semantic: `create_relationship`.
6. **Validate** — `validate_model` to confirm SysML v2 validity before moving on.
7. **Export** — `export_sysml` to generate SysML v2 textual notation for review.

## Tools Used

- `query_elements`
- `get_project_state`
- `create_element`
- `create_bdd_structure`
- `create_connection`        ← port-to-port connections (SysML v2 valid)
- `create_relationship`     ← typed relationships (FeatureTyping, SatisfyRequirementUsage, etc.)
- `query_relationships`
- `validate_model`
- `create_diagram`
- `export_sysml`
