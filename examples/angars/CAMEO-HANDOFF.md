# ANGARS Cameo CE Import Handoff

## 1. What Was Proven Before This Handoff

The following pipeline chain ran to completion. Each stage's evidence file is listed.

| Stage | Verdict | Evidence |
|-------|---------|----------|
| corpus → 5-pillar store (Gate 1) | PASS: 0 error findings, 0 fabrications, 0 drops | `examples/angars/model/E2E-REPORT.md` § Gate 1 |
| SysML v2 export (Gate 2) | PASS: 0 grammar errors | `examples/angars/model/angars-full.sysml` (897 lines) |
| 6 pillar views rendered | PASS: all 6 PNGs read-verified | `examples/angars/diagrams/e2e/` |
| IBD N2 direction spot-check | CONFIRMED: corpus triple n2-95919510e5fb96e9 (sourceLabel=Power Subsystem → targetLabel=Command & Control Subsystem, flow=28VDC) matches arrow in `e2e-ibd-system-1.png` | `examples/angars/diagrams/e2e/e2e-ibd-system-1.png` |
| 6 slices emitted + per-slice Gate 2 | PASS: all 6 slices exit 0 | `examples/angars/model/slices/` (local-only, gitignored) |

## 2. Why Slices

The full model emits **1058 elements** (packages + defs + usages + trace statements). Cameo CE's
scratch SysML v2 editor blocks model creation past **500 major elements**. Slicing divides the
model into pillar-scoped files, each under 450 elements, with self-contained name declarations so
Cameo resolves every reference inside the same paste session.

## 3. Slice Inventory

| File (in `examples/angars/model/slices/`) | Pillar | Element Count | Gate 2 | Self-contained |
|-------------------------------------------|--------|---------------|--------|----------------|
| `slice-requirements.sysml` | Requirements (16 needs + 182 reqs + package) | 387 | PASS (0 errors) | unresolved-in-slice: 0 |
| `slice-structure.sysml` | Structure (6 subsystem defs + 34 components + ANGARS System + 87 flows) | 55 | PASS (0 errors) | 44 cross-subsystem flow targets in same package |
| `slice-behavior.sysml` | Behavior (9 action defs + 54 leaf actions + 45 successions + 22 flows) | 72 | PASS (0 errors) | unresolved-in-slice: 0 |
| `slice-trace-satisfy.sysml` | Trace — satisfy (action + req usages + 110 satisfy edges) | 274 | PASS (0 errors) | unresolved-in-slice: 0 |
| `slice-trace-derive.sysml` | Trace — derive (req + need usages + 187 dependency edges) | 384 | PASS (0 errors) | unresolved-in-slice: 0 |
| `slice-trace-verify.sysml` | Trace — verify (verification + req usages + 182 verify in objective bodies) | 395 | PASS (0 errors) | unresolved-in-slice: 0 |

**Expected result per slice after a successful import:**
- `slice-requirements.sysml`: containment tree shows 1 package with ~198 requirement usages; no red error markers.
- `slice-structure.sysml`: containment tree shows `ANGARS Structure` package with 7 part defs (including `ANGARS System`) and nested part usages; flow connections visible on the IBD if opened.
- `slice-behavior.sysml`: containment tree shows `ANGARS Behavior` package with 9 action defs, each containing leaf action usages.
- `slice-trace-satisfy.sysml`: containment tree shows `ANGARS Trace Satisfy` package with satisfy relationships visible in the traceability view.
- `slice-trace-derive.sysml`: containment tree shows `ANGARS Trace Derive` package with dependency relationships.
- `slice-trace-verify.sysml`: containment tree shows `ANGARS Trace Verify` package; each verification usage has an objective compartment listing verify targets.

## 4. Import Procedure (Per Slice — Your Steps on Your Mac)

**Prerequisites:** Cameo Community Edition installed, SysML v2 plugin active, scratch editor open.

**Per-slice steps:**

1. Open a new or existing SysML v2 scratch project in Cameo CE.
2. Open the built-in SysML v2 textual editor (the scratch editor pane).
3. Copy the entire contents of the slice file (e.g. `slice-requirements.sysml`).
4. Paste into the scratch editor.
5. Press **Alt+S** (Synchronize) to materialize the model from the text.
6. Check the notification window: expect **zero errors**. Elements appear in the containment tree.
7. If errors appear, capture the exact LSP error text + slice name + line number and file it for the next session.

**Recommended import order (smallest blast radius first, R3/R4 risky last):**

1. `slice-requirements.sysml` — requirements only, no trace; safe baseline
2. `slice-structure.sysml` — structural model; exercises part def + part usage nesting
3. `slice-behavior.sysml` — behavioral model; exercises action def + succession
4. `slice-trace-derive.sysml` — derive/dependency trace; exercises `dependency from X to Y;`
5. `slice-trace-satisfy.sysml` — satisfy trace; exercises `satisfy <req> by <action>;`
6. `slice-trace-verify.sysml` — **import last**: exercises the R3 `objective { verify ...; }` pattern and the R4 usage-typed operands — historically the highest-risk import

## 5. Known Caveats

**TF-10 (ScalarValues / scalar-typed attributes):**
The model emits no scalar-typed attributes anywhere — all usages are untyped (`part p;`, not
`part p : RealType;`). No `import ScalarValues::*;` is present in any slice. The TF-10 risk
(ScalarValues not resolving in Cameo's scratch editor) does **not apply** to these slices.

**R4 — usage-typed operands (semantic, not grammar):**
The local grammar validator checks syntax only. Cameo CE enforces an additional semantic rule:
`satisfy`, `allocate`, and `verify` operands must be **usages** (Features), not definitions.
The pipeline's Gate 1 R4 self-check confirmed 0 def-operand violations (see `E2E-REPORT.md`
§ Gate 1). All participating elements are usages throughout. However: the local gate cannot
catch a definition operand that parses clean — confirm visually in Cameo's containment tree
that each trace operand resolves to a usage node, not a definition node.

**R3 — `verify` placement:**
`verify` is only legal inside `objective { ... }` of a `verification` body.
`slice-trace-verify.sysml` emits exactly this pattern. Top-level `verify X by Y;` is invalid
(Cameo reports "extraneous input 'verify'") — the pipeline does not emit that form. See
`docs/sysml-v2-reference/cheatsheet.md` section 5 for the canonical pattern.

**Satisfy corpus anomaly:**
44 of 154 corpus satisfy entries reference function IDs not present in the functions list
(stale IDs from a prior extraction). The 110 resolvable satisfy edges are in
`slice-trace-satisfy.sysml`. The 44 stale-ID entries are absent from all slices — not silently
skipped, explicitly logged in `E2E-REPORT.md` § Anomalies.

**What to capture if an import fails:**
Record the exact LSP error text from Cameo's notification window, the slice filename, and the
line number. This is the minimum needed to diagnose from the grammar in
`docs/sysml-v2-reference/grammar/` in the next session.

## 6. Hard Stop

The manual Cameo import spot-check is the user's action.
Nothing below this line is claimable by the pipeline.

---

*Generated by e2e-proof.ts pipeline — Phase 7 Plan 03*
*Pipeline run date: 2026-06-10*
*Branch: feat/angars-cc-traceability*
