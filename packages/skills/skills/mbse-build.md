---
name: mbse-build
description: Build SysML v2 artifacts from requirements — BDD, IBD, activity, sequence, state, parametric
---

# MBSE Build

The construction layer. Build specific SysML v2 model artifacts from requirements via subcommands.

## Subcommands

### `mbse-build bdd`
Build Block Definition Diagram equivalent — part definitions, generalizations, compositions.
- Query requirements to understand what blocks are needed
- Generate PartDefinition elements for each subsystem/component
- Create composition relationships between parent and child parts
- Create satisfy relationships from parts to requirements

### `mbse-build ibd`
Build Internal Block Diagram equivalent — part usages, ports, connections, flows.
- Query existing PartDefinitions
- Generate PartUsage instances within containing blocks
- Define PortDefinition and PortUsage for interfaces
- Create ConnectionUsage between ports
- Define item flows through connections

### `mbse-build activity`
Build activity/action diagrams — action definitions, control and data flows.
- Query use cases and operational scenarios
- Generate ActionDefinition elements for each step
- Define control flow (succession) between actions
- Define data flow (item flow) between actions

### `mbse-build sequence`
Build sequence diagrams — interaction usages, message ordering.
- Query existing blocks and their interfaces
- Generate interaction definitions
- Define message ordering between participants

### `mbse-build state`
Build state machine diagrams — state definitions, transitions.
- Query blocks that have behavioral modes
- Generate StateDefinition and StateUsage elements
- Define transitions with triggers and guards

### `mbse-build parametric`
Build parametric/constraint diagrams — constraint definitions, analysis bindings.
- Query KPPs and quantitative requirements
- Generate ConstraintDefinition elements for analysis equations
- Bind constraint parameters to block attributes

### Keyword Reference

SysML v2 textual keywords used by build subcommands:

| Subcommand | SysML v2 Keywords |
|---|---|
| bdd | `part def`, `part`, `:>` (specialization) |
| ibd | `part`, `port def`, `port`, `connection`, `interface`, `flow` |
| activity | `action def`, `action`, `first...then`, `flow` |
| sequence | `action def`, `action`, `accept`, `send` |
| state | `state def`, `state`, `transition`, `entry`, `do`, `exit` |
| parametric | `constraint def`, `constraint`, `calc def`, `attribute` |

## Common Workflow

1. **Read session state** — know what phase we're in, what exists.
2. **Query existing elements** — understand what requirements and blocks exist.
3. **Present plan** — show what will be generated, ask for approval.
4. **Create elements** — via MCP `create_element` calls.
5. **Create relationships** — satisfy, compose, connect via `create_relationship`.
6. **Update session** — record what was built.
7. **Export and render** — write `.sysml` files, generate Mermaid diagram of the result.

## MCP Tools Used

- `query_elements`
- `create_element`
- `create_relationship`
- `query_relationships`
- `export_sysml`
- `get_project_state`
