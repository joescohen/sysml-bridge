# SysML v2 renderers / viewers

Tools that turn a model into graphical diagrams, and how they relate to our
textual-notation (`.sysml`) emission. Verified hands-on 2026-06; re-check versions
before relying (the ecosystem moves fast).

## The core finding
There is **no free, drop-in tool** that ingests standard SysML v2 textual notation and
renders Cameo-fidelity diagrams. The interchange splits into three layers, and none is
"drop in `.sysml`, get Cameo's diagrams":
1. **Textual notation** (`.sysml`) — conformant parsers: Cameo (proprietary), the OMG
   Pilot (reference Xtext), SysIDE. SysON has its own *partial* importer.
2. **OMG SysML v2 API & Services** (REST/JSON) — pure data exchange, **zero diagram/layout
   info**. A data API, not a renderer.
3. **Proprietary** — Cameo `.mdzip`; SysON's Sirius-Web store via GraphQL.

## Options
| Tool | Free | Ingests `.sysml` | Renders | Notes |
|---|---|---|---|---|
| **Cameo / Magic Systems of Systems Architect SysML v2 CE** | CE free (500-elem cap) | ✅ conformant | ✅ all views, Cameo-fidelity | Synchronize (Alt+S) materializes; 500 "major element" cap blocks creation past it. The fidelity bar. |
| **DeciSym `sysmlv2-gui`** (github.com/DeciSym/sysmlv2-gui, MIT/Apache, Rust) | ✅ | ✅ direct | headless PDF CLI (`export_figures`) | Pre-alpha upstream. **We forked + extended it** — see below. |
| **Eclipse SysON** (Obeo, EPL-2) | ✅ no cap | ⚠️ partial importer | ✅ web UI, GUI-only export | Renders via its own Sirius model, NOT a faithful `.sysml` path. This repo tried it and abandoned it (`dashboard/` + `docker/` remnants). |
| **Hollando `sysml-reactflow`** (npm, MIT) | ✅ | ❌ needs factory objects | ✅ broad views, browser-only | Polished React Flow library; no `.sysml` parser, no headless export. |
| **OMG Pilot `%viz`** (Jupyter/PlantUML) | ✅ | ✅ native | ⚠️ rough auto-graphs, sub-Cameo | Good for round-trip checks, not polished demos. |

## Our DeciSym fork (the working path)
Location: a fork of DeciSym `sysmlv2-gui` (currently in a scratch dir — **move into the repo**,
e.g. `tools/decisym-viewer/`, so it isn't lost). The `export_figures` PDF exporter was extended
to render, headless from `.sysml`, all the Cameo pillars:
- **IBD** (parts + directioned ports + connections), **BDD** (composition ◆ / specialization ▷ /
  multiplicity / attribute compartments), **Activity** (actions, control flow, object/item flow with
  labels, initial ●/final ◉, decision ◇, fork/join ▬, guards), **State** (entry/do/exit, transitions
  with trigger/[guard], cyclic back-edge routing), **Requirements** (id + text boxes, derive tree,
  satisfy/verify/derive edges), **Traceability** (cross-pillar web, all 4 trace kinds).
- Recurring gotcha fixed repeatedly: DeciSym's parser **silently discards** what it doesn't model
  (successions, transition endpoints, entry/do/exit, satisfy/derive/verify, `= value`). Each new
  view needs a parser-capture step, not just a renderer.
- Build: `rustup` then `cargo build --release --bin export_figures`; run `export_figures <file.sysml> <out-dir>`.

## How we use it
Our serializer emits standard `.sysml`; **DeciSym (our fork) is the headless renderer** for demo
artifacts (no Cameo, no 500-cap). Keep Cameo for hero/high-fidelity proofs.
