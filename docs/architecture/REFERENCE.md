# Reference — the teachings behind this doc

This architecture doc (`index.html`) is built on the patterns from Anthropic's writing on using
HTML as a rich output medium for Claude Code. Captured here so the source of the technique lives
in-repo, not just in a browser tab.

## Source articles

- **"Using Claude Code: the unreasonable effectiveness of HTML"** (Thariq Shihipar, Anthropic) —
  https://claude.com/blog/using-claude-code-the-unreasonable-effectiveness-of-html
- **`anthropics/html-effectiveness`** — the template gallery —
  https://github.com/anthropics/html-effectiveness
- **Hosted gallery** — https://thariqs.github.io/html-effectiveness/

## Why HTML over Markdown (the argument)

1. **Information density** — tables, CSS, SVG, scripts, and interaction carry far more than prose.
2. **Visual clarity** — large specs become *readable* through structure, tabs, illustration, links.
   ("I tend to not actually read more than a 100-line Markdown file.")
3. **Shareability** — one file you can send as a link; more likely to actually get read.
4. **Two-way interaction** — sliders, toggles, and "copy" buttons turn a doc into a small tool.
5. **Data ingestion** — Claude Code can pull from the file system, git history, and MCP servers to
   ground the document in real project state.

## Patterns this doc copies

From the article and the `html-effectiveness` templates (13-flowchart, 03-code-review,
05-design-system, 14/15-research-explainer, 19-editor-feature-flags):

- **Hand-authored SVG** for flowcharts and architecture diagrams — `<rect>`/`<path>`/`<text>`,
  arrowhead markers, animated `stroke-dashoffset` for flow.
- **A single semantic color system** — every pipeline stage owns one color, reused across every
  diagram, so the reader builds one mental map. Legend-driven.
- **Tabbed views** — one dense page split into navigable sections; degrades to a long scroll with
  JS off.
- **Comparison grids** — side-by-side "what each gate catches" using CSS Grid.
- **Hover tooltips + detail-on-demand** — nodes explain themselves without cluttering the diagram.
- **Lightweight interactive controls** — a backend-swap toggle and a filterable skills/tools grid
  (the "feature-flags / editor" pattern), copy-to-clipboard buttons (the "copy back to prompt"
  pattern).
- **Zero dependencies, single file** — inline CSS + SVG + vanilla JS; works offline; version-controls
  cleanly. ANGARS render PNGs are inlined as base64 so the one file is fully portable.

## How this file is built

`index.html` is generated from `template.html` by `build.mjs`, which inlines the ANGARS diagram
PNGs (`examples/angars/diagrams/*.png`) as base64 data URIs. To regenerate after editing the
template or refreshing a diagram:

```bash
node docs/architecture/build.mjs
```
