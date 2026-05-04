---
name: mbse-trace
description: Build and update traceability links — requirements to model elements to verification
---

# MBSE Trace

Build, update, and visualize traceability relationships across the model.

## Workflow

1. **Query all elements and relationships** — build a complete picture of current traceability.
2. **Identify gaps** — requirements with no satisfying element, elements with no requirement, verification cases with no linked requirement.
3. **Propose new links** — suggest satisfy, verify, refine, allocate relationships based on naming and context.
4. **Create relationships** — via `create_relationship` for approved links.
5. **Generate traceability matrix** — requirements (rows) × model elements (columns) showing satisfy/verify/allocate.
6. **Update session** — record traceability state.

## Relationship Types

| Type | Meaning | Example |
|---|---|---|
| satisfy | Block satisfies a requirement | Boom PartDef → satisfy → SYS-042 |
| verify | Test case verifies a requirement | BoomTest → verify → SYS-042 |
| refine | Requirement refines stakeholder need | SYS-042 → refine → SN-003 |
| allocate | Function allocated to component | RefuelAction → allocate → Boom |

## MCP Tools Used

- `query_elements`
- `query_relationships`
- `create_relationship`
- `get_traceability`
- `get_project_state`
