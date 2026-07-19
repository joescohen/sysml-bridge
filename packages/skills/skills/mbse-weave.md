---
name: mbse-weave
description: Run the gap-driven weave loop — audit the model, propose links to close gaps into the review queue, then (after human approval) recompose, re-audit, and record the pass
---

# /mbse-weave — close the gaps

The weave loop is how the model gets *more complete* without anyone hand-authoring the missing
links. It reads the audit, turns each completeness gap into a scoped inference query, proposes
candidate links to the **review queue**, and stops. A human approves in the review UI; only then
does the pass close — recompose, re-audit, and record whether the gaps actually shrank.

The iron rule: **weave PROPOSES, it never approves.** An open pass writes zero dispositions. This
is the same no-auto-approve invariant the rest of the pipeline enforces — fuzzy / model-authored
links are candidates, never facts, until a human says so.

## The two-phase pass

A pass is deliberately split so a human sits in the middle:

1. **Open the pass — `weave_pass`.** Point it at a project directory. It runs the Gate audit,
   maps each `GATE02-*` completeness finding to a scoped query (unsatisfied requirement → satisfy
   family; orphan element → allocation family; uncovered need → derive family), runs bounded
   targeted inference, and writes the proposals to the project's review queue
   (`<project>/candidates/inference-candidates.json`). Then it **STOPS**. No disposition is
   written. Unmapped finding ids are reported, never silently skipped.

   ```
   weave_pass  { "project": "<project-dir>" }
   ```

   Optional inputs: a USD budget cap (logged when exceeded), a dry-run that plans queries without
   calling the provider, and a mock flag that forces the deterministic mock provider even when a
   key is present. With no `ANTHROPIC_API_KEY` the pass runs the deterministic mock provider, so
   it works with zero API key.

2. **Human review.** Open the review UI and approve or reject each proposed link. Approvals land
   in `<project>/dispositions/`. Nothing about the model changes until you do this — the pass is
   inert until a human acts.

3. **Close the pass — `close_pass`.** Recomposes the model against the human dispositions,
   re-audits, writes the pass record to `<project>/passes/pass-NNN.json`, and enforces the HARD
   convergence gate.

   ```
   close_pass  { "project": "<project-dir>" }
   ```

   Convergence is a real gate: a closed pass must end with **zero error findings** and must never
   have **more** errors than it began with. If error findings increased or remain, `close_pass`
   returns an error (non-zero exit) and the session does not advance — fix the model or the
   dispositions and run another pass. The per-rule warning delta is reported but not gated (soft).

## Where weave sits in the lifecycle

Weave is the **enrich** stage: it runs after the initial build and trace, and before validation.
The full lifecycle is init → ingest → build → trace → **enrich** → validate → render. A successful
`weave_pass` or a convergent `close_pass` advances the session to `enrich`; a failed close does
not advance. Validation semantics are unchanged — after enriching, run `validate_model` and clear
any error-severity findings, then `/mbse-render`.

## MCP tools

- `weave_pass` — open a gap-driven pass: audit → plan queries → propose to the review queue →
  STOP. Writes no disposition.
- `close_pass` — close the pending pass: recompose → re-audit → record → enforce the convergence
  gate. Reads human dispositions; writes none.
- `validate_model` — the semantic audit weave is closing gaps against; run it after a pass to
  confirm the error findings are clear before rendering.

To approve the proposed links, use `/mbse-approve`. To inspect the model between passes, use
`/mbse-query`. For lifecycle position, use `/mbse` (weaving advances the session to `enrich`).
