---
name: mbse-edit
description: NL edit-loop skill — store-tool-only mutations, diff-approved, Gate-1-enforced. Add/rename/retarget/remove model elements exclusively through MCP store tools; never through import_sysml or by editing .sysml text.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Edit

Drive model mutations through a human-on-the-pipe edit loop. Every NL request to add,
rename, retarget, or remove a model element is translated into MCP store tool calls,
presented as a structured diff for explicit approval, then executed with a full Gate 1
audit (`validate_model`) run automatically after the batch. A failing gate blocks the
mutation from standing.

---

## Hard constraints

### TF-3 — Store tools are the ONLY edit surface; `.sysml` is output-only

`mbse-edit` mutates the model EXCLUSIVELY through the four MCP store tools:
`create_element`, `create_relationship`, `update_element`, `delete_element`.

**`import_sysml` is NEVER called by this skill.** The `.sysml` artifact is output-only.

Reason: the SysML v2 parser (`import_sysml`) recognizes only a subset of what the
serializer emits. A round-trip silently drops structure: `connect` usage ports,
`first`/`then` action sequencing, state transition bodies, typed-usage expressions,
and `objective`/verification body members are all lost on re-parse. An
import→edit→re-serialize path is therefore destructive and is permanently forbidden
as an edit route.

### Honest rollback — FileStore has no transaction

The file-native store has no transaction or undo journal. "Rollback" in this skill
means exactly:

- For each element or relationship **CREATED** in the failed batch (you have its `id`
  from the tool response), call `delete_element` as a compensating action.
- For any `update_element` that mutated a **pre-existing** element, no automatic
  inverse exists. STOP and report exactly which element changed and what its prior
  values were (captured in the diff step) — instruct the user to re-issue a
  corrective `update_element` if desired.
- If a compensating delete is itself blocked (e.g., `EDIT-delete-would-dangle`),
  STOP and report the exact residual state.

**Never report "rolled back" unless every compensating delete succeeded without error.**

---

## Supported mutations

Map each NL verb to its store tool. These are the ONLY mutations this skill performs.

| NL verb | Store tool | Key parameters |
|---------|-----------|----------------|
| ADD element | `create_element` | `type`, `name`, `attributes` (must include `provenanceSourceId`) |
| ADD relationship | `create_relationship` | `type`, `source_id`, `target_id`, `attributes` |
| RENAME / re-type / re-attribute | `update_element` | `element_id`, `updates` (fields to merge) |
| RETARGET relationship | `update_element` | `element_id`, `updates` with new `source`/`target` arrays |
| REMOVE element or relationship | `delete_element` | `element_id` |

### Provenance requirement (Pillar C)

Every element CREATED by `create_element` MUST carry a `provenanceSourceId` attribute
that resolves into `examples/angars/model/extracted.json`. The tool gate (GATE03 in
`packages/mcp-server/src/audit/structural.ts`) rejects a create without a resolvable
provenance id — the store is left untouched. This skill must not attempt a `create_element`
call without first confirming a valid corpus id for the `provenanceSourceId`.

An un-sourced create is a Pillar C violation. If the user requests creation of an
element with no corpus grounding, STOP and explain: hand-invented elements not traceable
to the corpus are forbidden. The user must supply a corpus id or the create cannot proceed.

---

## The edit loop

Execute these steps in order. Do not deviate.

### Step 1 — Parse the NL request into a mutation plan

Translate the user's request into a concrete list of tool calls. For each planned call,
identify: tool name, all required parameters, and the dependency order (elements before
the relationships that reference them).

### Step 2 — Resolve IDs

Use `query_elements` and `query_relationships` to turn element names into element IDs.
Never fabricate IDs. If a referenced element does not exist in the live store, STOP
and report which element is missing. Do not proceed until all IDs are resolved.

For ADD operations: confirm the `provenanceSourceId` corpus id resolves (see Pillar C
above). If it does not, STOP.

### Step 3 — Present the diff and WAIT for approval

Print the structured diff block (format in the next section). Then emit the literal
approval gate line and STOP. Do NOT call any mutating tool before the user replies
`approve`.

### Step 4 — Execute (after approval only)

Call the mutating tools in dependency order: create elements before the relationships
that reference them; delete relationships before the endpoint elements they reference.

Watch each tool response:

- **Reject shape** (`rejected: true, findings: [...]`) — the tool gate blocked this
  call; the store is untouched for that call. Surface the findings immediately. Do NOT
  pass `allow_invalid: true` to force past an error-severity finding unless the user
  explicitly requests it and acknowledges the consequence. If a call was blocked,
  report which calls succeeded before the block (if any) — those changes ARE in the
  store.
- **Bypass shape** (`element: {...}, findings: [...]`) — the user-authorized bypass
  landed but the full findings (including error-severity ones) are returned. Record
  them for the Gate 1 step.
- **Clean shape** (bare element or `deleted: true`) — call succeeded, no gate concerns.

### Step 5 — Auto-Gate-1

After the batch (all planned tool calls complete, or after any blocking rejection),
call `validate_model` with no arguments. Read the response:

- `findings[]` — audit findings with `severity` field; error-severity findings indicate
  structural violations.
- `issues[]` — human-readable issue strings (unsatisfied requirements, dangling
  relationships, provenance gaps, orphan elements).
- `coverage` — percentages and lists for forward/verify/backward trace and provenance.

Present the gate result to the user in a concise block (see Auto-Gate-1 section below).

### Step 6 — Gate enforcement decision

If `validate_model` returns error-severity findings **attributable to this batch**, the
mutation does not stand — proceed to the rollback procedure (Step 7). If the findings are
pre-existing (existed before this batch), REPORT but do not rollback the current batch
(the pre-existing issue is not caused by this edit).

If `issues[]` is empty and no error-severity findings exist: the gate PASSES. Report
the result and the model is in a confirmed-clean state.

### Step 7 — Rollback (only if gate FAILS on this batch)

Follow the Honest rollback procedure (see section below). Report the outcome precisely.

---

## Diff approval format

Print this block before calling any mutating tool. Replace placeholders with actual
values derived from Steps 1–2.

```
PENDING MUTATION BATCH
──────────────────────
CREATED:
  <element-type> "<name>"  provenanceSourceId=<corpus-id>
  <relationship-type>  source=<source-id> → target=<target-id>

MODIFIED:
  <element-id> (<name>)
    name:              "<old-value>" → "<new-value>"
    type:              <old-type>    → <new-type>
    source:            [<old-ids>]   → [<new-ids>]
    target:            [<old-ids>]   → [<new-ids>]
    provenanceSourceId: "<old>"      → "<new>"

REMOVED:
  <element-id> (<name>, <type>)
  WARNING — the following relationships will be left dangling if this element is deleted
  without prior retarget/delete:
    <rel-id> (<rel-type>)  [EDIT-delete-would-dangle]

──────────────────────
Reply `approve` to apply, or describe changes.
```

Omit sections that have no entries (e.g., omit MODIFIED if nothing is being modified).
For REMOVED entries, run `delete_element` with `element_id` and capture its pre-check
`would-dangle` report BEFORE printing the diff — surface any dangling warnings in the
REMOVED section so the user can decide whether to also delete/retarget those relationships
in the same batch.

**The skill MUST NOT call any mutating tool until the user replies `approve`.**

---

## Auto-Gate-1 enforcement (EDIT-02)

Two layers enforce the gate after every approved batch:

**Layer A — per-call store-tool pre-check (check-before-mutate):**
Each of `create_element`, `create_relationship`, `update_element`, and `delete_element`
runs structural checks before touching the store. Rules enforced:

- `R4-def-operand` — rejects retarget/create where source or target resolves to a
  Definition type (operands must be usages per SAIC discipline, Pillar A).
- `GATE02-dangling-endpoint` — rejects when source/target ID is not in the existing
  element+relationship set.
- `GATE03-unresolvable-provenance` — rejects when a corpus is loaded and
  `provenanceSourceId` is not in the resolution set.
- `EDIT-delete-would-dangle` — rejects deletion when any relationship's `sourceIds` or
  `targetIds` contains the element being deleted (unless `allow_invalid` is passed).

A rejected call returns `{ rejected: true, findings: [...] }` with `isError: true`.
The store is untouched for that call.

**Layer B — post-batch `validate_model` full audit:**
After the batch, `validate_model` checks completeness and fidelity across the whole
model: forward/verify/backward trace coverage, orphan elements, provenance coverage,
dangling relationships, and GATE-01 audit findings. Error-severity findings here catch
regressions that the per-call gate (which checks one mutation at a time) cannot see in
aggregate.

**A failing post-batch gate (error-severity finding caused by this batch) means the
mutation does not stand — proceed immediately to the rollback procedure.**

Present the gate result in this format:

```
AUTO-GATE-1 RESULT
──────────────────
VERDICT: [PASS | FAIL]

issues:
  - <issue string>          ← from issues[] (each entry)

error findings:
  - <ruleId>: <message>     ← from findings[] where severity==="error"

coverage:
  forwardPercent:    <value> / 100
  verifyPercent:     <value> / 100
  backwardPercent:   <value> / 100
  orphanElements:    <count>
  danglingRels:      <count>
  provenanceCoverage: <value>%
──────────────────
```

---

## Honest rollback

When the post-batch gate FAILS with error-severity findings attributable to this batch,
execute the following procedure precisely.

**1. Identify batch-created elements and relationships.**
From the tool responses you received during the batch, collect every element ID that
was newly created (successful `create_element` or `create_relationship` response — bare
element or `{ element, findings }` shape). You MUST have recorded these IDs during
Step 4.

**2. Delete batch-created relationships first** (to avoid would-dangle on their endpoints).
For each relationship created in the batch, call:
```
delete_element(element_id: <rel-id>)
```
If `delete_element` returns `{ rejected: true, findings }` for a relationship deletion,
STOP and report the exact residual: "Compensating delete of relationship `<rel-id>` was
blocked — `<finding message>`. The model is NOT fully rolled back. Residual state: [list
elements and relationships that landed]."

**3. Delete batch-created elements** (after their relationships are removed).
For each element created in the batch (not a relationship), call:
```
delete_element(element_id: <elem-id>)
```
Same blocked-delete protocol as above.

**4. For update_element mutations on pre-existing elements — NO automatic inverse.**
STOP and report:
```
ROLLBACK NOTE — update_element has no automatic inverse.
Element: <element-id> (<name>)
Changed fields (prior values captured in diff):
  <field>: prior value = "<old>"  (now = "<new>")
To restore: re-issue update_element({ element_id: "<id>", updates: { <field>: "<old>" } }).
```
Do NOT report the update as "rolled back." It is not.

**5. Completion report.**
If ALL compensating deletes succeeded:
```
ROLLBACK COMPLETE — all batch-created elements and relationships deleted.
The model is back to its pre-batch state for created artifacts.
[Include update_element note if any updates were made.]
```
If any compensating delete was blocked:
```
ROLLBACK PARTIAL — see residual state above. Manual intervention required.
```

---

## Worked examples

### Example A — Successful edit (rename + relationship add, gate passes)

**User:** "Rename PartUsage `Cooler` to `ThermalController` and add a satisfy link from it
to requirement `REQ-THERMAL-001`."

**Step 1 — mutation plan:**
- `update_element` on Cooler's element ID: `{ updates: { name: "ThermalController" } }`
- `create_relationship`: type `SatisfyRequirementUsage`, source = Cooler's ID, target = REQ-THERMAL-001's ID

**Step 2 — resolve IDs:**
```
query_elements("PartDefinition")  →  find element with name "Cooler"  →  id: "elem-p42"
query_elements("RequirementDefinition")  →  find "REQ-THERMAL-001"  →  id: "elem-r08"
corpus check: does "elem-r08" have a provenanceSourceId in examples/angars/model/extracted.json?
  → provenanceSourceId: "angars-req-thermal-001"  (resolved in corpus — OK)
```

**Step 3 — diff:**
```
PENDING MUTATION BATCH
──────────────────────
MODIFIED:
  elem-p42 (Cooler, PartDefinition)
    name: "Cooler" → "ThermalController"

CREATED:
  SatisfyRequirementUsage  source=elem-p42 → target=elem-r08
    provenanceSourceId=angars-req-thermal-001

──────────────────────
Reply `approve` to apply, or describe changes.
```

**User:** `approve`

**Step 4 — execute:**
```
update_element({ element_id: "elem-p42", updates: { name: "ThermalController" } })
→ { "@id": "elem-p42", name: "ThermalController", ... }  (clean shape — no findings)

create_relationship({ type: "SatisfyRequirementUsage", source_id: "elem-p42", target_id: "elem-r08" })
→ { "@id": "rel-s77", type: "SatisfyRequirementUsage", ... }  (clean shape)
```

**Step 5 — Auto-Gate-1:**
```
validate_model()
→ issues: []
→ findings: [] (no error-severity)
→ coverage: forwardPercent: 100, verifyPercent: 100, ...
```

```
AUTO-GATE-1 RESULT
──────────────────
VERDICT: PASS

issues: (none)
error findings: (none)
coverage:
  forwardPercent:    100 / 100
  verifyPercent:     100 / 100
  backwardPercent:   100 / 100
  orphanElements:    0
  danglingRels:      0
  provenanceCoverage: 100%
──────────────────
```

Edit complete. `Cooler` is renamed `ThermalController` and `SatisfyRequirementUsage
rel-s77` is live.

---

### Example B — Gate failure + rollback (satisfy targets a Definition — R4 violation)

**User:** "Add a satisfy link from PartUsage `CoolerUsage` (id: `elem-p55`) to
`RequirementDefinition` `COOLING-REQ` (id: `elem-r12`, a Definition type)."

**Step 3 — diff:**
```
PENDING MUTATION BATCH
──────────────────────
CREATED:
  SatisfyRequirementUsage  source=elem-p55 → target=elem-r12
    provenanceSourceId=angars-req-cooling

──────────────────────
Reply `approve` to apply, or describe changes.
```

**User:** `approve`

**Step 4 — execute:**
```
create_relationship({ type: "SatisfyRequirementUsage", source_id: "elem-p55", target_id: "elem-r12" })
→ {
    rejected: true,
    findings: [{
      elementId: "elem-r12",
      ruleId: "R4-def-operand",
      severity: "error",
      message: "Relationship target 'elem-r12' is a Definition (RequirementDefinition); trace operands must be usages (Features), not Definitions.",
      suggestedFix: "Use a RequirementUsage (usage-level element) as the target, not a RequirementDefinition."
    }]
  }
```

The store is untouched — the tool gate rejected the call before persisting.

**Step 5 — Auto-Gate-1** (run regardless — confirms store unchanged):
```
validate_model()  →  same result as before this batch (no new issues introduced)
```

**Report to user:**
```
MUTATION BLOCKED BY GATE
Tool: create_relationship
Rule: R4-def-operand
Message: Relationship target 'elem-r12' is a Definition (RequirementDefinition);
         trace operands must be usages, not Definitions.
Fix: target must be a RequirementUsage (usage element), not a RequirementDefinition.

No changes landed — the store is in its prior state.
AUTO-GATE-1: PASS (no new issues — store unchanged confirmed).
```

No rollback required (no calls succeeded). The model is unchanged.

---

## MCP Tools Used

**Mutation tools** (require user approval before call):
- `create_element`
- `create_relationship`
- `update_element`
- `delete_element`

**Query tools** (used in Steps 1–2, may be called without approval):
- `query_elements`
- `query_relationships`

**Audit tool** (auto-run after every approved batch):
- `validate_model`

**Prohibited:** Never call `import_sysml` or any HTTP-server-only tool. The
`packages/mcp-server/src/tools/` directory contains the canonical tool list — only
file-native stdio tools are valid in this skill. The `.sysml` file at
`examples/angars/model/cc-subsystem.sysml` is never edited as a mutation path; it is
output-only, written by `export_sysml`.
