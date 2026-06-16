# Architecture (visual)

A single-file, interactive visual map of the **sysml-bridge** system architecture.

**Open it:** [`index.html`](index.html) — double-click, or `open docs/architecture/index.html`.
Zero dependencies, works offline, fully self-contained (the ANGARS diagrams are inlined).

It diagrams the *current* architecture (corpus + NL → human-gated candidate generation → IR →
15 skills + MCP server → serializer → local ANTLR validator → Cameo → decisym-viewer), in six
tabbed views: Overview, Layered architecture, Corpus → IR, Authoring core, the Validation gate,
and Repo & ANGARS.

- [`template.html`](template.html) — editable source (uses `%%IMG:<name>%%` tokens).
- [`build.mjs`](build.mjs) — inlines the diagram PNGs as base64 → regenerates `index.html`.
- [`REFERENCE.md`](REFERENCE.md) — the HTML-as-rich-output technique this doc is built on.

Regenerate after editing the template: `node docs/architecture/build.mjs`.
