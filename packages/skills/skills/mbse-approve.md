---
name: mbse-approve
description: The human gate — review candidate records with their citations, record an explicit approve or reject; there is no auto-approve path
---

# /mbse-approve — the human gate

Candidate records (LLM-extracted prose entries and inference dispositions) may enter the
composed IR **only** after an explicit human decision, recorded here. This skill is that gate.
It presents each candidate with its evidence and captures a decision; it does not build the
model. Approved entries flow downstream into the composed IR via composeIR (in
`packages/model`); rejected entries never do.

## There is no auto-approve

**No auto-approve path exists, and you may not invent one.** A candidate never becomes an
approved entry on its own — not on a confidence threshold, not on a majority debate score, not
because it "looks right", not to save the user a step. Every approval is an explicit human
"yes" recorded against a specific candidate id. If the user has not said yes to a specific
candidate, it stays a candidate. Never write an approval the user did not make, and never
compose an unreviewed candidate into the IR.

## What to show for each candidate

Present candidates one at a time (or in a small reviewable batch). For each, show:

1. **The claim** — the element or relationship the candidate proposes (name, type, and for a
   relationship, its endpoints).
2. **The citation** — where the candidate came from: the source document and location
   (page / row / cell) that the extraction points to.
3. **The source excerpt** — the actual quoted text from that citation, so the human judges the
   claim against the evidence, not against the model's paraphrase of it.
4. For an inference candidate, also show the propose/debate rationale that produced it.

A candidate with no resolvable citation is a red flag — surface it as such; an uncited claim is
exactly what this gate exists to catch.

## Recording the decision

For each candidate, capture an explicit disposition:

- **Approve** — record who approved, when, and the candidate id it was approved from. The entry
  becomes eligible to compose into the IR.
- **Reject** — record the rejection so the candidate is excluded and not re-surfaced as new.

Present the citation and excerpt, ask the human to approve or reject, and record exactly what
they decide. Do not batch a single "approve all" over candidates the human has not individually
seen — the point of the gate is per-candidate human judgment against the evidence.

## After the gate

Approved dispositions flow into the composed IR through composeIR, which merges the extracted
corpus with the approved prose layer. From there the model is built through the store tools
(`/mbse-edit`), validated (`validate_model`, via `/mbse`), and rendered (`/mbse-render`). This
skill's only MCP interaction is confirming model state when needed — use `get_project_state` or
`query_elements` (read-only) to check what is already built before composing new approvals. It
performs no model mutations itself.
