# sysmlv2-gui

A lightweight graphical viewer and editor for SysML v2 models,
implemented in Rust with egui. Runs as a native desktop application
(OpenBSD, Windows, Linux, macOS) and as a web application
(WASM/WebGL) from the same codebase.

## Philosophy

Existing SysML v2 graphical tools (Eclipse SysON, Cameo, etc.) are
heavyweight Java/web applications that try to do everything. This
project takes the opposite approach, drawing from three traditions:

### OpenBSD: Simplicity and Correctness

- **Do one thing well**: render SysML v2 graphical views correctly
- **Coordinate, don't subsume**: integrate with external tools for
  editing (Emacs/sysml-mode), validation (sysmlv2-validator), and AI
  assistance (sysmlv2-skill) rather than reimplementing them
- **Prefer clarity over cleverness**: straightforward Rust, immediate
  mode rendering, minimal abstraction

### Shneiderman's Mantra: Overview First, Zoom and Filter, Then Details-on-Demand

Ben Shneiderman's Visual Information Seeking Mantra
(*The Eyes Have It*, 1996) defines a three-phase interaction pattern
for exploring information spaces:

1. **Overview first** — present the full scope so the user orients
   before acting
2. **Zoom and filter** — let the user narrow focus by spatial zoom
   and semantic filtering
3. **Details-on-demand** — reveal specifics for the item under
   attention, without leaving context

This mantra governs how we structure navigation through a SysML model:
the browser gives overview, the canvas gives spatial zoom, and the
properties panel gives details-on-demand for the selected element.

### Raskin: The Humane Interface

Jef Raskin's *The Humane Interface: New Directions for Designing
Interactive Systems* (Addison-Wesley, 2000) establishes principles
that protect the user's attention and reduce cognitive load:

- **Modelessness** — the same gesture should always produce the same
  result; modes are a source of errors
- **Locus of attention** — the interface should keep the user's focus
  on content (the model), not on chrome (panels, toolbars, dialogs)
- **Habituation** — consistent, predictable responses to actions
  allow the user to develop automatic motor patterns
- **Monotony of design** — interaction patterns should be uniform
  across all parts of the interface
- **Quantifiable quality** — interface efficiency should be measured
  (GOMS keystrokes, pointer distance), not assumed

These principles push toward a content-dominant canvas, minimal mode
switching, and uniform click/zoom/select behavior everywhere.

### Synthesis

The GUI reads `.sysml` text files, parses their structure, and renders
the graphical notation prescribed by the OMG SysML v2 specification.
It is a viewer and navigator first, with editing capabilities added
incrementally.

## Deployment Targets

### Native Desktop

The primary development platform is OpenBSD. The native build uses
eframe with the glow backend (OpenGL ES 2.0+ / OpenGL 3.2+), the same
rendering stack proven on OpenBSD by Alacritty. Native builds also
target Windows and Linux/macOS.

In native mode the tool has full access to the local filesystem,
the `validate-sysml` validator, and file watching for live
synchronization with text editors.

### Web Application (WASM/WebGL)

The same Rust codebase compiles to WebAssembly and renders via WebGL
in any modern browser. eframe provides first-class WASM support — the
glow backend maps directly to WebGL with no code changes.

Web deployment serves two purposes:

- **Demos and evaluation**: host on decisym.ai for prospects to
  interact with SysML v2 models without installing software
- **Enterprise and DoD environments**: deploy as a containerized web
  service for organizations whose managed assets prohibit local
  software installation. Users access the tool through a browser
  against a server that holds the model files and runs validation.

In web mode, the validator runs server-side. The WASM client
communicates with a backend API for file access and validation
results.

### GPU Acceleration (Future)

The glow (OpenGL) backend is sufficient for standard diagram
rendering. For advanced visualizations — large system-of-systems
graphs, force-directed layouts with thousands of nodes, interactive
exploration of deep model hierarchies — the backend can be switched
to wgpu on platforms that support it (Windows/DX12, Linux/Vulkan,
web/WebGPU). This is a Cargo feature flag change, not a code rewrite.
OpenBSD continues to use the glow backend.

## SysML v2 Graphical Notation

The OMG SysML v2 specification (formal/25-09-03) defines a formal
graphical concrete syntax alongside the textual concrete syntax. The
graphical notation is specified via Graphical BNF (GBNF) in subclause
8.2.3 of the specification. Key rendering rules:

### Definitions vs. Usages

The fundamental visual distinction in SysML v2:

| Concept | Corner Style | Example Keywords |
|---------|-------------|-----------------|
| **Definition** (reusable type) | Sharp corners | `part def`, `action def`, `port def` |
| **Usage** (instance in context) | Rounded corners | `part`, `action`, `port` |

This pattern applies universally across all element kinds.

### Element Rendering

Each element is a rectangle with:

- **Header compartment**: keyword label + name (+ `: Type` for usages)
- **Feature compartments**: separated by horizontal lines, containing
  attributes, parts, ports, references, constraints, requirements,
  actions, or documentation
- **Compartment text**: may use textual notation identical to the
  `.sysml` source

### Relationship Connectors

| Relationship | Line Style | End Decoration |
|-------------|-----------|----------------|
| Specialization (`:>`) | Solid | Hollow triangle at supertype |
| Composition | Solid | Filled diamond at owner |
| Reference | Dashed | Open arrowhead at target |
| Connection | Solid | None (labeled) |
| Dependency | Dashed | Open arrowhead |
| Satisfy | Dashed | `satisfy` keyword label |
| Flow (object) | Solid arrow | Arrowhead at target |
| Flow (control) | Dashed arrow | Arrowhead at target |

### Ports

Ports are small rectangles straddling the boundary of their owning
element (half inside, half outside). Port definitions have sharp
corners; port usages have rounded corners. Conjugated ports (`~`) are
marked distinctly.

### View Types

SysML v2 replaces fixed diagram types with a view/viewpoint mechanism.
Standard view types:

| View Type | Replaces (v1) | Shows |
|-----------|--------------|-------|
| General View | BDD, Package, Req, UC | Definitions, usages, relationships |
| Interconnection View | IBD | Parts, ports, connections in context |
| Action Flow View | Activity | Actions, control/object flow |
| State Transition View | State Machine | States, transitions |
| Sequence View | Sequence | Event occurrences on lifelines |
| Geometry View | *(new)* | Spatial items in 2D/3D |
| Grid View | *(new)* | Tabular element layout |
| Browser View | *(new)* | Hierarchical membership tree |

### Behavior Elements

| Element | Definition Shape | Usage Shape |
|---------|-----------------|-------------|
| Action | Sharp rectangle | Rounded rectangle |
| State | Sharp rectangle | Rounded rectangle |
| Decision/Merge | Diamond | Diamond |
| Fork/Join | Synchronization bar | Synchronization bar |

### Diagram Frame

Views are enclosed in a frame with a tab (upper-left) showing the view
kind keyword and name.

## Architecture

```
sysmlv2-gui (Rust/egui)
    |
    +-- reads .sysml files (textual notation)
    +-- parses model structure
    +-- renders graphical views (egui Painter API)
    |
    +-- native: calls validate-sysml (Java CLI) for validation
    +-- native: watches files for external edits (Emacs/sysml-mode)
    +-- web: calls backend API for file access and validation
    |
    +-- glow backend: OpenGL (native) / WebGL (browser)
    +-- wgpu backend: DX12/Vulkan/WebGPU (future, GPU-intensive views)
```

### Component Design

- **Parser**: reads `.sysml` textual notation into an in-memory model
  representation. Does not need full semantic analysis — delegates
  that to the validator.
- **Model**: internal representation of SysML v2 elements,
  relationships, and containment hierarchy. Lightweight; not a full
  KerML metamodel implementation.
- **Renderer**: translates model elements into graphical primitives
  (rectangles, lines, text, ports) following the specification's
  rendering rules. Same code path for native and web.
- **Layout**: automatic positioning of elements and routing of
  connectors. Starts simple (force-directed or layered), improves
  incrementally.
- **Panels**: model browser (tree), properties panel, diagram canvas,
  validation output, source text.
- **Platform layer**: abstracts file I/O and validation behind a trait
  so native mode calls the filesystem and CLI directly while web mode
  calls a backend API.

### External Dependencies

| Tool | Purpose | Interface | Availability |
|------|---------|-----------|-------------|
| [sysmlv2-validator](https://github.com/decisym/sysmlv2-validator) | Syntax/semantic validation | CLI: `validate-sysml <file>`, GNU error format | Native + server-side |
| [sysml-mode](https://github.com/decisym/sysml-mode) | Emacs editing | File watching for live reload | Native only |
| [sysmlv2-skill](https://github.com/decisym/sysmlv2-skill) | AI modeling guidance | Agent skill (Claude Code) | Development |
| Java 21+ | Required by validator | Runtime dependency | Native + server-side |

## Technology

- **Language**: Rust (edition 2024)
- **GUI**: [egui](https://github.com/emilk/egui) with eframe
- **Native rendering**: glow (OpenGL ES 2.0+ / OpenGL 3.2+)
- **Web rendering**: glow (WebGL), same backend
- **Future GPU**: wgpu (DX12, Vulkan, WebGPU) via feature flag
- **Platforms**: OpenBSD (primary), Windows, Linux, macOS, web browsers

### Why egui

egui was selected after evaluating nine Rust GUI toolkits (egui, iced,
fltk-rs, Slint, Makepad, Dioxus, Tauri, xilem, x11rb) against five
criteria: cross-platform deployment, OpenBSD support, custom 2D
drawing, widget availability, and simplicity.

egui is the only toolkit that satisfies all deployment targets from a
single Rust codebase:

1. **OpenBSD**: eframe uses winit + glutin + glow — the same stack
   proven on OpenBSD by Alacritty
2. **Windows**: native support, drop-in
3. **Web/WASM**: eframe compiles to WebAssembly with WebGL rendering,
   same Painter API, same rendering code — no separate frontend
4. **GPU upgrade path**: swap the feature flag from glow to wgpu on
   platforms that support it; no application code changes
5. **Diagramming + widgets**: Painter API for custom 2D drawing
   alongside built-in panels, menus, and text editing
6. **Simplicity**: immediate mode paradigm, minimal boilerplate,
   idiomatic Rust
7. **License**: MIT OR Apache-2.0

Alternatives were eliminated for specific reasons:

- **fltk-rs**: no viable WASM/web path (most "OpenBSD-ish" for
  native-only, but fails the web deployment requirement)
- **iced**: OpenBSD requires tiny-skia CPU fallback (risky); web
  support deprioritized
- **Slint**: no Canvas widget for custom 2D diagramming
- **Tauri/Dioxus**: require two codebases (Rust + JS/TS) or route
  custom drawing through JavaScript eval
- **Makepad**: unproven on OpenBSD, immature API, sparse documentation

## Building

### Native (OpenBSD / Windows / Linux / macOS)

```sh
cargo build --release
```

Requires:
- Rust toolchain (edition 2024)
- OpenGL-capable display
- For validation: Java 21+, `validate-sysml` on PATH

### Static figure export

For slide decks and documents, the native exporter can render a small
set of SysML v2 graphical views directly to PDF from a `.sysml`
model:

```sh
cargo run --bin export_figures -- path/to/model.sysml path/to/output-dir
```

The exporter is intended for deterministic document assets, not for
interactive browsing. It reuses the same parser, layout logic, and
graphical-notation theme as the GUI.

### Web (WASM)

```sh
trunk serve                    # development server with hot reload
trunk build --release          # production WASM build
```

Requires:
- Rust toolchain with `wasm32-unknown-unknown` target
- [Trunk](https://trunkrs.dev/) for WASM bundling

### Container (web service deployment)

```sh
docker build -t sysmlv2-gui .
docker run -p 8080:8080 -v /path/to/models:/models sysmlv2-gui
```

The container bundles the WASM frontend, a static file server, and
the `validate-sysml` Java validator for server-side validation.

## Authoritative References

### OMG Specifications

- **SysML 2.0 Specification — Language** (formal/25-09-03):
  https://www.omg.org/spec/SysML/2.0/About-SysML
  - Graphical concrete syntax: subclause 8.2.3 (Graphical BNF)
  - View/Viewpoint mechanism: clause 12

- **KerML 1.0 Specification** (formal/25-09-01):
  https://www.omg.org/spec/KerML/1.0/About-KerML

- **Systems Modeling API 1.0** (formal/25-09-07):
  https://www.omg.org/spec/SystemsModelingAPI/1.0/About-SystemsModelingAPI

### Implementation References

- **SysML v2 Release Repository** (GBNF files, standard libraries):
  https://github.com/Systems-Modeling/SysML-v2-Release

- **SysML v2 Pilot Implementation** (reference parser/validator):
  https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation

- **Graphical Notation Introduction** (tutorial PDF):
  https://github.com/Systems-Modeling/SysML-v2-Release/blob/2025-12/doc/Intro%20to%20the%20SysML%20v2%20Language-Graphical%20Notation.pdf

- **Eclipse SysON** (open-source graphical tool, view implementation
  reference): https://doc.mbse-syson.org/

- **Sodius/Willert SysML v2 Cheat Sheet**:
  https://www.sodiuswillert.com/hubfs/Downloadables/SodiusWillertSysMLv2CheatSheet.pdf

### OMG Approval

SysML v2.0 was formally adopted by the OMG on July 21, 2025:
https://www.omg.org/news/releases/pr2025/07-21-25.htm

## License

MIT OR Apache-2.0
