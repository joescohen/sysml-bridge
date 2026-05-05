---
name: mbse-trace
description: Build and update traceability links — requirements to model elements to verification
---

# MBSE Trace

Build, update, and visualize traceability relationships across the model.

## Workflow

1. **Query all elements** — `query_elements` to get the full element set (requirements, parts, actions).
2. **Query existing relationships** — `query_relationships` to understand current traceability coverage.
3. **Identify gaps** — requirements with no satisfying element, elements with no requirement link,
   verification cases with no linked requirement.
4. **Propose new links** — suggest satisfy, verify, allocate relationships based on names and context.
   Present the proposed matrix to the user before creating anything.
5. **Create relationships** — `create_relationship` for each approved link.
6. **Re-query to confirm** — `query_relationships` after creation to verify links persisted in SysON.
7. **Generate traceability matrix** — build a requirements × model elements table from the
   `query_relationships` results showing satisfy/verify/allocate coverage.

## Relationship Types

| Type                      | `relationship_type` arg       | Meaning                                      |
|---------------------------|-------------------------------|----------------------------------------------|
| SatisfyRequirementUsage   | `SatisfyRequirementUsage`     | Block/part satisfies a requirement           |
| VerifyRequirementUsage    | `VerifyRequirementUsage`      | Test/verification case verifies requirement  |
| AllocationUsage           | `Allocation`                  | Function allocated to a physical component   |
| Dependency                | `Dependency`                  | General dependency (derive, refine)          |
| FeatureTyping             | `FeatureTyping`               | Part usage typed by part definition          |

## Example calls

```
create_relationship("SatisfyRequirementUsage", boomPartDefId, fuelTransferReqId)
create_relationship("VerifyRequirementUsage", boomTestCaseId, fuelTransferReqId)
create_relationship("Allocation", refuelActionId, boomSubsystemId)
```

## Building the traceability matrix

There is no single `get_traceability` tool. Build the matrix from:
1. `query_elements(type_filter: "RequirementDefinition")` — all requirements (rows)
2. `query_elements(type_filter: "PartDefinition")` — all blocks (columns)
3. `query_relationships(type_filter: "SatisfyRequirementUsage")` — satisfy links
4. `query_relationships(type_filter: "VerifyRequirementUsage")` — verify links
5. `query_relationships(type_filter: "AllocationUsage")` — allocation links

Cross-reference the `source` and `target` IDs from step 3-5 against step 1-2 to fill the matrix.

## Tools Used

- `query_elements`
- `query_relationships`
- `create_relationship`
- `get_project_state`
