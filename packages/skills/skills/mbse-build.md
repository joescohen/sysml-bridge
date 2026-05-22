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
2. Create the root PartDefinition: `create_element("PartDefinition", systemName, packageId)`.
3. Create each subsystem as a PartUsage owned by the root:
   `create_element("PartUsage", subsystemName, rootPartDefId)`.
4. For each PartUsage that represents an independent subsystem, type it with
   `create_relationship("FeatureTyping", partUsageId, partDefId)` so the model is SysML v2 valid.
5. For specialization (block hierarchy), use
   `create_relationship("Subclassification", childPartDefId, parentPartDefId)`.
6. Use `create_relationship("SatisfyRequirementUsage", partDefId, reqId)` to link blocks to requirements.

**Never** call `create_bdd_structure` — it does not exist in the MCP server.

### `mbse-build ibd`
Build Internal Block Diagram — ports, connections, flows.

**Mandatory sequence for SysML v2 validity:**

1. **Find or create ports** — `create_element("PortUsage", portName, partUsageId)` for each
   interface port on each PartUsage that participates in connections.
2. **Create connections** — `create_relationship("ConnectionUsage", sourcePortId, targetPortId)`
   for each port-to-port interface. This is the available MCP path for port-to-port connections.
   If `create_relationship` cannot wire the endpoints correctly, use `import_sysml` with textual
   SysML v2 connection syntax: `connection <name> connect <partA>::<portA> to <partB>::<portB>;`
   — SysON parses this natively and wires `ReferenceSubsetting` to the referenced ports.
   If either path fails, invoke the **Fallback-Path Protocol** before continuing.
3. **Validate** — `validate_model()` must show no broken connector references and no orphaned
   ports before the IBD is considered complete.
4. **Update topology** — call `GET /api/projects/:id/elements` to resolve port `@id` values,
   then confirm the server's `/api/projects/:id/connections` endpoint returns the new edges.
   If connections are not surfaced by the server endpoint, invoke the Fallback-Path Protocol.

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
4. Use `export_sysml` to emit the state machine text; SysON renders state views from the model. Do NOT call `create_diagram` — it does not exist in the MCP server.

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
2. **Verify tool surface** — before executing any subcommand, confirm you are only calling tools
   from the verified list in `## Tools Used` below. If a subcommand step references a tool not
   in that list, do NOT attempt the call — invoke the **Fallback-Path Protocol** instead.
3. **Query existing elements** — `query_elements` to find requirements and blocks.
4. **Present plan** — show what will be created, ask for approval. Include any tool gaps
   identified in step 2 and their fallback paths.
5. **Create elements** — via `create_element` calls.
6. **Create relationships/connections** — use `create_relationship` for all typed relationships
   including port connections; use `import_sysml` for textual connection syntax if needed.
7. **Validate** — `validate_model` to confirm SysML v2 validity before moving on.
8. **Export** — `export_sysml` to generate SysML v2 textual notation for review.

## Fallback-Path Protocol

Whenever a step in any subcommand cannot be completed using the verified MCP tools below,
the pipeline MUST do the following before continuing:

1. **Stop and surface the gap** — tell the user:
   - Which operation could not be performed through chat.
   - What was done instead (if anything), described precisely.
   - Whether any files or external state were modified outside the MCP tool surface.

2. **Emit a reproducible procedure** — provide the user with explicit, step-by-step
   instructions they can follow to reproduce the same operation through chat or a documented
   manual process. The procedure must require no knowledge the user hasn't been given in this
   session, reference only tools or interfaces the user has access to, and be stated in
   imperative steps, not descriptions.

3. **Document in export** — when `export_sysml` is called, the exported artifact must include
   a comment block listing every operation performed outside the MCP tool surface and the
   reproducible procedure for each.

4. **Do not complete the step silently** — a step that required a workaround is NOT done until
   the fallback procedure has been surfaced and the user has acknowledged it.

**Operations that always trigger this protocol:**
- Any file edited directly (e.g., topology.json, config files, seed data).
- Any GraphQL or REST mutation issued outside the MCP server.
- Any element ID or value looked up manually and hard-coded into a file or response.
- Any SysON view or diagram created through the SysON UI rather than through tool calls.

## Tools Used

This project has two MCP servers. Check which one is active before calling tools.

### stdio MCP server (`packages/mcp-server`) — connects to SMAPS (port 9000)
Available when Claude Code connects via the `sysml-bridge` stdio MCP:
- `init_project`
- `query_elements`
- `get_project_state`
- `create_element`
- `create_relationship`     ← FeatureTyping, SatisfyRequirementUsage, Subclassification, etc.
- `query_relationships`
- `validate_model`
- `import_sysml`            ← parses SysML v2 text and commits elements via SMAPS
- `export_sysml`

### HTTP MCP server (`dashboard/server.js`) — connects to SysON (port 8080)
Available when working through the dashboard chat (port 6121):
- All of the above, plus:
- `create_connection`       ← creates ConnectionUsage in SysON + writes topology.json edge
- `create_bdd_structure`    ← creates PartDefinition + PartUsages + SysON General View
- `create_diagram`          ← creates a SysON diagram representation
- `delete_element`, `update_element`, `create_project`

**Rule:** if the Batmobile or any SysON-backed project is the target, the HTTP server tools
(`create_connection`, `create_bdd_structure`, `create_diagram`) are available. If working
against SMAPS, use only the stdio server tools. When in doubt, call `get_project_state`
first — if it returns elements, the current MCP connection is active for that project.
