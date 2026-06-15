# Reference — leverageable MBSE infrastructure

Project-rooted, pullable references for the **external tools, standards, and
infrastructure** the sysml-bridge workflow leverages or interoperates with. This
is curated reference material (what a thing is, what it gives us, how we use or
interoperate with it, source links) — distinct from:

- `docs/sysml-v2-reference/` — the vendored OMG SysML v2 grammar + cheatsheet (the
  emission source of truth).
- `docs/design.md` — this project's own architecture.
- `.planning/research/` — raw research notes (unfiltered; this folder is the curated digest).

**When to add here:** any time we evaluate, depend on, or follow an external
infrastructure (a validator, a renderer, a standard, a vendor tool, a guideline set),
capture a one-file reference so future work can pull it on instead of re-discovering it.

Each entry: what it is · what it gives us · how we use / interoperate · status &
maturity · source links · verification caveats (flag anything not verified at source).

## Index

### Validation & rules
- [SAIC Digital Engineering Validation Tool](saic-devt.md) — 251 Cameo validation rules
  (language + style); the basis for "SAIC-style relational consistency."

### Ingestion (prose / PDF corpora)
- [SEPAL corpus engine](sepal-corpus-engine.md) — sibling project's production ingestion
  (7-format parsers, chunk citations, RAG strategies, CUI/ITAR banner refusal); the
  integration target for prose ingestion — do not rebuild parsing.

### Renderers / viewers (SysML v2 graphical)
- [SysML v2 renderers](sysml-v2-renderers.md) — Cameo vs the free landscape (DeciSym fork,
  Eclipse SysON, Hollando sysml-reactflow, OMG pilot); which ingest `.sysml` and render which views.

### Standards
- _(add as referenced: OMG SysML v2 formal/25-09-03; IEEE 15288 §6.3.3 traceability; INCOSE.)_
