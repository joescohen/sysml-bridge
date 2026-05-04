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

function normalizeDiagramType(type) {
  const value = String(type).trim().toLowerCase();
  if (value === 'bdd' || value === 'block definition diagram') return 'General View';
  if (value === 'ibd' || value === 'internal block diagram') return 'Interconnection View';
  if (value === 'state' || value === 'state machine' || value === 'state machine diagram') return 'State Transition View';
  return type;
}

async function getChildCreationDescriptions(editingContextId, containerId) {
  const data = await sysonGql(
    `query($ecId: ID!, $cid: ID!) { viewer { editingContext(editingContextId: $ecId) { childCreationDescriptions(containerId: $cid) { id label } } } }`,
    { ecId: editingContextId, cid: containerId },
  );
  return data.viewer.editingContext.childCreationDescriptions;
}

async function createChildByLabel(editingContextId, containerId, label) {
  const descriptions = await getChildCreationDescriptions(editingContextId, containerId);
  const match = descriptions.find(d => d.label.toLowerCase() === label.toLowerCase());
  if (!match) {
    throw new Error(`"${label}" is not a valid child type for this container. Valid types: ${descriptions.map(d => d.label).join(', ')}`);
  }

  const result = await sysonGql(
    `mutation($input: CreateChildInput!) { createChild(input: $input) { __typename ... on CreateChildSuccessPayload { object { id label kind } } ... on ErrorPayload { message } } }`,
    { input: { id: randomUUID(), editingContextId, objectId: containerId, childCreationDescriptionId: match.id } },
  );
  const payload = result.createChild;
  if (payload.__typename === 'ErrorPayload') throw new Error(payload.message);
  return payload.object;
}

async function renameElement(projectId, element, name) {
  await sysonRestCommit(projectId, [{
    '@type': 'DataVersion',
    identity: { '@id': element['@id'], '@type': 'DataIdentity' },
    payload: { '@type': element['@type'], '@id': element['@id'], declaredName: name, name },
  }]);
}

async function ensureNamedChild(projectId, editingContextId, parentId, childLabel, elementType, name) {
  let elements = await getAllElements(projectId);
  const existing = elements.find(e =>
    e['@type'] === elementType &&
    e.owner?.['@id'] === parentId &&
    (e.declaredName === name || e.name === name)
  );
  if (existing) return existing;

  const beforeIds = new Set(elements.map(e => e['@id']));
  const createdObject = await createChildByLabel(editingContextId, parentId, childLabel);
  elements = await getAllElements(projectId);
  const created = elements.find(e =>
    e['@type'] === elementType &&
    e.owner?.['@id'] === parentId &&
    !beforeIds.has(e['@id'])
  ) ?? elements.find(e =>
    e['@type'] === elementType &&
    e.declaredName === createdObject.label &&
    !beforeIds.has(e['@id'])
  );
  if (!created) throw new Error(`Created ${childLabel}, but could not resolve the new ${elementType}`);

  await renameElement(projectId, created, name);
  elements = await getAllElements(projectId);
  return elements.find(e => e['@id'] === created['@id']) ?? created;
}

async function getRepresentations(editingContextId) {
  const data = await sysonGql(
    `query($ecId: ID!) { viewer { editingContext(editingContextId: $ecId) { representations { edges { node { id label kind } } } } } }`,
    { ecId: editingContextId },
  );
  return data.viewer.editingContext.representations.edges.map(e => e.node);
}

async function createRepresentation(editingContextId, elementId, diagramType, name) {
  const repDescs = await sysonGql(
    `query($ecId: ID!, $oid: ID!) { viewer { editingContext(editingContextId: $ecId) { representationDescriptions(objectId: $oid) { edges { node { id label } } } } } }`,
    { ecId: editingContextId, oid: elementId },
  );
  const options = repDescs.viewer.editingContext.representationDescriptions.edges.map(e => e.node);
  const requestedType = normalizeDiagramType(diagramType);
  const match = options.find(o => o.label.toLowerCase() === requestedType.toLowerCase());
  if (!match) {
    throw new Error(`"${requestedType}" not available. Options: ${options.map(o => o.label).join(', ')}`);
  }

  const result = await sysonGql(
    `mutation($input: CreateRepresentationInput!) { createRepresentation(input: $input) { __typename ... on CreateRepresentationSuccessPayload { representation { id label kind } } ... on ErrorPayload { message } } }`,
    { input: { id: randomUUID(), editingContextId, objectId: elementId, representationDescriptionId: match.id, representationName: name } },
  );
  const rep = result.createRepresentation;
  if (rep.__typename === 'ErrorPayload') throw new Error(rep.message);
  return rep.representation;
}

async function dropOnDiagram(editingContextId, representationId, objectIds) {
  if (!objectIds.length) return;
  await sysonGql(
    `mutation($input: DropOnDiagramInput!) { dropOnDiagram(input: $input) { __typename ... on DropOnDiagramSuccessPayload { diagram { id } } ... on ErrorPayload { message } } }`,
    { input: { id: randomUUID(), editingContextId, representationId, diagramTargetElementId: representationId, objectIds, startingPositionX: 80, startingPositionY: 80 } },
  );
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
    name: 'create_bdd_structure',
    description: 'Create or repair a real SysON-backed Block Definition Diagram structure: a system PartDefinition, owned subsystem PartUsages, and a SysON General View representation. Use this for BDD requests instead of dropping peer PartDefinitions onto a blank diagram.',
    input_schema: {
      type: 'object',
      required: ['system_name', 'subsystems'],
      properties: {
        system_name: { type: 'string', description: 'Top-level system/block name, e.g. "ANGARS"' },
        subsystems: {
          type: 'array',
          description: 'Subsystem names. Each becomes an owned PartUsage under the system PartDefinition.',
          items: { type: 'string' },
        },
        parent_id: { type: 'string', description: 'Optional parent container @id for the system PartDefinition. Defaults to the first Package with model contents.' },
        diagram_name: { type: 'string', description: 'Optional diagram name. Defaults to "<system_name> BDD".' },
        recreate_diagram: { type: 'boolean', description: 'When true, create a fresh diagram even if one with the same name already exists.' },
      },
    },
  },
  {
    name: 'create_ibd_connection',
    description: 'Create or update a React Flow IBD connection between two PortUsage elements. This powers the dashboard IBD canvas when SysON ConnectionUsage connectorEnd data is unavailable.',
    input_schema: {
      type: 'object',
      required: ['source_port_id', 'target_port_id', 'label'],
      properties: {
        source_port_id: { type: 'string', description: '@id of the source PortUsage' },
        target_port_id: { type: 'string', description: '@id of the target PortUsage' },
        label: { type: 'string', description: 'Connection label shown on the IBD edge' },
        connection_id: { type: 'string', description: 'Optional stable connection id. Generated when omitted.' },
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
      const createdObj = await createChildByLabel(ecId, input.parent_id, input.element_type);

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
      const representation = await createRepresentation(ecId, input.element_id, input.diagram_type, input.name);
      const repId = representation.id;

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

      return { success: true, representation };
    }

    if (name === 'create_bdd_structure') {
      const ecId = await getEditingContextId(projectId);
      const elements = await getAllElements(projectId);
      const parentId = input.parent_id ?? (
        elements.find(e => e['@type'] === 'Package' && (e.ownedElement ?? []).length > 0)?.['@id']
        ?? elements.find(e => e['@type'] === 'Package')?.['@id']
      );
      if (!parentId) return { error: 'No Package element found to contain the system PartDefinition.' };

      const system = await ensureNamedChild(projectId, ecId, parentId, 'Part Definition', 'PartDefinition', input.system_name);
      const subsystemNames = [...new Set((input.subsystems ?? []).map(s => String(s).trim()).filter(Boolean))];
      if (!subsystemNames.length) return { error: 'At least one subsystem name is required.' };

      const partUsages = [];
      for (const subsystem of subsystemNames) {
        const usageName = subsystem.charAt(0).toLowerCase() + subsystem.slice(1).replace(/\s+/g, '');
        partUsages.push(await ensureNamedChild(projectId, ecId, system['@id'], 'Part', 'PartUsage', usageName));
      }

      const diagramName = input.diagram_name ?? `${input.system_name} BDD`;
      const existingRep = !input.recreate_diagram
        ? (await getRepresentations(ecId)).find(r => r.label === diagramName)
        : undefined;
      const representation = existingRep ?? await createRepresentation(ecId, system['@id'], 'BDD', diagramName);

      try {
        await dropOnDiagram(ecId, representation.id, [system['@id'], ...partUsages.map(p => p['@id'])]);
      } catch (dropErr) {
        console.error('dropOnDiagram failed (non-fatal):', dropErr.message);
      }

      return {
        success: true,
        system: { id: system['@id'], type: system['@type'], name: input.system_name },
        partUsages: partUsages.map(p => ({ id: p['@id'], type: p['@type'], name: p.declaredName ?? p.name })),
        representation,
      };
    }

    if (name === 'create_ibd_connection') {
      const allElements = await getAllElements(projectId);
      const ports = new Map(
        allElements
          .filter(e => e['@type'] === 'PortUsage')
          .map(e => [e['@id'], e]),
      );
      if (!ports.has(input.source_port_id)) return { error: `Source port not found: ${input.source_port_id}` };
      if (!ports.has(input.target_port_id)) return { error: `Target port not found: ${input.target_port_id}` };

      const topo = loadTopology();
      if (!topo[projectId]) topo[projectId] = { edges: [] };
      const edge = {
        id: input.connection_id ?? randomUUID(),
        label: input.label,
        sourcePort: input.source_port_id,
        targetPort: input.target_port_id,
      };
      topo[projectId].edges = topo[projectId].edges.filter(e => e.id !== edge.id);
      topo[projectId].edges.push(edge);
      saveTopology(topo);

      return { success: true, edge };
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
- Shorthand accepted: "BDD" → General View, "IBD" → Interconnection View, "state machine" → State Transition View

For BDDs / block definition diagrams, use create_bdd_structure first. It creates or repairs the system PartDefinition, owned subsystem PartUsages, and SysON BDD representation. Do not create BDDs by only dropping peer PartDefinitions onto a blank General View.

Use create_ibd_connection after creating or finding PortUsage elements to connect source and target ports in the React Flow IBD. SysON may create ConnectionUsage elements without connectorEnd data, so this topology tool is the reliable dashboard IBD connection path.

The dashboard displays SysON diagrams in embedded iframes alongside generated BDD/state fallbacks and a React Flow IBD.

Be concise. Use the tools to interact with the model.

${buildMbseSkillContext(messages)}`;

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

function saveTopology(topo) {
  writeFileSync(TOPOLOGY_PATH, JSON.stringify(topo, null, 2));
}

const MBSE_SKILLS = [
  { command: '/mbse-init', file: 'mbse-init.md', purpose: 'Bootstrap a project with stakeholder needs, system context, and CONOPS.' },
  { command: '/mbse-requirements', file: 'mbse-requirements.md', purpose: 'Generate or refine requirements with IDs, hierarchy, and verifiability.' },
  { command: '/mbse-build', file: 'mbse-build.md', purpose: 'Build BDD, IBD, activity, sequence, state, and parametric artifacts.' },
  { command: '/mbse-trace', file: 'mbse-trace.md', purpose: 'Build traceability from requirements to model elements to verification.' },
  { command: '/mbse-validate', file: 'mbse-validate.md', purpose: 'Check completeness, consistency, orphaned elements, and coverage gaps.' },
  { command: '/mbse-verify', file: 'mbse-verify.md', purpose: 'Plan V&V with Test, Analysis, Inspection, and Demonstration methods.' },
  { command: '/mbse-trade', file: 'mbse-trade.md', purpose: 'Run weighted trade studies with decision rationale.' },
  { command: '/mbse-kpp', file: 'mbse-kpp.md', purpose: 'Define and assess KPPs, MOEs, and MOPs.' },
  { command: '/mbse-views', file: 'mbse-views.md', purpose: 'Generate stakeholder-specific viewpoints and views.' },
  { command: '/mbse-diagram', file: 'mbse-diagram.md', purpose: 'Render model structure as Mermaid or SysON diagrams.' },
  { command: '/mbse-query', file: 'mbse-query.md', purpose: 'Answer natural-language questions grounded in actual model elements.' },
];

function readMbseSkill(skill) {
  const path = join(__root, 'packages', 'skills', 'skills', skill.file);
  return existsSync(path) ? readFileSync(path, 'utf8') : `# ${skill.command}\n\n${skill.purpose}`;
}

function getLatestUserText(messages) {
  const latest = [...messages].reverse().find(m => m.role === 'user' && typeof m.content === 'string');
  return latest?.content?.trim() ?? '';
}

function matchMbseSkill(text) {
  const lower = text.toLowerCase();
  return MBSE_SKILLS.find(skill => lower === skill.command || lower.startsWith(`${skill.command} `));
}

function buildMbseSkillContext(messages) {
  const latestText = getLatestUserText(messages);
  const activeSkill = matchMbseSkill(latestText);
  const commandList = MBSE_SKILLS.map(skill => `- ${skill.command}: ${skill.purpose}`).join('\n');
  const activeWorkflow = activeSkill
    ? `\n\nACTIVE MBSE COMMAND:\nThe latest user message starts with ${activeSkill.command}. Treat this as invoking the following skill playbook. Follow it as the workflow for this chat turn, adapting older MCP tool names to the currently available SysON dashboard tools.\n\n${readMbseSkill(activeSkill)}`
    : '';

  return `
MBSE SLASH COMMAND FLOW:
The chatbox supports these MBSE lifecycle slash commands. They are not decorative shortcuts; treat them as skill invocations and route the conversation through the matching workflow:
${commandList}

Routing rules:
- If the user starts a message with one of these commands, use the matching skill workflow as the controlling playbook for that turn.
- Use query_elements first when the command depends on current model state.
- For /mbse-build bdd, use create_bdd_structure to create the system PartDefinition, owned subsystem PartUsages, and SysON BDD representation. Never satisfy BDD creation by dropping unowned peer blocks onto a blank diagram.
- For /mbse-build ibd, create or find PartUsage and PortUsage elements, then use create_ibd_connection for dashboard-visible port connections.
- For /mbse-diagram, prefer real SysON representations via create_diagram when the user wants previewable diagrams; Mermaid output is acceptable when the user asks for text/rendered documentation.
- For /mbse-query and /mbse-validate, ground answers in query_elements results and explicitly distinguish observed facts from recommendations.
- Some historical skill specs mention tools not yet exposed in this dashboard, such as create_relationship, query_relationships, get_traceability, get_project_state, and validate_model. Do not pretend those tools ran. Use available tools when possible, and clearly report relationship/session/traceability items as proposed or not-yet-persisted when there is no available tool.
- Keep the workflow stateful inside the conversation: summarize what phase/command you are executing, what data you found, what you changed, and what remains.
${activeWorkflow}`;
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
  saveTopology(topo);
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
