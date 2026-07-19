# The gate pattern

This repository generates SysML v2 with an LLM in the loop, and imports the result into Cameo
Enterprise Architecture. Both facts make it easy to ship output that *looks* right and is wrong:
a model can read fluently and still invent parts, and a `.sysml` file can parse in your head and
be rejected by the tool that matters. The discipline below is what keeps that from happening. It
is four rules, each born from a specific failure, each backed by a mechanism in this tree, and each
portable to any project where a generator's output is trusted before a human has checked it.

The rules are ordered by how early they catch a defect: syntax, then claim, then substance, then
the gates themselves.

## 1. Grammar-as-truth

**The failure.** The serializer was written from memory of SysML v2 syntax. Forms that felt
correct were emitted, and the errors surfaced only on manual import into Cameo — slow, and each
attempt burned a scarce import cycle. Recollection is not a specification.

**The mechanism here.** The grammar is vendored in `docs/sysml-v2-reference/` (the `.g4` files
plus a cheatsheet of validator-passing forms) and it is the source of truth. The serializer in
`packages/sysml/src/sysml-serializer.ts` must conform to it. When a form is uncertain, the answer
is read from the vendored grammar, never reconstructed from memory. The cheatsheet's examples are
canonical — copy them rather than rebuild the syntax.

**Anywhere.** Vendor the real specification of whatever your code emits — a grammar, a schema, an
API contract — into the repo next to the emitter. Make "read the vendored spec" the rule and "I
remember how this works" the thing you never do.

## 2. Validate-before-claim

**The failure.** "It should import" was said about files that did not. A file the author believed
was correct is not evidence; the only evidence is a clean run of the tool that will consume it.

**The mechanism here.** `tools/sysml-validator/run.sh` runs a committed ANTLR grammar validator
over any generated or edited `.sysml` file. A non-zero exit means the file will not import, full
stop — no claim that it imports is permitted without a zero-error run on the exact file. This is
Gate 2 in the pipeline, and it stops the build on failure before Cameo is ever opened.

**Anywhere.** Put the cheapest faithful check of your output in the path *before* the claim, and
gate the claim on its exit code, not on the author's confidence. "Passes locally" must mean a
command ran and returned zero on the specific artifact, not that it looked fine.

## 3. Provenance-or-reject

**The failure.** On 2026-06-09 a hand-authored model quietly invented subsystems that appeared
nowhere in the source corpus. Nothing in the output announced the fabrication; it read as
plausibly as the real elements. A generator that can invent is a generator whose every element
needs a receipt.

**The mechanism here.** Every model element carries a `provenanceSourceId` pointing back to the
corpus row it came from. Gate 1 (`packages/gates/src/provenance.ts`) enforces it: an element with
no provenance, or a provenance id that does not resolve to a real corpus entity, is a finding, and
a non-empty findings list stops the build. The LLM-assisted layers extend the same contract —
prose-ingest and inference *propose*, they never write. A candidate arrives with citations and
cannot enter the composed model without an explicit human approval record; the no-auto-approve
invariant is proven in `packages/candidates/src/__tests__/no-auto-approve.test.ts`, whose
source-scanning ratchet walks the live tree and fails if any production file ever calls the
approval-record writers directly.

*Known gap (follow-up filed).* The provenance presence check currently scopes to three legacy
Definition types (`RequirementDefinition`, `PartDefinition`, `ActionDefinition`); an uncited
*Usage* produces no finding. The seeded-defect harness surfaced this, and the seed is a
`PartDefinition` for exactly that reason. Widening the check to Usages is tracked separately — the
point of naming it here is that a rule with a known blind spot is documented, not quietly trusted.

**Anywhere.** Require every generated record to carry a resolvable pointer to its source, and
reject — do not merely warn about — records whose pointer resolves to nothing. Where a model
proposes and a human accepts, make the acceptance an explicit artifact and forbid the code from
writing it.

## 4. Positive controls

**The failure.** A gate that has never been seen to reject anything is indistinguishable from a
gate that cannot reject anything. A trust check that only ever passes might be passing vacuously —
asserting the absence of a problem against data where the problem could never appear.

**The mechanism here.** Each gate ships with a known-bad input and is proven to fail on it.
`tools/sysml-validator/fixtures/smoke-bad.sysml` is malformed on purpose, and CI fails if the
validator *accepts* it. `examples/angars/pipeline/seeded-defects.ts` (run as `pnpm demo:seeded`
in CI, no API key needed) plants three defects with fixed ids into a copy of the clean ANGARS
build and asserts each gate reports its specific defect by rule id and element id — a missing
provenance finding, a non-zero validator exit on a poisoned file, an R4 def-operand finding — plus
a paired clean control that must produce zero error findings, so the catches are real and not
vacuous.

**Anywhere.** For every check you rely on, commit an input the check must reject and wire CI to
fail if that input passes. Pair it with a clean control so a check that fires on everything is
caught too — a gate proven to fail on the bad case *and* pass on the good case is the only kind you
can trust.

## What this buys

A gate never proven to fail is not a gate — it is a green light you have not tested. Proving each
check fails on a known-bad input is what converts "the build is green" from a hope into evidence.

The layers matter because they catch different classes of defect, and no single layer catches all
of them. Two red-CI runs make this concrete. Run
[28831984796](https://github.com/joescohen/sysml-bridge/actions/runs/28831984796) caught a broken
`def` keyword that the seeded-defect demo gate could not see — a defect the unit tests caught and
the pipeline gate did not. Run
[28833309369](https://github.com/joescohen/sysml-bridge/actions/runs/28833309369) is the mirror:
the demo's Gate 2 caught a broken usage keyword that *every* unit test missed. Each layer was
independently proven to fail, on a defect the other layer was blind to. That is why the pattern is
layers, and why each layer earns its place only once it has been shown to catch something the
others would have let through.
