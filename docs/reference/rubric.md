# Notation Parity Rubric

Per-view checkable features the foundry viewer's renders are scored against, in
`docs/reference/parity-matrix.md`. Each feature is **one checkable sentence** —
a reviewer looking at the render PNG beside the Cameo reference PNG marks it
✅ (matches the notation), ⚠️ (present but visibly off), or ❌ (absent/wrong).

Features are grounded in two sources, cited per view:
- the **Cameo CE captures** in `docs/reference/cameo-notation/` (real 2026-06
  output — the notation the enterprise tool actually draws), and
- the **OMG SysML v2 graphical notation** as summarized in
  `tools/viewer/README.md` §"SysML v2 Graphical Notation" and the trace/verify
  patterns in `docs/sysml-v2-reference/cheatsheet.md`.

The **definition vs usage corner rule** is the spine of the whole notation:
definitions (`part def`, `action def`, `state def`) draw with **sharp** corners;
usages (`part`, `action`, `state`) draw with **rounded** corners
(README §"Definitions vs. Usages"; cheatsheet §1).

---

## BDD / General view

Reference: `cameo-notation/bdd-cameo.png`. Probes: `bdd-structure.sysml`,
`structural-pillar.sysml`.

- **B1 — Frame + heading.** The view is enclosed in a rectangular frame with an
  upper-left tab reading the view keyword + context name (e.g. `bdd C&C Architecture`).
- **B2 — Def blocks sharp-cornered.** `part def` blocks render as sharp-corner
  rectangles (README def/usage rule; cheatsheet §1).
- **B3 — `<<part def>>` stereotype text.** Each definition block shows a
  `<<part def>>` keyword/stereotype line above its name.
- **B4 — Name compartment.** Each block has a header compartment holding the
  element name, separated from the body by a horizontal line.
- **B5 — Attribute compartment + separators.** Attributes (`attribute voltage : Real`)
  appear in a compartment below the name, delimited by a horizontal separator line.
- **B6 — Parts compartment.** Owned parts are listed in the owner block's parts
  compartment as `part <name> : <Type>` lines (see `C&C Subsystem` in the reference).
- **B7 — Composition connector.** Ownership draws as a solid line with a **filled
  diamond** at the owner end (README relationship table; reference shows the diamond
  at `C&C Subsystem`).
- **B8 — Specialization connector.** `:>` renders as a solid line with a **hollow
  triangle** at the supertype end (README relationship table; `Redundant Power
  Module :> Power Module` in `bdd-structure.sysml`).

## IBD / Interconnection view

Reference: `cameo-notation/ibd-cameo.png`. Probes: `structural-ibd-complex.sysml`,
`structural-pillar.sysml`.

- **I1 — Frame + heading.** Frame with an `interconnection` tab in the upper-left.
- **I2 — Usage blocks rounded.** Part **usages** render as rounded-corner
  rectangles (README def/usage rule).
- **I3 — `<<part>>` stereotype + typed name.** Each usage block shows a `<<part>>`
  line and a `name : Type` header (e.g. `C&C Power Module : C&C Power Module`).
- **I4 — Ports straddle the boundary.** Ports draw as small squares centered on
  the owning block's edge, half inside / half outside (README §"Ports").
- **I5 — Port labels.** Each port shows a `port <name> : <PortType>` label
  (e.g. `port pwrOut : PowerPort`).
- **I6 — Orthogonal routing.** Connectors route as right-angle (orthogonal)
  polylines, not diagonal straight lines (reference routing).
- **I7 — Connections attach port-to-port.** Each connector's endpoints land on
  ports, not on block interiors, matching the `connect a.p to b.q` statements.
- **I8 — No overlapping blocks.** Blocks are laid out without overlap so every
  block and its ports are legible.

## Activity / ActionFlow view

Reference: `cameo-notation/activity-cameo.png`. Probes: `activity-control-flow.sysml`,
`behavioral-pillar.sysml`.

- **A1 — Frame + heading.** Frame with an `action <context>` tab (e.g.
  `action C&C Operations`).
- **A2 — Initial node.** A filled black disc marks the flow start (README
  §"Behavior Elements"; reference top-center disc).
- **A3 — Final node.** A filled disc inside a ring marks the flow end (reference
  bottom-center).
- **A4 — Action usages rounded.** Action **usages** render as rounded-corner
  boxes (README def/usage rule).
- **A5 — `<<action>>` stereotype text.** Each action box shows an `<<action>>`
  stereotype line above its name.
- **A6 — Succession arrows directed.** `first X then Y` successions draw as
  directed arrows from X to Y (open arrowhead at the target).
- **A7 — Guarded branches.** A decision splits into labeled/guarded successions
  (`if fuelSufficient` / `if fuelLow` in `activity-control-flow.sysml`); the
  branch condition is shown.
- **A8 — Decision / merge diamonds.** `decide` / `merge` control nodes render as
  diamonds (README §"Behavior Elements").
- **A9 — Fork / join bars.** `fork` / `join` control nodes render as
  synchronization bars (README §"Behavior Elements").
- **A10 — Object-flow labels.** `flow`/object flows carry their item label
  (e.g. `FuelData`, `PriorityList` in the reference).

## State / StateTransition view — **viewer support lands in Task 2**

Reference: `cameo-notation/state-cameo.png`. Probes: `state-machine-control.sysml`,
`behavioral-pillar.sysml`.

- **S1 — Frame + heading.** Frame with a `state <context>` tab (e.g. `state C&C Mode`).
- **S2 — Initial pseudo-state.** A filled disc marks the initial state, with an
  arrow into the first state.
- **S3 — Final node.** A ringed filled disc marks the terminal state.
- **S4 — State boxes rounded.** State **usages** render as rounded-corner boxes
  (README def/usage rule).
- **S5 — `<<state>>` stereotype text.** Each state box shows a `<<state>>`
  stereotype line above its name.
- **S6 — Transition arrows.** Every `transition first A then B` draws a directed
  arrow from A to B — **both endpoints present** (Task-2 regression target).
- **S7 — Transition triggers/guards.** Transition labels show the
  trigger/guard/effect (`accept requestReceived`, `if authenticated`) where present.
- **S8 — Entry/do/exit behaviors.** State internal behaviors
  (`entry openValve`, `do executeRefuel`, `exit closeValve`) appear in the state box.

## Requirements view — **viewer support lands in Task 3**

Reference: `cameo-notation/requirements-cameo.png` (and
`requirements-full-cameo.png` for scale). Probes: `requirements-trace.sysml`,
`requirements-pillar.sysml`.

- **Q1 — Frame + heading.** Frame with a `requirements <context>` tab (e.g.
  `requirements C&C Requirements`).
- **Q2 — `<<requirement>>` boxes.** Requirement usages render as boxes with a
  `<<requirement>>` stereotype line.
- **Q3 — Id + name header.** The header shows the short-name id and the name
  (e.g. `'R1' Power Management`).
- **Q4 — Statement/text compartment.** A compartment shows the requirement text
  (`attribute statement = The subsystem shall manage the power budget`).
- **Q5 — Derive edges.** `dependency from req to need` derive relationships draw
  as dashed edges with open arrowheads and a `derive`-style label (cheatsheet §4).
- **Q6 — Satisfy edges.** `satisfy req by feature` draws a dashed edge with a
  `satisfy` label from the satisfying feature to the requirement (cheatsheet §2).
- **Q7 — Verify edges.** The `verification def` objective's `verify` draws a
  `verify`-labeled edge to the requirement (cheatsheet §5).
- **Q8 — Verification def box.** The `verification def` renders as its own box
  with a `<<verification def>>` stereotype (sharp corners — it is a definition).

## Traceability view — **viewer support lands in Task 4**

Reference: `cameo-notation/traceability-cameo.png` (and
`traceability-full-cameo.png` for scale). Probes: `traceability-demo.sysml`,
`structural-pillar.sysml`.

- **T1 — Frame + heading.** Frame with a `traceability <context>` tab (e.g.
  `traceability C&C Trace`).
- **T2 — Node kinds distinguishable.** Requirement / part / action / verification
  nodes are visually distinct (stereotype text and/or shape:
  `<<requirement>>`, `<<part>>`, `<<action>>`, `<<verification def>>`).
- **T3 — Derive edges labeled.** Derive (`dependency`) edges are dashed with a
  `derive` label (cheatsheet §4).
- **T4 — Satisfy edges labeled.** `satisfy` edges are dashed with a `satisfy`
  label pointing to the requirement (cheatsheet §2).
- **T5 — Allocate edges labeled.** `allocate action to part` edges are dashed
  with an `allocate` label (cheatsheet §3).
- **T6 — Verify edges labeled.** `verify` edges (from `verification def` objective)
  are dashed with a `verify` label (cheatsheet §5).
- **T7 — Readable layered layout.** Nodes are placed in readable layers
  (needs → requirements → design → verification) with minimal edge crossings, not
  an overlapping tangle.
- **T8 — All four trace kinds present.** The rendered web shows all four trace
  kinds exercised by the probe (derive / satisfy / allocate / verify).
