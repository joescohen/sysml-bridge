---
name: mbse-init
description: Bootstrap a new MBSE project — stakeholder needs, system context, CONOPS sketch
---

# MBSE Init

Initialize a new Model-Based Systems Engineering project from a natural language system description.

## Workflow

1. **Check for existing session** — read `.mbse-session.json`. If it exists, warn and ask whether to create a new project or continue.
2. **Gather system description** — ask the user to describe the system in natural language. Prompt for:
   - What is the system? (one sentence)
   - Who are the stakeholders? (operators, maintainers, program managers, etc.)
   - What is the operational context? (environment, interfaces, constraints)
   - What are the top-level goals?
3. **Generate stakeholder needs** — from the description, derive stakeholder needs as SysML v2 RequirementDefinition elements. Each need gets a unique ID (SN-001, SN-002, ...).
4. **Generate CONOPS** — create UseCaseDefinition elements for primary operational scenarios.
5. **Create model structure** — via MCP tools:
   - `create_element(type: "Package", name: <project-name>)`
   - `create_element(type: "RequirementDefinition", ...)` for each stakeholder need
   - `create_element(type: "UseCaseDefinition", ...)` for each use case
6. **Write session file** — create `.mbse-session.json` with phase: "inception", element history, and pending items.
7. **Export to disk** — call `export_sysml` to write `.sysml` files.
8. **Render overview** — call `/mbse-diagram` to generate a Mermaid context diagram.

## MCP Tools Used

- `create_element`
- `export_sysml`
- `get_project_state`

## Output

- `.mbse-session.json` created
- `.sysml` files written to `model/` directory
- Mermaid context diagram rendered
- Summary of stakeholder needs and use cases presented to user
