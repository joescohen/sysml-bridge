# React Flow IBD Renderer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate `dashboard/` from a monolithic vanilla HTML file to a React + TypeScript + Vite app with a React Flow IBD canvas that renders SysML v2 blocks, proxy ports, and connections in SysON's visual idiom.

**Architecture:** Vite dev server proxies `/api` to the existing Express backend (unchanged). In production, `vite build` produces `dist/` which Express serves as static files. React Flow renders IBDs auto-generated from local elements; Mermaid handles all other diagram types.

**Tech Stack:** React 18, TypeScript 5, Vite 5, `@xyflow/react` v12, `elkjs` + `@xyflow/elk`, Mermaid 11, Vitest 2 (for pure function tests).

---

## File Structure

### Create (new)
| File | Responsibility |
|---|---|
| `dashboard/src/main.tsx` | React entry point, mounts App |
| `dashboard/src/index.css` | Global CSS tokens + React Flow overrides |
| `dashboard/src/App.tsx` | Three-column layout, project state |
| `dashboard/src/types/sysml.ts` | TypeScript types for all model elements |
| `dashboard/src/lib/api.ts` | fetch wrappers for all Express routes |
| `dashboard/src/lib/ibd-transformer.ts` | SMAPS + local elements → RF nodes/edges |
| `dashboard/src/lib/ibd-layout.ts` | ELK layout pass |
| `dashboard/src/lib/__tests__/ibd-transformer.test.ts` | Vitest tests for transformer |
| `dashboard/src/components/sysml/SysMLBlockNode.tsx` | Custom RF node: compartmented block |
| `dashboard/src/components/sysml/SysMLEdge.tsx` | Custom RF edge: labeled bezier |
| `dashboard/src/components/IBDViewer.tsx` | React Flow canvas + ELK |
| `dashboard/src/components/ChatPanel.tsx` | Streaming chat, SSE, conversations |
| `dashboard/src/components/Sidebar.tsx` | Project list + new project modal |
| `dashboard/src/components/DiagramPanel.tsx` | Tab bar: IBD + BDD + stored Mermaid |
| `dashboard/src/components/ProjectDetail.tsx` | Stats, type chips, elements table |
| `dashboard/index.html` | Vite entry HTML (replaces old inline HTML) |
| `dashboard/vite.config.ts` | Vite + Vitest config, `/api` proxy |
| `dashboard/tsconfig.json` | TypeScript config |

### Modify (existing)
| File | Change |
|---|---|
| `dashboard/package.json` | Add React, Vite, RF, ELK deps; add `dev`/`build`/`test` scripts |
| `dashboard/server.js` | Change `static(__dirname)` → `static(join(__dirname,'dist'))`, add SPA fallback |

---

## Task 1: Vite + React + TypeScript Scaffolding

**Files:**
- Modify: `dashboard/package.json`
- Create: `dashboard/vite.config.ts`
- Create: `dashboard/tsconfig.json`
- Create: `dashboard/index.html`
- Create: `dashboard/src/main.tsx`
- Create: `dashboard/src/index.css`

- [ ] **Step 1: Update package.json with new dependencies and scripts**

Replace `dashboard/package.json` entirely:

```json
{
  "name": "sysml-bridge-dashboard",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "start": "node server.js",
    "test": "vitest run"
  },
  "dependencies": {
    "@anthropic-ai/sdk": "^0.40.0",
    "@xyflow/elk": "^0.0.8",
    "@xyflow/react": "^12.3.0",
    "elkjs": "^0.9.3",
    "express": "^4.21.2",
    "mermaid": "^11.4.0",
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@types/react": "^18.3.1",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0",
    "vitest": "^2.0.0"
  }
}
```

- [ ] **Step 2: Install dependencies**

```bash
cd dashboard && npm install
```

Expected: `node_modules/` populated, no peer dep errors.

- [ ] **Step 3: Create vite.config.ts**

```typescript
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:6121',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
  },
  test: {
    environment: 'node',
    include: ['src/**/__tests__/**/*.test.ts'],
  },
});
```

- [ ] **Step 4: Create tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true
  },
  "include": ["src"]
}
```

- [ ] **Step 5: Create index.html (Vite entry point)**

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>sysml-bridge</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 6: Create src/index.css**

```css
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #0d0d12;
  --surface: #13131a;
  --surface2: #1a1a24;
  --border: #1e2030;
  --border2: #252538;
  --text: #e2e8f0;
  --text2: #94a3b8;
  --text3: #475569;
  --text4: #334155;
  --primary: #6366f1;
  --primary-dim: #1e2248;
  --primary-text: #818cf8;
  --green: #22c55e;
  --red: #ef4444;
  --amber: #f59e0b;
}

html, body, #root {
  height: 100%;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  background: var(--bg);
  color: var(--text);
  font-size: 13px;
}

/* React Flow dark overrides */
.react-flow__background { background: var(--surface) !important; }
.react-flow__controls button {
  background: var(--surface2) !important;
  border-color: var(--border2) !important;
  color: var(--text3) !important;
}
.react-flow__controls button:hover { color: var(--text) !important; }

/* SysML node styles */
.sysml-block {
  background: var(--surface);
  border: 1px solid var(--border2);
  border-radius: 6px;
  min-width: 160px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
.sysml-block.selected {
  border-color: var(--primary);
  box-shadow: 0 0 0 1px var(--primary);
}
.sysml-block-header {
  padding: 6px 12px 8px;
  border-bottom: 1px solid var(--border);
}
.sysml-stereotype {
  font-size: 10px;
  color: var(--text3);
  letter-spacing: 0.03em;
}
.sysml-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
.sysml-block-body {
  position: relative;
}
.sysml-port-label {
  position: absolute;
  font-size: 10px;
  font-family: monospace;
  color: var(--text3);
  pointer-events: none;
  white-space: nowrap;
}
.sysml-port-label.left  { left: 14px; }
.sysml-port-label.right { right: 14px; text-align: right; }

/* React Flow handle override — hollow circle */
.react-flow__handle {
  width: 10px !important;
  height: 10px !important;
  background: transparent !important;
  border: 2px solid var(--primary) !important;
  border-radius: 50% !important;
}
```

- [ ] **Step 7: Create src/main.tsx**

```tsx
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import { App } from './App';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
```

- [ ] **Step 8: Create a minimal src/App.tsx to verify the dev server starts**

```tsx
export function App() {
  return <div style={{ padding: 24, color: 'var(--text)' }}>sysml-bridge loading…</div>;
}
```

- [ ] **Step 9: Verify dev server starts**

```bash
cd dashboard && npm run dev
```

Expected output:
```
  VITE v5.x.x  ready in xxx ms
  ➜  Local:   http://localhost:5173/
```

Open `http://localhost:5173` — should show "sysml-bridge loading…" on dark background. Stop with Ctrl+C.

- [ ] **Step 10: Commit**

```bash
git add dashboard/package.json dashboard/package-lock.json dashboard/vite.config.ts dashboard/tsconfig.json dashboard/index.html dashboard/src/main.tsx dashboard/src/index.css dashboard/src/App.tsx
git commit -m "feat: scaffold React + Vite + TypeScript for dashboard frontend"
```

---

## Task 2: TypeScript Types

**Files:**
- Create: `dashboard/src/types/sysml.ts`

- [ ] **Step 1: Create src/types/sysml.ts**

```typescript
export interface SmapsElement {
  '@id': string;
  '@type': string;
  declaredName?: string;
  declaredShortName?: string;
  name?: string;
}

export interface ConnectorEnd {
  '@type': 'ConnectorEnd';
  connectedFeature: { '@id': string };
}

export interface LocalElement extends SmapsElement {
  _local: true;
  owner?: { '@id': string };
  type?: Array<{ '@id': string }>;
  connectorEnd?: ConnectorEnd[];
}

export interface Project {
  '@id': string;
  name: string;
}

export interface StoredDiagram {
  type: string;
  title: string;
  mermaid: string;
  updatedAt: string;
}
```

- [ ] **Step 2: Verify TypeScript accepts the types**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output (no errors).

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/types/sysml.ts
git commit -m "feat: add SysML TypeScript types"
```

---

## Task 3: API Layer

**Files:**
- Create: `dashboard/src/lib/api.ts`

- [ ] **Step 1: Create src/lib/api.ts**

```typescript
import type { SmapsElement, LocalElement, Project, StoredDiagram } from '../types/sysml';

async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, opts);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}: ${path}`);
  return res.json() as Promise<T>;
}

export function getProjects(): Promise<Project[]> {
  return apiFetch('/api/projects');
}

export function createProject(name: string): Promise<Project> {
  return apiFetch('/api/projects', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ '@type': 'Project', name }),
  });
}

export function deleteProject(id: string): Promise<unknown> {
  return apiFetch(`/api/projects/${id}`, { method: 'DELETE' });
}

export function getElements(projectId: string): Promise<SmapsElement[]> {
  return apiFetch(`/api/projects/${projectId}/elements`);
}

export function getLocalElements(projectId: string): Promise<LocalElement[]> {
  return apiFetch(`/api/projects/${projectId}/local-elements`);
}

export function deleteLocalElement(projectId: string, elementId: string): Promise<unknown> {
  return apiFetch(`/api/projects/${projectId}/local-elements/${elementId}`, { method: 'DELETE' });
}

export function getDiagrams(projectId: string): Promise<StoredDiagram[]> {
  return apiFetch(`/api/projects/${projectId}/diagrams`);
}

export function deleteDiagram(projectId: string, idx: number): Promise<unknown> {
  return apiFetch(`/api/projects/${projectId}/diagrams/${idx}`, { method: 'DELETE' });
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/lib/api.ts
git commit -m "feat: add API fetch layer"
```

---

## Task 4: IBD Transformer + Tests

**Files:**
- Create: `dashboard/src/lib/ibd-transformer.ts`
- Create: `dashboard/src/lib/__tests__/ibd-transformer.test.ts`

- [ ] **Step 1: Create the failing test file**

```typescript
// dashboard/src/lib/__tests__/ibd-transformer.test.ts
import { describe, it, expect } from 'vitest';
import { transformToIBD } from '../ibd-transformer';
import type { SmapsElement, LocalElement } from '../../types/sysml';

const BLOCK_A: SmapsElement = { '@id': 'block-a', '@type': 'PartDefinition', declaredName: 'Battery' };
const BLOCK_B: SmapsElement = { '@id': 'block-b', '@type': 'PartDefinition', declaredName: 'Motor' };

const PORT_OUT: LocalElement = {
  '@id': 'port-out', '@type': 'ProxyPortUsage', declaredName: 'pwr_out',
  _local: true, owner: { '@id': 'block-a' },
};
const PORT_IN: LocalElement = {
  '@id': 'port-in', '@type': 'ProxyPortUsage', declaredName: 'pwr_in',
  _local: true, owner: { '@id': 'block-b' },
};
const CONN: LocalElement = {
  '@id': 'conn-1', '@type': 'ConnectionUsage', declaredName: 'powerLine',
  _local: true,
  connectorEnd: [
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-out' } },
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-in' } },
  ],
};

describe('transformToIBD', () => {
  it('creates one node per PartDefinition', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B], []);
    expect(nodes).toHaveLength(2);
    expect(nodes[0].data.name).toBe('Battery');
    expect(nodes[1].data.name).toBe('Motor');
  });

  it('ignores non-PartDefinition SMAPS elements', () => {
    const req: SmapsElement = { '@id': 'req-1', '@type': 'RequirementDefinition', declaredName: 'R1' };
    const { nodes } = transformToIBD([BLOCK_A, req], []);
    expect(nodes).toHaveLength(1);
  });

  it('assigns ProxyPortUsage to its owning block', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B], [PORT_OUT, PORT_IN]);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor   = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports).toHaveLength(1);
    expect(battery.data.ports[0].name).toBe('pwr_out');
    expect(motor.data.ports).toHaveLength(1);
    expect(motor.data.ports[0].name).toBe('pwr_in');
  });

  it('places source port on right, target port on left', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B], [PORT_OUT, PORT_IN, CONN]);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor   = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports[0].position).toBe('right');
    expect(motor.data.ports[0].position).toBe('left');
  });

  it('creates one edge per ConnectionUsage', () => {
    const { edges } = transformToIBD([BLOCK_A, BLOCK_B], [PORT_OUT, PORT_IN, CONN]);
    expect(edges).toHaveLength(1);
    expect(edges[0].source).toBe('block-a');
    expect(edges[0].target).toBe('block-b');
    expect(edges[0].sourceHandle).toBe('port-out');
    expect(edges[0].targetHandle).toBe('port-in');
    expect(edges[0].label).toBe('powerLine');
  });

  it('drops edges with missing owner blocks', () => {
    const orphan: LocalElement = {
      '@id': 'orphan', '@type': 'ProxyPortUsage', declaredName: 'x',
      _local: true, owner: { '@id': 'nonexistent' },
    };
    const conn: LocalElement = {
      '@id': 'c2', '@type': 'ConnectionUsage', declaredName: 'c', _local: true,
      connectorEnd: [
        { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'orphan' } },
        { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-in' } },
      ],
    };
    const { edges } = transformToIBD([BLOCK_A, BLOCK_B], [orphan, PORT_IN, conn]);
    expect(edges).toHaveLength(0);
  });

  it('returns node type sysmlBlock', () => {
    const { nodes } = transformToIBD([BLOCK_A], []);
    expect(nodes[0].type).toBe('sysmlBlock');
  });
});
```

- [ ] **Step 2: Run tests — expect failures**

```bash
cd dashboard && npm test
```

Expected: `FAIL src/lib/__tests__/ibd-transformer.test.ts` — cannot find module `../ibd-transformer`.

- [ ] **Step 3: Create src/lib/ibd-transformer.ts**

```typescript
import type { Node, Edge } from '@xyflow/react';
import type { SmapsElement, LocalElement } from '../types/sysml';

export interface PortHandle {
  id: string;
  name: string;
  position: 'left' | 'right';
}

export interface SysMLBlockNodeData extends Record<string, unknown> {
  stereotype: string;
  name: string;
  ports: PortHandle[];
}

export function transformToIBD(
  smapsElements: SmapsElement[],
  localElements: LocalElement[],
): { nodes: Node<SysMLBlockNodeData>[]; edges: Edge[] } {
  const partDefs = smapsElements.filter(e => e['@type'] === 'PartDefinition');
  const portUsages = localElements.filter(e => e['@type'] === 'ProxyPortUsage');
  const connections = localElements.filter(e => e['@type'] === 'ConnectionUsage');

  const sourcePorts = new Set(
    connections
      .map(c => c.connectorEnd?.[0]?.connectedFeature?.['@id'])
      .filter((id): id is string => !!id),
  );
  const targetPorts = new Set(
    connections
      .map(c => c.connectorEnd?.[1]?.connectedFeature?.['@id'])
      .filter((id): id is string => !!id),
  );

  const nodes: Node<SysMLBlockNodeData>[] = partDefs.map(block => {
    const blockPorts = portUsages.filter(p => p.owner?.['@id'] === block['@id']);
    const ports: PortHandle[] = blockPorts.map(p => ({
      id: p['@id'],
      name: p.declaredName ?? p['@id'].slice(0, 8),
      position: targetPorts.has(p['@id']) && !sourcePorts.has(p['@id']) ? 'left' : 'right',
    }));
    return {
      id: block['@id'],
      type: 'sysmlBlock',
      position: { x: 0, y: 0 },
      data: {
        stereotype: 'part def',
        name: block.declaredName ?? block['@id'].slice(0, 8),
        ports,
      },
    };
  });

  const blockIds = new Set(partDefs.map(b => b['@id']));

  const edges: Edge[] = connections
    .filter(c => c.connectorEnd?.[0] && c.connectorEnd?.[1])
    .map(c => {
      const srcPortId = c.connectorEnd![0].connectedFeature['@id'];
      const tgtPortId = c.connectorEnd![1].connectedFeature['@id'];
      const srcPort = portUsages.find(p => p['@id'] === srcPortId);
      const tgtPort = portUsages.find(p => p['@id'] === tgtPortId);
      const srcBlock = srcPort?.owner?.['@id'] ?? '';
      const tgtBlock = tgtPort?.owner?.['@id'] ?? '';
      if (!blockIds.has(srcBlock) || !blockIds.has(tgtBlock)) return null;
      return {
        id: c['@id'],
        source: srcBlock,
        sourceHandle: srcPortId,
        target: tgtBlock,
        targetHandle: tgtPortId,
        type: 'sysmlEdge',
        label: c.declaredName ?? '',
        data: { label: c.declaredName ?? '' },
      } as Edge;
    })
    .filter((e): e is Edge => e !== null);

  return { nodes, edges };
}
```

- [ ] **Step 4: Run tests — expect all passing**

```bash
cd dashboard && npm test
```

Expected:
```
 ✓ src/lib/__tests__/ibd-transformer.test.ts (6)
 Test Files  1 passed (1)
 Tests  6 passed (6)
```

- [ ] **Step 5: Commit**

```bash
git add dashboard/src/lib/ibd-transformer.ts dashboard/src/lib/__tests__/ibd-transformer.test.ts
git commit -m "feat: add IBD transformer with tests"
```

---

## Task 5: ELK Layout

**Files:**
- Create: `dashboard/src/lib/ibd-layout.ts`

- [ ] **Step 1: Create src/lib/ibd-layout.ts**

```typescript
import ELK from 'elkjs/lib/elk.bundled.js';
import type { Node, Edge } from '@xyflow/react';
import type { SysMLBlockNodeData } from './ibd-transformer';

const elk = new ELK();

const BLOCK_WIDTH  = 180;
const BLOCK_HEIGHT = 120;

export async function applyELKLayout(
  nodes: Node<SysMLBlockNodeData>[],
  edges: Edge[],
): Promise<Node<SysMLBlockNodeData>[]> {
  if (nodes.length === 0) return nodes;

  const graph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.spacing.nodeNode': '60',
      'elk.layered.spacing.nodeNodeBetweenLayers': '80',
    },
    children: nodes.map(n => ({
      id: n.id,
      width: BLOCK_WIDTH,
      height: Math.max(BLOCK_HEIGHT, (n.data.ports.length + 1) * 24 + 24),
    })),
    edges: edges.map(e => ({
      id: e.id,
      sources: [e.source],
      targets: [e.target],
    })),
  };

  try {
    const layout = await elk.layout(graph);
    return nodes.map(n => {
      const laid = layout.children?.find(c => c.id === n.id);
      return laid
        ? { ...n, position: { x: laid.x ?? 0, y: laid.y ?? 0 } }
        : n;
    });
  } catch (err) {
    console.warn('ELK layout failed, using fallback positions:', err);
    return nodes.map((n, i) => ({
      ...n,
      position: { x: (i % 3) * (BLOCK_WIDTH + 80), y: Math.floor(i / 3) * (BLOCK_HEIGHT + 60) },
    }));
  }
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/lib/ibd-layout.ts
git commit -m "feat: add ELK layout utility"
```

---

## Task 6: SysML Custom Nodes

**Files:**
- Create: `dashboard/src/components/sysml/SysMLBlockNode.tsx`
- Create: `dashboard/src/components/sysml/SysMLEdge.tsx`

- [ ] **Step 1: Create src/components/sysml/SysMLBlockNode.tsx**

```tsx
import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { SysMLBlockNodeData } from '../../lib/ibd-transformer';

const PORT_ROW_HEIGHT = 24;
const BODY_PADDING    = 12;

export function SysMLBlockNode({ data, selected }: NodeProps<{ data: SysMLBlockNodeData }>) {
  const leftPorts  = data.ports.filter(p => p.position === 'left');
  const rightPorts = data.ports.filter(p => p.position === 'right');
  const bodyHeight = Math.max(leftPorts.length, rightPorts.length, 1) * PORT_ROW_HEIGHT + BODY_PADDING * 2;

  return (
    <div className={`sysml-block${selected ? ' selected' : ''}`} style={{ minWidth: 160 }}>
      <div className="sysml-block-header">
        <div className="sysml-stereotype">«{data.stereotype}»</div>
        <div className="sysml-name">{data.name}</div>
      </div>

      <div className="sysml-block-body" style={{ height: bodyHeight }}>
        {leftPorts.map((port, i) => {
          const top = BODY_PADDING + i * PORT_ROW_HEIGHT;
          return (
            <Handle
              key={port.id}
              type="target"
              id={port.id}
              position={Position.Left}
              style={{ top: top + 4 }}
            >
              <span
                className="sysml-port-label left"
                style={{ top: top - 1 }}
              >
                {port.name}
              </span>
            </Handle>
          );
        })}

        {rightPorts.map((port, i) => {
          const top = BODY_PADDING + i * PORT_ROW_HEIGHT;
          return (
            <Handle
              key={port.id}
              type="source"
              id={port.id}
              position={Position.Right}
              style={{ top: top + 4 }}
            >
              <span
                className="sysml-port-label right"
                style={{ top: top - 1, right: 14 }}
              >
                {port.name}
              </span>
            </Handle>
          );
        })}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Create src/components/sysml/SysMLEdge.tsx**

```tsx
import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  type EdgeProps,
} from '@xyflow/react';

export function SysMLEdge({
  id,
  sourceX, sourceY, sourcePosition,
  targetX, targetY, targetPosition,
  label,
}: EdgeProps) {
  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX, sourceY, sourcePosition,
    targetX, targetY, targetPosition,
  });

  return (
    <>
      <BaseEdge
        id={id}
        path={edgePath}
        style={{ stroke: '#475569', strokeWidth: 1.5 }}
        markerEnd="url(#arrow)"
      />
      {label && (
        <EdgeLabelRenderer>
          <div
            style={{
              position: 'absolute',
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
              fontSize: 10,
              fontFamily: 'monospace',
              background: 'var(--surface2)',
              color: 'var(--text3)',
              padding: '1px 5px',
              borderRadius: 4,
              border: '1px solid var(--border)',
              pointerEvents: 'none',
            }}
          >
            {String(label)}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
```

- [ ] **Step 3: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add dashboard/src/components/sysml/SysMLBlockNode.tsx dashboard/src/components/sysml/SysMLEdge.tsx
git commit -m "feat: add SysML custom React Flow nodes"
```

---

## Task 7: IBDViewer

**Files:**
- Create: `dashboard/src/components/IBDViewer.tsx`

- [ ] **Step 1: Create src/components/IBDViewer.tsx**

```tsx
import { useEffect, useCallback } from 'react';
import {
  ReactFlow, Background, Controls,
  useNodesState, useEdgesState,
  type Node,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { SysMLBlockNode } from './sysml/SysMLBlockNode';
import { SysMLEdge } from './sysml/SysMLEdge';
import { transformToIBD, type SysMLBlockNodeData } from '../lib/ibd-transformer';
import { applyELKLayout } from '../lib/ibd-layout';
import { getElements, getLocalElements } from '../lib/api';

const nodeTypes = { sysmlBlock: SysMLBlockNode };
const edgeTypes = { sysmlEdge: SysMLEdge };

interface IBDViewerProps {
  projectId: string;
}

export function IBDViewer({ projectId }: IBDViewerProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<SysMLBlockNodeData>>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);

  const load = useCallback(async () => {
    try {
      const [smapsElements, localElements] = await Promise.all([
        getElements(projectId),
        getLocalElements(projectId),
      ]);
      const { nodes: rawNodes, edges: rawEdges } = transformToIBD(smapsElements, localElements);
      const laidOut = await applyELKLayout(rawNodes, rawEdges);
      setNodes(laidOut);
      setEdges(rawEdges);
    } catch (err) {
      console.error('IBDViewer load error:', err);
    }
  }, [projectId, setNodes, setEdges]);

  useEffect(() => { load(); }, [load]);

  return (
    <div style={{ height: 420 }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        fitView
        colorMode="dark"
        proOptions={{ hideAttribution: true }}
      >
        <Background />
        <Controls />
      </ReactFlow>
    </div>
  );
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/components/IBDViewer.tsx
git commit -m "feat: add IBDViewer React Flow canvas"
```

---

## Task 8: ChatPanel

**Files:**
- Create: `dashboard/src/components/ChatPanel.tsx`

- [ ] **Step 1: Create src/components/ChatPanel.tsx**

```tsx
import { useState, useEffect, useRef, useCallback } from 'react';
import type { Project } from '../types/sysml';

interface Message {
  role: 'user' | 'assistant';
  content: string;
  tools?: ToolCall[];
}

interface ToolCall {
  id: string;
  name: string;
  status: 'pending' | 'running' | 'done';
}

interface Conversation {
  id: string;
  title: string;
  messages: Message[];
}

function genId() { return Math.random().toString(36).slice(2) + Date.now().toString(36); }

function loadConvs(): Record<string, Conversation[]> {
  try { return JSON.parse(localStorage.getItem('sysml_convs') ?? '{}'); } catch { return {}; }
}
function saveConvs(c: Record<string, Conversation[]>) {
  localStorage.setItem('sysml_convs', JSON.stringify(c));
}

const TOOL_LABELS: Record<string, string> = {
  query_elements: 'Queried elements', create_element: 'Created element',
  create_local_element: 'Created local element', query_local_elements: 'Queried local elements',
  delete_local_element: 'Deleted local element', render_diagram: 'Rendered diagram',
  export_sysml: 'Exported SysML', create_project: 'Created project',
};

function escHtml(s: string) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function renderMarkdown(text: string) {
  return escHtml(text)
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/`([^`]+)`/g, '<code style="font-family:monospace;background:var(--surface2);padding:1px 4px;border-radius:3px">$1</code>');
}

interface ChatPanelProps {
  project: Project | null;
  onModelChanged: () => void;
}

export function ChatPanel({ project, onModelChanged }: ChatPanelProps) {
  const [convs, setConvs] = useState<Record<string, Conversation[]>>(loadConvs);
  const [activeId, setActiveId] = useState<Record<string, string>>({});
  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [showConvList, setShowConvList] = useState(false);
  const [streamText, setStreamText] = useState('');
  const [streamTools, setStreamTools] = useState<ToolCall[]>([]);
  const readerRef = useRef<ReadableStreamDefaultReader<Uint8Array> | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const pid = project?.['@id'] ?? null;

  useEffect(() => {
    if (!pid) return;
    setConvs(prev => {
      if (prev[pid]?.length) return prev;
      const conv = { id: genId(), title: 'New conversation', messages: [] };
      const updated = { ...prev, [pid]: [conv] };
      saveConvs(updated);
      return updated;
    });
  }, [pid]);

  useEffect(() => {
    if (!pid) return;
    setActiveId(prev => {
      if (prev[pid]) return prev;
      const first = convs[pid]?.[0]?.id;
      return first ? { ...prev, [pid]: first } : prev;
    });
  }, [pid, convs]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [streamText, streamTools, convs, activeId]);

  const activeConv = pid ? (convs[pid] ?? []).find(c => c.id === activeId[pid]) : null;

  const updateConv = useCallback((pid: string, id: string, fn: (c: Conversation) => Conversation) => {
    setConvs(prev => {
      const updated = {
        ...prev,
        [pid]: (prev[pid] ?? []).map(c => c.id === id ? fn(c) : c),
      };
      saveConvs(updated);
      return updated;
    });
  }, []);

  async function sendMessage(text: string) {
    if (!pid || !text.trim() || isStreaming) return;
    if (!activeConv) return;

    const convId = activeConv.id;
    updateConv(pid, convId, c => ({
      ...c,
      title: c.title === 'New conversation' ? text.slice(0, 40) + (text.length > 40 ? '…' : '') : c.title,
      messages: [...c.messages, { role: 'user', content: text }],
    }));

    setInput('');
    setIsStreaming(true);
    setStreamText('');
    setStreamTools([]);

    const apiMessages = [...activeConv.messages, { role: 'user' as const, content: text }]
      .map(m => ({ role: m.role, content: m.content }));

    try {
      const response = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ messages: apiMessages, projectId: pid }),
      });
      if (!response.ok) {
        const err = await response.json().catch(() => ({ error: response.statusText }));
        throw new Error((err as { error: string }).error ?? response.statusText);
      }

      const reader = response.body!.getReader();
      readerRef.current = reader;
      const decoder = new TextDecoder();
      let buffer = '';
      let accText = '';
      const tools: ToolCall[] = [];
      const toolMap: Record<string, ToolCall> = {};

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop()!;
        for (const line of lines) {
          if (!line.startsWith('data: ')) continue;
          const event = JSON.parse(line.slice(6)) as Record<string, string>;
          if (event.type === 'text') {
            accText += event.text;
            setStreamText(accText);
          } else if (event.type === 'tool_start') {
            const t: ToolCall = { id: event.id, name: event.name, status: 'pending' };
            tools.push(t); toolMap[event.id] = t;
            setStreamTools([...tools]);
          } else if (event.type === 'tool_running') {
            if (toolMap[event.id]) { toolMap[event.id].status = 'running'; setStreamTools([...tools]); }
          } else if (event.type === 'tool_done') {
            if (toolMap[event.id]) { toolMap[event.id].status = 'done'; setStreamTools([...tools]); }
          } else if (event.type === 'done') {
            break;
          } else if (event.type === 'error') {
            throw new Error(event.message);
          }
        }
      }

      updateConv(pid, convId, c => ({
        ...c,
        messages: [...c.messages, { role: 'assistant', content: accText, tools: [...tools] }],
      }));

      const mutating = ['create_element', 'create_project', 'create_local_element', 'delete_local_element'];
      if (tools.some(t => mutating.includes(t.name) && t.status === 'done') ||
          tools.some(t => t.name === 'render_diagram' && t.status === 'done')) {
        onModelChanged();
      }
    } catch (err) {
      if (err instanceof Error && err.name !== 'AbortError') {
        setStreamText(`Error: ${err.message}`);
      }
    } finally {
      readerRef.current = null;
      setIsStreaming(false);
      setStreamText('');
      setStreamTools([]);
    }
  }

  function newConversation() {
    if (!pid) return;
    const conv: Conversation = { id: genId(), title: 'New conversation', messages: [] };
    setConvs(prev => {
      const updated = { ...prev, [pid]: [conv, ...(prev[pid] ?? [])] };
      saveConvs(updated);
      return updated;
    });
    setActiveId(prev => ({ ...prev, [pid]: conv.id }));
    setShowConvList(false);
  }

  function deleteConv(id: string) {
    if (!pid) return;
    setConvs(prev => {
      const filtered = (prev[pid] ?? []).filter(c => c.id !== id);
      const next = filtered.length ? filtered : [{ id: genId(), title: 'New conversation', messages: [] }];
      const updated = { ...prev, [pid]: next };
      saveConvs(updated);
      return updated;
    });
    setActiveId(prev => {
      const newActive = (convs[pid] ?? []).find(c => c.id !== id)?.id ?? genId();
      return { ...prev, [pid]: newActive };
    });
  }

  const messages = activeConv?.messages ?? [];

  // ── render helpers ────────────────────────────────────────────────────────

  const s = {
    panel: { borderLeft: '1px solid var(--border)', display: 'flex', flexDirection: 'column' as const, minHeight: 0, background: 'var(--surface)', height: '100%' },
    topbar: { borderBottom: '1px solid var(--border)', padding: '0 12px', height: 44, display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 },
    avatar: { width: 26, height: 26, borderRadius: 8, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    title: { fontSize: 12, fontWeight: 600, flex: 1 },
    convBtn: { fontSize: 11, color: 'var(--text3)', background: 'none', border: 'none', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4, padding: '3px 6px', borderRadius: 5, maxWidth: 120, overflow: 'hidden' as const },
    newBtn: { width: 24, height: 24, borderRadius: 5, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' },
    convList: { borderBottom: '1px solid var(--border)', background: 'var(--bg)', maxHeight: 160, overflowY: 'auto' as const, flexShrink: 0 },
    msgArea: { flex: 1, overflowY: 'auto' as const, padding: '16px 12px', display: 'flex', flexDirection: 'column' as const, gap: 14 },
    inputArea: { borderTop: '1px solid var(--border)', padding: '10px', flexShrink: 0, display: 'flex', gap: 7, alignItems: 'flex-end', background: 'var(--surface)' },
    textarea: { flex: 1, resize: 'none' as const, overflow: 'hidden' as const, background: 'var(--bg)', border: '1px solid var(--border2)', borderRadius: 9, padding: '8px 11px', color: 'var(--text)', fontSize: 12.5, fontFamily: 'inherit', lineHeight: 1.5, minHeight: 36, maxHeight: 120, outline: 'none' },
    sendBtn: { width: 34, height: 34, borderRadius: 8, border: 'none', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, background: 'var(--primary)', color: '#fff', opacity: isStreaming || !input.trim() ? 0.35 : 1 },
    stopBtn: { width: 34, height: 34, borderRadius: 8, border: '1px solid var(--border2)', background: 'var(--surface2)', color: 'var(--text2)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
  };

  return (
    <div style={s.panel}>
      {/* Topbar */}
      <div style={s.topbar}>
        <div style={s.avatar}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
        </div>
        <span style={s.title}>MBSE Assistant</span>
        <button style={s.convBtn} onClick={() => setShowConvList(v => !v)}>
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{activeConv?.title ?? 'New conversation'}</span>
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="m6 9 6 6 6-6"/></svg>
        </button>
        <button style={s.newBtn} onClick={newConversation} title="New conversation">+</button>
      </div>

      {/* Conversation list */}
      {showConvList && pid && (
        <div style={s.convList}>
          {(convs[pid] ?? []).map(c => (
            <div key={c.id}
              onClick={() => { setActiveId(prev => ({ ...prev, [pid]: c.id })); setShowConvList(false); }}
              style={{ padding: '8px 12px', fontSize: 11.5, color: c.id === activeId[pid] ? 'var(--primary-text)' : 'var(--text2)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, background: c.id === activeId[pid] ? 'var(--primary-dim)' : 'transparent' }}
            >
              <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.title}</span>
              <button onClick={e => { e.stopPropagation(); deleteConv(c.id); }} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text4)', fontSize: 14 }}>×</button>
            </div>
          ))}
        </div>
      )}

      {/* Messages */}
      <div style={s.msgArea}>
        {!messages.length && !isStreaming && (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', textAlign: 'center', padding: '20px 8px', gap: 12 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: '#f1f5f9' }}>MBSE Assistant</div>
            <div style={{ fontSize: 11, color: 'var(--text3)', lineHeight: 1.6 }}>Ask me to query, create, or analyze your SysML model.</div>
            {pid && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6, width: '100%', marginTop: 4 }}>
                {['List all elements in this project', 'Add a new PartDefinition called Sensor', 'Export this model as SysML v2 text'].map(q => (
                  <button key={q} onClick={() => sendMessage(q)} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 11px', borderRadius: 8, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text3)', fontSize: 11, cursor: 'pointer', textAlign: 'left' }}>
                    {q}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        {messages.map((msg, i) => msg.role === 'user' ? (
          <div key={i} style={{ display: 'flex', justifyContent: 'flex-end' }}>
            <div style={{ maxWidth: '85%', background: 'var(--primary)', color: '#fff', borderRadius: '14px 14px 4px 14px', padding: '9px 13px', fontSize: 12.5, lineHeight: 1.55, whiteSpace: 'pre-wrap' }}>{msg.content}</div>
          </div>
        ) : (
          <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
            <div style={{ width: 26, height: 26, borderRadius: 8, flexShrink: 0, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: 1 }}>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
            </div>
            <div style={{ maxWidth: '88%', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: '4px 14px 14px 14px', overflow: 'hidden' }}>
              {msg.content && <div style={{ padding: '9px 13px', fontSize: 12.5, lineHeight: 1.6, color: 'var(--text)' }} dangerouslySetInnerHTML={{ __html: renderMarkdown(msg.content) }} />}
              {(msg.tools ?? []).length > 0 && (
                <div style={{ padding: '8px 13px 9px', display: 'flex', flexDirection: 'column', gap: 5, borderTop: msg.content ? '1px solid var(--border)' : undefined }}>
                  {(msg.tools ?? []).map(t => (
                    <div key={t.id} style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
                      {t.status === 'done'
                        ? <div style={{ width: 13, height: 13, borderRadius: '50%', background: 'rgba(34,197,94,.15)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}><svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="#22c55e" strokeWidth="3" strokeLinecap="round"><polyline points="20 6 9 17 4 12"/></svg></div>
                        : <div style={{ width: 13, height: 13, borderRadius: '50%', border: '1.5px solid var(--primary-dim)', borderTopColor: 'var(--primary)', animation: 'spin 0.7s linear infinite', flexShrink: 0 }} />}
                      <span style={{ fontSize: 10.5, color: 'var(--text3)' }}>{(TOOL_LABELS[t.name] ?? t.name.replace(/_/g, ' ')) + (t.status === 'done' ? '' : '…')}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        ))}

        {isStreaming && (
          <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
            <div style={{ width: 26, height: 26, borderRadius: 8, flexShrink: 0, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: 1 }}>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
            </div>
            <div style={{ maxWidth: '88%', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: '4px 14px 14px 14px', overflow: 'hidden' }}>
              {streamText && <div style={{ padding: '9px 13px', fontSize: 12.5, lineHeight: 1.6, color: 'var(--text)' }} dangerouslySetInnerHTML={{ __html: renderMarkdown(streamText) }} />}
              {streamTools.length > 0 && (
                <div style={{ padding: '8px 13px 9px', display: 'flex', flexDirection: 'column', gap: 5, borderTop: streamText ? '1px solid var(--border)' : undefined }}>
                  {streamTools.map(t => (
                    <div key={t.id} style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
                      {t.status === 'done'
                        ? <div style={{ width: 13, height: 13, borderRadius: '50%', background: 'rgba(34,197,94,.15)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}><svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="#22c55e" strokeWidth="3" strokeLinecap="round"><polyline points="20 6 9 17 4 12"/></svg></div>
                        : <div style={{ width: 13, height: 13, borderRadius: '50%', border: '1.5px solid var(--primary-dim)', borderTopColor: 'var(--primary)', animation: 'spin 0.7s linear infinite', flexShrink: 0 }} />}
                      <span style={{ fontSize: 10.5, color: 'var(--text3)' }}>{(TOOL_LABELS[t.name] ?? t.name.replace(/_/g, ' ')) + (t.status === 'done' ? '' : '…')}</span>
                    </div>
                  ))}
                </div>
              )}
              {!streamText && !streamTools.length && (
                <div style={{ display: 'flex', gap: 4, alignItems: 'center', padding: '10px 13px' }}>
                  {[0, 200, 400].map(delay => (
                    <div key={delay} style={{ width: 5, height: 5, borderRadius: '50%', background: 'var(--text4)', animation: `pulse 1.2s ease-in-out ${delay}ms infinite` }} />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div style={s.inputArea}>
        <textarea
          style={s.textarea}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(input); } }}
          placeholder={pid ? 'Ask about your model…' : 'Select a project first'}
          disabled={!pid}
          rows={1}
        />
        {isStreaming
          ? <button style={s.stopBtn} onClick={() => readerRef.current?.cancel()}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><rect x="4" y="4" width="16" height="16" rx="2"/></svg>
            </button>
          : <button style={s.sendBtn} disabled={!input.trim() || !pid || isStreaming} onClick={() => sendMessage(input)}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
            </button>
        }
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add spin + pulse keyframes to index.css**

Append to `dashboard/src/index.css`:

```css
@keyframes spin  { to { transform: rotate(360deg); } }
@keyframes pulse { 0%, 80%, 100% { opacity: 0.3; transform: scale(0.9); } 40% { opacity: 1; transform: scale(1.1); } }
```

- [ ] **Step 3: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add dashboard/src/components/ChatPanel.tsx dashboard/src/index.css
git commit -m "feat: add ChatPanel with streaming SSE chat"
```

---

## Task 9: Sidebar

**Files:**
- Create: `dashboard/src/components/Sidebar.tsx`

- [ ] **Step 1: Create src/components/Sidebar.tsx**

```tsx
import { useState } from 'react';
import type { Project } from '../types/sysml';
import { createProject, deleteProject } from '../lib/api';

interface SidebarProps {
  projects: Project[];
  currentProjectId: string | null;
  onSelect: (id: string) => void;
  onProjectsChanged: () => void;
}

export function Sidebar({ projects, currentProjectId, onSelect, onProjectsChanged }: SidebarProps) {
  const [showModal, setShowModal] = useState(false);
  const [newName, setNewName] = useState('');

  async function handleCreate() {
    if (!newName.trim()) return;
    await createProject(newName.trim());
    setNewName('');
    setShowModal(false);
    onProjectsChanged();
  }

  async function handleDelete(e: React.MouseEvent, id: string, name: string) {
    e.stopPropagation();
    if (!confirm(`Delete "${name}"? This cannot be undone.`)) return;
    await deleteProject(id);
    onProjectsChanged();
  }

  const s = {
    sidebar: { borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column' as const, overflow: 'hidden', height: '100%' },
    header: { padding: '14px 16px 10px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0 },
    label: { fontSize: 10, fontWeight: 700, letterSpacing: '0.1em', textTransform: 'uppercase' as const, color: 'var(--text3)' },
    addBtn: { width: 22, height: 22, borderRadius: 5, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 16, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' },
    list: { flex: 1, overflowY: 'auto' as const, padding: '0 8px 16px' },
  };

  return (
    <aside style={s.sidebar}>
      <div style={s.header}>
        <span style={s.label}>Projects</span>
        <button style={s.addBtn} onClick={() => setShowModal(true)} title="New project">+</button>
      </div>

      <div style={s.list}>
        {!projects.length && <div style={{ padding: '8px 12px', fontSize: 11.5, color: 'var(--text4)' }}>No projects</div>}
        {projects.map(p => {
          const active = p['@id'] === currentProjectId;
          return (
            <div
              key={p['@id']}
              onClick={() => onSelect(p['@id'])}
              style={{ padding: '8px 10px', borderRadius: 7, cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 9, background: active ? 'var(--primary-dim)' : 'transparent', marginBottom: 2 }}
            >
              <div style={{ width: 8, height: 8, borderRadius: 2, flexShrink: 0, background: active ? 'var(--primary)' : 'var(--border2)' }} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12.5, color: active ? 'var(--primary-text)' : 'var(--text2)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</div>
                <div style={{ fontSize: 10, color: 'var(--text4)', fontFamily: 'monospace' }}>{p['@id'].slice(0, 8)}</div>
              </div>
              <button
                onClick={e => handleDelete(e, p['@id'], p.name)}
                style={{ width: 18, height: 18, borderRadius: 4, border: 'none', background: 'transparent', color: 'var(--text4)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
              >
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </button>
            </div>
          );
        })}
      </div>

      {showModal && (
        <div style={{ position: 'fixed', inset: 0, zIndex: 100, background: 'rgba(0,0,0,.6)', display: 'flex', alignItems: 'center', justifyContent: 'center', backdropFilter: 'blur(4px)' }} onClick={() => setShowModal(false)}>
          <div style={{ background: 'var(--surface)', border: '1px solid var(--border2)', borderRadius: 12, padding: 24, width: 340, maxWidth: '90vw' }} onClick={e => e.stopPropagation()}>
            <div style={{ fontSize: 15, fontWeight: 700, color: '#f1f5f9', marginBottom: 6 }}>New SysML Project</div>
            <div style={{ fontSize: 11.5, color: 'var(--text3)', marginBottom: 18, lineHeight: 1.5 }}>Creates a new project in the SMAPS repository.</div>
            <input
              style={{ width: '100%', background: 'var(--bg)', border: '1px solid var(--border2)', borderRadius: 8, padding: '9px 12px', color: 'var(--text)', fontSize: 13, fontFamily: 'inherit', outline: 'none', marginBottom: 14, display: 'block' }}
              placeholder="Project name, e.g. DroneSystem"
              value={newName}
              onChange={e => setNewName(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleCreate(); if (e.key === 'Escape') setShowModal(false); }}
              autoFocus
            />
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={() => setShowModal(false)} style={{ padding: '7px 14px', borderRadius: 7, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 12, cursor: 'pointer' }}>Cancel</button>
              <button onClick={handleCreate} disabled={!newName.trim()} style={{ padding: '7px 14px', borderRadius: 7, border: 'none', background: 'var(--primary)', color: '#fff', fontSize: 12, fontWeight: 600, cursor: 'pointer', opacity: newName.trim() ? 1 : 0.35 }}>Create Project</button>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/components/Sidebar.tsx
git commit -m "feat: add Sidebar component"
```

---

## Task 10: DiagramPanel

**Files:**
- Create: `dashboard/src/components/DiagramPanel.tsx`

- [ ] **Step 1: Create src/components/DiagramPanel.tsx**

```tsx
import { useState, useEffect, useRef } from 'react';
import mermaid from 'mermaid';
import { IBDViewer } from './IBDViewer';
import { getDiagrams, deleteDiagram } from '../lib/api';
import type { SmapsElement, LocalElement, StoredDiagram } from '../types/sysml';

mermaid.initialize({
  startOnLoad: false, theme: 'dark',
  themeVariables: { background: '#161622', primaryColor: '#1e2030', primaryTextColor: '#e2e8f0', lineColor: '#475569', fontSize: '12px' },
});

function MermaidViewer({ code, id }: { code: string; id: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const uid = `mermaid-${id}-${Date.now()}`;
    mermaid.render(uid, code)
      .then(({ svg }) => { if (ref.current) ref.current.innerHTML = svg; })
      .catch(err => { if (ref.current) ref.current.innerHTML = `<div style="color:#ef4444;font-size:11px;padding:8px">Render error: ${String(err.message)}</div>`; });
  }, [code, id]);
  return <div ref={ref} />;
}

interface DiagramPanelProps {
  projectId: string;
  smapsElements: SmapsElement[];
  localElements: LocalElement[];
}

export function DiagramPanel({ projectId, smapsElements, localElements }: DiagramPanelProps) {
  const [activeTab, setActiveTab] = useState(0);
  const [stored, setStored] = useState<StoredDiagram[]>([]);

  useEffect(() => {
    getDiagrams(projectId).then(setStored).catch(() => setStored([]));
  }, [projectId]);

  const hasProxyPorts = localElements.some(e => e['@type'] === 'ProxyPortUsage');
  const partDefs = smapsElements.filter(e => e['@type'] === 'PartDefinition');

  const bddCode = partDefs.length
    ? ['classDiagram', ...partDefs.map(p => `    class ${(p.declaredName ?? 'Unknown').replace(/\s+/g, '_').replace(/[^a-zA-Z0-9_]/g, '')} {\n        <<part def>>\n    }`)].join('\n')
    : null;

  type Tab = { key: string; label: string; kind: 'ibd' | 'bdd' | 'stored'; storedIdx?: number };

  const tabs: Tab[] = [
    ...(hasProxyPorts ? [{ key: 'ibd', label: 'IBD', kind: 'ibd' as const }] : []),
    ...(bddCode ? [{ key: 'bdd', label: 'BDD', kind: 'bdd' as const }] : []),
    ...stored.map((d, i) => ({ key: `s${i}`, label: d.type, kind: 'stored' as const, storedIdx: i })),
  ];

  const safeIdx = Math.min(activeTab, tabs.length - 1);
  const activeTab_ = tabs[safeIdx];

  async function handleDeleteStored(e: React.MouseEvent, idx: number) {
    e.stopPropagation();
    await deleteDiagram(projectId, idx);
    setStored(prev => prev.filter((_, i) => i !== idx));
    setActiveTab(0);
  }

  if (!tabs.length) {
    return (
      <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No diagrams yet. Ask the assistant to build an IBD.
      </div>
    );
  }

  return (
    <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22 }}>
      {/* Tab bar */}
      <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap' as const, marginBottom: 12 }}>
        {tabs.map((t, i) => (
          <button
            key={t.key}
            onClick={() => setActiveTab(i)}
            style={{ padding: '4px 11px', borderRadius: 5, border: '1px solid', borderColor: i === safeIdx ? 'var(--primary)' : 'var(--border)', background: i === safeIdx ? 'var(--primary-dim)' : 'transparent', color: i === safeIdx ? 'var(--primary-text)' : 'var(--text3)', fontSize: 11, cursor: 'pointer', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          >
            {t.label}
            {t.kind === 'stored' && (
              <span onClick={e => handleDeleteStored(e, t.storedIdx!)} style={{ fontSize: 13, color: 'var(--text4)', cursor: 'pointer', lineHeight: 1 }}>×</span>
            )}
          </button>
        ))}
      </div>

      {/* Active tab content */}
      {activeTab_?.kind === 'ibd' && <IBDViewer projectId={projectId} />}
      {activeTab_?.kind === 'bdd' && bddCode && <MermaidViewer code={bddCode} id={`bdd-${projectId}`} />}
      {activeTab_?.kind === 'stored' && activeTab_.storedIdx !== undefined && stored[activeTab_.storedIdx] && (
        <MermaidViewer code={stored[activeTab_.storedIdx].mermaid} id={`stored-${activeTab_.storedIdx}`} />
      )}
    </div>
  );
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/components/DiagramPanel.tsx
git commit -m "feat: add DiagramPanel with IBD + Mermaid tabs"
```

---

## Task 11: ProjectDetail

**Files:**
- Create: `dashboard/src/components/ProjectDetail.tsx`

- [ ] **Step 1: Create src/components/ProjectDetail.tsx**

```tsx
import { useState, useEffect, useCallback } from 'react';
import type { Project, SmapsElement, LocalElement } from '../types/sysml';
import { getElements, getLocalElements, deleteLocalElement } from '../lib/api';
import { DiagramPanel } from './DiagramPanel';

interface ProjectDetailProps {
  project: Project;
  onBack: () => void;
  refreshKey: number;
}

export function ProjectDetail({ project, onBack, refreshKey }: ProjectDetailProps) {
  const [smapsElements, setSmapsElements] = useState<SmapsElement[]>([]);
  const [localElements, setLocalElements] = useState<LocalElement[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [s, l] = await Promise.all([
        getElements(project['@id']),
        getLocalElements(project['@id']),
      ]);
      setSmapsElements(s);
      setLocalElements(l);
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => { load(); }, [load, refreshKey]);

  async function handleDeleteLocal(eid: string) {
    await deleteLocalElement(project['@id'], eid);
    load();
  }

  if (loading) return <div style={{ padding: 32, color: 'var(--text4)', fontSize: 12 }}>Loading model…</div>;

  const typeCounts: Record<string, number> = {};
  for (const e of [...smapsElements, ...localElements]) {
    typeCounts[e['@type']] = (typeCounts[e['@type']] ?? 0) + 1;
  }
  const partDefs = smapsElements.filter(e => e['@type'] === 'PartDefinition');
  const reqDefs  = smapsElements.filter(e => e['@type'] === 'RequirementDefinition');

  const escHtml = (s: string) => s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');

  return (
    <div style={{ padding: '24px 28px', overflowY: 'auto' as const, height: '100%' }}>
      {/* Header */}
      <div style={{ marginBottom: 20 }}>
        <button onClick={onBack} style={{ background: 'none', border: 'none', color: 'var(--text3)', cursor: 'pointer', fontSize: 11, display: 'flex', alignItems: 'center', gap: 5, padding: '4px 0', marginBottom: 6 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="m15 18-6-6 6-6"/></svg>
          All Projects
        </button>
        <div style={{ fontSize: 16, fontWeight: 700, color: '#f1f5f9' }}>{project.name}</div>
        <div style={{ fontSize: 10, fontFamily: 'monospace', color: 'var(--text4)', marginTop: 3 }}>{project['@id'].slice(0, 8)}…</div>
      </div>

      {/* Stats */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 10, marginBottom: 22 }}>
        {[
          { label: 'Elements', value: smapsElements.length, sub: 'in SMAPS' },
          { label: 'Part Defs', value: partDefs.length, sub: 'PartDefinition' },
          { label: 'Requirements', value: reqDefs.length, sub: 'RequirementDef' },
          { label: 'Local', value: localElements.length, sub: 'ports & connectors' },
        ].map(s => (
          <div key={s.label} style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 14 }}>
            <div style={{ fontSize: 10, color: 'var(--text3)', textTransform: 'uppercase', letterSpacing: '0.07em', marginBottom: 5 }}>{s.label}</div>
            <div style={{ fontSize: 26, fontWeight: 700, color: '#f1f5f9' }}>{s.value}</div>
            <div style={{ fontSize: 10, color: 'var(--text4)', marginTop: 3, fontFamily: 'monospace' }}>{s.sub}</div>
          </div>
        ))}
      </div>

      {/* Type chips */}
      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Element Types</div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))', gap: 7, marginBottom: 22 }}>
        {Object.entries(typeCounts).sort((a, b) => b[1] - a[1]).map(([type, count]) => {
          const isLocal = !smapsElements.some(e => e['@type'] === type);
          return (
            <div key={type} style={{ background: 'var(--surface)', border: `1px solid ${isLocal ? 'rgba(245,158,11,.3)' : 'var(--border)'}`, borderRadius: 6, padding: '8px 12px', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ color: isLocal ? '#f59e0b' : 'var(--text2)', fontSize: 11 }}>{type}</span>
              <span style={{ color: 'var(--primary-text)', fontWeight: 700, fontFamily: 'monospace', fontSize: 11 }}>{count}</span>
            </div>
          );
        })}
      </div>

      {/* Diagrams */}
      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Diagrams</div>
      <DiagramPanel projectId={project['@id']} smapsElements={smapsElements} localElements={localElements} />

      {/* Elements table */}
      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>All Elements</div>
      <div style={{ background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden', marginBottom: 24 }}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 11.5 }}>
          <thead>
            <tr>{['Name', 'Type', 'ID'].map(h => <th key={h} style={{ textAlign: 'left', padding: '7px 12px', fontSize: 9.5, fontWeight: 700, letterSpacing: '0.09em', textTransform: 'uppercase', color: 'var(--text4)', borderBottom: '1px solid var(--border)' }}>{h}</th>)}</tr>
          </thead>
          <tbody>
            {smapsElements.map(e => (
              <tr key={e['@id']}>
                <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text)' }}>{escHtml(e.declaredName ?? e.name ?? '<unnamed>')}</td>
                <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: '#818cf8', fontFamily: 'monospace', fontSize: 10.5 }}>{e['@type']}</td>
                <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text4)', fontFamily: 'monospace', fontSize: 10 }}>{e['@id'].slice(0, 12)}…</td>
              </tr>
            ))}
            {localElements.map(e => {
              const owner = smapsElements.find(x => x['@id'] === e.owner?.['@id']);
              return (
                <tr key={e['@id']}>
                  <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text)' }}>
                    {escHtml(e.declaredName ?? '<unnamed>')}
                    {owner && <span style={{ color: 'var(--text3)', fontSize: 10, marginLeft: 5 }}>on {escHtml(owner.declaredName ?? '')}</span>}
                  </td>
                  <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)' }}>
                    <span style={{ color: '#818cf8', fontFamily: 'monospace', fontSize: 10.5 }}>{e['@type']}</span>
                    <span style={{ display: 'inline-block', fontSize: 9, fontWeight: 600, padding: '1px 5px', borderRadius: 4, marginLeft: 5, background: 'rgba(245,158,11,.12)', color: '#f59e0b', border: '1px solid rgba(245,158,11,.25)' }}>local</span>
                  </td>
                  <td
                    style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text4)', fontFamily: 'monospace', fontSize: 10, cursor: 'pointer' }}
                    title="Delete local element"
                    onClick={() => handleDeleteLocal(e['@id'])}
                  >
                    {e['@id'].slice(0, 12)}… <span style={{ color: 'var(--text4)', fontSize: 10 }}>×</span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/components/ProjectDetail.tsx
git commit -m "feat: add ProjectDetail component"
```

---

## Task 12: App Shell + Projects Home

**Files:**
- Modify: `dashboard/src/App.tsx`

- [ ] **Step 1: Replace src/App.tsx with full implementation**

```tsx
import { useState, useEffect, useCallback } from 'react';
import type { Project } from './types/sysml';
import { getProjects } from './lib/api';
import { Sidebar } from './components/Sidebar';
import { ProjectDetail } from './components/ProjectDetail';
import { ChatPanel } from './components/ChatPanel';

const GRADS = [
  ['#1e3a5f','#0f2040'], ['#1a3a2a','#0d2015'], ['#3a1a3a','#200d20'],
  ['#3a2a1a','#201508'], ['#1a2a3a','#0a1520'], ['#2a1a3a','#150a20'],
];
function gradFor(name: string) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xfffff;
  const [a, b] = GRADS[h % GRADS.length];
  return `linear-gradient(135deg, ${a} 0%, ${b} 100%)`;
}

export function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [currentProjectId, setCurrentProjectId] = useState<string | null>(null);
  const [smapsOnline, setSmapsOnline] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);

  const loadProjects = useCallback(async () => {
    try {
      const list = await getProjects();
      setProjects(list);
      setSmapsOnline(true);
    } catch {
      setSmapsOnline(false);
    }
  }, []);

  useEffect(() => { loadProjects(); }, [loadProjects]);
  useEffect(() => {
    const id = setInterval(loadProjects, 30_000);
    return () => clearInterval(id);
  }, [loadProjects]);

  const currentProject = projects.find(p => p['@id'] === currentProjectId) ?? null;

  function handleModelChanged() {
    setRefreshKey(k => k + 1);
  }

  const layout = {
    wrapper: { display: 'flex', flexDirection: 'column' as const, height: '100vh' },
    header: { height: 52, padding: '0 20px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 10, flexShrink: 0, background: 'var(--bg)', zIndex: 10 },
    body: { display: 'grid', gridTemplateColumns: '248px 1fr 340px', flex: 1, minHeight: 0, overflow: 'hidden' },
    main: { overflowY: 'auto' as const, display: 'flex', flexDirection: 'column' as const },
  };

  return (
    <div style={layout.wrapper}>
      {/* Header */}
      <header style={layout.header}>
        <div style={{ width: 7, height: 7, borderRadius: '50%', background: smapsOnline ? 'var(--green)' : 'var(--red)', flexShrink: 0, transition: 'background 0.3s' }} />
        <h1 style={{ fontSize: 14, fontWeight: 600, color: '#f1f5f9', letterSpacing: '0.02em' }}>sysml-bridge</h1>
        <span style={{ color: 'var(--border2)' }}>·</span>
        <span style={{ fontSize: 11, color: 'var(--text3)', fontFamily: 'monospace' }}>localhost:9000</span>
        <div style={{ flex: 1 }} />
        <button onClick={loadProjects} style={{ padding: '5px 8px', borderRadius: 6, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 11, cursor: 'pointer' }}>↻ refresh</button>
      </header>

      <div style={layout.body}>
        <Sidebar
          projects={projects}
          currentProjectId={currentProjectId}
          onSelect={setCurrentProjectId}
          onProjectsChanged={loadProjects}
        />

        <main style={layout.main}>
          {!currentProject && (
            projects.length === 0
              ? (
                <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 16, textAlign: 'center', padding: 40 }}>
                  <div style={{ width: 56, height: 56, borderRadius: 14, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="1.5" strokeLinecap="round"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="m8 21 4-4 4 4"/><path d="M12 17v4"/></svg>
                  </div>
                  <div style={{ fontSize: 17, fontWeight: 700, color: '#f1f5f9' }}>No projects yet</div>
                  <div style={{ fontSize: 12, color: 'var(--text3)', maxWidth: 260, lineHeight: 1.6 }}>Create your first SysML v2 project to start modeling.</div>
                </div>
              )
              : (
                <div style={{ padding: '28px 28px 0', flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 20 }}>
                    <div>
                      <div style={{ fontSize: 18, fontWeight: 700, color: '#f1f5f9' }}>Your Models</div>
                      <div style={{ fontSize: 12, color: 'var(--text3)', marginTop: 2 }}>SysML v2 projects in SMAPS</div>
                    </div>
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: 14 }}>
                    {projects.map(p => (
                      <div key={p['@id']} onClick={() => setCurrentProjectId(p['@id'])} style={{ border: '1px solid var(--border)', borderRadius: 10, overflow: 'hidden', cursor: 'pointer', background: 'var(--surface)', transition: 'border-color 0.15s, transform 0.15s' }}>
                        <div style={{ height: 80, background: gradFor(p.name), position: 'relative', display: 'flex', alignItems: 'flex-end', padding: '10px 12px' }}>
                          <div style={{ position: 'absolute', top: 14, left: 12, width: 32, height: 32, borderRadius: 8, background: 'rgba(255,255,255,.1)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,.7)" strokeWidth="1.5" strokeLinecap="round"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/></svg>
                          </div>
                          <span style={{ marginLeft: 'auto', fontSize: 11, fontWeight: 700, fontFamily: 'monospace', color: 'rgba(255,255,255,.75)', background: 'rgba(0,0,0,.25)', padding: '2px 7px', borderRadius: 99 }}>{p['@id'].slice(0, 8)}</span>
                        </div>
                        <div style={{ padding: '12px 14px' }}>
                          <div style={{ fontSize: 13, fontWeight: 600, color: '#f1f5f9', marginBottom: 4 }}>{p.name}</div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )
          )}

          {currentProject && (
            <ProjectDetail
              key={currentProject['@id']}
              project={currentProject}
              onBack={() => setCurrentProjectId(null)}
              refreshKey={refreshKey}
            />
          )}
        </main>

        <ChatPanel project={currentProject} onModelChanged={handleModelChanged} />
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd dashboard && npx tsc --noEmit
```

Expected: no output.

- [ ] **Step 3: Run the dev server and verify the full app**

```bash
cd dashboard && npm run dev
```

Open `http://localhost:5173`. Expected:
- Dark header with status dot + "sysml-bridge" title
- Sidebar with project list (if SMAPS is running)
- Projects home grid on the main area
- Chat panel on the right with MBSE Assistant

Click a project → ProjectDetail opens with stats, type chips, diagrams, elements table.

- [ ] **Step 4: Commit**

```bash
git add dashboard/src/App.tsx
git commit -m "feat: add App shell — full three-column React layout wired up"
```

---

## Task 13: Update server.js for Production + Final Tests

**Files:**
- Modify: `dashboard/server.js`

- [ ] **Step 1: Read the current static file serving line in server.js**

Open `dashboard/server.js` and find this line (around line 27):

```js
app.use(express.static(__dirname));
```

- [ ] **Step 2: Replace it to serve dist/ and add SPA fallback**

Change:
```js
app.use(express.static(__dirname));
```

To:
```js
app.use(express.static(join(__dirname, 'dist')));
```

Then add a SPA fallback **after all `/api` routes** (at the end of the file, just before `app.listen`):

```js
app.get('*', (req, res) => {
  res.sendFile(join(__dirname, 'dist', 'index.html'));
});
```

- [ ] **Step 3: Run all tests to verify nothing is broken**

```bash
cd dashboard && npm test
```

Expected:
```
 ✓ src/lib/__tests__/ibd-transformer.test.ts (6)
 Test Files  1 passed (1)
 Tests  6 passed (6)
```

- [ ] **Step 4: Run a production build**

```bash
cd dashboard && npm run build
```

Expected: `dist/` directory created with `index.html`, `assets/*.js`, `assets/*.css`.

- [ ] **Step 5: Test the production build via Express**

```bash
cd dashboard && npm start
```

Open `http://localhost:6121`. Expected: same UI as the Vite dev server.

- [ ] **Step 6: Commit**

```bash
git add dashboard/server.js
git commit -m "feat: update server.js to serve Vite dist/ in production"
```

---

## Self-Review

**Spec coverage:**
- ✅ Vite + React + TypeScript scaffolding (Task 1)
- ✅ `src/types/sysml.ts` (Task 2)
- ✅ API fetch wrappers (Task 3)
- ✅ IBD transformer with tests (Task 4)
- ✅ ELK layout, `layered`, `RIGHT` direction, fallback (Task 5)
- ✅ `SysMLBlockNode`: stereotype header, compartment body, hollow circle handles on boundary (Task 6)
- ✅ `SysMLEdge`: bezier + label chip (Task 6)
- ✅ `IBDViewer`: React Flow canvas with `fitView`, pan/zoom, `colorMode="dark"` (Task 7)
- ✅ `ChatPanel`: SSE streaming, tool display, localStorage conversations (Task 8)
- ✅ `Sidebar`: project list, new project modal, delete (Task 9)
- ✅ `DiagramPanel`: IBD tab (only when ProxyPortUsage exists), BDD tab, stored Mermaid tabs (Task 10)
- ✅ `ProjectDetail`: stats, type chips, elements table with LOCAL badge (Task 11)
- ✅ `App`: three-column layout, projects home grid, status dot, refresh (Task 12)
- ✅ `server.js`: `static('dist')` + SPA fallback (Task 13)
- ✅ Error handling: ELK fallback in Task 5, Mermaid error in Task 10

**No placeholders found.**

**Type consistency:** `SysMLBlockNodeData` defined in `ibd-transformer.ts` and imported by `SysMLBlockNode.tsx`, `IBDViewer.tsx`, `ibd-layout.ts` consistently. `LocalElement` / `SmapsElement` defined in `types/sysml.ts` and used identically across transformer, API layer, and components.
