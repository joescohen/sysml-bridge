---
name: mbse-trade
description: Weighted trade studies — Pugh matrices, MOE/MOP scoring, decision rationale
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Trade

Conduct structured trade studies with traceability to requirements and KPPs.

## Workflow

1. **Define the decision** — what are we choosing between? (e.g., boom vs. drogue refueling)
2. **Identify alternatives** — 2-5 design alternatives.
3. **Define evaluation criteria** — derived from requirements and KPPs. Each criterion gets a weight (0-1, summing to 1).
4. **Score alternatives** — rate each alternative against each criterion (1-5 scale).
5. **Calculate weighted scores** — produce ranked results.
6. **Record decision** — create AnalysisCaseDefinition in the model with rationale.
7. **Trace to requirements** — link the chosen alternative and decision to relevant requirements.
8. **Update session** — record trade study completion.

## Output Format

| Criterion | Weight | Alt A | Alt B | Alt C |
|---|---|---|---|---|
| Range | 0.3 | 4 (1.2) | 3 (0.9) | 5 (1.5) |
| Weight | 0.2 | 3 (0.6) | 5 (1.0) | 2 (0.4) |
| ... | ... | ... | ... | ... |
| **Total** | **1.0** | **3.8** | **3.2** | **4.1** |

## MCP Tools Used

- `query_elements`
- `create_element`
- `create_relationship`
- `get_project_state`
