---
name: mbse-query
description: Ask natural language questions about the model — get answers grounded in actual model elements
---

# MBSE Query

Ask questions about the model in natural language. Returns answers grounded in real model elements, not hallucinated structure.

## Example Queries

- "What requirements does the Boom subsystem satisfy?"
- "Which requirements have no verification case?"
- "Show me all connections between the Tanker and Receiver"
- "What is the current KPP status?"
- "How many requirements are quantitative vs qualitative?"

## Workflow

1. **Parse the question** — determine what elements, relationships, or metrics are being asked about.
2. **Query the model** — use appropriate MCP tools to retrieve real data.
3. **Synthesize answer** — present the answer in natural language, citing specific element IDs and names.
4. **Offer follow-up** — suggest related queries or next actions.

## MCP Tools Used

- `query_elements`
- `query_relationships`
- `get_traceability`
- `get_project_state`
