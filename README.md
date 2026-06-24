# sysml-bridge

Claude Code skill suite + MCP server for natural-language → SysML v2 MBSE workflows, validated locally and imported into Cameo Enterprise Architecture.

## What is this?

A bridge between Claude Code and the SysML v2 ecosystem. Describe a system in natural
language — or ingest an existing corpus of requirements, specs, and conversation — and the
tool builds a SysML v2 model through the full MBSE lifecycle: requirements, architecture,
behavior, traceability, verification.

The model is held in a **file-native store**, serialized to OMG **SysML v2 textual notation**,
checked by an **in-repo grammar validator**, and materialized in **Cameo** ("Magic Systems of
Systems Architect — SysML v2 Community 2026x"). On top of the authoring core sit two
corpus-driven layers, both **human-gated**:

- **prose-ingest** — parses unstructured documents (PDFs, specs) into candidate requirements
  with citations, for a human to approve into the model.
- **inference (F8)** — proposes high-confidence links (allocations, mode memberships, flow
  typing, control joins) from the model graph via an LLM propose/debate pipeline, again for
  human approval.

> The discipline that makes this reliable: the serializer conforms to the **vendored SysML v2
> grammar** (never to memory), and **every generated `.sysml` is validated locally before any
> claim that it imports**. See [CLAUDE.md](CLAUDE.md) and [docs/sysml-v2-reference/](docs/sysml-v2-reference/).

## How it works

```
   Natural language  +  corpus (PDFs, spreadsheets, conversation)
                          │
        ┌─────────────────┴───────────────────┐
        │   prose-ingest          inference    │   candidate generation
        │   (NL → candidates)     (F8 propose/  │
        │                          debate)      │
        └─────────────────┬───────────────────┘
                          │   human-approval gate
                          ▼
            IR  —  extracted.json + approved prose/inferred layers (composeIR)
                          │
        ┌─────────────────┴───────────────────┐
        │   15 MBSE skills        MCP server   │
        │   (Claude Code)         (file-native │
        │                          model store)│
        └─────────────────┬───────────────────┘
                          │   sysml-serializer
                          ▼
              .sysml  —  SysML v2 textual notation
                          │
              local ANTLR validator  —  grammar gate (must be 0 errors)
                          │
                          ▼
       Cameo  —  paste into Textual Editor → Synchronize (Alt+S) → materialized model
                          │
              decisym-viewer  →  static PDF / PNG figures
```

The MCP server is the only component that knows where the model lives. By default it uses a
**local file-native store** (a diffable JSON document on disk — no server required). Set
`SYSML_BRIDGE_BACKEND=smaps` to point the same tools at a live SysML v2 REST API endpoint
(SMAPS / Cameo) instead — portability is a config change, not a rewrite.

## Skills

Fifteen Claude Code skills covering the MBSE lifecycle. They operate **only** through the MCP
store tools and the canonical `extracted.json` IR contract.

**Authoring**

| Skill | Purpose |
|---|---|
| `/mbse-init` | Bootstrap a project — stakeholder needs, system context, CONOPS |
| `/mbse-requirements` | Create requirements + needs from the IR, with provenance backpointers and derive edges |
| `/mbse-build` | Build BDD / IBD structure and F1–F9 activity functions (with N2 item flows) from the corpus |
| `/mbse-decompose` | Build F→subfunction decomposition trees from `behaviorDecomp[]` data |
| `/mbse-trace` | Build satisfy / allocate / verify traceability edges from the IR |

**Analysis & V&V**

| Skill | Purpose |
|---|---|
| `/mbse-validate` | Binary traceability gate — every requirement satisfied AND verified; zero orphans/dangles |
| `/mbse-verify` | V&V planning — map requirements to Test / Analysis / Inspection / Demonstration |
| `/mbse-trade` | Weighted trade studies — Pugh matrices, MOE/MOP scoring, rationale |
| `/mbse-kpp` | Key Performance Parameters, Measures of Effectiveness / Performance |

**Views & query**

| Skill | Purpose |
|---|---|
| `/mbse-views` | Stakeholder-specific model views (operator / maintainer / PM) |
| `/mbse-diagram` | Render model structure as Mermaid diagrams |
| `/mbse-query` | Natural-language questions answered against actual model elements |

**Corpus-driven (human-gated)**

| Skill | Purpose |
|---|---|
| `/mbse-ingest` | Review prose-ingestion candidates and approve/reject each — no auto-approve path |
| `/mbse-infer` | Review F8 inferred-link candidates and approve/reject each — no auto-approve path |
| `/mbse-edit` | NL edit loop — add / rename / retarget / remove via store tools only, diff-approved |

## MCP server tools

The `sysml-bridge` MCP server exposes 11 backend-agnostic tools over a `ModelStore` interface:

| Tool | Purpose |
|---|---|
| `init_project` | Initialize or load a project (must be called first) |
| `create_element` | Create any SysML v2 element (requirement, part, action, state, …) |
| `query_elements` | Find elements by type and/or name pattern |
| `update_element` | Rename / retype / retarget an element (GATE-05 coupling check) |
| `delete_element` | Remove an element (refuses to strand relationship endpoints) |
| `create_relationship` | Link elements — satisfy, allocate, derive, verify, dependency, … |
| `query_relationships` | Get an element's relationships (in / out / both) |
| `validate_model` | Completeness / consistency checks + binary traceability gate |
| `export_sysml` | Serialize the model (or a scope) to SysML v2 textual notation |
| `import_sysml` | Parse `.sysml` text and load elements into the store |
| `get_project_state` | Element counts by type + project / branch / commit summary |

## Quick Start

### Prerequisites

- Node.js >= 20 and [pnpm](https://pnpm.io/)
- Python 3 (the local SysML validator uses a committed ANTLR parser; `run.sh` manages a `.venv`)
- Claude Code with skills + MCP support
- *(optional)* Rust toolchain — only to build the `decisym-viewer` figure renderer
- *(optional)* Cameo "Magic Systems of Systems Architect — SysML v2 Community 2026x" to materialize models

### 1. Clone and install

```bash
git clone https://github.com/joescohen/sysml-bridge.git
cd sysml-bridge
pnpm install
```

### 2. Build the MCP server

```bash
pnpm build
```

### 3. (Optional) Anthropic API key

The **inference** and **prose-ingest** layers call the Anthropic API. Set a key only if you
use those layers:

```bash
cp .env.example .env
# set ANTHROPIC_API_KEY=sk-ant-...
```

The core authoring path (skills + MCP store + serializer + validator) needs no API key.

### 4. Configure Claude Code

Add the server to your `.mcp.json`. The default file-native backend needs no endpoint:

```json
{
  "mcpServers": {
    "sysml-bridge": {
      "command": "node",
      "args": ["packages/mcp-server/dist/index.js"]
    }
  }
}
```

To target a live SysML v2 REST API instead, add
`"env": { "SYSML_BRIDGE_BACKEND": "smaps", "SMAPS_ENDPOINT": "http://localhost:9000" }`.

### 5. Use the skills

```
/mbse-init
> Describe your system: "Autonomous aerial refueling control subsystem..."

/mbse-requirements      # requirements + needs with provenance
/mbse-build bdd         # part definitions for subsystems
/mbse-trace             # satisfy / allocate / verify edges
/mbse-validate          # binary gate: every requirement satisfied AND verified
```

## The validation gate

Models are not claimed to import until the local grammar validator passes. The authoritative
workflow (see [CLAUDE.md](CLAUDE.md)):

1. Edit the serializer — `packages/mcp-server/src/utils/sysml-serializer.ts`
2. Regenerate — `pnpm tsx scripts/generate-cc-model.ts`
3. Validate — **must report 0 errors:**
   ```bash
   pnpm validate:sysml examples/angars/model/cc-subsystem.sysml
   ```
4. **Only then** import to Cameo.

A non-zero result at step 3 stops the gate — fix the syntax from the vendored grammar in
[docs/sysml-v2-reference/](docs/sysml-v2-reference/), never by guessing, and rerun from step 2.

Run the unit tests when you touch serializer or generator code:
`pnpm --filter mcp-server test`.

Want one command that proves the gate holds at breadth? `pnpm stress:sysml` builds dozens of
diverse and deliberately hostile models (reserved-keyword names, embedded quotes, deep nesting,
every SysML aspect), serializes each, and asserts **every one passes the grammar validator**. It
exits non-zero if any model would fail to import — a CI-ready guard on the core claim.

## Importing into Cameo

The local validator checks **grammar**; Cameo is the binding **semantic** gate. To materialize
a validated `.sysml` file:

1. New Project → **SysML v2 Project**.
2. Right-click the `[Model]` node → **Open** the Textual Editor.
3. Paste the `.sysml` text → press **Alt+S (Synchronize)** → OK. *(`Cmd+S` only saves the
   project; it does not create model elements.)*
4. To diagram: right-click an element → **Create View** → pick a symbolic view (General ≈ BDD,
   Interconnection ≈ IBD, Action Flow ≈ activity, State Transition ≈ state machine) → **Display**
   parts/actions → **Display Connectors**.

Cameo CE caps a project at ~500 "major elements," so the serializer emits a **lean, usages-only**
projection rather than duplicating definitions and usages.

## Tools

- **`tools/sysml-validator/`** — in-repo grammar validator. A committed Python ANTLR parser
  (from the vendored OMG `.g4`) checks `.sysml` syntax with no Java needed at runtime. Run via
  `pnpm validate:sysml <file>` or `tools/sysml-validator/run.sh <file>`.
- **`tools/decisym-viewer/`** — a cross-platform (Rust + egui) SysML v2 viewer and figure
  exporter. `pnpm render:views <input.sysml> <out-dir> [--spec views.json] [--png]` produces
  static PDF/PNG diagrams. See [tools/decisym-viewer/README.md](tools/decisym-viewer/README.md).

## Example: ANGARS

[`examples/angars/`](examples/angars/) is the end-to-end demo — a real Aerial-refueling
Command & Control subsystem rebuilt through the tool. It contains the source corpus
(spreadsheets + specs), the extracted IR (`extracted.json`), generated `.sysml`, prose/inferred
candidate and approval records, traceability audits, rendered diagrams (BDD, IBD, activity,
state, traceability), and a [CAMEO-HANDOFF.md](examples/angars/CAMEO-HANDOFF.md) import guide.
The C&C subsystem (requirements + parts + actions + verification + trace) materializes fully in
Cameo with 100% traceability fidelity.

## Project structure

```
sysml-bridge/
├── packages/
│   ├── mcp-server/     # MCP server — file-native store, serializer, parser, 11 tools
│   ├── ir/             # canonical extracted.json contract (zod) + three-layer composeIR
│   ├── prose-ingest/   # PDF → chunk → LLM requirement detection → candidates (human-gated)
│   ├── inference/      # F8 engine — candidate gen → type gate → propose → debate (human-gated)
│   └── skills/         # 15 Claude Code MBSE skills
├── tools/
│   ├── sysml-validator/  # local ANTLR grammar validator (committed Python parser)
│   └── decisym-viewer/   # Rust/egui viewer + static figure exporter
├── scripts/            # generators, extractors, e2e proofs, review/disposition helpers
├── examples/angars/    # end-to-end ANGARS C&C demo (corpus → model → Cameo)
└── docs/               # design notes + vendored SysML v2 grammar reference
```

## Security

- The **MCP server has full read/write access to your local model files** — treat it like any
  tool that can edit your repo.
- The **inference** and **prose-ingest** layers send model and corpus content to the **Anthropic
  API**. Do not ingest material you are not permitted to send to a third-party LLM.
- Cameo is driven locally; nothing here exposes a network service.

## License

[MIT](LICENSE)
