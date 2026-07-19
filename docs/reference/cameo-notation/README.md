# Cameo Notation Reference Gallery

These images are the **ground-truth graphical notation** the foundry viewer is
tuned against. They are **real Cameo Enterprise Architecture (Cameo CE) output**,
captured during the 2026-06 live modeling sessions on the old repo
(`sysml-bridge`) corpus. Each was produced by importing an ANGARS C&C `.sysml`
model into Cameo CE and exporting the rendered diagram.

They are the "reference" column of `docs/reference/parity-matrix.md`: the viewer's
own renders of the `probes/*.sysml` fixtures are scored ✅/⚠️/❌ against these
captures, feature by feature, per `docs/reference/rubric.md`.

## Provenance

Every file here is real Cameo CE output from the 2026-06 live sessions. Source
paths are in the old repo `sysml-bridge` at
`examples/angars/diagrams/`. `.pdf` sources were rasterized here with
`pdftoppm -png -r 150`; `.png` sources are the native Cameo raster, copied
verbatim.

| File (here)                    | Original name (source)                     | Source path                                        | Rasterization         | Provenance                                  |
|--------------------------------|--------------------------------------------|----------------------------------------------------|-----------------------|---------------------------------------------|
| `bdd-cameo.png`                | `angars-bdd.png`                           | `sysml-bridge/examples/angars/diagrams/angars-bdd.png`            | native PNG (verbatim) | Real Cameo CE output, 2026-06 live sessions |
| `ibd-cameo.png`                | `angars-ibd.png`                           | `sysml-bridge/examples/angars/diagrams/angars-ibd.png`            | native PNG (verbatim) | Real Cameo CE output, 2026-06 live sessions |
| `activity-cameo.png`           | `angars-activity.png`                      | `sysml-bridge/examples/angars/diagrams/angars-activity.png`       | native PNG (verbatim) | Real Cameo CE output, 2026-06 live sessions |
| `state-cameo.png`              | `angars-state.png`                         | `sysml-bridge/examples/angars/diagrams/angars-state.png`          | native PNG (verbatim) | Real Cameo CE output, 2026-06 live sessions |
| `requirements-cameo.png`       | `requirements-focused.pdf`                 | `sysml-bridge/examples/angars/diagrams/requirements-focused.pdf`  | `pdftoppm -png -r 150`| Real Cameo CE output, 2026-06 live sessions |
| `requirements-full-cameo.png`  | `angars-requirements.pdf`                  | `sysml-bridge/examples/angars/diagrams/angars-requirements.pdf`   | `pdftoppm -png -r 150`| Real Cameo CE output, 2026-06 live sessions |
| `traceability-cameo.png`       | `traceability-focused.pdf`                 | `sysml-bridge/examples/angars/diagrams/traceability-focused.pdf`  | `pdftoppm -png -r 150`| Real Cameo CE output, 2026-06 live sessions |
| `traceability-full-cameo.png`  | `angars-traceability.pdf`                  | `sysml-bridge/examples/angars/diagrams/angars-traceability.pdf`   | `pdftoppm -png -r 150`| Real Cameo CE output, 2026-06 live sessions |

## What each capture shows

- **`bdd-cameo.png`** — Block Definition Diagram of `C&C Architecture`. Frame tab
  `bdd C&C Architecture`. Sharp-corner `<<part def>>` blocks, attribute
  compartments (`attribute voltage : Real`), a filled composition diamond at the
  `C&C Subsystem` owner, parts-compartment listing the six contained parts.
  Reference for the **BDD / General** view.
- **`ibd-cameo.png`** — Internal Block (Interconnection) view. Frame tab
  `interconnection`. Rounded `<<part>>` usage blocks, ports as small squares
  straddling the block boundary, orthogonal (right-angle) connector routing.
  Reference for the **IBD / Interconnection** view.
- **`activity-cameo.png`** — Action Flow of `C&C Operations`. Frame tab
  `action C&C Operations`. Filled initial disc, rounded `<<action>>` boxes,
  object flows labeled (`OperatorCmd`, `FuelData`, `PriorityList`, ...), filled
  final node. Reference for the **Activity / ActionFlow** view.
- **`state-cameo.png`** — State Transition view of `C&C Mode`. Frame tab
  `state C&C Mode`. Initial pseudo-state disc, rounded `<<state>>` boxes,
  transition arrows, final node. Reference for the **State** view (viewer support
  lands in Task 2).
- **`requirements-cameo.png`** — Requirements view of `C&C Requirements`
  (focused). Frame tab `requirements C&C Requirements`. `<<requirement>>` boxes
  with an id+name header (`'R1' Power Management`) and an `attribute statement =`
  compartment, plus a `<<verification def>>` box; `derive` / `satisfy` / `verify`
  dashed edges with open arrowheads and keyword labels. Reference for the
  **Requirements** view (viewer support lands in Task 3).
- **`requirements-full-cameo.png`** — The full ANGARS 34-requirement corpus
  requirements view (`angars-requirements`). Very wide single-row layout; kept as
  the scale reference for the full model. Task-3 target.
- **`traceability-cameo.png`** — Traceability view of `C&C Trace` (focused).
  Frame tab `traceability C&C Trace`. Kind-distinct nodes (`<<part>>`,
  `<<requirement>>`, `<<verification def>>`, `<<action>>`) with
  `derive` / `satisfy` / `verify` / `allocate` labeled dashed edges. Reference for
  the **Traceability** view (viewer support lands in Task 4).
- **`traceability-full-cameo.png`** — The full ANGARS cross-pillar trace web
  (`angars-traceability`). Very wide single-row layout; kept as the scale
  reference for the full model. Task-4 target.
