# sysml-bridge

Claude Code skill suite + MCP server for bidirectional natural-language to SysML v2 MBSE workflows, portable from open-source dev to Cameo Enterprise Architecture.

## What is this?

A bridge between Claude Code and the SysML v2 ecosystem. Describe your system in natural language, generate valid SysML v2 models, and work through the full MBSE lifecycle — requirements, architecture, traceability, verification — all from the CLI.

## Architecture

```
┌───────────────────────────────────────┐
│       NATURAL LANGUAGE LAYER          │
│  Claude Code + 11 MBSE skills         │
└──────────────────┬────────────────────┘
                   │ MCP tools
┌──────────────────▼────────────────────┐
│          BRIDGE LAYER                 │
│  MCP server → SMAPS REST API          │
└──────────────────┬────────────────────┘
                   │ SysML v2
┌──────────────────▼────────────────────┐
│          MODEL LAYER                  │
│  .sysml files + SMAPS model store     │
│  Eclipse / Cameo for visualization    │
└───────────────────────────────────────┘
```

## Skills

| Skill | Purpose |
|---|---|
| `/mbse-init` | Bootstrap project — stakeholder needs, CONOPS |
| `/mbse-requirements` | Generate/refine requirements with IDs and hierarchy |
| `/mbse-build` | Build BDD, IBD, activity, sequence, state, parametric artifacts |
| `/mbse-trace` | Traceability links — requirements to blocks to verification |
| `/mbse-validate` | Model completeness and consistency checks |
| `/mbse-verify` | V&V planning — Test/Analysis/Inspection/Demonstration |
| `/mbse-trade` | Weighted trade studies with Pugh matrices |
| `/mbse-kpp` | Key Performance Parameters, MOEs, MOPs |
| `/mbse-views` | Stakeholder-specific model views |
| `/mbse-diagram` | Render Mermaid/PlantUML diagrams from model |
| `/mbse-query` | Natural language questions about the model |

## Quick Start

### Prerequisites

- Node.js >= 20
- pnpm
- Docker (for the SysML v2 API server)
- Claude Code with skills support

### Setup

```bash
# Clone and install
git clone https://github.com/joescohen/sysml-bridge.git
cd sysml-bridge
pnpm install

# Start the SysML v2 API server
cd docker && docker compose up -d

# Build the MCP server
pnpm build
```

### Configure Claude Code

Add to your `.mcp.json`:

```json
{
  "mcpServers": {
    "sysml-bridge": {
      "command": "node",
      "args": ["packages/mcp-server/dist/index.js"],
      "env": {
        "SMAPS_ENDPOINT": "http://localhost:9000",
        "PROJECT_ID": "my-project"
      }
    }
  }
}
```

### Use

```
/mbse-init
> Describe your system: "Autonomous aerial refueling system for fixed-wing aircraft..."

/mbse-requirements
> Generates 189 structured requirements from stakeholder needs

/mbse-build bdd
> Creates part definitions for 6 subsystems

/mbse-validate
> Reports: 12 requirements unsatisfied, 3 orphaned blocks
```

## Portability

The MCP server talks to the SMAPS REST API — an OMG standard. Change the endpoint to point at Cameo Enterprise Architecture and the same skills work against your production modeling environment.

## Project Structure

```
sysml-bridge/
├── packages/
│   ├── mcp-server/     # MCP server wrapping SMAPS API
│   └── skills/         # Claude Code skills for MBSE
├── docker/             # SysML v2 Pilot (SMAPS server)
├── examples/angars/    # Demo: rebuild ANGARS capstone
└── docs/               # Design spec
```

## License

MIT
