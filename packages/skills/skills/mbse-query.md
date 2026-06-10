---
name: mbse-query
description: Answer natural-language questions grounded in actual model elements
---

# MBSE Query

Answer natural-language questions about the model by grounding every answer in the file-native model store (queried via `query_elements` and `query_relationships`).

## Workflow

1. **Identify the question type** — element lookup, relationship traversal, coverage gap, or
   general model health.
2. **Query the model** — use the minimal set of tool calls needed to answer:
   - Element questions → `query_elements([type_filter])`
   - Relationship questions → `query_relationships([element_id], [type_filter])`
   - Overall health → `get_project_state` then `validate_model`
3. **Ground the answer** — every factual claim must cite a specific element `@id` or relationship
   returned by a tool call. Clearly separate observed facts from inferences or recommendations.
4. **Report gaps** — if the question asks about something not yet in the model, say so explicitly
   and offer to create it.

## Common question patterns

| Question                              | Tool sequence                                                   |
|---------------------------------------|-----------------------------------------------------------------|
| "What requirements exist?"            | `query_elements("RequirementDefinition")`                       |
| "Is REQ-001 satisfied?"               | `query_relationships(reqId, "SatisfyRequirementUsage")`         |
| "What connects to port X?"            | `query_relationships(portId, "ConnectionUsage")`                |
| "What parts exist?"                   | `query_elements("PartDefinition")`                              |
| "Show me the full model"              | `get_project_state`                                             |
| "Are there validity issues?"          | `validate_model`                                                |
| "What is allocated to subsystem Y?"   | `query_relationships(subsystemId, "AllocationUsage")`           |

## Traceability queries

There is no `get_traceability` tool. Build traceability answers from:

1. `query_relationships(type_filter: "SatisfyRequirementUsage")` — all satisfy links
2. `query_relationships(type_filter: "VerifyRequirementUsage")` — all verify links
3. `query_relationships(type_filter: "AllocationUsage")` — all allocation links
4. Cross-reference `source` and `target` IDs against `query_elements` results to produce
   a human-readable traceability matrix.

## Tools Used

- `query_elements`
- `query_relationships`
- `get_project_state`
- `validate_model`
