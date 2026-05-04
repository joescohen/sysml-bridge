import express from 'express';
import Anthropic from '@anthropic-ai/sdk';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs';
import { randomUUID } from 'crypto';

// Load .env from project root if present
const __root = join(dirname(fileURLToPath(import.meta.url)), '..');
const envPath = join(__root, '.env');
if (existsSync(envPath)) {
  for (const line of readFileSync(envPath, 'utf8').split('\n')) {
    const m = line.match(/^\s*([A-Z_][A-Z0-9_]*)=(.*)$/);
    if (m && !process.env[m[1]]) process.env[m[1]] = m[2].replace(/^['"]|['"]$/g, '');
  }
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const DATA_DIR  = join(__dirname, 'data');
if (!existsSync(DATA_DIR)) mkdirSync(DATA_DIR, { recursive: true });

const app  = express();
const PORT = parseInt(process.env.PORT ?? '6121', 10);
const SMAPS = process.env.SMAPS_ENDPOINT ?? 'http://localhost:9000';

app.use(express.json());
app.use(express.static(join(__dirname, 'dist')));

// ── Diagram store (persisted per project in data/<id>.json) ──────────────────

function loadProjectData(projectId) {
  const f = join(DATA_DIR, `${projectId}.json`);
  try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return { diagrams: [] }; }
}

function saveProjectData(projectId, data) {
  writeFileSync(join(DATA_DIR, `${projectId}.json`), JSON.stringify(data, null, 2));
}

function loadLocalElements(projectId) {
  return loadProjectData(projectId).localElements ?? [];
}

function saveLocalElement(projectId, element) {
  const data = loadProjectData(projectId);
  if (!data.localElements) data.localElements = [];
  const idx = data.localElements.findIndex(e => e['@id'] === element['@id']);
  if (idx >= 0) data.localElements[idx] = element;
  else data.localElements.push(element);
  saveProjectData(projectId, data);
  return element;
}

function deleteLocalElement(projectId, elementId) {
  const data = loadProjectData(projectId);
  data.localElements = (data.localElements ?? []).filter(e => e['@id'] !== elementId);
  saveProjectData(projectId, data);
}

// ── SMAPS helpers ─────────────────────────────────────────────────────────────

async function smapsFetch(path, method = 'GET', body = null) {
  const opts = { method, headers: { 'Content-Type': 'application/json' } };
  if (body) opts.body = JSON.stringify(body);
  const res = await fetch(`${SMAPS}${path}`, opts);
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`SMAPS ${method} ${path} → ${res.status}: ${text}`);
  }
  return res.json();
}

async function getHeadCommitId(projectId) {
  const commits = await smapsFetch(`/projects/${projectId}/commits`).catch(() => []);
  return commits[0]?.['@id'] ?? null;
}

async function queryElements(projectId) {
  const headId = await getHeadCommitId(projectId);
  if (!headId) return [];
  const res = await fetch(`${SMAPS}/projects/${projectId}/query-results?commitId=${headId}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ '@type': 'Query' }),
  });
  return res.ok ? res.json() : [];
}

async function createCommit(projectId, elementData) {
  const headId = await getHeadCommitId(projectId);
  const payload = {
    '@type': 'Commit',
    change: [{ '@type': 'DataVersion', payload: elementData }],
  };
  if (headId) payload.previousCommit = [{ '@id': headId }];
  const commit = await smapsFetch(`/projects/${projectId}/commits`, 'POST', payload);
  const changes = await smapsFetch(`/projects/${projectId}/commits/${commit['@id']}/changes`);
  return { commit, element: changes[0]?.payload };
}

// ── Tool definitions ──────────────────────────────────────────────────────────

const TOOLS = [
  {
    name: 'query_elements',
    description: 'Query all SysML elements in the current project. Optionally filter by type.',
    input_schema: {
      type: 'object',
      properties: {
        type_filter: {
          type: 'string',
          description: 'Filter by SysML type: PartDefinition or RequirementDefinition',
        },
      },
    },
  },
  {
    name: 'create_element',
    description: 'Create a new SysML element. IMPORTANT: Only PartDefinition and RequirementDefinition are supported by this SMAPS API version. Do NOT attempt other types.',
    input_schema: {
      type: 'object',
      required: ['element_type', 'name'],
      properties: {
        element_type: {
          type: 'string',
          enum: ['PartDefinition', 'RequirementDefinition'],
          description: 'Must be PartDefinition or RequirementDefinition — those are the only types this API supports.',
        },
        name: { type: 'string', description: 'Declared name of the element' },
        short_name: { type: 'string', description: 'Short ID, e.g. SYS-004' },
      },
    },
  },
  {
    name: 'render_diagram',
    description: 'Render and save a Mermaid diagram for this project (IBD, BDD, activity, sequence, state, etc). The diagram is stored and displayed in the dashboard. Use Mermaid syntax — graph TD/LR for IBDs, classDiagram for BDDs, sequenceDiagram, stateDiagram-v2, etc.',
    input_schema: {
      type: 'object',
      required: ['diagram_type', 'title', 'mermaid_code'],
      properties: {
        diagram_type: {
          type: 'string',
          description: 'Type label shown in the dashboard, e.g. "IBD", "BDD", "Activity", "Sequence", "State"',
        },
        title: {
          type: 'string',
          description: 'Human-readable title for this diagram, e.g. "Drone [ibd]"',
        },
        mermaid_code: {
          type: 'string',
          description: 'Complete valid Mermaid diagram code. For IBDs use graph TD or LR with subgraph for the container block and labelled arrows for connectors. Must be renderable by Mermaid v11.',
        },
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
    description: 'Create a new SysML project in the SMAPS repository.',
    input_schema: {
      type: 'object',
      required: ['name'],
      properties: { name: { type: 'string', description: 'Project name' } },
    },
  },
  {
    name: 'create_local_element',
    description: 'Create a SysML v2 element stored locally in the dashboard (not sent to SMAPS). Use this for any type SMAPS does not support: ProxyPortDefinition, ProxyPortUsage, PartUsage, ConnectionUsage, ConnectionDefinition, InterfaceDefinition, InterfaceUsage, etc. Local elements appear in the All Elements table and can be referenced by their @id in other local elements.',
    input_schema: {
      type: 'object',
      required: ['element_type', 'name'],
      properties: {
        element_type: {
          type: 'string',
          description: 'SysML v2 @type — e.g. ProxyPortDefinition, ProxyPortUsage, PartUsage, ConnectionUsage, ConnectionDefinition, InterfaceDefinition',
        },
        name: { type: 'string', description: 'declaredName of the element' },
        short_name: { type: 'string', description: 'declaredShortName / short ID, e.g. PP-001' },
        owner_id: { type: 'string', description: 'ID of the owning element (use the SMAPS @id for a block, or local @id for a locally created element)' },
        type_id: { type: 'string', description: 'ID of the type definition this usage references (e.g. ProxyPortDefinition @id for a ProxyPortUsage)' },
        source_id: { type: 'string', description: 'Source port/feature @id for ConnectionUsage (first connector end)' },
        target_id: { type: 'string', description: 'Target port/feature @id for ConnectionUsage (second connector end)' },
      },
    },
  },
  {
    name: 'query_local_elements',
    description: 'Query elements stored locally in the dashboard (created with create_local_element). Returns ProxyPortDefinition, ProxyPortUsage, PartUsage, ConnectionUsage, and any other locally created SysML v2 types.',
    input_schema: {
      type: 'object',
      properties: {
        type_filter: { type: 'string', description: 'Filter by SysML v2 type, e.g. ProxyPortUsage' },
      },
    },
  },
  {
    name: 'delete_local_element',
    description: 'Delete a locally stored SysML element by its @id.',
    input_schema: {
      type: 'object',
      required: ['element_id'],
      properties: {
        element_id: { type: 'string', description: 'The @id of the local element to delete' },
      },
    },
  },
];

async function executeTool(name, input, projectId) {
  try {
    if (name === 'query_elements') {
      const elements = await queryElements(projectId);
      const filtered = input.type_filter
        ? elements.filter(e => e['@type'] === input.type_filter)
        : elements;
      return {
        count: filtered.length,
        elements: filtered.map(e => ({
          id: e['@id'],
          type: e['@type'],
          name: e.declaredName ?? e.name ?? '<unnamed>',
          shortName: e.declaredShortName,
        })),
      };
    }

    if (name === 'create_element') {
      if (!['PartDefinition', 'RequirementDefinition'].includes(input.element_type)) {
        return { error: `Unsupported type "${input.element_type}". Only PartDefinition and RequirementDefinition work in this SMAPS API version.` };
      }
      const elementData = { '@type': input.element_type, declaredName: input.name };
      if (input.short_name) elementData.declaredShortName = input.short_name;
      const { element } = await createCommit(projectId, elementData);
      return {
        success: true,
        element: { id: element?.['@id'], type: element?.['@type'], name: element?.declaredName },
      };
    }

    if (name === 'render_diagram') {
      const data = loadProjectData(projectId);
      // Replace existing diagram of same type+title, or append
      const idx = data.diagrams.findIndex(d => d.type === input.diagram_type && d.title === input.title);
      const entry = { type: input.diagram_type, title: input.title, mermaid: input.mermaid_code, updatedAt: new Date().toISOString() };
      if (idx >= 0) data.diagrams[idx] = entry;
      else data.diagrams.push(entry);
      saveProjectData(projectId, data);
      return { success: true, diagram: { type: input.diagram_type, title: input.title } };
    }

    if (name === 'export_sysml') {
      const elements = await queryElements(projectId);
      const local    = loadLocalElements(projectId);
      const projects = await smapsFetch('/projects');
      const project  = projects.find(p => p['@id'] === projectId);
      const partDefs = elements.filter(e => e['@type'] === 'PartDefinition');
      const reqDefs  = elements.filter(e => e['@type'] === 'RequirementDefinition');
      const portDefs = local.filter(e => e['@type'] === 'ProxyPortDefinition');
      const portUsages = local.filter(e => e['@type'] === 'ProxyPortUsage');
      const connections = local.filter(e => e['@type'] === 'ConnectionUsage');
      const otherLocal = local.filter(e => !['ProxyPortDefinition','ProxyPortUsage','ConnectionUsage'].includes(e['@type']));

      const lines = [`package ${project?.name ?? 'Model'} {`, ''];
      for (const p of partDefs) lines.push(`    part def ${p.declaredName ?? p['@id']};`);
      if (partDefs.length) lines.push('');
      for (const p of portDefs) lines.push(`    port def ${p.declaredName ?? p['@id']};`);
      if (portDefs.length) lines.push('');
      if (reqDefs.length) {
        for (const r of reqDefs) {
          const id = r.declaredShortName ? ` <'${r.declaredShortName}'>` : '';
          lines.push(`    requirement def${id} ${r.declaredName ?? r['@id']} {`);
          lines.push('        doc /* requirement */');
          lines.push('    }');
          lines.push('');
        }
      }
      if (portUsages.length || connections.length || otherLocal.length) {
        lines.push('    // — Local elements (not yet in SMAPS) —');
        for (const e of portUsages) {
          const owner = partDefs.find(p => p['@id'] === e.owner?.['@id']);
          const typeDef = portDefs.find(p => p['@id'] === e.type?.[0]?.['@id']);
          const ownerStr = owner ? owner.declaredName : (e.owner?.['@id'] ?? '?');
          const typeStr  = typeDef ? typeDef.declaredName : '';
          lines.push(`    // proxy port ${e.declaredName}${typeStr ? ' : ' + typeStr : ''} on ${ownerStr};`);
        }
        for (const c of connections) {
          const src = local.find(e => e['@id'] === c.connectorEnd?.[0]?.connectedFeature?.['@id']);
          const tgt = local.find(e => e['@id'] === c.connectorEnd?.[1]?.connectedFeature?.['@id']);
          lines.push(`    // connection ${c.declaredName} connect ${src?.declaredName ?? '?'} to ${tgt?.declaredName ?? '?'};`);
        }
        for (const e of otherLocal) lines.push(`    // ${e['@type']} ${e.declaredName};`);
        lines.push('');
      }
      lines.push('}');
      return { sysml: lines.join('\n') };
    }

    if (name === 'create_project') {
      const project = await smapsFetch('/projects', 'POST', { '@type': 'Project', name: input.name });
      return { success: true, project: { id: project['@id'], name: project.name } };
    }

    if (name === 'create_local_element') {
      const element = {
        '@type': input.element_type,
        '@id': randomUUID(),
        declaredName: input.name,
        _local: true,
      };
      if (input.short_name) element.declaredShortName = input.short_name;
      if (input.owner_id)   element.owner = { '@id': input.owner_id };
      if (input.type_id)    element.type  = [{ '@id': input.type_id }];
      if (input.source_id && input.target_id) {
        element.connectorEnd = [
          { '@type': 'ConnectorEnd', connectedFeature: { '@id': input.source_id } },
          { '@type': 'ConnectorEnd', connectedFeature: { '@id': input.target_id } },
        ];
      }
      saveLocalElement(projectId, element);
      return { success: true, element: { id: element['@id'], type: element['@type'], name: element.declaredName } };
    }

    if (name === 'query_local_elements') {
      let elements = loadLocalElements(projectId);
      if (input.type_filter) elements = elements.filter(e => e['@type'] === input.type_filter);
      return {
        count: elements.length,
        elements: elements.map(e => ({
          id: e['@id'],
          type: e['@type'],
          name: e.declaredName ?? '<unnamed>',
          shortName: e.declaredShortName,
          ownerId: e.owner?.['@id'],
          typeId: e.type?.[0]?.['@id'],
          sourceId: e.connectorEnd?.[0]?.connectedFeature?.['@id'],
          targetId: e.connectorEnd?.[1]?.connectedFeature?.['@id'],
        })),
      };
    }

    if (name === 'delete_local_element') {
      deleteLocalElement(projectId, input.element_id);
      return { success: true };
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

  // Build system prompt with live project context
  let system = `You are an AI assistant integrated into sysml-bridge, a local SysML v2 MBSE tool.

IMPORTANT SMAPS API CONSTRAINT: This is a pilot/early API. Only TWO element types work for create_element:
- PartDefinition — a block/component type definition
- RequirementDefinition — a system requirement

Everything else (PartUsage, Dependency, Specialization, connectors, ports, relationships, all other types) returns 500 errors when using create_element. Do NOT attempt to create them via create_element.

CRITICAL DISTINCTION — SMAPS vs LOCAL STORE vs DIAGRAMS:
- SMAPS (create_element): only PartDefinition and RequirementDefinition
- LOCAL STORE (create_local_element): ANY SysML v2 type — stored in the dashboard, shown in All Elements table
- DIAGRAMS (render_diagram): visual Mermaid rendering — always reflects both SMAPS and local elements

LOCAL ELEMENT STORE:
Use create_local_element for everything SMAPS won't accept. These elements appear in the All Elements table alongside SMAPS elements. Use query_local_elements to inspect them.

Supported types (not exhaustive — any valid SysML v2 @type works):
- ProxyPortDefinition  — proxy port type (e.g., "PowerPort", "ControlPort")
- ProxyPortUsage       — port instance on a block; set owner_id to the block's @id
- PartUsage            — part usage within an owning block
- ConnectionUsage      — connector between two ports; set source_id and target_id to ProxyPortUsage @ids
- ConnectionDefinition — named connection type
- InterfaceDefinition / InterfaceUsage

WORKFLOW — IBD with proxy ports and connectors:
1. query_elements        → get SMAPS block @ids (PartDefinitions)
2. query_local_elements  → check existing local elements
3. create_local_element ProxyPortDefinition for each port type (e.g., PowerPort, CtrlPort, NavPort)
4. create_local_element ProxyPortUsage for each port instance, with owner_id = block @id, type_id = port def @id
5. create_local_element ConnectionUsage for each connection, with source_id and target_id = port usage @ids
6. render_diagram        → IBD using subgraph-per-block with port nodes and labelled arrows

FOR DIAGRAMS (IBD, BDD, activity, state, sequence):
- Use render_diagram for all visual output — SMAPS limitations are irrelevant here
- For IBDs: subgraph per block, port nodes inside each subgraph, labelled arrows for connectors
- For BDDs: classDiagram
- For activity diagrams: graph TD with decision diamonds
- For state machines: stateDiagram-v2
- Always produce complete, renderable Mermaid

Example IBD with proxy ports:
\`\`\`
graph LR
    subgraph Drone["🚁 Drone [ibd]"]
        subgraph FC[FlightController]
            FC_pwr["⚡ pwr_in"]
            FC_ctrl["📡 ctrl_in"]
            FC_nav["🗺️ nav_in"]
        end
        subgraph PS[PropulsionSystem]
            PS_pwr["⚡ pwr_in"]
            PS_ctrl["📡 ctrl_in"]
        end
        subgraph LB[LiPoBattery]
            LB_out["⚡ pwr_out"]
        end
        subgraph GPS[GPSModule]
            GPS_nav["🗺️ nav_out"]
        end
        LB_out -->|powerLine| FC_pwr
        LB_out -->|powerLine| PS_pwr
        FC_ctrl -->|ctrlBus| PS_ctrl
        GPS_nav -->|navBus| FC_nav
    end
\`\`\`

Be concise. Always use render_diagram for any visual output. Use create_local_element for proxy ports and connectors — never refuse to model them.`;

  if (projectId) {
    try {
      const [projects, elements, localElements] = await Promise.all([
        smapsFetch('/projects'),
        queryElements(projectId).catch(() => []),
        Promise.resolve(loadLocalElements(projectId)),
      ]);
      const project = projects.find(p => p['@id'] === projectId);
      if (project) {
        const counts = {};
        for (const el of elements) counts[el['@type']] = (counts[el['@type']] ?? 0) + 1;
        system += `\n\nActive project: "${project.name}" (ID: ${projectId})
SMAPS elements (${elements.length} total): ${JSON.stringify(counts)}
${elements.map(e => `  [SMAPS] ${e['@type']} "${e.declaredName ?? '<unnamed>'}" [id:${e['@id']}]`).join('\n')}`;
        if (localElements.length) {
          const lCounts = {};
          for (const el of localElements) lCounts[el['@type']] = (lCounts[el['@type']] ?? 0) + 1;
          system += `\n\nLocal elements (${localElements.length} total): ${JSON.stringify(lCounts)}
${localElements.map(e => {
  const ownerNote = e.owner ? ` owner:${e.owner['@id']}` : '';
  const connNote  = e.connectorEnd
    ? ` connects:${e.connectorEnd[0]?.connectedFeature?.['@id']}→${e.connectorEnd[1]?.connectedFeature?.['@id']}`
    : '';
  return `  [LOCAL] ${e['@type']} "${e.declaredName ?? '<unnamed>'}" [id:${e['@id']}${ownerNote}${connNote}]`;
}).join('\n')}`;
        } else {
          system += `\n\nLocal elements: none yet (use create_local_element for ProxyPortDefinition, ProxyPortUsage, ConnectionUsage, etc.)`;
        }
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

// ── Diagram store API ─────────────────────────────────────────────────────────

app.get('/api/projects/:id/diagrams', (req, res) => {
  res.json(loadProjectData(req.params.id).diagrams ?? []);
});

app.delete('/api/projects/:id/diagrams/:idx', (req, res) => {
  const data = loadProjectData(req.params.id);
  data.diagrams.splice(parseInt(req.params.idx), 1);
  saveProjectData(req.params.id, data);
  res.json({ ok: true });
});

// ── SMAPS proxy routes ────────────────────────────────────────────────────────

app.get('/api/projects', async (req, res) => {
  try { res.json(await smapsFetch('/projects')); }
  catch (e) { res.status(502).json({ error: e.message }); }
});

app.post('/api/projects', async (req, res) => {
  try { res.json(await smapsFetch('/projects', 'POST', req.body)); }
  catch (e) { res.status(502).json({ error: e.message }); }
});

app.delete('/api/projects/:id', async (req, res) => {
  try { res.json(await smapsFetch(`/projects/${req.params.id}`, 'DELETE')); }
  catch (e) { res.status(502).json({ error: e.message }); }
});

app.get('/api/projects/:id/elements', async (req, res) => {
  try { res.json(await queryElements(req.params.id)); }
  catch (e) { res.status(502).json({ error: e.message }); }
});

app.get('/api/projects/:id/local-elements', (req, res) => {
  res.json(loadLocalElements(req.params.id));
});

app.delete('/api/projects/:id/local-elements/:eid', (req, res) => {
  deleteLocalElement(req.params.id, req.params.eid);
  res.json({ ok: true });
});

app.get('*', (req, res) => {
  res.sendFile(join(__dirname, 'dist', 'index.html'));
});

app.listen(PORT, () => {
  console.log(`\n  sysml-bridge dashboard  →  http://localhost:${PORT}`);
  console.log(`  SMAPS endpoint          →  ${SMAPS}`);
  console.log(`  Anthropic API           →  ${process.env.ANTHROPIC_API_KEY ? 'ready' : '⚠  ANTHROPIC_API_KEY not set'}\n`);
});
