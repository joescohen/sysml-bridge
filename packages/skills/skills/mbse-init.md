---
name: mbse-init
description: Bootstrap a new MBSE project — stakeholder needs, system context, CONOPS sketch
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Init

Initialize a new Model-Based Systems Engineering project from a natural language system description.

## Workflow

1. **Check for existing model** — call `get_project_state`. If elements already exist, warn and ask whether to extend or start fresh.
2. **Gather system description** — ask the user to describe the system in natural language. Prompt for:
   - What is the system? (one sentence)
   - Who are the stakeholders? (operators, maintainers, program managers, etc.)
   - What is the operational context? (environment, interfaces, constraints)
   - What are the top-level goals?
3. **Generate stakeholder needs** — from the description, derive stakeholder needs as SysML v2 RequirementDefinition elements. Each need gets a unique ID (SN-001, SN-002, ...).
4. **Generate CONOPS** — create UseCaseDefinition elements for primary operational scenarios.
5. **Create model structure** — via tools:
   - `create_element("Package", <project-name>, <root-package-id>)` if a sub-namespace is needed
   - `create_element("Requirement Definition", "SN-001: <need>", packageId)` for each stakeholder need
   - `create_element("Use Case Definition", "<scenario>", packageId)` for each use case
6. **Export and summarize** — call `export_sysml` to generate SysML v2 text. Present a summary of stakeholder needs and use cases, with element IDs for traceability downstream.
7. **Render overview** — call `/mbse-diagram` to generate a Mermaid context diagram showing the system boundary and actors.

## Phase State

There is no `.mbse-session.json` file. Track phase state conversationally:
- After init: summarize what was created (element names and @ids)
- In later turns: call `get_project_state` to rediscover current model contents

## MCP Tools Used

- `get_project_state`
- `create_element`
- `create_relationship`
- `export_sysml`

## Output

- RequirementDefinition elements (SN-001...) in the file-native model store
- UseCaseDefinition elements in the file-native model store
- Mermaid context diagram in chat
- Summary of stakeholder needs and use cases with @ids
