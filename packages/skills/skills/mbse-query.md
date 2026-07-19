---
name: mbse-query
description: Answer natural-language questions about the system, grounded strictly in live MCP tool results — never from memory of the model
---

# /mbse-query — read-only questions

Answer natural-language questions about the model by querying the live store through the MCP
tools. This skill is **read-only**: it never mutates the model.

## The one rule

**Never answer from memory of the model.** Every factual claim about the model — what elements
exist, how they relate, whether a requirement is satisfied — comes from a fresh tool result in
the current turn. Do not reuse element names, IDs, or counts you "remember" from an earlier
turn; re-query. A model the user edited between turns will have moved on, and a stale answer is
worse than a slow one. If a claim is not backed by a tool result you just read, do not make it.

## Tools

- `query_elements` — list model elements, optionally filtered by type
  (e.g. `RequirementDefinition`, `PartDefinition`, `ActionDefinition`). Returns each element's
  id, name, and attributes.
- `query_relationships` — list relationships, optionally filtered by type or endpoint
  (e.g. `SatisfyRequirementUsage`, `AllocationUsage`, `ConnectionUsage`). Returns source and
  target ids.
- `get_project_state` — the whole model at once: every element and relationship. Use for
  "show me the model" or when a question spans many types.

Do NOT call any write tool from this skill — no `create_element`, `create_relationship`,
`update_element`, `delete_element`, or `import_sysml`. If the user asks for a change, hand off
to `/mbse-edit`.

## Routing a question

| Question shape | Tool sequence |
|----------------|---------------|
| "What requirements exist?" | `query_elements` (type `RequirementDefinition`) |
| "What parts / actions exist?" | `query_elements` (the matching type) |
| "Is REQ-X satisfied?" | `query_relationships` filtered to that requirement + `SatisfyRequirementUsage` |
| "What is allocated to Y?" | `query_relationships` filtered to Y + `AllocationUsage` |
| "What connects to port P?" | `query_relationships` filtered to P |
| "Show me the whole model" | `get_project_state` |
| "How many X are there?" | `query_elements` (matching type), then count the returned array |

There is no dedicated "get traceability" tool. Build a traceability answer by combining
`query_relationships` results across the trace types (satisfy / allocate / verify) and
cross-referencing endpoint ids against `query_elements` to render human-readable names.

## Answering

1. Choose the tool(s) from the table. Issue the calls.
2. Read the results. Derive the answer only from what came back.
3. Report element names alongside their ids so the user can trace anything downstream.
4. If a query returns nothing, say the model contains no such element — do not guess that it
   "probably" exists. Absence in the tool result is the answer.

For questions about lifecycle position rather than model contents, defer to `/mbse` (which
reads the session file). For "are there validity issues", call `validate_model` and summarize
its `issues` and `findings` — but do not treat that as advancing the lifecycle; only `/mbse`
routes stages.
