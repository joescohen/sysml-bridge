---
name: mbse-views
description: Generate SysML v2 viewpoints and views — stakeholder-specific model slices
---

# MBSE Views

Generate viewpoints and views that present the model from different stakeholder perspectives.

## Standard Viewpoints

- **Operator View** — operational scenarios, use cases, interfaces the operator interacts with
- **Maintainer View** — physical architecture, replaceable units, maintenance procedures
- **Program Manager View** — requirements coverage, KPP status, schedule-relevant metrics
- **Safety View** — hazard analysis, safety-critical requirements, fault trees
- **Test View** — verification cases, test procedures, coverage matrix

## Workflow

1. **Select viewpoint** — ask user which stakeholder view to generate, or generate all.
2. **Query relevant elements** — filter model by element types relevant to the viewpoint.
3. **Create ViewpointDefinition** — `create_element(type: "ViewpointDefinition", ...)`.
4. **Create ViewDefinition** — `create_element(type: "ViewDefinition", ...)` scoped to relevant elements.
5. **Render view** — generate Mermaid diagram showing only elements in scope.
6. **Update session** — record views created.

## MCP Tools Used

- `query_elements`
- `query_relationships`
- `create_element`
- `export_sysml`
- `get_project_state`
