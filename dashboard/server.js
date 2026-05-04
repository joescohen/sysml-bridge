import express from 'express';
import Anthropic from '@anthropic-ai/sdk';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import { readFileSync, existsSync, writeFileSync } from 'fs';
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
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`SysON GraphQL → ${res.status}: ${text}`);
  }
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
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`SysON commit → ${res.status}: ${text}`);
  }
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

// ── Tool definitions ──────────────────────────────────────────────────────────

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
      const repId = rep.representation.id;

      // Auto-populate: find elements whose logical owner (first non-Membership ancestor) is element_id
      try {
        const allElements = await getAllElements(projectId);
        const byId = new Map(allElements.map(e => [e['@id'], e]));
        const SKIP_TYPES = new Set(['FeatureTyping','Subsetting','Redefinition','ReferenceSubsetting','MembershipExpose','FeatureInverting','TypeFeaturing']);

        function logicalOwner(el) {
          let cur = el.owner?.['@id'];
          const seen = new Set();
          while (cur) {
            if (seen.has(cur)) return undefined;
            seen.add(cur);
            const owner = byId.get(cur);
            if (!owner) return undefined;
            if (!owner['@type'].endsWith('Membership')) return owner['@id'];
            cur = owner.owner?.['@id'];
          }
          return undefined;
        }

        const toDrop = allElements
          .filter(e => !e['@type'].endsWith('Membership') && !SKIP_TYPES.has(e['@type']))
          .filter(e => logicalOwner(e) === input.element_id)
          .map(e => e['@id']);

        if (toDrop.length) {
          await sysonGql(
            `mutation($input: DropOnDiagramInput!) { dropOnDiagram(input: $input) { __typename ... on DropOnDiagramSuccessPayload { diagram { id } } ... on ErrorPayload { message } } }`,
            { input: { id: randomUUID(), editingContextId: ecId, representationId: repId, diagramTargetElementId: repId, objectIds: toDrop, startingPositionX: 50, startingPositionY: 50 } },
          );
        }
      } catch (dropErr) {
        console.error('dropOnDiagram failed (non-fatal):', dropErr.message);
      }

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
          // skip root package
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

// ── Chat endpoint ─────────────────────────────────────────────────────────────

app.post('/api/chat', async (req, res) => {
  const { messages, projectId } = req.body;

  if (!process.env.ANTHROPIC_API_KEY) {
    res.status(500).json({ error: 'ANTHROPIC_API_KEY not set. Add it to sysml-bridge/.env and restart the server.' });
    return;
  }

  res.setHeader('Content-Type', 'text/event-stream');
  res.setHeader('Cache-Control', 'no-cache');
  res.setHeader('Connection', 'keep-alive');

  const sse = data => res.write(`data: ${JSON.stringify(data)}\n\n`);
  const anthropic = new Anthropic();

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

  try {
    let allMessages = messages.map(m => ({ role: m.role, content: m.content }));

    while (true) {
      const stream = anthropic.messages.stream({
        model: 'claude-haiku-4-5-20251001',
        max_tokens: 4096,
        system,
        messages: allMessages,
        tools: projectId ? TOOLS : [],
      });

      stream.on('streamEvent', event => {
        if (event.type === 'content_block_start' && event.content_block.type === 'tool_use') {
          sse({ type: 'tool_start', id: event.content_block.id, name: event.content_block.name });
        }
      });

      stream.on('text', text => sse({ type: 'text', text }));

      const message = await stream.finalMessage();
      allMessages.push({ role: 'assistant', content: message.content });

      if (message.stop_reason !== 'tool_use') break;

      const toolUses    = message.content.filter(b => b.type === 'tool_use');
      const toolResults = [];

      for (const tu of toolUses) {
        sse({ type: 'tool_running', id: tu.id, name: tu.name });
        const result = await executeTool(tu.name, tu.input, projectId);
        sse({ type: 'tool_done', id: tu.id, name: tu.name, result });
        toolResults.push({ type: 'tool_result', tool_use_id: tu.id, content: JSON.stringify(result) });
      }

      allMessages.push({ role: 'user', content: toolResults });
    }

    sse({ type: 'done' });
    res.end();
  } catch (e) {
    sse({ type: 'error', message: e.message });
    res.end();
  }
});

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
    const result = await sysonGql(
      `mutation($input: DeleteProjectInput!) { deleteProject(input: $input) { __typename ... on ErrorPayload { message } } }`,
      { input: { id: randomUUID(), projectId: req.params.id } },
    );
    if (result.deleteProject.__typename === 'ErrorPayload') throw new Error(result.deleteProject.message);
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

const TOPOLOGY_PATH = join(__dirname, '..', 'topology.json');

function loadTopology() {
  try { return existsSync(TOPOLOGY_PATH) ? JSON.parse(readFileSync(TOPOLOGY_PATH, 'utf8')) : {}; }
  catch { return {}; }
}

app.get('/api/projects/:id/topology', (req, res) => {
  const topo = loadTopology();
  res.json(topo[req.params.id] ?? { edges: [] });
});

app.post('/api/projects/:id/topology/edges', (req, res) => {
  const { id: projectId } = req.params;
  const edge = req.body; // { id, label, sourcePort, targetPort }
  const topo = loadTopology();
  if (!topo[projectId]) topo[projectId] = { edges: [] };
  topo[projectId].edges = topo[projectId].edges.filter(e => e.id !== edge.id);
  topo[projectId].edges.push(edge);
  writeFileSync(TOPOLOGY_PATH, JSON.stringify(topo, null, 2));
  res.json({ ok: true });
});

app.get('*', (req, res) => {
  res.sendFile(join(__dirname, 'dist', 'index.html'));
});

app.listen(PORT, () => {
  console.log(`\n  sysml-bridge dashboard  →  http://localhost:${PORT}`);
  console.log(`  SysON endpoint          →  ${SYSON}`);
  console.log(`  Anthropic API           →  ${process.env.ANTHROPIC_API_KEY ? 'ready' : '⚠  ANTHROPIC_API_KEY not set'}\n`);
});
