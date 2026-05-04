# React Flow IBD Renderer — Design Spec

## Goal

Migrate the `dashboard/` frontend from a monolithic vanilla HTML file to a React + TypeScript + Vite app, replacing Mermaid IBD diagrams with a React Flow canvas that renders SysML v2 Internal Block Diagrams using SysON's visual idiom: compartmented blocks, hollow-circle proxy port handles on block boundaries, and bezier connection edges.

## Architecture

The Express backend (`server.js`) is **unchanged**. The React app is a drop-in frontend replacement that calls the same API routes.

**Dev:** Vite dev server on port 5173 proxies `/api/*` → Express on port 6121.  
**Production:** `vite build` → `dist/`. Express serves `dist/` via `express.static`. One Railway service, no split deployment.

```
dashboard/
├── src/
│   ├── main.tsx
│   ├── App.tsx                        # three-column layout shell
│   ├── components/
│   │   ├── Sidebar.tsx                # project list, new project button
│   │   ├── ProjectDetail.tsx          # stats cards, type chips, elements table
│   │   ├── DiagramPanel.tsx           # tab bar — IBD tab + Mermaid tabs
│   │   ├── IBDViewer.tsx              # React Flow canvas with ELK layout
│   │   ├── ChatPanel.tsx              # streaming chat, conversation switcher
│   │   └── sysml/
│   │       ├── SysMLBlockNode.tsx     # custom RF node: compartmented block
│   │       └── SysMLEdge.tsx          # custom RF edge: labeled bezier
│   ├── lib/
│   │   ├── api.ts                     # fetch wrappers for all Express routes
│   │   ├── ibd-transformer.ts         # SMAPS + local elements → RF nodes/edges
│   │   └── ibd-layout.ts              # ELK layout pass
│   └── types/
│       └── sysml.ts                   # TypeScript types for model elements
├── index.html                         # Vite entry point
├── vite.config.ts                     # proxy /api → :6121 in dev
├── tsconfig.json
└── package.json                       # new deps; server.js deps unchanged
```

## Components

### App
Three-column CSS grid: Sidebar (248px) | ProjectDetail (flex 1) | ChatPanel (340px). Manages `currentProject` state and passes it down. Identical layout to today.

### Sidebar
Project list fetched from `/api/projects`. New project modal. Unchanged behavior from current implementation.

### ProjectDetail
Stats cards (element counts), element type chips, elements table (SMAPS + local elements with LOCAL badge). Contains `DiagramPanel`.

### DiagramPanel
Tab bar with:
- **IBD tab** — shown only when `localElements` contains at least one `ProxyPortUsage`. Renders `IBDViewer`.
- **BDD tab** — auto-generated Mermaid `classDiagram` from PartDefinitions. Always shown.
- **Stored diagram tabs** — one per entry in `/api/projects/:id/diagrams` (activity, state, sequence, any Mermaid stored by Claude). Rendered via `mermaid.render()`.

### IBDViewer
React Flow canvas with:
- Calls `ibd-transformer` to produce `nodes` + `edges` from props
- Calls `ibd-layout` to run ELK and set node positions
- Renders with `<ReactFlow>`, `fitView`, pan/zoom controls enabled
- Falls back to React Flow default layout if ELK fails

### ChatPanel
Streaming SSE chat via `/api/chat`. Conversation list in localStorage. Unchanged behavior from current implementation.

## IBD Data Transform

**Input:**
- `smapsElements: Element[]` — PartDefinitions from `/api/projects/:id/elements`
- `localElements: Element[]` — from `/api/projects/:id/local-elements`

**Output:** `{ nodes: Node[], edges: Edge[] }` for React Flow

**Mapping:**
| Source | React Flow output |
|---|---|
| `PartDefinition` (SMAPS) | `SysMLBlockNode` — one node per block |
| `ProxyPortUsage` (local) | `Handle` on the owning `SysMLBlockNode` |
| `ConnectionUsage` (local) | `SysMLEdge` between source and target handles |

Handle placement: if a `ProxyPortUsage` appears as `connectorEnd[0]` in any `ConnectionUsage` it is placed on the RIGHT edge (source); if `connectorEnd[1]` it is placed on the LEFT edge (target). If both or neither, defaults to RIGHT. Multiple handles on the same side stack vertically.

## ELK Layout

Algorithm: `layered`, direction: `LEFT_TO_RIGHT`, port constraints: `FIXED_SIDE`.  
Library: `elkjs` + `@xyflow/elk`.  
This replicates the left-to-right hierarchical layout of SysON's Interconnection View with sources on the right and sinks on the left of each block.

On failure: catch ELK errors, fall back to evenly spaced default positions (no layout library) so something always renders.

## SysML Node Visual Design

### SysMLBlockNode
```
┌──────────────────────────────┐
│ «part def»                   │   ← stereotype label, small muted text
│ FlightController             │   ← block name, bold
├──────────────────────────────┤   ← divider
○ pwr_in                       │   ← handles: hollow circles on LEFT boundary
○ nav_in          ctrl_out ○   │   ← RIGHT boundary handles for source ports
○ sensor_in                    │
└──────────────────────────────┘
```

- Colors use existing CSS tokens: `--surface` background, `--border` outline, `--primary-text` for name, `--text3` for stereotype
- Handle style: 12px hollow circle (`background: transparent`, `border: 2px solid --primary`)
- Port name labels: 10px monospace, positioned inside block next to handle
- Selected state: `--primary` border glow

### SysMLEdge
- React Flow `BezierEdge` base
- Edge label: `ConnectionUsage.declaredName` in small monospace with `--surface2` background chip
- Arrow marker at target end

## Error Handling

| Scenario | Behavior |
|---|---|
| SMAPS offline | Status dot red; project list shows "API offline"; IBD canvas shows "SMAPS unavailable" message |
| No local elements | IBD tab hidden; diagram panel shows BDD + stored tabs only |
| ELK layout failure | Fall back to React Flow default auto-layout; log warning |
| Mermaid render error | Show inline error message in diagram box (same as today) |

## Dependencies

| Package | Purpose |
|---|---|
| `react` + `react-dom` | UI framework |
| `@xyflow/react` | React Flow v12 canvas engine |
| `elkjs` | ELK layout engine |
| `@xyflow/elk` | ELK adapter for React Flow |
| `mermaid` | Mermaid rendering (was CDN, now npm) |
| `vite` + `@vitejs/plugin-react` | Build tooling |
| `typescript` | Type checking |

`server.js` dependencies (`express`, `@anthropic-ai/sdk`) are unchanged.

## What Does NOT Change

- `server.js` — entirely unchanged
- All Express API routes and tool definitions
- SMAPS Docker stack
- `packages/mcp-server/` — the Claude Code MCP server for skills
- Conversation persistence (localStorage)
- Dark color theme and CSS token values
- Chat tool suite (query_elements, create_local_element, render_diagram, etc.)
