---
name: mbse-verify
description: V&V planning — map requirements to verification methods (Test/Analysis/Inspection/Demonstration)
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Verify

Plan verification and validation for all requirements.

## Verification Methods (TAID)

- **Test** — physical or simulation test that produces measurable results
- **Analysis** — mathematical or computational analysis (modeling, simulation)
- **Inspection** — visual or physical examination
- **Demonstration** — functional exercise showing capability

## Workflow

1. **Query all requirements** — get the full requirement set.
2. **Assess each requirement** — based on the requirement text and type, recommend a verification method.
3. **Present verification plan** — table of requirements × recommended method.
4. **Create verification cases** — `create_element(type: "VerificationCaseDefinition", ...)` for each.
5. **Link to requirements** — `create_relationship(type: "RequirementVerificationMembership", ...)`.
6. **Generate verification matrix** — requirements × verification cases × methods.
7. **Update session** — record V&V state.

## MCP Tools Used

- `query_elements`
- `create_element`
- `create_relationship`
- `get_project_state`
