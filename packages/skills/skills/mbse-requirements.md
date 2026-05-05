---
name: mbse-requirements
description: Generate or refine system requirements — IDs, hierarchy, verifiability assessment
---

# MBSE Requirements

Generate, refine, or extend system requirements from stakeholder needs and CONOPS.

## Workflow

1. **Query existing model** — `get_project_state` to see what elements exist, then `query_elements(type_filter: "RequirementDefinition")` to see current requirements and `query_elements(type_filter: "UseCaseDefinition")` to see CONOPS.
2. **Determine mode**:
   - **Fresh**: No requirements exist yet → derive system requirements from stakeholder needs (SN-* elements). Structure as parent-child hierarchy. Assign IDs (SYS-001, SYS-001.1, ...).
   - **Refine**: Requirements exist → ask the user what to add, change, or extend. Show current requirements first.
3. **Assess verifiability** — for each requirement, classify as Quantitative (measurable threshold) or Qualitative (compliance statement). Flag vague requirements.
4. **Create in model** — `create_element("Requirement Definition", "SYS-001: <text>", packageId)` for each requirement.
5. **Trace to stakeholder needs** — `create_relationship("Dependency", reqId, snId)` for each requirement that derives from a stakeholder need.
6. **Summarize** — present the requirement list with IDs, verifiability classification, and @ids for downstream use in `/mbse-trace` and `/mbse-build`.

## MCP Tools Used

- `get_project_state`
- `query_elements`
- `create_element`
- `create_relationship`
- `export_sysml`

## Output

- RequirementDefinition elements (SYS-001...) in SysON
- Dependency relationships to stakeholder needs
- Verifiability assessment (% quantitative)
- Requirement summary with @ids
