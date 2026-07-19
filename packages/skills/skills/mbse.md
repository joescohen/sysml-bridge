---
name: mbse
description: Stateful MBSE orchestrator — reads the lifecycle session, reports where the model is, routes the next step through the MCP tools
---

# /mbse — the orchestrator

The stateful entry point for corpus-grounded SysML v2 authoring. Every heavy operation runs
through the sysml-bridge MCP tools; this skill only reads lifecycle state, reports position,
and routes to the next verb. It never invents model facts and never edits the `.sysml` by hand.

## Where the session lives

The lifecycle state is a JSON file the MCP server writes as a side effect of tool calls:

```
<model-dir>/.mbse/session.json
```

`<model-dir>` defaults to `.sysml-bridge/models` and is overridden by the
`SYSML_FOUNDRY_MODEL_DIR` environment variable. So with the default, read
`.sysml-bridge/models/.mbse/session.json`. If that file is absent, the model has not been
initialized — start at `init`.

The file records the furthest lifecycle stage reached. Progress is **forward-only**: the six
stages advance in order and never regress.

## The lifecycle

```
init → ingest → build → trace → validate → render
```

| Stage | What is true | Advanced by |
|-------|--------------|-------------|
| `init` | project exists, empty | `init_project` |
| `ingest` | a source `.sysml` was imported | `import_sysml` |
| `build` | elements have been created | `create_element` |
| `trace` | relationships link elements | `create_relationship` |
| `validate` | a CLEAN Gate-1 audit passed | `validate_model` (clean run only) |
| `render` | views were exported | `export_sysml` |

`ingest` is optional — a model built from scratch goes `init → build` directly. `validate`
only advances on a **clean** `validate_model` run: `issues` empty AND no error-severity
`findings`. A dirty validation leaves the session where it was.

## Routing procedure

1. Read `<model-dir>/.mbse/session.json` (path rules above). Report the current stage plainly.
2. If the file is missing, or no project exists, call `init_project` and route to build.
3. Route to the next verb by stage:
   - at `init`/`ingest` → build the model: `/mbse-edit` (uses `create_element`) or, for a
     corpus/prose source, `import_sysml` to reach `ingest` first.
   - at `build` → add traceability: `/mbse-edit` (uses `create_relationship`).
   - at `trace` → gate it: call `validate_model` (see below).
   - at `validate` → render: `/mbse-render` (drives `export_sysml` + the viewer).
   - at `render` → the pipeline is complete; offer `/mbse-query` to inspect or `/mbse-render`
     to refresh.
4. For questions rather than progress, hand off to `/mbse-query`. For approving candidate
   records, hand off to `/mbse-approve`.

Never claim a stage was reached without the session file confirming it. If a tool call is
supposed to advance the session but the file did not move, say so — do not assume.

## Reading a validate_model result

`validate_model` returns JSON with two problem lists plus a fidelity block:

- `issues` — a string array of completeness/consistency problems (unsatisfied requirements,
  orphaned elements, missing connections). Non-empty means NOT clean.
- `findings` — Gate-1 audit records, each with `elementId`, `ruleId`, `severity`
  (`error` | `warning` | `info`), `message`, and `suggestedFix`. Any `error`-severity finding
  means NOT clean.
- `fidelity` — the corpus-coverage numbers.

To clear the gate: read each finding, apply its `suggestedFix` to the offending `elementId`
via `/mbse-edit`, then re-run `validate_model`. Report findings back to the user grouped by
`severity`, quoting `ruleId` and `suggestedFix` verbatim. Warnings and info do not block
advancement; only `error` findings and non-empty `issues` do.

## Two rules that bind every stage

- **R4 — trace operands are Usages.** When building traceability, `satisfy` / `allocate` /
  `verify` participants MUST be usages (Features), never Definitions. The grammar validator
  cannot catch a Definition operand; the Gate-1 relational audit (in `validate_model`
  `findings`) does, and Cameo rejects it semantically. Emit usage-correct trace participants.
- **R2 — validate before claiming import.** Never tell the user the model imports into Cameo
  without a clean local validator run. The exported `.sysml` must pass
  `pnpm validate:sysml <file>.sysml` at exit 0. "It should import" is not a claim you may make
  without that exit-0 run on the exact file. `/mbse-render` runs this gate before rendering.
