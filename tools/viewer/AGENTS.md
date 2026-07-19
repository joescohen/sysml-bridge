# sysmlv2-gui Development Guide

## Project Overview

Lightweight SysML v2 graphical viewer/editor in Rust with egui.
Renders the graphical notation prescribed by OMG SysML v2
(formal/25-09-03). Runs as a native desktop app (OpenBSD, Windows)
and as a web app (WASM/WebGL) from the same codebase. Coordinates
with external tools rather than reimplementing their functionality.

## Build & Run

### Native

```sh
cargo build                # debug build
cargo build --release      # release build
cargo run                  # run debug
cargo clippy               # lint
cargo test                 # run tests
```

### Web (WASM)

```sh
rustup target add wasm32-unknown-unknown   # one-time setup
cargo install trunk                        # one-time setup
trunk serve                                # dev server with hot reload
trunk build --release                      # production build to dist/
```

### Container

```sh
docker build -t sysmlv2-gui .
docker run -p 8080:8080 -v /path/to/models:/models sysmlv2-gui
```

## Deployment Targets

This project targets three deployment modes. All share the same Rust
rendering code.

### Native Desktop (OpenBSD primary, Windows, Linux, macOS)

- eframe with glow backend (OpenGL ES 2.0+ / OpenGL 3.2+)
- Direct filesystem access, CLI validator, file watching
- Primary development and testing platform: OpenBSD

### Web Application (WASM/WebGL)

- Same eframe/glow code compiles to WASM; glow maps to WebGL
- Two use cases:
  - **Demos on decisym.ai**: prospects interact without installing
  - **Enterprise/DoD deployment**: containerized web service for
    managed assets that prohibit software installation
- File access and validation via backend API (not local filesystem)

### GPU Acceleration (Future)

- Switch from glow to wgpu via Cargo feature flag for GPU-intensive
  views (large graphs, force-directed layout)
- wgpu targets DX12 (Windows), Vulkan (Linux), WebGPU (browsers)
- OpenBSD stays on glow (no Vulkan)
- Application code does not change; only the eframe backend feature

## Architecture

### Module Structure

```
src/
  main.rs          -- entry point, eframe setup (native + web)
  app.rs           -- top-level App implementing eframe::App
  model/           -- SysML v2 model representation
    mod.rs         -- element types, relationships, containment
    parse.rs       -- .sysml file parser (structural, not semantic)
  render/          -- graphical notation rendering
    mod.rs         -- view rendering dispatch
    elements.rs    -- definition/usage rectangles, compartments
    connectors.rs  -- relationship lines, arrowheads, decorations
    ports.rs       -- port symbols on element boundaries
    layout.rs      -- automatic element positioning
    frame.rs       -- diagram frame with view type tab
  panels/          -- UI panels
    browser.rs     -- model element tree (egui_ltreeview)
    canvas.rs      -- diagram canvas (egui Painter)
    properties.rs  -- element property editor
    validation.rs  -- validation output display
    source.rs      -- textual notation viewer
  platform/        -- platform abstraction
    mod.rs         -- trait definitions for file I/O and validation
    native.rs      -- filesystem + CLI validator (cfg not wasm)
    web.rs         -- HTTP API client (cfg wasm)
  validator.rs     -- interface to validate-sysml CLI (native)
  watcher.rs       -- file system watching for live reload (native)
```

### Platform Abstraction

File I/O and validation differ between native and web. Abstract
behind traits and use `#[cfg(target_arch = "wasm32")]` to select
the implementation:

```rust
pub trait ModelSource {
    fn load(&self, path: &str) -> Result<String>;
    fn watch(&self, path: &str, callback: impl Fn());
}

pub trait Validator {
    fn validate(&self, content: &str) -> Vec<Diagnostic>;
}
```

- **Native** (`platform/native.rs`): reads files from disk, calls
  `validate-sysml` as a subprocess, uses `notify` for file watching.
- **Web** (`platform/web.rs`): fetches model text from a backend API,
  posts content for server-side validation, no file watching (uses
  polling or WebSocket for updates).

### Key Design Rules

1. **Definitions get sharp corners, usages get rounded corners.** This
   is the fundamental visual rule of SysML v2. Never mix them up.

2. **The model representation is lightweight.** It captures structure
   (elements, features, relationships, containment) sufficient for
   rendering. It does NOT implement the full KerML metamodel. Semantic
   analysis is delegated to the validator.

3. **Parsing is structural, not semantic.** The parser extracts element
   kinds, names, types, features, and nesting from `.sysml` text. It
   does not resolve references, check types, or validate semantics.

4. **Validation is external.** On native, call `validate-sysml` as a
   subprocess. On web, call the backend validation API. Parse
   GNU-format output (`file:line:col: severity: message`). Never
   reimplement validation logic.

5. **Rendering follows the specification.** Consult the GBNF in
   subclause 8.2.3 of formal/25-09-03 and the graphical notation
   intro PDF for rendering rules. When in doubt, check what SysON
   does.

6. **Same rendering code for all platforms.** The `render/` module
   must not contain platform-specific code. All platform differences
   live in `platform/`.

### Spec Conformance Invariant

Compliance with the OMG SysML v2 graphical notation specification
(formal/25-09-03) is an invariant of this project. No change may
violate the specification. Specifically:

- **Corner radii**: `DEFINITION_CORNER_RADIUS` (0.0) for definitions
  and packages; `USAGE_CORNER_RADIUS` (6.0) for usages. This applies
  at every LOD level and in every rendering path (full, compact,
  label, dot, overview).
- **Named constants only**: every numeric value in `src/render/` that
  controls drawing (stroke widths, font scales, spacing, sizes) must
  reference a named constant in `theme.rs`. No bare magic numbers in
  rendering code.
- **All stroke widths named**: every `Stroke::new()` call in
  rendering code must use a `theme::` constant for its width argument.
- **Keyword format**: keywords are lowercase text rendered inside
  `<<` `>>` guillemets.
- **Port geometry**: ports are `PORT_SIZE` x `PORT_SIZE` squares with
  `CornerRadius::ZERO`, centered on the parent element boundary.
- **Frame geometry**: frames use `CornerRadius::ZERO` with the tab
  anchored at the top-left corner.
- **Connector styles**: flow connectors have filled arrowheads;
  bind and allocate connectors are dashed; each relationship kind
  has a distinct color.

These rules are enforced by `tests/spec_conformance.rs`. That test
suite must pass before any change is merged. When adding new rendering
code, add corresponding spec conformance tests.

## Coding Conventions

### Rust Style

- Edition 2024. Use current idioms.
- `cargo clippy` must pass with no warnings.
- Prefer `&str` over `String` in function signatures where possible.
- Use `thiserror` for error types, not string errors.
- No `unwrap()` in library code. `unwrap()` is acceptable only in
  tests and in `main()` for fatal setup errors.
- Keep functions short. Extract when a function exceeds ~40 lines.
- Minimize `pub` surface. Default to private; expose only what panels
  and renderer need.

### Cross-Platform Rules

- Use `#[cfg(not(target_arch = "wasm32"))]` for native-only code.
- Use `#[cfg(target_arch = "wasm32")]` for web-only code.
- Never use `std::fs`, `std::process`, `std::net`, or `notify` in
  code that compiles for WASM. These do not exist on `wasm32`.
- The `model/`, `render/`, and `panels/` modules must be
  platform-agnostic. Only `platform/`, `validator.rs`, and
  `watcher.rs` may use `cfg` gates.

### egui Conventions

- Use the **glow** backend. Feature flags:
  `eframe = { default-features = false, features = ["glow"] }`
- For WASM builds, eframe automatically uses WebGL via glow.
- Use `egui::Painter` for all diagram rendering. Do not drop to raw
  OpenGL/WebGL unless absolutely necessary.
- Panels use `egui::SidePanel`, `egui::TopBottomPanel`,
  `egui::CentralPanel`. Use `egui_tiles` for user-rearrangeable
  docking.
- Immediate mode: compute layout and draw every frame. Cache
  expensive computations (parsing, layout) but not draw calls.
- Colors and sizes should be defined as constants in a theme module,
  not hardcoded in rendering code.

### SysML v2 Rendering Constants

These rendering rules come from the specification:

```rust
// Corner radius for usage elements (definitions use radius 0.0)
const USAGE_CORNER_RADIUS: f32 = 6.0;

// Port size (small square straddling element boundary)
const PORT_SIZE: f32 = 12.0;

// Compartment separator line
const COMPARTMENT_LINE_WIDTH: f32 = 1.0;

// Connector line widths
const CONNECTOR_LINE_WIDTH: f32 = 1.5;
const CONNECTOR_DASH_LENGTH: f32 = 6.0;
const CONNECTOR_DASH_GAP: f32 = 4.0;
```

### Connector Routing Design Principles

Seven principles govern connector routing. All routing distances
reference `ROUTE_*` constants in `theme.rs`. Zero magic numbers in
`layout.rs`.

1. **Frame encompasses all content.** The bounding box includes
   elements, ports, AND connector waypoints. `Layout::content_bounds()`
   is the single source of truth for frame sizing.

2. **Dedicated routing corridors.** Explicit space between the frame
   edge and the element field is reserved for connector detours.
   Elements start at `FRAME_PADDING + ROUTE_FRAME_MARGIN` vertically
   and `FRAME_PADDING + ROUTE_FRAME_MARGIN + ROUTE_COLUMN_GAP_MARGIN`
   horizontally.

3. **Minimum clearances.** Routes maintain `ROUTE_ELEMENT_CLEARANCE`
   (20px) from element edges and `ROUTE_FRAME_MARGIN` (30px) from
   the frame edge. Based on professional tool standards (yEd, draw.io,
   Visio).

4. **Named constants only.** Every routing distance references a
   `ROUTE_*` constant. The constants and their values:

   ```rust
   ROUTE_STUB_LENGTH       = 20.0  // port exit before first bend
   ROUTE_ELEMENT_CLEARANCE = 20.0  // min gap from route to element
   ROUTE_PARALLEL_SPACING  =  8.0  // min y-separation for overlapping h-segments
   ROUTE_FRAME_MARGIN      = 30.0  // min distance from route to frame edge
   ROUTE_CHANNEL_SPREAD    =  8.0  // x-offset between parallel vertical channels
   ROUTE_COLUMN_MERGE      = 10.0  // threshold for merging adjacent columns
   ROUTE_COLUMN_GAP_MARGIN = 25.0  // margin outside first/last column
   ROUTE_EDGE_TOLERANCE    =  2.0  // threshold for matching point to edge
   ROUTE_BOUNDARY_MARGIN   =  1.0  // inset for crossing checks
   ```

5. **Parallel deconfliction.** Horizontal segments from different
   connectors that overlap in x are offset by `ROUTE_PARALLEL_SPACING`
   (8px, derived from PCB 3W rule: >=3x the 1.5px connector line
   width plus margin). Implemented via `deconflict_y()` which tracks
   `used_h_levels` across all routed connectors.

6. **Drawing order.** Connectors drawn BEFORE elements. Element fill
   rectangles visually occlude lines that pass through element areas.
   This is industry standard in yEd, Visio, and draw.io.

7. **Visual hierarchy.** Connector stroke (1.5px, colored per
   relationship type) is distinct from frame stroke (2.0px, dark
   blue-gray). Never confusable.

## External Tool Integration

### validate-sysml (native mode)

```sh
validate-sysml path/to/file.sysml
```

- Expects `validate-sysml` on PATH or at
  `~/.local/share/sysmlv2-validator/validate-sysml`
- Requires Java 21+
- Output: GNU format errors to stderr
- Exit code: 0 = valid, 1 = errors

### Validation API (web mode)

The containerized deployment runs `validate-sysml` server-side. The
WASM client posts model content to a REST endpoint and receives
diagnostics as JSON. The API contract:

```
POST /api/validate
Content-Type: text/plain
Body: <.sysml file content>

Response: 200
Content-Type: application/json
Body: [{"file": "...", "line": 1, "col": 1, "severity": "error", "message": "..."}]
```

### File Watching (native mode only)

Watch `.sysml` files for changes from external editors (Emacs
sysml-mode). On change: re-parse, optionally re-validate, update
display. Use `notify` crate. Not available in WASM builds.

## SysML v2 Skill

When working on this project with AI assistance, the `sysmlv2-skill`
provides authoritative syntax guidance. Key rules:

- Imports require visibility: `private import ScalarValues::*;`
- Attribute defs must specialize: `attribute def Region :> String;`
- Import ScalarValues for basic types (String, Real, Integer, Boolean)
- See `../sysmlv2-skill/references/SYNTAX.md` for complete rules

## Testing Strategy

- **Unit tests**: model parsing, rendering geometry calculations,
  layout algorithms
- **Integration tests**: parse `.sysml` files from the SysML v2
  Release repository's examples and verify structural extraction
- **Visual tests**: screenshot comparison (later milestone)
- **WASM tests**: verify the build compiles and runs under
  `wasm32-unknown-unknown` in CI
- Test `.sysml` files go in `tests/fixtures/`

## Dependencies

Keep dependencies minimal. Justified additions:

| Crate | Purpose | Platforms | Required |
|-------|---------|-----------|----------|
| `eframe` | egui native + web app framework (glow backend) | All | Yes |
| `egui` | Immediate mode GUI | All | Yes |
| `egui_extras` | Additional egui widgets | All | Yes |
| `egui_tiles` | Panel docking/tiling | All | Yes |
| `notify` | File system watching | Native only | Yes |
| `thiserror` | Error type derivation | All | Yes |
| `reqwest` | HTTP client for backend API | WASM only | Yes (web) |
| `serde` / `serde_json` | Serialization for API responses | All | Yes |

Add dependencies only when they provide clear value over a simple
implementation. Prefer stdlib solutions. Mark native-only deps with
`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
