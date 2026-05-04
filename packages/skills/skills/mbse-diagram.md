---
name: mbse-diagram
description: Render model structure as Mermaid or PlantUML diagrams — BDD, IBD, sequence, activity, state
---

# MBSE Diagram

Generate visual diagram renderings from the current model state.

## Diagram Types

| Type | Mermaid Format | What it shows |
|---|---|---|
| `bdd` | classDiagram | Part definitions, compositions, generalizations |
| `ibd` | flowchart | Part usages, ports, connections, flows |
| `activity` | flowchart | Action definitions, control/data flow |
| `sequence` | sequenceDiagram | Interaction participants and messages |
| `state` | stateDiagram-v2 | States, transitions, triggers |
| `context` | flowchart | System boundary, external actors, interfaces |
| `trace` | flowchart | Requirements → blocks → verification links |

## Workflow

1. **Determine diagram type** — from subcommand or infer from context.
2. **Query relevant elements** — filter by type appropriate to the diagram.
3. **Query relationships** — get structural/behavioral connections.
4. **Generate Mermaid** — build the diagram syntax from elements and relationships.
5. **Present to user** — render as a Mermaid code block.

## MCP Tools Used

- `query_elements`
- `query_relationships`
- `get_traceability`
