# sysml-bridge

Claude Code skill suite + MCP server for bidirectional natural-language to SysML v2 MBSE workflows, portable from open-source dev to Cameo Enterprise Architecture.

## What is this?

A bridge between Claude Code and the SysML v2 ecosystem. Describe your system in natural language, generate valid SysML v2 models, and work through the full MBSE lifecycle — requirements, architecture, traceability, verification — all from the CLI.

## Architecture

```
                     Claude Code CLI
                          │
              ┌───────────┴───────────┐
              │                       │
     11 MBSE skills            MCP server
     (natural language         (query, create,
      workflows)                validate models)
              │                       │
              └───────────┬───────────┘
                          │ SysML v2 REST API
              ┌───────────▼───────────┐
              │     SysON / SMAPS     │
              │  (model persistence)  │
              └───────────────────────┘
```

The MCP server talks to the SysML v2 REST API — an OMG standard. Point it at SysON for open-source development, or change the endpoint to Cameo Enterprise Architecture for production modeling.

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
- Docker (for the SysML v2 backend services)
- Claude Code with skills support

### 1. Clone and install

```bash
git clone https://github.com/joescohen/sysml-bridge.git
cd sysml-bridge
pnpm install
```

### 2. Start the SysML v2 backend

```bash
cd docker
docker compose up -d
```

This starts SysON (port 8080), the SMAPS API (port 9000), and PostgreSQL. See [docker/INFRASTRUCTURE.md](docker/INFRASTRUCTURE.md) for details.

### 3. Configure environment

```bash
cp .env.example .env
# Edit .env and set your ANTHROPIC_API_KEY (for the dashboard chat feature)
```

### 4. Build the MCP server

```bash
pnpm build
```

### 5. Configure Claude Code

Add to your `.mcp.json`:

```json
{
  "mcpServers": {
    "sysml-bridge": {
      "command": "node",
      "args": ["packages/mcp-server/dist/index.js"],
      "env": {
        "SMAPS_ENDPOINT": "http://localhost:9000"
      }
    }
  }
}
```

### 6. Use the skills

```
/mbse-init
> Describe your system: "Autonomous aerial refueling system for fixed-wing aircraft..."

/mbse-requirements
> Generates structured requirements from stakeholder needs

/mbse-build bdd
> Creates part definitions for subsystems

/mbse-validate
> Reports: unsatisfied requirements, orphaned blocks, missing traceability
```

## Dashboard

A web-based dashboard for visualizing and interacting with SysML v2 models.

```bash
cd dashboard
npm install
npm start
# Open http://localhost:6121
```

Features:
- Containment tree browser for SysML model elements
- Interactive IBD (Internal Block Diagram) viewer built with React Flow
- BDD, requirements, and activity diagram generation
- Chat interface for natural-language model queries (requires Anthropic API key)

## Project Structure

```
sysml-bridge/
├── packages/
│   ├── mcp-server/     # MCP server wrapping the SysML v2 REST API
│   └── skills/         # 11 Claude Code skills for MBSE workflows
├── dashboard/          # React + Express web dashboard
├── docker/             # Docker Compose for SysON + SMAPS + PostgreSQL
├── examples/           # Example SysML models
└── docs/               # Design documentation
```

## Security

The dashboard server binds to `127.0.0.1` (localhost only) by default. **Do not expose it to untrusted networks** — it has no authentication and provides full read/write access to your SysML models. If you need remote access, use an SSH tunnel or authenticated reverse proxy.

## License

[MIT](LICENSE)
