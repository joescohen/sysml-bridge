---
name: mbse-kpp
description: Define and track Key Performance Parameters, MOEs, and MOPs
---

# MBSE KPP

Define, track, and assess Key Performance Parameters (KPPs), Measures of Effectiveness (MOEs), and Measures of Performance (MOPs).

## Hierarchy

- **KPP** — top-level performance parameters critical to mission success (must-meet thresholds)
- **MOE** — measures of how well the system achieves its mission (effectiveness)
- **MOP** — measures of specific technical performance characteristics

## Workflow

1. **Query requirements** — identify quantitative requirements that map to KPPs.
2. **Define KPPs** — for each, specify: name, threshold, objective, unit, current value.
3. **Define MOEs/MOPs** — supporting measures that roll up to KPPs.
4. **Create in model** — `create_element(type: "AttributeDefinition", ...)` with metadata for thresholds and units.
5. **Trace to requirements** — link KPPs to the requirements they measure.
6. **Generate KPP dashboard** — summary table showing current vs. threshold vs. objective.
7. **Update session** — record KPP state.

## Output Format

| KPP | Threshold | Objective | Current | Status |
|---|---|---|---|---|
| Max Refueling Time | ≤ 15 min | ≤ 10 min | 12 min | PASS |
| Transfer Rate | ≥ 400 gal/min | ≥ 600 gal/min | 450 gal/min | PASS |
| Miss Distance | ≤ 2.0 m | ≤ 0.5 m | — | TBD |

## MCP Tools Used

- `query_elements`
- `create_element`
- `create_relationship`
- `get_project_state`
