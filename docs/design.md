# SysML Bridge — Design Spec

> Claude Code skill suite + MCP server that turns natural language (and existing corpora) into
> **grammar-valid SysML v2** models, validated locally and materialized in Cameo Enterprise
> Architecture. A rich, interactive companion to this spec lives at
> [`docs/architecture/index.html`](architecture/index.html).

## Problem

MBSE in Cameo is manual, slow, and license-locked. SysML v2's **textual notation** makes
LLM-driven model generation viable for the first time — but an LLM writing SysML v2 from memory
produces syntax that only fails when you try to import it, burning slow manual import attempts. The
bridge solves both halves: it lets Claude Code author the model in natural language, and it makes
the output **provably importable** by validating every `.sysml` file against the vendored OMG
grammar before any claim that it imports.

## The pipeline (what actually runs)

```
   Natural language  +  corpus (PDFs, spreadsheets, conversation)
                          │
        ┌─────────────────┴───────────────────┐
        │   prose-ingest          inference    │   candidate generation
        │   (NL → candidates)     (F8 propose/ │   (both LLM-driven)
        │                          debate)      │
        └─────────────────┬───────────────────┘
                          │   HUMAN-APPROVAL GATE  (no auto-approve path)
                          ▼
            IR  —  extracted.json + approved prose/inferred layers (composeIR)
                          │
        ┌─────────────────┴───────────────────┐
        │   15 MBSE skills        MCP server   │
        │   (Claude Code)         (ModelStore) │
        └─────────────────┬───────────────────┘
                          │   sysml-serializer
                          ▼
              .sysml  —  SysML v2 textual notation
                          │
              local ANTLR validator  —  GRAMMAR GATE (must be 0 errors)
                          │
                          ▼
       Cameo  —  Textual Editor → Synchronize (Alt+S) → materialized model
                          │
              decisym-viewer  →  static PDF / PNG figures
```

Two discipline rules make this reliable (see [CLAUDE.md](../CLAUDE.md)):

1. **The grammar is the source of truth.** The serializer conforms to the grammar vendored in
   [`docs/sysml-v2-reference/`](sysml-v2-reference/), never to model memory.
2. **Validate before claiming import.** Every generated or edited `.sysml` passes the local
   validator (0 errors) before it is claimed to import — and the live Cameo import remains the
   semantic backstop for the def-vs-usage rules grammar can't catch (R4).

## Architecture — three boundaries

### 1. Model store — `ModelStore` interface (backend-agnostic)

The MCP server is the **only** component that knows where the model lives. Every tool depends on
the `ModelStore` interface ([`packages/mcp-server/src/store.ts`](../packages/mcp-server/src/store.ts)),
never on a concrete backend, so the backend is a one-line swap:

| Backend | When | Selected by |
|---|---|---|
| `FileStore` (**default**) | file-native — a diffable JSON model doc on disk, no server | `SYSML_BRIDGE_BACKEND=file` |
| `SmapsClient` | live SysML v2 REST API (SMAPS / Cameo server) | `SYSML_BRIDGE_BACKEND=smaps` + `SMAPS_ENDPOINT` |

The research consensus (and the demo default) is **file-native**: zero infrastructure, fully
diffable, no Docker. SMAPS remains available behind the same interface for the live-server path.

### 2. Serializer + validator — the grammar gate

[`sysml-serializer.ts`](../packages/mcp-server/src/utils/sysml-serializer.ts) emits SysML v2
textual notation from the store's elements/relationships.
[`tools/sysml-validator/`](../tools/sysml-validator/) is a committed Python ANTLR parser (generated
from the vendored OMG `.g4`) that checks `.sysml` **syntax** with no Java at runtime. The serializer
is held to the grammar by:

- **Name safety** — names that collide with the 173 SysML v2 reserved keywords (e.g. a port named
  `out`, a part named `state`) are emitted quoted (`'out'`); embedded quotes/backslashes are escaped
  per the grammar's `STRING` token. (Both behaviors are regression-tested.)
- **Trace operands are usages** (R4), `verify` appears only inside a `verification def`'s
  `objective{}` body (R3), and trace edges follow the grammar's
  `satisfy/allocate/dependency` productions.

### 3. Two corpus-driven layers — always human-gated

On top of the authoring core sit two LLM-driven layers that **never** write to the model on their
own:

- **prose-ingest** ([`packages/prose-ingest/`](../packages/prose-ingest/)) — parses unstructured
  documents (PDFs, specs) into candidate requirements with citations.
- **inference / F8** ([`packages/inference/`](../packages/inference/)) — proposes high-confidence
  links (allocations, mode memberships, flow typing, control joins) from the model graph via an LLM
  propose/debate pipeline.

Both emit **candidates** that a human approves or rejects one at a time (`/mbse-ingest`,
`/mbse-infer`). There is **no auto-approve path**: approvals require an explicit `AskUserQuestion`
turn, rejections persist and are skipped on re-ingest, and only approved entries enter the IR via
`composeIR` ([`packages/ir/`](../packages/ir/)). This is enforced by structural proofs in
[`scripts/gd-*-proof.ts`](../scripts/).

## Skill suite — 15 skills

Skills operate **only** through the MCP store tools and the canonical `extracted.json` IR contract —
they never touch files or an API directly.

**Authoring:** `/mbse-init`, `/mbse-requirements`, `/mbse-build` (BDD/IBD + F1–F9 activities),
`/mbse-decompose`, `/mbse-trace`.
**Analysis & V&V:** `/mbse-validate` (binary traceability gate), `/mbse-verify`, `/mbse-trade`,
`/mbse-kpp`.
**Views & query:** `/mbse-views`, `/mbse-diagram` (Mermaid), `/mbse-query`.
**Corpus-driven (human-gated):** `/mbse-ingest`, `/mbse-infer`, `/mbse-edit`.

## MCP server — 11 backend-agnostic tools

`init_project`, `create_element`, `query_elements`, `update_element` (GATE-05 coupling check),
`delete_element` (refuses to strand relationship endpoints), `create_relationship`,
`query_relationships`, `validate_model` (completeness/consistency + binary traceability gate),
`export_sysml`, `import_sysml`, `get_project_state`.

**Design decisions:**
- **Tools are element-agnostic.** `create_element(type, name, attributes)` handles every SysML v2
  type; the `type` string drives serialization. No per-type tool sprawl.
- **Relationships are first-class.** Traceability is the point of MBSE — `create_relationship` /
  `query_relationships` are distinct tools, and in the file store relationships are persisted as
  elements carrying `source`/`target` id arrays.
- **`export_sysml` / `import_sysml` enable the file workflow.** Author through skills, export to
  `.sysml`, hand off to Cameo; or import external `.sysml` back into the store.

## Repo structure

```
sysml-bridge/
├── packages/
│   ├── mcp-server/     # ModelStore (FileStore default, SmapsClient optional), serializer,
│   │                   #   parser, audit/validate gate, 11 MCP tools
│   ├── ir/             # canonical extracted.json contract (zod) + composeIR (prose + inferred layers)
│   ├── prose-ingest/   # PDF → chunk → LLM requirement detection → candidates (human-gated)
│   ├── inference/      # F8 engine — candidate gen → type gate → propose → debate (human-gated)
│   └── skills/         # 15 Claude Code MBSE skills (+ _shared helpers)
├── tools/
│   ├── sysml-validator/  # committed Python ANTLR grammar validator (run.sh auto-bootstraps .venv)
│   └── decisym-viewer/   # Rust/egui viewer + static PDF/PNG figure exporter
├── scripts/            # generators, extractors, gate-discipline proofs, synthetic-stress harness
├── examples/
│   ├── angars/         # end-to-end ANGARS C&C demo (corpus → model → Cameo handoff)
│   └── demos/          # standalone grammar-valid .sysml demos (one per SysML aspect)
└── docs/
    ├── architecture/   # interactive HTML architecture doc (index.html)
    └── sysml-v2-reference/  # vendored OMG grammar (.g4) + cheatsheet — the source of truth
```

## Error handling

- **No backend required.** The default file store is always reachable; nothing needs Docker. When
  the SMAPS backend *is* selected and unreachable, tools fail with a clear, actionable endpoint
  error and lose no authored work (the store is on disk).
- **Invalid SysML on import.** `import_sysml` parses before writing and returns line-level errors;
  the skill layer surfaces them as fixes, not raw parser output.
- **Invalid SysML on export.** The grammar gate (`pnpm validate:sysml`) is the stop sign — a
  non-zero result blocks the Cameo handoff until the serializer is corrected from the vendored
  grammar.

## Testing strategy

| Layer | Approach |
|---|---|
| MCP tools / store | Unit tests with the in-memory/file backend (no live server needed) |
| Serializer / parser | Unit tests with known `.sysml` fixtures + reserved-keyword/escape regression tests |
| **Grammar gate** | `tools/sysml-validator` over every committed `.sysml`; a CI test refuses to silently skip when the venv is absent |
| **Synthetic stress** | `pnpm stress:sysml` ([`scripts/synthetic-stress.ts`](../scripts/synthetic-stress.ts)) builds dozens of diverse + adversarial models, serializes each, and asserts **every one passes the grammar gate** (the demo claim, mechanized) |
| Human-gate discipline | `scripts/gd-*-proof.ts` prove no-auto-approve + reject-persistence invariants |
| End-to-end | `scripts/e2e-proof.ts` runs the full ANGARS corpus → IR → store → export → validate pipeline (needs the local-only corpus) |
| Live backend | `integration.test.ts` runs against a SMAPS endpoint when one is configured (skipped otherwise) |

## Demo story — ANGARS

[`examples/angars/`](../examples/angars/) is the end-to-end proof: a real Aerial-refueling Command &
Control subsystem rebuilt through the tool — source corpus → extracted IR → generated `.sysml` →
grammar-valid → materialized in Cameo with 100% traceability fidelity, with rendered BDD/IBD/
activity/state/traceability figures and a [CAMEO-HANDOFF.md](../examples/angars/CAMEO-HANDOFF.md)
import guide. The headline a VP sees: *describe a system, get a model that imports — and we can prove
the output is valid before we ever open Cameo.*
