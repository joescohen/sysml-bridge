---
name: mbse-validate
description: Check model completeness, consistency, orphaned elements, coverage gaps
---

# MBSE Validate

Run comprehensive validation checks against the model.

## Checks Performed

1. **Requirement coverage** — what % of requirements have a satisfying element?
2. **Orphaned elements** — blocks, actions, states with no requirement traceability.
3. **Dangling ports** — port definitions with no connections.
4. **Missing verification** — requirements with no verification case.
5. **Hierarchy consistency** — child requirements covered but parent not, or vice versa.
6. **Session reconciliation** — sync `.mbse-session.json` with actual SMAPS model state.

## Workflow

1. **Reconcile session** — call `get_project_state` and update `.mbse-session.json` if drifted.
2. **Run all checks** — query elements and relationships systematically.
3. **Classify findings** — Error (must fix), Warning (should fix), Info (awareness).
4. **Present report** — structured validation report with actionable items.
5. **Update pending list** — add findings to session pending items.

## MCP Tools Used

- `validate_model`
- `query_elements`
- `query_relationships`
- `get_project_state`
