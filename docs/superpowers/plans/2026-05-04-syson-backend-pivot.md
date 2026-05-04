# SysON Backend Pivot — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace SMAPS + local JSON with SysON as the single model store, gaining full SysML v2 containment hierarchy, all element types, and native diagram rendering.

**Architecture:** Express server becomes a thin proxy to SysON REST (`:8080/api/rest/`) for element reads and SysON GraphQL (`:8080/api/graphql`) for mutations. Frontend replaces flat elements table with a containment tree, embeds SysON diagrams via iframe, and keeps React Flow IBD as a secondary viewer fed by SysON data.

**Tech Stack:** Express + fetch (no new deps), React + TypeScript (Vite), SysON REST + GraphQL APIs, React Flow v12 (kept), ELK.js (kept)

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `dashboard/src/lib/containment.ts` | Build tree from flat SysON element array — skip membership wrappers, resolve parents |
| `dashboard/src/components/ContainmentTree.tsx` | Expandable tree component rendering the containment hierarchy |

### Modified files

| File | Change summary |
|---|---|
| `dashboard/src/types/sysml.ts` | Replace SmapsElement/LocalElement with unified SysONElement; add tree/diagram types |
| `dashboard/server.js` | Replace SMAPS with SysON REST + GraphQL; replace all chat tools |
| `dashboard/src/lib/api.ts` | Update endpoints; remove local-element/diagram CRUD; add representations query |
| `dashboard/src/components/Sidebar.tsx` | SysON project CRUD via Express proxy |
| `dashboard/src/App.tsx` | SysON health check; remove SMAPS references |
| `dashboard/src/components/ProjectDetail.tsx` | Replace elements table with ContainmentTree; update stats |
| `dashboard/src/components/DiagramPanel.tsx` | SysON iframe tabs + React Flow IBD; remove Mermaid |
| `dashboard/src/lib/ibd-transformer.ts` | Consume SysON elements; remove prefix-matching; remove LocalElement |
| `dashboard/src/components/IBDViewer.tsx` | Fetch from single elements endpoint |
| `dashboard/src/components/ChatPanel.tsx` | Update tool labels |
| `dashboard/src/lib/__tests__/ibd-transformer.test.ts` | Update fixtures to SysON element shapes |
| `dashboard/package.json` | Remove mermaid dependency |

### Removed files

| File | Reason |
|---|---|
| `dashboard/data/*.json` | Local element store deprecated |

### Unchanged files

| File | Reason |
|---|---|
| `dashboard/src/components/sysml/SysMLBlockNode.tsx` | Reused by React Flow IBD |
| `dashboard/src/components/sysml/SysMLEdge.tsx` | Reused by React Flow IBD |
| `dashboard/src/lib/ibd-layout.ts` | ELK layout unchanged |
| `dashboard/src/index.css` | Dark theme unchanged |
| `dashboard/vite.config.ts` | Proxy stays on :6121 |

---

## SysON API Reference (for implementers)

These are the live-tested API calls used throughout the plan. All UUIDs below are examples.

### REST reads

```
GET http://localhost:8080/api/rest/projects
→ Array<{ "@id": string, name: string, ... }>

GET http://localhost:8080/api/rest/projects/{pid}/commits
→ Array<{ "@id": string, ... }>  (first = HEAD)

GET http://localhost:8080/api/rest/projects/{pid}/commits/{cid}/elements
→ Array<SysONElement>  (full containment: ownedElement[], owner, @type, declaredName, etc.)
```

### REST commit for rename

```
POST http://localhost:8080/api/rest/projects/{pid}/commits
Content-Type: application/json
{
  "@type": "Commit",
  "change": [{
    "@type": "DataVersion",
    "identity": { "@id": "{elementId}", "@type": "DataIdentity" },
    "payload": { "@type": "{elementType}", "@id": "{elementId}", "declaredName": "NewName" }
  }]
}
```

### REST commit for delete

```
POST http://localhost:8080/api/rest/projects/{pid}/commits
Content-Type: application/json
{
  "@type": "Commit",
  "change": [{
    "@type": "DataVersion",
    "identity": { "@id": "{elementId}", "@type": "DataIdentity" },
    "payload": null
  }]
}
```

### GraphQL queries

```graphql
# Get editing context ID (required for all mutations)
query { viewer { project(projectId: "{pid}") { currentEditingContext { id } } } }

# Get valid child types for a container
query { viewer { editingContext(editingContextId: "{ecid}") {
  childCreationDescriptions(containerId: "{containerId}") { id label }
} } }

# Get object info
query { viewer { editingContext(editingContextId: "{ecid}") {
  object(objectId: "{oid}") { id label kind }
} } }

# List diagram representations
query { viewer { editingContext(editingContextId: "{ecid}") {
  representations { edges { node { id label kind } } }
} } }

# Get representation descriptions (for creating diagrams)
query { viewer { editingContext(editingContextId: "{ecid}") {
  representationDescriptions(objectId: "{oid}") { edges { node { id label } } }
} } }
```

### GraphQL mutations

```graphql
# Create project
mutation { createProject(input: {
  id: "{uuid}", name: "ProjectName",
  natures: ["siriusComponents://nature?kind=siriusWeb"],
  templateId: "sysmlv2-template"
}) { __typename ... on CreateProjectSuccessPayload { project { id } } ... on ErrorPayload { message } } }

# Delete project
mutation { deleteProject(input: { id: "{uuid}", projectId: "{pid}" })
  { __typename ... on SuccessPayload { messages { body } } ... on ErrorPayload { message } } }

# Create child element (auto-named)
mutation { createChild(input: {
  id: "{uuid}", editingContextId: "{ecid}", objectId: "{parentId}",
  childCreationDescriptionId: "{descId}"
}) { __typename ... on CreateChildSuccessPayload { object { id label kind } } ... on ErrorPayload { message } } }

# Create diagram representation
mutation { createRepresentation(input: {
  id: "{uuid}", editingContextId: "{ecid}", objectId: "{elementId}",
  representationDescriptionId: "{descId}", representationName: "MyDiagram"
}) { __typename ... on CreateRepresentationSuccessPayload { representation { id label kind } } ... on ErrorPayload { message } } }
```

### Key childCreationDescriptionId patterns

| Label | ID pattern | Context |
|---|---|---|
| Part Definition | `SysMLv2EditService-PartDefinition` | Under Package |
| Requirement | `SysMLv2EditService-RequirementDefinition` | Under Package |
| Port | `SysMLv2EditService-PortUsage` | Under PartDefinition |
| Part | `SysMLv2EditService-PartUsage` | Under PartDefinition |
| Action | `SysMLv2EditService-ActionUsage` | Under ActionDefinition |
| Connection | `SysMLv2EditService-ConnectionUsage` | Under PartDefinition |
| State | `SysMLv2EditService-StateUsage` | Under StateDefinition |

### Diagram representation description IDs

Query `representationDescriptions(objectId)` for a specific element. Common results:
- "General View" — general-purpose SysML diagram
- "Interconnection View" — IBD
- "State Transition View" — state machine
- "Requirements Table View" — requirements table

### Element ID after createChild

`createChild` returns `object.id` which is the **OwningMembership** wrapper, not the actual element. To get the actual element ID for REST operations:
1. Call `GET /api/rest/projects/{pid}/commits/{cid}/elements`
2. Find the element with matching `declaredName` (auto-generated, e.g. `"port1"`) under the specified parent
3. Use that element's `@id` for REST commit rename/delete

Alternative: the auto-generated name is deterministic (type + incrementing number), so the element can be found by scanning the containment tree.

---

## Task 1: Update types and add containment tree library

**Files:**
- Modify: `dashboard/src/types/sysml.ts`
- Create: `dashboard/src/lib/containment.ts`
- Create: `dashboard/src/lib/__tests__/containment.test.ts`

- [ ] **Step 1: Write the containment tree test**

```typescript
// dashboard/src/lib/__tests__/containment.test.ts
import { describe, it, expect } from 'vitest';
import { buildContainmentTree, type TreeNode } from '../containment';

const PKG = {
  '@id': 'pkg-1', '@type': 'Package', declaredName: 'DroneSystem',
  ownedElement: [{ '@id': 'mem-1' }], owner: null,
};
const MEM = {
  '@id': 'mem-1', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'part-1' }], owner: { '@id': 'pkg-1' },
};
const PART = {
  '@id': 'part-1', '@type': 'PartDefinition', declaredName: 'FlightController',
  ownedElement: [{ '@id': 'mem-2' }], owner: { '@id': 'mem-1' },
};
const MEM2 = {
  '@id': 'mem-2', '@type': 'FeatureMembership', declaredName: null,
  ownedElement: [{ '@id': 'port-1' }], owner: { '@id': 'part-1' },
};
const PORT = {
  '@id': 'port-1', '@type': 'PortUsage', declaredName: 'pwr_in',
  ownedElement: [], owner: { '@id': 'mem-2' },
};

const elements = [PKG, MEM, PART, MEM2, PORT];

describe('buildContainmentTree', () => {
  it('skips membership wrappers', () => {
    const roots = buildContainmentTree(elements as any);
    expect(roots).toHaveLength(1);
    expect(roots[0].element['@type']).toBe('Package');
    expect(roots[0].children).toHaveLength(1);
    expect(roots[0].children[0].element['@type']).toBe('PartDefinition');
  });

  it('nests ports under their logical parent', () => {
    const roots = buildContainmentTree(elements as any);
    const part = roots[0].children[0];
    expect(part.children).toHaveLength(1);
    expect(part.children[0].element['@type']).toBe('PortUsage');
    expect(part.children[0].element.declaredName).toBe('pwr_in');
  });

  it('returns empty array for empty input', () => {
    expect(buildContainmentTree([])).toEqual([]);
  });

  it('handles elements with no ownedElement field', () => {
    const lone = { '@id': 'x', '@type': 'PartDefinition', declaredName: 'X' };
    const roots = buildContainmentTree([lone as any]);
    expect(roots).toHaveLength(1);
    expect(roots[0].children).toEqual([]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd dashboard && npx vitest run src/lib/__tests__/containment.test.ts`
Expected: FAIL — `Cannot find module '../containment'`

- [ ] **Step 3: Update types**

Replace the entire content of `dashboard/src/types/sysml.ts`:

```typescript
// dashboard/src/types/sysml.ts
export interface SysONElement {
  '@id': string;
  '@type': string;
  declaredName?: string | null;
  declaredShortName?: string | null;
  name?: string | null;
  ownedElement?: Array<{ '@id': string }>;
  owner?: { '@id': string } | null;
}

export interface Project {
  '@id': string;
  name: string;
}

export interface Representation {
  id: string;
  label: string;
  kind: string;
}
```

- [ ] **Step 4: Write containment tree builder**

```typescript
// dashboard/src/lib/containment.ts
import type { SysONElement } from '../types/sysml';

export interface TreeNode {
  element: SysONElement;
  children: TreeNode[];
}

function isMembership(type: string): boolean {
  return type.endsWith('Membership');
}

export function buildContainmentTree(elements: SysONElement[]): TreeNode[] {
  const byId = new Map<string, SysONElement>();
  for (const el of elements) byId.set(el['@id'], el);

  const childrenOf = new Map<string, SysONElement[]>();
  for (const el of elements) {
    const ownerId = el.owner?.['@id'];
    if (!ownerId) continue;
    if (!childrenOf.has(ownerId)) childrenOf.set(ownerId, []);
    childrenOf.get(ownerId)!.push(el);
  }

  function buildNode(el: SysONElement): TreeNode | null {
    if (isMembership(el['@type'])) return null;
    const directChildren = childrenOf.get(el['@id']) ?? [];
    const logicalChildren: TreeNode[] = [];
    for (const child of directChildren) {
      if (isMembership(child['@type'])) {
        const grandchildren = childrenOf.get(child['@id']) ?? [];
        for (const gc of grandchildren) {
          const node = buildNode(gc);
          if (node) logicalChildren.push(node);
        }
      } else {
        const node = buildNode(child);
        if (node) logicalChildren.push(node);
      }
    }
    return { element: el, children: logicalChildren };
  }

  const roots: TreeNode[] = [];
  for (const el of elements) {
    if (el.owner === null || el.owner === undefined) {
      if (!isMembership(el['@type'])) {
        const node = buildNode(el);
        if (node) roots.push(node);
      }
    }
  }

  if (roots.length === 0) {
    const hasOwner = new Set(elements.filter(e => e.owner).map(e => e['@id']));
    for (const el of elements) {
      const ownerId = el.owner?.['@id'];
      if (ownerId && !byId.has(ownerId) && !isMembership(el['@type'])) {
        const node = buildNode(el);
        if (node) roots.push(node);
      }
    }
  }

  return roots;
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd dashboard && npx vitest run src/lib/__tests__/containment.test.ts`
Expected: PASS — all 4 tests

- [ ] **Step 6: Commit**

```bash
git add dashboard/src/types/sysml.ts dashboard/src/lib/containment.ts dashboard/src/lib/__tests__/containment.test.ts
git commit -m "feat: add SysON element types and containment tree builder"
```

---

## Task 2: Replace server.js SMAPS backend with SysON

**Files:**
- Modify: `dashboard/server.js`

This is the largest task — the entire backend switches from SMAPS to SysON.

- [ ] **Step 1: Replace SMAPS helpers with SysON helpers**

In `dashboard/server.js`, replace everything from line 1 through line 100 (the imports, DATA_DIR setup, loadProjectData/saveProjectData/loadLocalElements/saveLocalElement/deleteLocalElement functions, and smapsFetch/getHeadCommitId/queryElements/createCommit functions) with:

```javascript
import express from 'express';
import Anthropic from '@anthropic-ai/sdk';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import { readFileSync, existsSync } from 'fs';
import { randomUUID } from 'crypto';

const __root = join(dirname(fileURLToPath(import.meta.url)), '..');
const envPath = join(__root, '.env');
if (existsSync(envPath)) {
  for (const line of readFileSync(envPath, 'utf8').split('\n')) {
    const m = line.match(/^\s*([A-Z_][A-Z0-9_]*)=(.*)$/);
    if (m && !process.env[m[1]]) process.env[m[1]] = m[2].replace(/^['"]|['"]$/g, '');
  }
}

const __dirname = dirname(fileURLToPath(import.meta.url));

const app  = express();
const PORT = parseInt(process.env.PORT ?? '6121', 10);
const SYSON = process.env.SYSON_ENDPOINT ?? 'http://localhost:8080';
const SYSON_GQL = `${SYSON}/api/graphql`;

app.use(express.json());
app.use(express.static(join(__dirname, 'dist')));

// ── SysON helpers ────────────────────────────────────────────────────────────

async function sysonRest(path) {
  const res = await fetch(`${SYSON}/api/rest${path}`);
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`SysON REST ${path} → ${res.status}: ${text}`);
  }
  return res.json();
}

async function sysonGql(query, variables = {}) {
  const res = await fetch(SYSON_GQL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, variables }),
  });
  const json = await res.json();
  if (json.errors?.length) throw new Error(json.errors[0].message);
  return json.data;
}

async function sysonRestCommit(projectId, changes) {
  const res = await fetch(`${SYSON}/api/rest/projects/${projectId}/commits`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ '@type': 'Commit', change: changes }),
  });
  return res.json();
}

const ecCache = new Map();

async function getEditingContextId(projectId) {
  if (ecCache.has(projectId)) return ecCache.get(projectId);
  const data = await sysonGql(
    `query($pid: ID!) { viewer { project(projectId: $pid) { currentEditingContext { id } } } }`,
    { pid: projectId },
  );
  const ecId = data.viewer.project.currentEditingContext.id;
  ecCache.set(projectId, ecId);
  return ecId;
}

async function getHeadCommitId(projectId) {
  const commits = await sysonRest(`/projects/${projectId}/commits`).catch(() => []);
  return commits[0]?.['@id'] ?? null;
}

async function getAllElements(projectId) {
  const commitId = await getHeadCommitId(projectId);
  if (!commitId) return [];
  return sysonRest(`/projects/${projectId}/commits/${commitId}/elements`);
}
```

- [ ] **Step 2: Replace tool definitions**

Replace the entire `TOOLS` array (lines 102-211) with:

```javascript
const TOOLS = [
  {
    name: 'query_elements',
    description: 'Query all SysML v2 elements in the current project. Returns the full containment hierarchy from SysON. Optionally filter by @type.',
    input_schema: {
      type: 'object',
      properties: {
        type_filter: {
          type: 'string',
          description: 'Filter by SysML v2 @type, e.g. PartDefinition, PortUsage, ActionDefinition, RequirementDefinition, etc.',
        },
      },
    },
  },
  {
    name: 'create_element',
    description: 'Create a new SysML v2 element under a parent container in SysON. The server queries SysON for valid child types under the parent and creates the element. Works with ALL SysML v2 types — PartDefinition, PortUsage, ActionDefinition, RequirementDefinition, ConnectionUsage, StateUsage, etc.',
    input_schema: {
      type: 'object',
      required: ['element_type', 'name', 'parent_id'],
      properties: {
        element_type: {
          type: 'string',
          description: 'Human-readable element type label as shown in SysON, e.g. "Part Definition", "Port", "Action", "Requirement", "Connection", "State", "Item", "Interface", "Flow Usage", "Attribute"',
        },
        name: { type: 'string', description: 'Declared name for the element' },
        parent_id: { type: 'string', description: '@id of the parent container element. Use the Package ID for top-level elements, or a PartDefinition ID for ports/parts/etc.' },
      },
    },
  },
  {
    name: 'delete_element',
    description: 'Delete a SysML v2 element from the SysON model by its @id.',
    input_schema: {
      type: 'object',
      required: ['element_id'],
      properties: {
        element_id: { type: 'string', description: 'The @id of the element to delete' },
      },
    },
  },
  {
    name: 'create_diagram',
    description: 'Create a SysON diagram view for a model element. Returns the representation ID so it can be displayed in the dashboard. Available types: "General View", "Interconnection View", "State Transition View", "Requirements Table View".',
    input_schema: {
      type: 'object',
      required: ['element_id', 'diagram_type', 'name'],
      properties: {
        element_id: { type: 'string', description: '@id of the element to create the diagram for' },
        diagram_type: { type: 'string', description: 'Diagram type label, e.g. "General View", "Interconnection View"' },
        name: { type: 'string', description: 'Name for the diagram' },
      },
    },
  },
  {
    name: 'export_sysml',
    description: 'Export the current project as SysML v2 textual notation.',
    input_schema: { type: 'object', properties: {} },
  },
  {
    name: 'create_project',
    description: 'Create a new SysML v2 project in SysON.',
    input_schema: {
      type: 'object',
      required: ['name'],
      properties: { name: { type: 'string', description: 'Project name' } },
    },
  },
];
```

- [ ] **Step 3: Replace executeTool function**

Replace the entire `executeTool` function (lines 213-354) with:

```javascript
async function executeTool(name, input, projectId) {
  try {
    if (name === 'query_elements') {
      const elements = await getAllElements(projectId);
      const nonMembership = elements.filter(e => !e['@type'].endsWith('Membership'));
      const filtered = input.type_filter
        ? nonMembership.filter(e => e['@type'] === input.type_filter)
        : nonMembership;
      return {
        count: filtered.length,
        elements: filtered.map(e => ({
          id: e['@id'],
          type: e['@type'],
          name: e.declaredName ?? e.name ?? '<unnamed>',
          shortName: e.declaredShortName,
          ownedElementCount: (e.ownedElement ?? []).length,
          ownerId: e.owner?.['@id'],
        })),
      };
    }

    if (name === 'create_element') {
      const ecId = await getEditingContextId(projectId);
      const descs = await sysonGql(
        `query($ecId: ID!, $cid: ID!) { viewer { editingContext(editingContextId: $ecId) { childCreationDescriptions(containerId: $cid) { id label } } } }`,
        { ecId, cid: input.parent_id },
      );
      const descriptions = descs.viewer.editingContext.childCreationDescriptions;
      const match = descriptions.find(d => d.label.toLowerCase() === input.element_type.toLowerCase());
      if (!match) {
        return { error: `"${input.element_type}" is not a valid child type for this container. Valid types: ${descriptions.map(d => d.label).join(', ')}` };
      }

      const createResult = await sysonGql(
        `mutation($input: CreateChildInput!) { createChild(input: $input) { __typename ... on CreateChildSuccessPayload { object { id label kind } } ... on ErrorPayload { message } } }`,
        { input: { id: randomUUID(), editingContextId: ecId, objectId: input.parent_id, childCreationDescriptionId: match.id } },
      );
      const payload = createResult.createChild;
      if (payload.__typename === 'ErrorPayload') return { error: payload.message };

      const createdObj = payload.object;

      // Rename via REST commit — find the actual element ID
      const allElements = await getAllElements(projectId);
      const created = allElements.find(e =>
        e.declaredName === createdObj.label && !e['@type'].endsWith('Membership')
      );

      if (created && input.name !== createdObj.label) {
        await sysonRestCommit(projectId, [{
          '@type': 'DataVersion',
          identity: { '@id': created['@id'], '@type': 'DataIdentity' },
          payload: { '@type': created['@type'], '@id': created['@id'], declaredName: input.name },
        }]);
      }

      return {
        success: true,
        element: {
          id: created?.['@id'] ?? createdObj.id,
          type: created?.['@type'] ?? createdObj.kind?.split('entity=')[1] ?? input.element_type,
          name: input.name,
        },
      };
    }

    if (name === 'delete_element') {
      await sysonRestCommit(projectId, [{
        '@type': 'DataVersion',
        identity: { '@id': input.element_id, '@type': 'DataIdentity' },
        payload: null,
      }]);
      return { success: true };
    }

    if (name === 'create_diagram') {
      const ecId = await getEditingContextId(projectId);
      const repDescs = await sysonGql(
        `query($ecId: ID!, $oid: ID!) { viewer { editingContext(editingContextId: $ecId) { representationDescriptions(objectId: $oid) { edges { node { id label } } } } } }`,
        { ecId, oid: input.element_id },
      );
      const options = repDescs.viewer.editingContext.representationDescriptions.edges.map(e => e.node);
      const match = options.find(o => o.label.toLowerCase() === input.diagram_type.toLowerCase());
      if (!match) {
        return { error: `"${input.diagram_type}" not available. Options: ${options.map(o => o.label).join(', ')}` };
      }

      const result = await sysonGql(
        `mutation($input: CreateRepresentationInput!) { createRepresentation(input: $input) { __typename ... on CreateRepresentationSuccessPayload { representation { id label kind } } ... on ErrorPayload { message } } }`,
        { input: { id: randomUUID(), editingContextId: ecId, objectId: input.element_id, representationDescriptionId: match.id, representationName: input.name } },
      );
      const rep = result.createRepresentation;
      if (rep.__typename === 'ErrorPayload') return { error: rep.message };
      return { success: true, representation: rep.representation };
    }

    if (name === 'export_sysml') {
      const elements = await getAllElements(projectId);
      const nonMem = elements.filter(e => !e['@type'].endsWith('Membership'));
      const projects = await sysonRest('/projects');
      const project = projects.find(p => p['@id'] === projectId);

      const lines = [`package ${project?.name ?? 'Model'} {`, ''];
      for (const e of nonMem) {
        const indent = '    ';
        const name = e.declaredName ?? e['@id'].slice(0, 8);
        const shortName = e.declaredShortName ? ` <'${e.declaredShortName}'>` : '';
        const childCount = (e.ownedElement ?? []).length;
        if (e['@type'] === 'PartDefinition') {
          lines.push(`${indent}part def${shortName} ${name}${childCount ? ' { ... }' : ';'}`);
        } else if (e['@type'] === 'RequirementDefinition') {
          lines.push(`${indent}requirement def${shortName} ${name} { doc /* requirement */ }`);
        } else if (e['@type'] === 'PortUsage') {
          lines.push(`${indent}port ${name};`);
        } else if (e['@type'] === 'ActionDefinition') {
          lines.push(`${indent}action def ${name}${childCount ? ' { ... }' : ';'}`);
        } else if (e['@type'] === 'Package') {
          // Skip — it's the root
        } else if (!e['@type'].endsWith('Membership')) {
          lines.push(`${indent}${e['@type']} ${name};`);
        }
      }
      lines.push('}');
      return { sysml: lines.join('\n') };
    }

    if (name === 'create_project') {
      const result = await sysonGql(
        `mutation($input: CreateProjectInput!) { createProject(input: $input) { __typename ... on CreateProjectSuccessPayload { project { id } } ... on ErrorPayload { message } } }`,
        { input: { id: randomUUID(), name: input.name, natures: ['siriusComponents://nature?kind=siriusWeb'], templateId: 'sysmlv2-template' } },
      );
      const payload = result.createProject;
      if (payload.__typename === 'ErrorPayload') return { error: payload.message };
      return { success: true, project: { id: payload.project.id, name: input.name } };
    }

    return { error: `Unknown tool: ${name}` };
  } catch (e) {
    return { error: e.message };
  }
}
```

- [ ] **Step 4: Update the system prompt in the chat endpoint**

Replace the entire `system` string assignment (lines 374-440 in the original, inside `app.post('/api/chat', ...)`) with:

```javascript
  let system = `You are an AI assistant integrated into sysml-bridge, a local SysML v2 MBSE tool powered by SysON.

SysON provides the full SysML v2 metamodel. You can create ANY valid SysML v2 element type:
- Part Definition, Part, Port, Attribute, Reference
- Action, State, Constraint, Requirement
- Connection, Interface, Flow Usage, Allocation
- Item, Use Case, Verification Case, Analysis Case
- And many more — use query_elements to see what exists

ELEMENT CREATION:
Use create_element with:
- element_type: human-readable label (e.g. "Part Definition", "Port", "Action")
- name: the element's declared name
- parent_id: the @id of the container element (Package for top-level, PartDefinition for ports/parts)

The server validates element types against SysON's metamodel rules — if a child type isn't valid under a container, it tells you what types ARE valid.

DIAGRAMS:
Use create_diagram to create SysON diagram views:
- "General View" — general-purpose SysML diagram
- "Interconnection View" — internal block diagram (IBD)
- "State Transition View" — state machine diagram
- "Requirements Table View" — requirements table

The dashboard displays SysON diagrams in embedded iframes alongside a React Flow IBD.

Be concise. Use the tools to interact with the model.`;
```

- [ ] **Step 5: Update the system prompt context injection**

Replace the project context injection block (lines 442-471 in the original) with:

```javascript
  if (projectId) {
    try {
      const elements = await getAllElements(projectId).catch(() => []);
      const nonMem = elements.filter(e => !e['@type'].endsWith('Membership'));
      const projects = await sysonRest('/projects');
      const project = projects.find(p => p['@id'] === projectId);
      if (project) {
        const counts = {};
        for (const el of nonMem) counts[el['@type']] = (counts[el['@type']] ?? 0) + 1;
        system += `\n\nActive project: "${project.name}" (ID: ${projectId})
Elements (${nonMem.length} total): ${JSON.stringify(counts)}
${nonMem.slice(0, 50).map(e => `  ${e['@type']} "${e.declaredName ?? e.name ?? '<unnamed>'}" [id:${e['@id']}] children:${(e.ownedElement ?? []).length}`).join('\n')}`;
      }
    } catch { /* proceed without context */ }
  }
```

- [ ] **Step 6: Update the mutating-tool check for onModelChanged**

In the SSE stream handler, update the `mutating` tools list. Find the line with:
```javascript
const mutating = ['create_element', 'create_project', 'create_local_element', 'delete_local_element'];
```

Replace with:
```javascript
const mutating = ['create_element', 'create_project', 'delete_element', 'create_diagram'];
```

- [ ] **Step 7: Replace the REST routes section**

Remove the diagram store API and SMAPS proxy routes sections (lines 520-566 in the original). Replace with:

```javascript
// ── SysON proxy routes ──────────────────────────────────────────────────────

app.get('/api/projects', async (req, res) => {
  try { res.json(await sysonRest('/projects')); }
  catch (e) { res.status(502).json({ error: e.message }); }
});

app.post('/api/projects', async (req, res) => {
  try {
    const result = await sysonGql(
      `mutation($input: CreateProjectInput!) { createProject(input: $input) { __typename ... on CreateProjectSuccessPayload { project { id } } ... on ErrorPayload { message } } }`,
      { input: { id: randomUUID(), name: req.body.name, natures: ['siriusComponents://nature?kind=siriusWeb'], templateId: 'sysmlv2-template' } },
    );
    const payload = result.createProject;
    if (payload.__typename === 'ErrorPayload') throw new Error(payload.message);
    res.json({ '@id': payload.project.id, name: req.body.name });
  } catch (e) { res.status(502).json({ error: e.message }); }
});

app.delete('/api/projects/:id', async (req, res) => {
  try {
    await sysonGql(
      `mutation($input: DeleteProjectInput!) { deleteProject(input: $input) { __typename ... on ErrorPayload { message } } }`,
      { input: { id: randomUUID(), projectId: req.params.id } },
    );
    ecCache.delete(req.params.id);
    res.json({ ok: true });
  } catch (e) { res.status(502).json({ error: e.message }); }
});

app.get('/api/projects/:id/elements', async (req, res) => {
  try { res.json(await getAllElements(req.params.id)); }
  catch (e) { res.status(502).json({ error: e.message }); }
});

app.get('/api/projects/:id/representations', async (req, res) => {
  try {
    const ecId = await getEditingContextId(req.params.id);
    const data = await sysonGql(
      `query($ecId: ID!) { viewer { editingContext(editingContextId: $ecId) { representations { edges { node { id label kind } } } } } }`,
      { ecId },
    );
    res.json(data.viewer.editingContext.representations.edges.map(e => e.node));
  } catch (e) { res.status(502).json({ error: e.message }); }
});

app.get('*', (req, res) => {
  res.sendFile(join(__dirname, 'dist', 'index.html'));
});
```

- [ ] **Step 8: Update the startup log**

Replace the `app.listen` block at the bottom:

```javascript
app.listen(PORT, () => {
  console.log(`\n  sysml-bridge dashboard  →  http://localhost:${PORT}`);
  console.log(`  SysON endpoint          →  ${SYSON}`);
  console.log(`  Anthropic API           →  ${process.env.ANTHROPIC_API_KEY ? 'ready' : '⚠  ANTHROPIC_API_KEY not set'}\n`);
});
```

- [ ] **Step 9: Verify server starts**

Run: `cd dashboard && node server.js &`
Expected: Server starts, prints SysON endpoint, no import errors.
Then: `kill %1`

- [ ] **Step 10: Commit**

```bash
git add dashboard/server.js
git commit -m "feat: replace SMAPS backend with SysON REST + GraphQL"
```

---

## Task 3: Update frontend API layer

**Files:**
- Modify: `dashboard/src/lib/api.ts`

- [ ] **Step 1: Replace api.ts**

Replace the entire content of `dashboard/src/lib/api.ts`:

```typescript
import type { SysONElement, Project, Representation } from '../types/sysml';

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
    body: JSON.stringify({ name }),
  });
}

export function deleteProject(id: string): Promise<unknown> {
  return apiFetch(`/api/projects/${id}`, { method: 'DELETE' });
}

export function getElements(projectId: string): Promise<SysONElement[]> {
  return apiFetch(`/api/projects/${projectId}/elements`);
}

export function getRepresentations(projectId: string): Promise<Representation[]> {
  return apiFetch(`/api/projects/${projectId}/representations`);
}
```

- [ ] **Step 2: Verify no TypeScript errors**

Run: `cd dashboard && npx tsc --noEmit 2>&1 | head -30`
Expected: Type errors in files that still import old types (SmapsElement, LocalElement, etc.) — these will be fixed in subsequent tasks. The api.ts itself should compile clean.

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/lib/api.ts
git commit -m "feat: update API layer for SysON endpoints"
```

---

## Task 4: Build ContainmentTree component

**Files:**
- Create: `dashboard/src/components/ContainmentTree.tsx`

- [ ] **Step 1: Write the ContainmentTree component**

```tsx
// dashboard/src/components/ContainmentTree.tsx
import { useState } from 'react';
import type { TreeNode } from '../lib/containment';

const TYPE_ICONS: Record<string, string> = {
  Package: '📦',
  PartDefinition: '🔧',
  PartUsage: '🔩',
  PortUsage: '⚡',
  ActionDefinition: '🎬',
  ActionUsage: '▶',
  StateDefinition: '🔄',
  StateUsage: '🔄',
  RequirementDefinition: '📋',
  RequirementUsage: '📝',
  ConnectionUsage: '🔗',
  ConnectionDefinition: '🔗',
  InterfaceDefinition: '🔌',
  InterfaceUsage: '🔌',
  ItemDefinition: '📎',
  ItemUsage: '📎',
  FlowConnectionUsage: '➡',
  AllocationUsage: '📐',
  ConstraintUsage: '⛓',
  DecisionNode: '◆',
  ForkNode: '⑂',
  JoinNode: '⑃',
  MergeNode: '◇',
  Documentation: '📝',
};

function getIcon(type: string): string {
  return TYPE_ICONS[type] ?? '○';
}

function TreeNodeRow({ node, depth }: { node: TreeNode; depth: number }) {
  const [expanded, setExpanded] = useState(depth < 2);
  const hasChildren = node.children.length > 0;
  const name = node.element.declaredName ?? node.element.name ?? '<unnamed>';
  const type = node.element['@type'];

  return (
    <>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '3px 8px',
          paddingLeft: depth * 18 + 8,
          cursor: hasChildren ? 'pointer' : 'default',
          borderRadius: 4,
          fontSize: 12,
        }}
        onClick={() => hasChildren && setExpanded(e => !e)}
      >
        <span style={{ width: 14, textAlign: 'center', fontSize: 10, color: 'var(--text4)', flexShrink: 0 }}>
          {hasChildren ? (expanded ? '▼' : '▶') : ''}
        </span>
        <span style={{ fontSize: 13, flexShrink: 0 }}>{getIcon(type)}</span>
        <span style={{ color: 'var(--text4)', fontSize: 10, fontFamily: 'monospace', flexShrink: 0 }}>{type}</span>
        <span style={{ color: 'var(--text)', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</span>
      </div>
      {expanded && node.children.map(child => (
        <TreeNodeRow key={child.element['@id']} node={child} depth={depth + 1} />
      ))}
    </>
  );
}

interface ContainmentTreeProps {
  roots: TreeNode[];
}

export function ContainmentTree({ roots }: ContainmentTreeProps) {
  if (!roots.length) {
    return (
      <div style={{ padding: 16, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No elements in this project yet.
      </div>
    );
  }

  return (
    <div style={{
      background: 'var(--bg)',
      border: '1px solid var(--border)',
      borderRadius: 8,
      padding: '8px 0',
      marginBottom: 24,
      maxHeight: 400,
      overflowY: 'auto',
    }}>
      {roots.map(root => (
        <TreeNodeRow key={root.element['@id']} node={root} depth={0} />
      ))}
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/components/ContainmentTree.tsx
git commit -m "feat: add ContainmentTree component"
```

---

## Task 5: Update ProjectDetail to use ContainmentTree

**Files:**
- Modify: `dashboard/src/components/ProjectDetail.tsx`

- [ ] **Step 1: Rewrite ProjectDetail**

Replace the entire content of `dashboard/src/components/ProjectDetail.tsx`:

```tsx
import { useState, useEffect, useCallback } from 'react';
import type { Project, SysONElement } from '../types/sysml';
import { getElements } from '../lib/api';
import { buildContainmentTree } from '../lib/containment';
import { ContainmentTree } from './ContainmentTree';
import { DiagramPanel } from './DiagramPanel';

interface ProjectDetailProps {
  project: Project;
  onBack: () => void;
  refreshKey: number;
}

export function ProjectDetail({ project, onBack, refreshKey }: ProjectDetailProps) {
  const [elements, setElements] = useState<SysONElement[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setElements(await getElements(project['@id']));
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => { load(); }, [load, refreshKey]);

  if (loading) return <div style={{ padding: 32, color: 'var(--text4)', fontSize: 12 }}>Loading model…</div>;

  const nonMembership = elements.filter(e => !e['@type'].endsWith('Membership'));
  const typeCounts: Record<string, number> = {};
  for (const e of nonMembership) {
    typeCounts[e['@type']] = (typeCounts[e['@type']] ?? 0) + 1;
  }

  const roots = buildContainmentTree(elements);

  return (
    <div style={{ padding: '24px 28px', overflowY: 'auto', height: '100%' }}>
      <div style={{ marginBottom: 20 }}>
        <button onClick={onBack} style={{ background: 'none', border: 'none', color: 'var(--text3)', cursor: 'pointer', fontSize: 11, display: 'flex', alignItems: 'center', gap: 5, padding: '4px 0', marginBottom: 6 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="m15 18-6-6 6-6"/></svg>
          All Projects
        </button>
        <div style={{ fontSize: 16, fontWeight: 700, color: '#f1f5f9' }}>{project.name}</div>
        <div style={{ fontSize: 10, fontFamily: 'monospace', color: 'var(--text4)', marginTop: 3 }}>{project['@id'].slice(0, 8)}…</div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 10, marginBottom: 22 }}>
        {[
          { label: 'Elements', value: nonMembership.length, sub: 'in SysON' },
          { label: 'Types', value: Object.keys(typeCounts).length, sub: 'unique @types' },
          { label: 'Depth', value: roots.length, sub: 'root nodes' },
        ].map(s => (
          <div key={s.label} style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 14 }}>
            <div style={{ fontSize: 10, color: 'var(--text3)', textTransform: 'uppercase', letterSpacing: '0.07em', marginBottom: 5 }}>{s.label}</div>
            <div style={{ fontSize: 26, fontWeight: 700, color: '#f1f5f9' }}>{s.value}</div>
            <div style={{ fontSize: 10, color: 'var(--text4)', marginTop: 3, fontFamily: 'monospace' }}>{s.sub}</div>
          </div>
        ))}
      </div>

      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Element Types</div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))', gap: 7, marginBottom: 22 }}>
        {Object.entries(typeCounts).sort((a, b) => b[1] - a[1]).map(([type, count]) => (
          <div key={type} style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 6, padding: '8px 12px', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span style={{ color: 'var(--text2)', fontSize: 11 }}>{type}</span>
            <span style={{ color: 'var(--primary-text)', fontWeight: 700, fontFamily: 'monospace', fontSize: 11 }}>{count}</span>
          </div>
        ))}
      </div>

      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Diagrams</div>
      <DiagramPanel projectId={project['@id']} elements={elements} />

      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Containment Tree</div>
      <ContainmentTree roots={roots} />
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/components/ProjectDetail.tsx
git commit -m "feat: replace flat elements table with containment tree"
```

---

## Task 6: Update DiagramPanel — SysON iframes + React Flow IBD, remove Mermaid

**Files:**
- Modify: `dashboard/src/components/DiagramPanel.tsx`

- [ ] **Step 1: Rewrite DiagramPanel**

Replace the entire content of `dashboard/src/components/DiagramPanel.tsx`:

```tsx
import { useState, useEffect } from 'react';
import { IBDViewer } from './IBDViewer';
import { getRepresentations } from '../lib/api';
import type { SysONElement, Representation } from '../types/sysml';

interface DiagramPanelProps {
  projectId: string;
  elements: SysONElement[];
}

export function DiagramPanel({ projectId, elements }: DiagramPanelProps) {
  const [activeTab, setActiveTab] = useState(0);
  const [representations, setRepresentations] = useState<Representation[]>([]);

  useEffect(() => {
    getRepresentations(projectId).then(setRepresentations).catch(() => setRepresentations([]));
  }, [projectId]);

  const hasPortUsage = elements.some(e => e['@type'] === 'PortUsage');

  type Tab =
    | { key: string; label: string; kind: 'syson'; repId: string }
    | { key: string; label: string; kind: 'ibd' };

  const tabs: Tab[] = [
    ...representations.map(r => ({
      key: r.id,
      label: r.label,
      kind: 'syson' as const,
      repId: r.id,
    })),
    ...(hasPortUsage ? [{ key: 'ibd-rf', label: 'IBD (React Flow)', kind: 'ibd' as const }] : []),
  ];

  const safeIdx = Math.min(activeTab, Math.max(tabs.length - 1, 0));
  const activeTab_ = tabs[safeIdx];

  if (!tabs.length) {
    return (
      <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No diagrams yet. Ask the assistant to create a diagram view.
      </div>
    );
  }

  const sysonBase = import.meta.env.VITE_SYSON_URL ?? 'http://localhost:8080';

  return (
    <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22 }}>
      <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap', marginBottom: 12 }}>
        {tabs.map((t, i) => (
          <button
            key={t.key}
            onClick={() => setActiveTab(i)}
            style={{
              padding: '4px 11px', borderRadius: 5,
              border: '1px solid', borderColor: i === safeIdx ? 'var(--primary)' : 'var(--border)',
              background: i === safeIdx ? 'var(--primary-dim)' : 'transparent',
              color: i === safeIdx ? 'var(--primary-text)' : 'var(--text3)',
              fontSize: 11, cursor: 'pointer',
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      {activeTab_?.kind === 'syson' && (
        <iframe
          src={`${sysonBase}/projects/${projectId}/edit/${(activeTab_ as Extract<Tab, { kind: 'syson' }>).repId}`}
          style={{
            width: '100%', height: 480, border: '1px solid var(--border)',
            borderRadius: 6, background: '#1a1a2e',
          }}
          title={activeTab_.label}
        />
      )}
      {activeTab_?.kind === 'ibd' && <IBDViewer projectId={projectId} />}
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/components/DiagramPanel.tsx
git commit -m "feat: replace Mermaid with SysON iframe diagrams + React Flow IBD"
```

---

## Task 7: Update IBDViewer and ibd-transformer for SysON elements

**Files:**
- Modify: `dashboard/src/lib/ibd-transformer.ts`
- Modify: `dashboard/src/components/IBDViewer.tsx`
- Modify: `dashboard/src/lib/__tests__/ibd-transformer.test.ts`

- [ ] **Step 1: Update test fixtures to use SysON element shapes**

Replace the entire content of `dashboard/src/lib/__tests__/ibd-transformer.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { transformToIBD } from '../ibd-transformer';
import type { SysONElement } from '../../types/sysml';

const BLOCK_A: SysONElement = {
  '@id': 'block-a', '@type': 'PartDefinition', declaredName: 'Battery',
  ownedElement: [{ '@id': 'mem-port-out' }], owner: { '@id': 'pkg' },
};
const MEM_PORT_OUT: SysONElement = {
  '@id': 'mem-port-out', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'port-out' }], owner: { '@id': 'block-a' },
};
const PORT_OUT: SysONElement = {
  '@id': 'port-out', '@type': 'PortUsage', declaredName: 'pwr_out',
  ownedElement: [], owner: { '@id': 'mem-port-out' },
};

const BLOCK_B: SysONElement = {
  '@id': 'block-b', '@type': 'PartDefinition', declaredName: 'Motor',
  ownedElement: [{ '@id': 'mem-port-in' }], owner: { '@id': 'pkg' },
};
const MEM_PORT_IN: SysONElement = {
  '@id': 'mem-port-in', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'port-in' }], owner: { '@id': 'block-b' },
};
const PORT_IN: SysONElement = {
  '@id': 'port-in', '@type': 'PortUsage', declaredName: 'pwr_in',
  ownedElement: [], owner: { '@id': 'mem-port-in' },
};

const MEM_CONN: SysONElement = {
  '@id': 'mem-conn', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'conn-1' }], owner: { '@id': 'block-a' },
};
const CONN: SysONElement = {
  '@id': 'conn-1', '@type': 'ConnectionUsage', declaredName: 'powerLine',
  ownedElement: [], owner: { '@id': 'mem-conn' },
  connectorEnd: [
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-out' } },
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-in' } },
  ],
};

const ALL = [BLOCK_A, MEM_PORT_OUT, PORT_OUT, BLOCK_B, MEM_PORT_IN, PORT_IN, MEM_CONN, CONN];

describe('transformToIBD', () => {
  it('creates one node per PartDefinition', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B] as any);
    expect(nodes).toHaveLength(2);
    expect(nodes[0].data.name).toBe('Battery');
    expect(nodes[1].data.name).toBe('Motor');
  });

  it('ignores non-PartDefinition elements', () => {
    const req: SysONElement = { '@id': 'req-1', '@type': 'RequirementDefinition', declaredName: 'R1' };
    const { nodes } = transformToIBD([BLOCK_A, req] as any);
    expect(nodes).toHaveLength(1);
  });

  it('assigns PortUsage to its owning block (resolving through membership)', () => {
    const { nodes } = transformToIBD(ALL as any);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports).toHaveLength(1);
    expect(battery.data.ports[0].name).toBe('pwr_out');
    expect(motor.data.ports).toHaveLength(1);
    expect(motor.data.ports[0].name).toBe('pwr_in');
  });

  it('places source port on right, target port on left', () => {
    const { nodes } = transformToIBD(ALL as any);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports[0].position).toBe('right');
    expect(motor.data.ports[0].position).toBe('left');
  });

  it('creates one edge per ConnectionUsage', () => {
    const { edges } = transformToIBD(ALL as any);
    expect(edges).toHaveLength(1);
    expect(edges[0].source).toBe('block-a');
    expect(edges[0].target).toBe('block-b');
    expect(edges[0].sourceHandle).toBe('port-out');
    expect(edges[0].targetHandle).toBe('port-in');
    expect(edges[0].label).toBe('powerLine');
  });

  it('returns node type sysmlBlock', () => {
    const { nodes } = transformToIBD([BLOCK_A] as any);
    expect(nodes[0].type).toBe('sysmlBlock');
  });
});
```

- [ ] **Step 2: Run tests to see them fail**

Run: `cd dashboard && npx vitest run src/lib/__tests__/ibd-transformer.test.ts`
Expected: FAIL — transformer still imports SmapsElement/LocalElement types

- [ ] **Step 3: Rewrite ibd-transformer for SysON elements**

Replace the entire content of `dashboard/src/lib/ibd-transformer.ts`:

```typescript
import type { Node, Edge } from '@xyflow/react';
import type { SysONElement } from '../types/sysml';

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

function resolveLogicalOwner(el: SysONElement, byId: Map<string, SysONElement>): string | undefined {
  let current = el.owner?.['@id'];
  while (current) {
    const owner = byId.get(current);
    if (!owner) return undefined;
    if (!owner['@type'].endsWith('Membership')) return owner['@id'];
    current = owner.owner?.['@id'];
  }
  return undefined;
}

export function transformToIBD(
  elements: SysONElement[],
): { nodes: Node<SysMLBlockNodeData>[]; edges: Edge[] } {
  const byId = new Map<string, SysONElement>();
  for (const el of elements) byId.set(el['@id'], el);

  const partDefs = elements.filter(e => e['@type'] === 'PartDefinition');
  const portUsages = elements.filter(e => e['@type'] === 'PortUsage');
  const connections = elements.filter(e => e['@type'] === 'ConnectionUsage');

  const sourcePorts = new Set(
    connections
      .map(c => (c as any).connectorEnd?.[0]?.connectedFeature?.['@id'])
      .filter((id): id is string => !!id),
  );
  const targetPorts = new Set(
    connections
      .map(c => (c as any).connectorEnd?.[1]?.connectedFeature?.['@id'])
      .filter((id): id is string => !!id),
  );

  const blockIds = new Set(partDefs.map(b => b['@id']));

  const nodes: Node<SysMLBlockNodeData>[] = partDefs.map(block => {
    const blockPorts = portUsages.filter(p => resolveLogicalOwner(p, byId) === block['@id']);
    const ports: PortHandle[] = blockPorts.map(p => ({
      id: p['@id'],
      name: p.declaredName ?? p.name ?? p['@id'].slice(0, 8),
      position: targetPorts.has(p['@id']) && !sourcePorts.has(p['@id']) ? 'left' : 'right',
    }));
    return {
      id: block['@id'],
      type: 'sysmlBlock',
      position: { x: 0, y: 0 },
      data: {
        stereotype: 'part def',
        name: block.declaredName ?? block.name ?? block['@id'].slice(0, 8),
        ports,
      },
    };
  });

  const edges: Edge[] = connections
    .filter(c => (c as any).connectorEnd?.[0] && (c as any).connectorEnd?.[1])
    .map(c => {
      const srcPortId = (c as any).connectorEnd[0].connectedFeature['@id'];
      const tgtPortId = (c as any).connectorEnd[1].connectedFeature['@id'];
      const srcPort = portUsages.find(p => p['@id'] === srcPortId);
      const tgtPort = portUsages.find(p => p['@id'] === tgtPortId);
      const srcBlock = srcPort ? resolveLogicalOwner(srcPort, byId) : undefined;
      const tgtBlock = tgtPort ? resolveLogicalOwner(tgtPort, byId) : undefined;
      if (!srcBlock || !tgtBlock || !blockIds.has(srcBlock) || !blockIds.has(tgtBlock)) return null;
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

- [ ] **Step 4: Update IBDViewer**

Replace the entire content of `dashboard/src/components/IBDViewer.tsx`:

```tsx
import { useEffect, useCallback } from 'react';
import {
  ReactFlow, Background, Controls,
  useNodesState, useEdgesState,
  type Node, type Edge,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { SysMLBlockNode } from './sysml/SysMLBlockNode';
import { SysMLEdge } from './sysml/SysMLEdge';
import { transformToIBD, type SysMLBlockNodeData } from '../lib/ibd-transformer';
import { applyELKLayout } from '../lib/ibd-layout';
import { getElements } from '../lib/api';

type SysMLNode = Node<SysMLBlockNodeData>;

const nodeTypes = { sysmlBlock: SysMLBlockNode };
const edgeTypes = { sysmlEdge: SysMLEdge };

interface IBDViewerProps {
  projectId: string;
}

export function IBDViewer({ projectId }: IBDViewerProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<SysMLNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  const load = useCallback(async () => {
    try {
      const elements = await getElements(projectId);
      const { nodes: rawNodes, edges: rawEdges } = transformToIBD(elements);
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

- [ ] **Step 5: Run tests**

Run: `cd dashboard && npx vitest run`
Expected: All tests pass (both containment and ibd-transformer)

- [ ] **Step 6: Commit**

```bash
git add dashboard/src/lib/ibd-transformer.ts dashboard/src/components/IBDViewer.tsx dashboard/src/lib/__tests__/ibd-transformer.test.ts
git commit -m "feat: update IBD transformer and viewer for SysON element shapes"
```

---

## Task 8: Update Sidebar and App for SysON

**Files:**
- Modify: `dashboard/src/components/Sidebar.tsx`
- Modify: `dashboard/src/App.tsx`

- [ ] **Step 1: Update Sidebar modal description**

In `dashboard/src/components/Sidebar.tsx`, find:
```tsx
<div style={{ fontSize: 11.5, color: 'var(--text3)', marginBottom: 18, lineHeight: 1.5 }}>Creates a new project in the SMAPS repository.</div>
```

Replace with:
```tsx
<div style={{ fontSize: 11.5, color: 'var(--text3)', marginBottom: 18, lineHeight: 1.5 }}>Creates a new SysML v2 project in SysON.</div>
```

- [ ] **Step 2: Update App.tsx**

In `dashboard/src/App.tsx`:

1. Find the header status label:
```tsx
<span style={{ fontSize: 11, color: 'var(--text3)', fontFamily: 'monospace' }}>localhost:9000</span>
```
Replace with:
```tsx
<span style={{ fontSize: 11, color: 'var(--text3)', fontFamily: 'monospace' }}>SysON :8080</span>
```

2. Find the empty-state subtitle:
```tsx
<div style={{ fontSize: 12, color: 'var(--text3)', maxWidth: 260, lineHeight: 1.6 }}>Create your first SysML v2 project to start modeling.</div>
```
No change needed — this text is generic enough.

3. Find the "Your Models" subtitle:
```tsx
<div style={{ fontSize: 12, color: 'var(--text3)', marginTop: 2 }}>SysML v2 projects in SMAPS</div>
```
Replace with:
```tsx
<div style={{ fontSize: 12, color: 'var(--text3)', marginTop: 2 }}>SysML v2 projects in SysON</div>
```

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/components/Sidebar.tsx dashboard/src/App.tsx
git commit -m "feat: update Sidebar and App references from SMAPS to SysON"
```

---

## Task 9: Update ChatPanel tool labels

**Files:**
- Modify: `dashboard/src/components/ChatPanel.tsx`

- [ ] **Step 1: Update TOOL_LABELS**

In `dashboard/src/components/ChatPanel.tsx`, find:
```typescript
const TOOL_LABELS: Record<string, string> = {
  query_elements: 'Queried elements', create_element: 'Created element',
  create_local_element: 'Created local element', query_local_elements: 'Queried local elements',
  delete_local_element: 'Deleted local element', render_diagram: 'Rendered diagram',
  export_sysml: 'Exported SysML', create_project: 'Created project',
};
```

Replace with:
```typescript
const TOOL_LABELS: Record<string, string> = {
  query_elements: 'Queried elements',
  create_element: 'Created element',
  delete_element: 'Deleted element',
  create_diagram: 'Created diagram',
  export_sysml: 'Exported SysML',
  create_project: 'Created project',
};
```

- [ ] **Step 2: Update the mutating tools check**

Find:
```typescript
const mutating = ['create_element', 'create_project', 'create_local_element', 'delete_local_element'];
if (tools.some(t => mutating.includes(t.name) && t.status === 'done') ||
    tools.some(t => t.name === 'render_diagram' && t.status === 'done')) {
```

Replace with:
```typescript
const mutating = ['create_element', 'create_project', 'delete_element', 'create_diagram'];
if (tools.some(t => mutating.includes(t.name) && t.status === 'done')) {
```

- [ ] **Step 3: Update the quick-action suggestions**

Find:
```tsx
{['List all elements in this project', 'Add a new PartDefinition called Sensor', 'Export this model as SysML v2 text'].map(q => (
```

Replace with:
```tsx
{['List all elements in this project', 'Create a PartDefinition called Sensor', 'Create a General View diagram for this project'].map(q => (
```

- [ ] **Step 4: Commit**

```bash
git add dashboard/src/components/ChatPanel.tsx
git commit -m "feat: update ChatPanel tool labels and suggestions for SysON"
```

---

## Task 10: Remove Mermaid dependency and local data files

**Files:**
- Modify: `dashboard/package.json`
- Remove: `dashboard/data/*.json`

- [ ] **Step 1: Remove mermaid from package.json**

In `dashboard/package.json`, remove the line:
```json
    "mermaid": "^11.4.0",
```

- [ ] **Step 2: Remove mermaid from node_modules**

Run: `cd dashboard && npm uninstall mermaid`

- [ ] **Step 3: Remove local data files**

Run: `rm -f dashboard/data/*.json && rmdir dashboard/data 2>/dev/null; true`

- [ ] **Step 4: Verify build**

Run: `cd dashboard && npx tsc --noEmit && npx vitest run`
Expected: No TypeScript errors, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A dashboard/package.json dashboard/package-lock.json
git rm -r --cached dashboard/data/ 2>/dev/null; true
git commit -m "chore: remove mermaid dependency and local data files"
```

---

## Task 11: End-to-end verification

**Files:** None (verification only)

- [ ] **Step 1: Start SysON (if not running)**

Run: `docker compose up -d syson` (or however SysON is started in this project)
Verify: `curl -s http://localhost:8080/api/rest/projects | head -1` returns JSON

- [ ] **Step 2: Build and start the dashboard**

Run: `cd dashboard && npm run build && node server.js &`
Expected: Console shows `SysON endpoint → http://localhost:8080`

- [ ] **Step 3: Verify project listing**

Run: `curl -s http://localhost:6121/api/projects | python3 -m json.tool | head -10`
Expected: JSON array of SysON projects

- [ ] **Step 4: Verify elements endpoint**

Run: `curl -s "http://localhost:6121/api/projects/{projectId}/elements" | python3 -c "import sys,json; elements=json.load(sys.stdin); print(f'{len(elements)} elements'); [print(f'  {e[\"@type\"]}: {e.get(\"declaredName\",\"?\")}') for e in elements[:5] if not e['@type'].endswith('Membership')]"`
Expected: Element list with containment data (ownedElement arrays populated)

- [ ] **Step 5: Verify representations endpoint**

Run: `curl -s "http://localhost:6121/api/projects/{projectId}/representations" | python3 -m json.tool`
Expected: JSON array of diagram representations (or empty array)

- [ ] **Step 6: Verify chat tools work**

Run: `curl -s -X POST http://localhost:6121/api/chat -H 'Content-Type: application/json' -d '{"messages":[{"role":"user","content":"list all elements"}],"projectId":"{projectId}"}' | head -20`
Expected: SSE stream with tool_start/tool_done events for query_elements

- [ ] **Step 7: Open in browser and verify**

Open `http://localhost:5173` (or `:6121` for built version):
1. Status dot should be green (SysON online)
2. Sidebar should list SysON projects
3. Click a project — should show stats, type chips, containment tree
4. Diagrams section should show SysON iframe tabs (if representations exist) and React Flow IBD tab (if PortUsage elements exist)
5. Chat should work with updated tool labels

- [ ] **Step 8: Stop server**

Run: `kill %1`
