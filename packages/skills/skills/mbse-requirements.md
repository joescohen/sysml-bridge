---
name: mbse-requirements
description: Generate or refine system requirements — IDs, hierarchy, verifiability assessment
---

# MBSE Requirements

Generate, refine, or extend system requirements from stakeholder needs and CONOPS.

## Phase Awareness

- **During inception/requirements phase**: Generate fresh requirements from stakeholder needs
- **During later phases**: Refine mode — ask what to add or change, trace to existing elements

## Workflow

1. **Read session state** — determine current phase and what exists.
2. **Query existing model** — `query_elements(type: "RequirementDefinition")` and `query_elements(type: "UseCaseDefinition")` to understand current state.
3. **Generate or refine**:
   - **Fresh**: Derive requirements from stakeholder needs. Structure as parent-child hierarchy. Assign IDs (SYS-001, SYS-001.1, etc.).
   - **Refine**: Ask user what to add/change. Show existing requirements first.
4. **Assess verifiability** — for each requirement, classify as Quantitative or Qualitative. Flag vague requirements.
5. **Create in model** — `create_element(type: "RequirementDefinition", ...)` for each requirement.
6. **Trace to stakeholder needs** — `create_relationship(type: "Dependency", ...)` from requirements to stakeholder needs (derive relationship).
7. **Update session** — increment element counts, update phase if needed, add pending items.
8. **Export and summarize** — write `.sysml` files, present requirement summary with coverage stats.

## MCP Tools Used

- `query_elements`
- `create_element`
- `create_relationship`
- `export_sysml`
- `get_project_state`

## Output

- Requirements created/updated in model
- Parent-child hierarchy established
- Traceability to stakeholder needs
- Verifiability assessment (% quantitative)
- Updated `.sysml` files
