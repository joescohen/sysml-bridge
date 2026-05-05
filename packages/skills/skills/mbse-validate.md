---
name: mbse-validate
description: Check model completeness, consistency, orphaned elements, coverage gaps
---

# MBSE Validate

Run comprehensive validation checks against the live SysON model.

## Checks Performed

The `validate_model` tool runs automatically:

1. **Broken connections** — `ConnectionUsage` elements missing `connectorEnd` references.
   Fix: `create_connection(sourcePortId, targetPortId)` to replace them.
2. **Orphaned ports** — `PortUsage` elements with no connection in SysON or topology.
   Fix: `create_connection` to wire them up, or `delete_element` if they are unused.
3. **Untyped parts** — `PartUsage` elements without a `FeatureTyping` relationship.
   Fix: `create_relationship(FeatureTyping, partUsageId, partDefId)`.
4. **Unsatisfied requirements** — `RequirementDefinition` elements with no
   `SatisfyRequirementUsage` or `VerifyRequirementUsage` pointing to them.
   Fix: `create_relationship(SatisfyRequirementUsage, partId, reqId)`.
5. **Empty IBD** — ports exist but no connections in SysON or topology.

## Workflow

1. **Get model snapshot** — `get_project_state` to understand element counts and types.
2. **Run automated checks** — `validate_model()`. Returns `valid: true/false` plus a
   structured `issues` array with `severity` (error/warning) and `category`.
3. **Query relationships** — `query_relationships` to surface coverage gaps not caught
   by the automated checks (e.g., missing allocation links, partial traceability).
4. **Classify findings** — errors must be fixed before the model is considered valid;
   warnings should be addressed for a complete model.
5. **Fix issues** — use the appropriate tool per finding type (see checks above).
6. **Re-validate** — run `validate_model` again to confirm all errors are resolved.
7. **Export** — `export_sysml` to generate SysML v2 textual notation. Any `WARNING`
   comments in the output indicate elements that need attention.

## Severity guide

| Severity | Meaning                                         | Action required?  |
|----------|-------------------------------------------------|-------------------|
| error    | SysML v2 structural violation or missing link   | Yes, before export |
| warning  | Model is incomplete but not structurally broken | Recommended        |

## Tools Used

- `validate_model`
- `get_project_state`
- `query_elements`
- `query_relationships`
- `create_connection`
- `create_relationship`
- `export_sysml`
