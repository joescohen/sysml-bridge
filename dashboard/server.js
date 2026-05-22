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

app.use(express.json({ limit: '1mb' }));
app.use(express.static(join(__dirname, 'dist')));

// ── SysON helpers ────────────────────────────────────────────────────────────

async function sysonRest(path, timeoutMs = 20_000) {
  const res = await fetch(`${SYSON}/api/rest${path}`, { signal: AbortSignal.timeout(timeoutMs) });
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
    signal: AbortSignal.timeout(20_000),
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
  invalidateProjectCache(projectId);
  return res.json();
}

const ecCache = new Map();
const commitIdCache = new Map(); // projectId -> { id, expiresAt }
const elementCache  = new Map(); // projectId -> { elements, expiresAt }
const CACHE_TTL_MS  = 300_000;  // 5 minutes — SysON takes ~26 s on first load; stay cached

function invalidateProjectCache(projectId) {
  commitIdCache.delete(projectId);
  elementCache.delete(projectId);
  elementFetchBackoff.delete(projectId); // new data incoming — allow immediate re-fetch
}

function findFirstPackage(elements) {
  return (
    elements.find(e => e['@type'] === 'Package' && (e.ownedElement ?? []).length > 0)?.['@id'] ??
    elements.find(e => e['@type'] === 'Package')?.['@id'] ??
    null
  );
}

function findPackageAncestor(el, byId) {
  let cur = el.owner?.['@id'];
  const seen = new Set();
  while (cur) {
    if (seen.has(cur)) return null;
    seen.add(cur);
    const owner = byId.get(cur);
    if (!owner) return null;
    if (owner['@type'] === 'Package') return owner['@id'];
    cur = owner.owner?.['@id'];
  }
  return null;
}

function findCommonPackage(el1, el2, byId) {
  return findPackageAncestor(el1, byId) ?? findPackageAncestor(el2, byId);
}

async function createSysONConnection(projectId, sourcePortId, targetPortId, name, parentId) {
  const connId = randomUUID();
  const end1Id = randomUUID();
  const end2Id = randomUUID();
  await sysonRestCommit(projectId, [
    {
      '@type': 'DataVersion',
      payload: {
        '@type': 'ConnectionUsage',
        '@id': connId,
        'declaredName': name,
        'owner': { '@id': parentId },
        'connectorEnd': [{ '@id': end1Id }, { '@id': end2Id }],
      },
    },
    {
      '@type': 'DataVersion',
      payload: {
        '@type': 'ConnectorEnd',
        '@id': end1Id,
        'owner': { '@id': connId },
        'connectedFeature': { '@id': sourcePortId },
      },
    },
    {
      '@type': 'DataVersion',
      payload: {
        '@type': 'ConnectorEnd',
        '@id': end2Id,
        'owner': { '@id': connId },
        'connectedFeature': { '@id': targetPortId },
      },
    },
  ]);
  return { id: connId, end1Id, end2Id };
}

// ── SysML v2 textual serializer ───────────────────────────────────────────────
// Generates hierarchically-correct SysML v2 text from SysON elements.
// Ownership is resolved through Membership nodes (SysON internal structure).

function quoteName(n) {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(n) ? n : `'${n.replace(/'/g, "\\'")}'`;
}

function generateSysMLv2Text(elements, rootName) {
  const byId = new Map(elements.map(e => [e['@id'], e]));
  const nonMem = elements.filter(e => !e['@type'].endsWith('Membership'));

  // Walk through Membership nodes to find real logical owner
  function logicalOwnerId(el) {
    let cur = el.owner?.['@id'];
    const seen = new Set();
    while (cur) {
      if (seen.has(cur)) return undefined;
      seen.add(cur);
      const p = byId.get(cur);
      if (!p) return undefined;
      if (!p['@type'].endsWith('Membership')) return p['@id'];
      cur = p.owner?.['@id'];
    }
    return undefined;
  }

  // Build parent → children map
  const childrenOf = new Map();
  for (const el of nonMem) {
    const pid = logicalOwnerId(el);
    if (!childrenOf.has(pid)) childrenOf.set(pid, []);
    childrenOf.get(pid).push(el);
  }

  function nameOf(el) {
    return quoteName(el.declaredName ?? el.name ?? el['@id'].slice(0, 8));
  }

  // Find the FeatureTyping target name for a feature element
  function typeName(el) {
    const typing = nonMem.find(
      e => e['@type'] === 'FeatureTyping' &&
           (e.source ?? []).some(s => s['@id'] === el['@id'])
    );
    if (!typing) return null;
    const typeEl = byId.get((typing.target ?? [])[0]?.['@id']);
    return typeEl ? nameOf(typeEl) : null;
  }

  // Qualified port reference: owner.port (for connection endpoints)
  function portRef(portEl) {
    const ownerId = logicalOwnerId(portEl);
    const owner = ownerId ? byId.get(ownerId) : null;
    const port = nameOf(portEl);
    return owner ? `${nameOf(owner)}.${port}` : port;
  }

  // Types that are serialized inline as children — skip as top-level
  const INLINE_TYPES = new Set([
    'FeatureTyping', 'Subsetting', 'Redefinition', 'ReferenceSubsetting',
    'FeatureInverting', 'TypeFeaturing', 'ConnectorEnd',
  ]);

  function serialize(el, depth) {
    if (INLINE_TYPES.has(el['@type'])) return [];
    if (el['@type'].endsWith('Membership')) return [];

    const pad = '    '.repeat(depth);
    const n = nameOf(el);
    const shortName = el.declaredShortName ? ` <'${el.declaredShortName}'>` : '';
    const type = typeName(el);
    const typeStr = type ? ` : ${type}` : '';
    const kids = (childrenOf.get(el['@id']) ?? [])
      .filter(c => !INLINE_TYPES.has(c['@type']) && !c['@type'].endsWith('Membership'));
    const hasBody = kids.length > 0;
    const open = hasBody ? ' {' : ';';
    const lines = [];

    const body = () => {
      if (!hasBody) return;
      for (const k of kids) lines.push(...serialize(k, depth + 1));
      lines.push(`${pad}}`);
    };

    switch (el['@type']) {
      case 'Package':
        lines.push(`${pad}package ${n} {`);
        body();
        if (!hasBody) lines.push(`${pad}}`);
        break;

      case 'PartDefinition':
        lines.push(`${pad}part def${shortName} ${n}${open}`);
        body();
        break;

      case 'PartUsage':
        lines.push(`${pad}part ${n}${typeStr}${open}`);
        body();
        break;

      case 'PortDefinition':
        lines.push(`${pad}port def${shortName} ${n}${open}`);
        body();
        break;

      case 'PortUsage':
        lines.push(`${pad}port ${n}${typeStr};`);
        break;

      case 'AttributeDefinition':
        lines.push(`${pad}attribute def${shortName} ${n}${open}`);
        body();
        break;

      case 'AttributeUsage':
        lines.push(`${pad}attribute ${n}${typeStr};`);
        break;

      case 'ItemDefinition':
        lines.push(`${pad}item def ${n}${open}`);
        body();
        break;

      case 'ItemUsage':
        lines.push(`${pad}item ${n}${typeStr};`);
        break;

      case 'ConnectionUsage': {
        const ends = el.connectorEnd ?? [];
        const src = ends[0]?.connectedFeature?.['@id'] ? byId.get(ends[0].connectedFeature['@id']) : null;
        const tgt = ends[1]?.connectedFeature?.['@id'] ? byId.get(ends[1].connectedFeature['@id']) : null;
        if (src && tgt) {
          const label = el.declaredName ? `${n} ` : '';
          lines.push(`${pad}connection ${label}connect ${portRef(src)} to ${portRef(tgt)};`);
        } else {
          lines.push(`${pad}connection ${n};  // WARNING: missing connectorEnd — run validate_model`);
        }
        break;
      }

      case 'FlowConnectionUsage': {
        const ends = el.connectorEnd ?? [];
        const src = ends[0]?.connectedFeature?.['@id'] ? byId.get(ends[0].connectedFeature['@id']) : null;
        const tgt = ends[1]?.connectedFeature?.['@id'] ? byId.get(ends[1].connectedFeature['@id']) : null;
        if (src && tgt) {
          lines.push(`${pad}flow ${el.declaredName ? n + ' ' : ''}from ${portRef(src)} to ${portRef(tgt)};`);
        }
        break;
      }

      case 'InterfaceUsage': {
        const ends = el.connectorEnd ?? [];
        const src = ends[0]?.connectedFeature?.['@id'] ? byId.get(ends[0].connectedFeature['@id']) : null;
        const tgt = ends[1]?.connectedFeature?.['@id'] ? byId.get(ends[1].connectedFeature['@id']) : null;
        if (src && tgt) {
          lines.push(`${pad}interface ${el.declaredName ? n + ' ' : ''}connect ${portRef(src)} to ${portRef(tgt)};`);
        }
        break;
      }

      case 'InterfaceDefinition':
        lines.push(`${pad}interface def ${n}${open}`);
        body();
        break;

      case 'BindingConnector': {
        const src = (el.source ?? [])[0]?.['@id'] ? byId.get(el.source[0]['@id']) : null;
        const tgt = (el.target ?? [])[0]?.['@id'] ? byId.get(el.target[0]['@id']) : null;
        if (src && tgt) lines.push(`${pad}binding ${n} bind ${nameOf(src)} = ${nameOf(tgt)};`);
        break;
      }

      case 'RequirementDefinition':
        lines.push(`${pad}requirement def${shortName} ${n}${open}`);
        body();
        break;

      case 'RequirementUsage':
        lines.push(`${pad}requirement ${n}${typeStr};`);
        break;

      case 'SatisfyRequirementUsage': {
        const reqId = (el.target ?? [])[0]?.['@id'];
        const srcId = (el.source ?? [])[0]?.['@id'];
        const req = reqId ? byId.get(reqId) : null;
        const src = srcId ? byId.get(srcId) : null;
        if (req) {
          lines.push(`${pad}satisfy requirement ${nameOf(req)}${src ? ` by ${nameOf(src)}` : ''};`);
        }
        break;
      }

      case 'VerifyRequirementUsage': {
        const reqId = (el.target ?? [])[0]?.['@id'];
        const req = reqId ? byId.get(reqId) : null;
        if (req) lines.push(`${pad}verify requirement ${nameOf(req)};`);
        break;
      }

      case 'AllocationUsage': {
        const srcId = (el.source ?? [])[0]?.['@id'];
        const tgtId = (el.target ?? [])[0]?.['@id'];
        const src = srcId ? byId.get(srcId) : null;
        const tgt = tgtId ? byId.get(tgtId) : null;
        if (src && tgt) lines.push(`${pad}allocation allocate ${nameOf(src)} to ${nameOf(tgt)};`);
        break;
      }

      case 'Dependency': {
        const srcId = (el.source ?? [])[0]?.['@id'];
        const tgtId = (el.target ?? [])[0]?.['@id'];
        const src = srcId ? byId.get(srcId) : null;
        const tgt = tgtId ? byId.get(tgtId) : null;
        if (src && tgt) {
          lines.push(`${pad}dependency ${el.declaredName ? n + ' ' : ''}from ${nameOf(src)} to ${nameOf(tgt)};`);
        }
        break;
      }

      case 'ActionDefinition':
        lines.push(`${pad}action def ${n}${open}`);
        body();
        break;

      case 'ActionUsage':
        lines.push(`${pad}action ${n}${typeStr}${open}`);
        body();
        break;

      case 'StateDefinition':
        lines.push(`${pad}state def ${n}${open}`);
        body();
        break;

      case 'StateUsage':
        lines.push(`${pad}state ${n}${typeStr}${open}`);
        body();
        break;

      case 'TransitionUsage': {
        const tgtId = (el.target ?? [])[0]?.['@id'];
        const tgt = tgtId ? byId.get(tgtId) : null;
        if (tgt) lines.push(`${pad}transition ${el.declaredName ? n + ' ' : ''}then ${nameOf(tgt)};`);
        break;
      }

      case 'SuccessionUsage': {
        const tgtId = (el.target ?? [])[0]?.['@id'];
        const tgt = tgtId ? byId.get(tgtId) : null;
        if (tgt) lines.push(`${pad}first ${n} then ${nameOf(tgt)};`);
        break;
      }

      case 'ConstraintDefinition':
        lines.push(`${pad}constraint def ${n}${open}`);
        body();
        break;

      case 'ConstraintUsage':
        lines.push(`${pad}constraint ${n}${typeStr};`);
        break;

      case 'UseCaseDefinition':
        lines.push(`${pad}use case def ${n}${open}`);
        body();
        break;

      case 'UseCaseUsage':
        lines.push(`${pad}use case ${n}${typeStr};`);
        break;

      case 'ViewpointDefinition':
        lines.push(`${pad}viewpoint def ${n}${open}`);
        body();
        break;

      case 'ViewDefinition':
        lines.push(`${pad}view def ${n}${open}`);
        body();
        break;

      case 'ViewUsage':
        lines.push(`${pad}view ${n}${typeStr};`);
        break;

      default:
        // Emit unknown types as a comment so nothing is silently lost
        lines.push(`${pad}// ${el['@type']} ${n};`);
        break;
    }
    return lines;
  }

  // Roots: elements whose logical owner is not in the element set (i.e., real top-level)
  const allIds = new Set(nonMem.map(e => e['@id']));
  const roots = nonMem.filter(el => {
    const pid = logicalOwnerId(el);
    return !pid || !allIds.has(pid);
  });

  const lines = [`package ${quoteName(rootName)} {`, ''];
  for (const root of roots) {
    // Skip the SysON root Package wrapper — emit its children directly
    if (root['@type'] === 'Package') {
      for (const child of (childrenOf.get(root['@id']) ?? [])
        .filter(c => !INLINE_TYPES.has(c['@type']) && !c['@type'].endsWith('Membership'))) {
        lines.push(...serialize(child, 1));
        lines.push('');
      }
    } else {
      lines.push(...serialize(root, 1));
      lines.push('');
    }
  }
  lines.push('}');
  return lines.join('\n');
}

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
  const cached = commitIdCache.get(projectId);
  if (cached && Date.now() < cached.expiresAt) return cached.id;
  const commits = await sysonRest(`/projects/${projectId}/commits`).catch(() => []);
  const id = commits[0]?.['@id'] ?? null;
  if (id) commitIdCache.set(projectId, { id, expiresAt: Date.now() + CACHE_TTL_MS }); // never cache null
  return id;
}

const pendingElementFetches = new Map(); // projectId -> Promise — coalesces concurrent callers
const elementFetchBackoff  = new Map(); // projectId -> retryAfter timestamp

async function getAllElements(projectId) {
  const cached = elementCache.get(projectId);
  if (cached && Date.now() < cached.expiresAt) return cached.elements;

  // Coalesce: if a SysON fetch is already in flight, reuse its promise
  if (pendingElementFetches.has(projectId)) return pendingElementFetches.get(projectId);

  // Back off after repeated failures — avoid hammering SysON while it's loading
  const retryAfter = elementFetchBackoff.get(projectId) ?? 0;
  if (Date.now() < retryAfter) return cached?.elements ?? [];

  const promise = (async () => {
    const commitId = await getHeadCommitId(projectId);
    if (!commitId) return [];
    // SysON takes ~26 s to copy default SysML libraries on first editing-context load
    const elements = await sysonRest(`/projects/${projectId}/commits/${commitId}/elements`, 90_000);
    elementCache.set(projectId, { elements, expiresAt: Date.now() + CACHE_TTL_MS });
    elementFetchBackoff.delete(projectId);
    return elements;
  })();

  promise.catch(() => {
    // After failure, back off 60s before allowing another SysON elements request
    elementFetchBackoff.set(projectId, Date.now() + 60_000);
  });
  promise.finally(() => pendingElementFetches.delete(projectId)).catch(() => {});
  pendingElementFetches.set(projectId, promise);
  return promise;
}

function clearElementBackoff(projectId) {
  elementFetchBackoff.delete(projectId);
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

async function createChildByLabel(editingContextId, containerId, label, projectId) {
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
  if (projectId) invalidateProjectCache(projectId);
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
  const createdObject = await createChildByLabel(editingContextId, parentId, childLabel, projectId);
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
    name: 'create_connection',
    description: 'Create a SysML v2 ConnectionUsage between two PortUsage elements. Writes a real ConnectionUsage with ConnectorEnd references into SysON (SysML v2 compliant) and also updates the React Flow IBD topology for display. Prefer this over the legacy create_ibd_connection.',
    input_schema: {
      type: 'object',
      required: ['source_port_id', 'target_port_id'],
      properties: {
        source_port_id: { type: 'string', description: '@id of the source PortUsage' },
        target_port_id: { type: 'string', description: '@id of the target PortUsage' },
        name: { type: 'string', description: 'Optional connection name. Defaults to "src→tgt" port names.' },
        parent_id: { type: 'string', description: 'Optional parent container @id. Defaults to the nearest Package ancestor.' },
      },
    },
  },
  {
    name: 'create_relationship',
    description: 'Create a typed SysML v2 relationship between elements in SysON. Supports: Allocation, Dependency, SatisfyRequirementUsage, VerifyRequirementUsage, FeatureTyping, Subsetting, Redefinition, FlowConnectionUsage, BindingConnector, InterfaceUsage.',
    input_schema: {
      type: 'object',
      required: ['relationship_type', 'source_id', 'target_id'],
      properties: {
        relationship_type: { type: 'string', description: 'SysML v2 relationship type, e.g. "Allocation", "Dependency", "SatisfyRequirementUsage", "FeatureTyping"' },
        source_id: { type: 'string', description: '@id of the source element' },
        target_id: { type: 'string', description: '@id of the target element' },
        name: { type: 'string', description: 'Optional relationship name' },
        parent_id: { type: 'string', description: 'Optional parent container @id. Defaults to nearest Package.' },
      },
    },
  },
  {
    name: 'update_element',
    description: 'Update properties of an existing SysML v2 element in SysON via a REST commit.',
    input_schema: {
      type: 'object',
      required: ['element_id', 'updates'],
      properties: {
        element_id: { type: 'string', description: '@id of the element to update' },
        updates: {
          type: 'object',
          description: 'Key-value pairs to update, e.g. { "declaredName": "newName", "isAbstract": true }',
          additionalProperties: true,
        },
      },
    },
  },
  {
    name: 'query_relationships',
    description: 'Query SysML v2 relationships from SysON. Returns ConnectionUsage, FlowConnectionUsage, Allocation, Dependency, SatisfyRequirementUsage, VerifyRequirementUsage, FeatureTyping, Subsetting, Redefinition, and other relationship elements with their source/target/connectorEnd data.',
    input_schema: {
      type: 'object',
      properties: {
        element_id: { type: 'string', description: 'Filter to relationships involving this element @id (as source, target, or connector end)' },
        type_filter: { type: 'string', description: 'Filter by @type, e.g. "ConnectionUsage", "Allocation", "Dependency"' },
      },
    },
  },
  {
    name: 'validate_model',
    description: 'Run SysML v2 completeness and consistency checks. Reports orphaned ports (not connected in SysON or topology), unsatisfied/unverified requirements, missing connection endpoints, and parts without connections.',
    input_schema: { type: 'object', properties: {} },
  },
  {
    name: 'get_project_state',
    description: 'Get a full snapshot of the current SysML v2 model — all elements with type/owner, element counts by type, relationship summary, and topology edge count.',
    input_schema: { type: 'object', properties: {} },
  },
  {
    name: 'create_ibd_connection',
    description: 'DEPRECATED — use create_connection instead. Kept for backward compatibility. Falls through to create_connection.',
    input_schema: {
      type: 'object',
      required: ['source_port_id', 'target_port_id', 'label'],
      properties: {
        source_port_id: { type: 'string' },
        target_port_id: { type: 'string' },
        label: { type: 'string' },
        connection_id: { type: 'string' },
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

      // Idempotency: return existing element if same name+type already exists under this parent
      const existingElements = await getAllElements(projectId);
      const SYSML_TYPE_MAP = {
        'Part Definition': 'PartDefinition', 'Part': 'PartUsage',
        'State Definition': 'StateDefinition', 'State': 'StateUsage',
        'Port Definition': 'PortDefinition', 'Port': 'PortUsage',
        'Requirement Definition': 'RequirementDefinition', 'Requirement': 'RequirementUsage',
        'Interface Definition': 'InterfaceDefinition', 'Interface': 'InterfaceUsage',
        'Action Definition': 'ActionDefinition', 'Action': 'ActionUsage',
        'Attribute Definition': 'AttributeDefinition', 'Attribute': 'AttributeUsage',
        'Package': 'Package',
      };
      const sysmlType = SYSML_TYPE_MAP[input.element_type] ?? input.element_type;
      const byId = new Map(existingElements.map(e => [e['@id'], e]));
      function logicalOwnerOf(el) {
        const visited = new Set();
        let cur = el.owner?.['@id'];
        while (cur) {
          if (visited.has(cur)) return undefined;
          visited.add(cur);
          const o = byId.get(cur);
          if (!o) return undefined;
          if (!o['@type'].endsWith('Membership')) return o['@id'];
          cur = o.owner?.['@id'];
        }
        return undefined;
      }
      const existing = existingElements.find(e =>
        e['@type'] === sysmlType &&
        (e.declaredName ?? e.name) === input.name &&
        logicalOwnerOf(e) === input.parent_id
      );
      if (existing) {
        return { success: true, already_existed: true, element: { id: existing['@id'], type: existing['@type'], name: input.name } };
      }

      const createdObj = await createChildByLabel(ecId, input.parent_id, input.element_type, projectId);

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

    if (name === 'create_connection' || name === 'create_ibd_connection') {
      const allElements = await getAllElements(projectId);
      const byId = new Map(allElements.map(e => [e['@id'], e]));

      const sourcePort = byId.get(input.source_port_id);
      const targetPort = byId.get(input.target_port_id);
      if (!sourcePort) return { error: `Source port not found: ${input.source_port_id}` };
      if (!targetPort) return { error: `Target port not found: ${input.target_port_id}` };

      const connName = input.name ?? input.label ??
        `${sourcePort.declaredName ?? 'src'}→${targetPort.declaredName ?? 'tgt'}`;
      const parentId = input.parent_id ?? findCommonPackage(sourcePort, targetPort, byId) ?? findFirstPackage(allElements);
      if (!parentId) return { error: 'No Package found to own the ConnectionUsage' };

      // Create real SysML v2 ConnectionUsage with ConnectorEnd references in SysON
      let sysonConn = null;
      try {
        sysonConn = await createSysONConnection(projectId, input.source_port_id, input.target_port_id, connName, parentId);
      } catch (sysonErr) {
        console.error('SysON ConnectionUsage commit failed (falling back to topology only):', sysonErr.message);
      }

      // Always write topology.json so the React Flow IBD displays the connection
      const topo = loadTopology();
      if (!topo[projectId]) topo[projectId] = { edges: [] };
      const edge = {
        id: input.connection_id ?? sysonConn?.id ?? randomUUID(),
        label: connName,
        sourcePort: input.source_port_id,
        targetPort: input.target_port_id,
      };
      topo[projectId].edges = topo[projectId].edges.filter(e => e.id !== edge.id);
      topo[projectId].edges.push(edge);
      saveTopology(topo);

      return {
        success: true,
        sysml_valid: !!sysonConn,
        connection: sysonConn
          ? { id: sysonConn.id, type: 'ConnectionUsage', name: connName, end1Id: sysonConn.end1Id, end2Id: sysonConn.end2Id }
          : edge,
        message: sysonConn
          ? 'ConnectionUsage created in SysON with ConnectorEnd references (SysML v2 compliant)'
          : 'SysON commit failed — stored in topology.json only',
      };
    }

    if (name === 'create_relationship') {
      const REL_TYPE_MAP = {
        'allocation': 'AllocationUsage', 'allocationusage': 'AllocationUsage',
        'dependency': 'Dependency',
        'satisfy': 'SatisfyRequirementUsage', 'satisfyrequirementusage': 'SatisfyRequirementUsage',
        'verify': 'VerifyRequirementUsage', 'verifyrequirementusage': 'VerifyRequirementUsage',
        'featuretyping': 'FeatureTyping', 'typing': 'FeatureTyping',
        'subsetting': 'Subsetting', 'subset': 'Subsetting',
        'redefinition': 'Redefinition', 'redefine': 'Redefinition',
        'flow': 'FlowConnectionUsage', 'flowconnectionusage': 'FlowConnectionUsage',
        'binding': 'BindingConnector', 'bindingconnector': 'BindingConnector',
        'interface': 'InterfaceUsage', 'interfaceusage': 'InterfaceUsage',
        'expose': 'MembershipExpose', 'membershipexpose': 'MembershipExpose',
        'succession': 'SuccessionUsage', 'successionusage': 'SuccessionUsage',
        'transition': 'TransitionUsage', 'transitionusage': 'TransitionUsage',
      };
      const normalized = input.relationship_type.toLowerCase().replace(/[\s_]+/g, '');
      const sysmlType = REL_TYPE_MAP[normalized] ?? input.relationship_type;

      const allElements = await getAllElements(projectId);
      const byId = new Map(allElements.map(e => [e['@id'], e]));
      if (!byId.has(input.source_id)) return { error: `Source element not found: ${input.source_id}` };
      if (!byId.has(input.target_id)) return { error: `Target element not found: ${input.target_id}` };

      // TransitionUsage: SysON's REST commit API only updates existing elements — it cannot
      // create new ones, and it silently ignores source/target on TransitionUsage anyway.
      // Correct approach: use insertTextualSysMLv2 with SysML v2 succession syntax
      // ("succession <name> first <srcName> then <tgtName>;") which creates a
      // SuccessionAsUsage with properly persisted source/target endpoints.
      if (sysmlType === 'TransitionUsage') {
        const srcEl = byId.get(input.source_id);
        const tgtEl = byId.get(input.target_id);
        const srcName = srcEl?.declaredName ?? srcEl?.name;
        const tgtName = tgtEl?.declaredName ?? tgtEl?.name;
        if (!srcName || !tgtName) return { error: 'Source or target element has no name — required for succession syntax' };

        // Find the StateDefinition that logically owns the source state
        function logicalOwner(el) {
          const seen = new Set();
          let cur = el?.owner?.['@id'];
          while (cur) {
            if (seen.has(cur)) return undefined;
            seen.add(cur);
            const o = byId.get(cur);
            if (!o) return undefined;
            if (!o['@type'].endsWith('Membership')) return o['@id'];
            cur = o.owner?.['@id'];
          }
          return undefined;
        }
        const stateDef = input.parent_id
          ? byId.get(input.parent_id)
          : byId.get(logicalOwner(srcEl));
        const ownerId = stateDef?.['@id'] ?? input.parent_id;
        if (!ownerId) return { error: 'Could not find StateDefinition to own this transition. Provide parent_id.' };

        const ecId = await getEditingContextId(projectId);
        const relName = input.name ?? `${srcName}_to_${tgtName}`;
        // SysML v2 succession syntax — creates SuccessionAsUsage with persisted source/target
        const sysmlText = `succession ${relName} first ${srcName} then ${tgtName};`;
        const gqlResult = await sysonGql(
          `mutation($input: InsertTextualSysMLv2Input!) { insertTextualSysMLv2(input: $input) { __typename ... on ErrorPayload { message } } }`,
          { input: { id: randomUUID(), editingContextId: ecId, objectId: ownerId, textualContent: sysmlText } },
        );
        const payload = gqlResult.insertTextualSysMLv2;
        if (payload.__typename !== 'SuccessPayload') {
          return { error: `SysON rejected succession: ${payload.message}` };
        }
        invalidateProjectCache(projectId);

        // Auto-create State Transition View if none exists for this StateDefinition
        if (stateDef?.['@type'] === 'StateDefinition') {
          try {
            const reps = await getRepresentations(ecId);
            const viewName = `${stateDef.declaredName ?? stateDef.name ?? 'State Machine'} State Machine`;
            if (!reps.find(r => r.label === viewName || /state\s*transition/i.test(r.label))) {
              await createRepresentation(ecId, stateDef['@id'], 'State Transition View', viewName);
            }
          } catch (e) { /* non-fatal */ }
        }

        return { success: true, relationship: { type: 'SuccessionAsUsage', name: relName, source: input.source_id, target: input.target_id } };
      }

      const parentId = input.parent_id ?? findFirstPackage(allElements);
      if (!parentId) return { error: 'No Package found to own the relationship' };

      const relId = randomUUID();
      await sysonRestCommit(projectId, [{
        '@type': 'DataVersion',
        payload: {
          '@type': sysmlType,
          '@id': relId,
          ...(input.name ? { declaredName: input.name } : {}),
          'owner': { '@id': parentId },
          'source': [{ '@id': input.source_id }],
          'target': [{ '@id': input.target_id }],
        },
      }]);

      return {
        success: true,
        relationship: { id: relId, type: sysmlType, source: input.source_id, target: input.target_id },
      };
    }

    if (name === 'update_element') {
      const allElements = await getAllElements(projectId);
      const el = allElements.find(e => e['@id'] === input.element_id);
      if (!el) return { error: `Element not found: ${input.element_id}` };

      await sysonRestCommit(projectId, [{
        '@type': 'DataVersion',
        identity: { '@id': el['@id'], '@type': 'DataIdentity' },
        payload: { '@type': el['@type'], '@id': el['@id'], ...input.updates },
      }]);

      return { success: true, element_id: el['@id'], updated: input.updates };
    }

    if (name === 'query_relationships') {
      const RELATIONSHIP_TYPES = new Set([
        'ConnectionUsage', 'FlowConnectionUsage', 'InterfaceUsage', 'BindingConnector',
        'AllocationUsage', 'Dependency', 'SatisfyRequirementUsage', 'VerifyRequirementUsage',
        'FeatureTyping', 'Subsetting', 'Redefinition', 'ReferenceSubsetting',
        'MembershipExpose', 'SuccessionUsage', 'TransitionUsage',
      ]);

      const allElements = await getAllElements(projectId);
      let rels = allElements.filter(e => RELATIONSHIP_TYPES.has(e['@type']));

      if (input.type_filter) {
        rels = rels.filter(e => e['@type'] === input.type_filter);
      }

      if (input.element_id) {
        rels = rels.filter(e => {
          const srcs = (e.source ?? []).map(s => s['@id']);
          const tgts = (e.target ?? []).map(t => t['@id']);
          const endFeatures = (e.connectorEnd ?? []).map(ce => ce.connectedFeature?.['@id']).filter(Boolean);
          return srcs.includes(input.element_id) || tgts.includes(input.element_id) || endFeatures.includes(input.element_id);
        });
      }

      return {
        count: rels.length,
        relationships: rels.map(e => ({
          id: e['@id'],
          type: e['@type'],
          name: e.declaredName ?? e.name ?? null,
          source: (e.source ?? []).map(s => s['@id']),
          target: (e.target ?? []).map(t => t['@id']),
          connectorEnd: (e.connectorEnd ?? []).map(ce => ({
            id: ce['@id'],
            connectedFeature: ce.connectedFeature?.['@id'] ?? null,
          })),
        })),
      };
    }

    if (name === 'validate_model') {
      const allElements = await getAllElements(projectId);
      const byId = new Map(allElements.map(e => [e['@id'], e]));

      const partDefs    = allElements.filter(e => e['@type'] === 'PartDefinition');
      const portUsages  = allElements.filter(e => e['@type'] === 'PortUsage');
      const connections = allElements.filter(e => ['ConnectionUsage', 'FlowConnectionUsage', 'InterfaceUsage'].includes(e['@type']));
      const requirements = allElements.filter(e => ['RequirementDefinition', 'RequirementUsage'].includes(e['@type']));
      const allocations = allElements.filter(e => ['AllocationUsage', 'Dependency'].includes(e['@type']));

      // Ports connected via SysON connectorEnd
      const sysonConnectedPortIds = new Set();
      for (const c of connections) {
        for (const ce of (c.connectorEnd ?? [])) {
          if (ce.connectedFeature?.['@id']) sysonConnectedPortIds.add(ce.connectedFeature['@id']);
        }
      }

      // Ports connected via topology.json fallback
      const topo = loadTopology();
      const topoEdges = topo[projectId]?.edges ?? [];
      const topoConnectedPortIds = new Set([
        ...topoEdges.map(e => e.sourcePort),
        ...topoEdges.map(e => e.targetPort),
      ]);

      const allConnectedPortIds = new Set([...sysonConnectedPortIds, ...topoConnectedPortIds]);
      const orphanedPorts = portUsages.filter(p => !allConnectedPortIds.has(p['@id']));

      // Requirements satisfied/verified
      const satisfiedReqIds = new Set();
      allElements
        .filter(e => ['SatisfyRequirementUsage', 'VerifyRequirementUsage'].includes(e['@type']))
        .forEach(s => (s.target ?? []).forEach(t => satisfiedReqIds.add(t['@id'])));
      const unsatisfiedReqs = requirements.filter(r => !satisfiedReqIds.has(r['@id']));

      // Connections missing endpoints (connectorEnd has no connectedFeature)
      const brokenConnections = connections.filter(c => {
        const ends = c.connectorEnd ?? [];
        return ends.length < 2 || ends.some(ce => !ce.connectedFeature?.['@id']);
      });

      // PartUsages without FeatureTyping — SysML v2 requires typed features
      const featureTypings = allElements.filter(e => e['@type'] === 'FeatureTyping');
      const typedFeatureIds = new Set(
        featureTypings.flatMap(ft => (ft.source ?? []).map(s => s['@id']))
      );
      const partUsages = allElements.filter(e => e['@type'] === 'PartUsage');
      const untypedParts = partUsages.filter(p => !typedFeatureIds.has(p['@id']));

      // PortUsages without FeatureTyping are acceptable (anonymous ports are valid SysML v2)
      // but PartUsages should be typed for a complete model

      const issues = [];
      if (orphanedPorts.length > 0) {
        issues.push({
          severity: 'warning', category: 'connectivity',
          message: `${orphanedPorts.length} port(s) not connected`,
          elements: orphanedPorts.map(p => ({ id: p['@id'], name: p.declaredName ?? p.name ?? null })),
        });
      }
      if (unsatisfiedReqs.length > 0) {
        issues.push({
          severity: 'error', category: 'requirements',
          message: `${unsatisfiedReqs.length} requirement(s) not satisfied or verified`,
          elements: unsatisfiedReqs.map(r => ({ id: r['@id'], name: r.declaredName ?? r.name ?? null })),
        });
      }
      if (brokenConnections.length > 0) {
        issues.push({
          severity: 'error', category: 'sysml-validity',
          message: `${brokenConnections.length} ConnectionUsage element(s) missing connectorEnd references — use create_connection to fix`,
          elements: brokenConnections.map(c => ({ id: c['@id'], name: c.declaredName ?? c.name ?? null })),
        });
      }
      if (untypedParts.length > 0) {
        issues.push({
          severity: 'warning', category: 'sysml-validity',
          message: `${untypedParts.length} PartUsage element(s) without FeatureTyping — use create_relationship(FeatureTyping, partUsageId, partDefId) to type them`,
          elements: untypedParts.map(p => ({ id: p['@id'], name: p.declaredName ?? p.name ?? null })),
        });
      }
      if (connections.length === 0 && topoEdges.length === 0 && portUsages.length > 0) {
        issues.push({
          severity: 'warning', category: 'connectivity',
          message: 'Ports exist but no connections found in SysON or topology — IBD is empty',
        });
      }

      return {
        valid: issues.every(i => i.severity !== 'error'),
        summary: {
          partDefinitions: partDefs.length,
          partUsages: partUsages.length,
          typedPartUsages: partUsages.length - untypedParts.length,
          portUsages: portUsages.length,
          sysonConnections: connections.length,
          sysonConnectedPorts: sysonConnectedPortIds.size,
          topologyConnections: topoEdges.length,
          requirements: requirements.length,
          satisfiedRequirements: satisfiedReqIds.size,
          allocations: allocations.length,
        },
        issues,
      };
    }

    if (name === 'get_project_state') {
      const [allElements, projects] = await Promise.all([
        getAllElements(projectId),
        sysonRest('/projects').catch(() => []),
      ]);
      const project = projects.find(p => p['@id'] === projectId);
      const nonMem = allElements.filter(e => !e['@type'].endsWith('Membership'));

      const counts = {};
      for (const el of nonMem) counts[el['@type']] = (counts[el['@type']] ?? 0) + 1;

      const topo = loadTopology();
      const topoEdges = topo[projectId]?.edges ?? [];

      return {
        project: { id: projectId, name: project?.name ?? 'Unknown' },
        totalElements: nonMem.length,
        elementCountsByType: counts,
        topologyEdges: topoEdges.length,
        elements: nonMem.map(e => ({
          id: e['@id'],
          type: e['@type'],
          name: e.declaredName ?? e.name ?? null,
          ownerId: e.owner?.['@id'] ?? null,
        })),
      };
    }

    if (name === 'export_sysml') {
      const elements = await getAllElements(projectId);
      const projects = await sysonRest('/projects');
      const project = projects.find(p => p['@id'] === projectId);
      return { sysml: generateSysMLv2Text(elements, project?.name ?? 'Model') };
    }

    if (name === 'create_project') {
      const result = await sysonGql(
        `mutation($input: CreateProjectInput!) { createProject(input: $input) { __typename ... on CreateProjectSuccessPayload { project { id } } ... on ErrorPayload { message } } }`,
        { input: { id: randomUUID(), name: input.name, templateId: 'sysmlv2-template', libraryIds: [] } },
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

SysON implements the full SysML v2 metamodel. All mutations are persisted as SysML v2-compliant elements via SysON's REST commit API.

─── TOOL REFERENCE ───────────────────────────────────────────────────────────

ELEMENTS
• query_elements([type_filter]) — list all elements, optionally filtered by @type
• create_element(element_type, name, parent_id) — create any SysML v2 element
  - element_type: human-readable SysON label ("Part Definition", "Port", "Action", "Requirement", etc.)
  - parent_id: Package @id for top-level, PartDefinition @id for ports/parts/actions
• update_element(element_id, updates) — patch element properties (name, isAbstract, etc.)
• delete_element(element_id) — remove element by @id

CONNECTIONS & RELATIONSHIPS
• create_connection(source_port_id, target_port_id, [name], [parent_id])
  - Creates a SysML v2 ConnectionUsage with two ConnectorEnd elements in SysON (language-compliant)
  - Also writes to topology.json so the React Flow IBD displays the edge immediately
  - Use this for ALL IBD port connections — it is the authoritative path
• create_relationship(relationship_type, source_id, target_id, [name], [parent_id])
  - Creates typed SysML v2 relationships persisted in SysON
  - Supported types: Allocation, Dependency, SatisfyRequirementUsage, VerifyRequirementUsage,
    FeatureTyping, Subsetting, Redefinition, FlowConnectionUsage, BindingConnector, InterfaceUsage,
    TransitionUsage, SuccessionUsage
• query_relationships([element_id], [type_filter]) — surface all relationship elements from SysON
  with source, target, and connectorEnd data

DIAGRAMS
• create_diagram(element_id, diagram_type, name) — create a SysON diagram representation
  - "General View" (BDD), "Interconnection View" (IBD), "State Transition View", "Requirements Table View"
• create_bdd_structure(system_name, subsystems, ...) — create a full BDD with PartDefinition + PartUsages

MODEL HEALTH
• validate_model() — completeness and consistency checks: orphaned ports, broken ConnectionUsage
  (missing connectorEnd), unsatisfied requirements, empty IBD
• get_project_state() — full element snapshot with counts by type and topology edge count
• export_sysml() — export current model as SysML v2 textual notation

PROJECT
• create_project(name) — create a new SysON project

─── IMPORTANT RULES ──────────────────────────────────────────────────────────

1. CONNECTIONS: Always use create_connection (not create_ibd_connection) for port-to-port connections.
   It creates a language-valid ConnectionUsage with ConnectorEnd references in SysON, which makes
   the Interconnection View in SysON and the React Flow IBD consistent.

2. BDDs: Always use create_bdd_structure for block definition diagrams. Never satisfy BDD creation
   by dropping unowned peer PartDefinitions onto a blank General View.

3. RELATIONSHIPS: Use create_relationship for semantic relationships (Allocation, SatisfyRequirementUsage,
   FeatureTyping). These are persisted in SysON and surfaced by query_relationships.

4. VALIDATION: Run validate_model after creating connections to confirm no broken connectorEnd refs
   or orphaned ports remain.

5. The server validates element types against SysON's metamodel — if a child type is invalid under
   a container, it returns the list of valid types.

─────────────────────────────────────────────────────────────────────────────

${buildMbseSkillContext(messages)}`;

  if (projectId) {
    try {
      const [elements, projects] = await Promise.all([
        getAllElements(projectId).catch(() => []),
        sysonRest('/projects').catch(() => []),
      ]);
      const nonMem = elements.filter(e => !e['@type'].endsWith('Membership'));
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
      { input: { id: randomUUID(), name: req.body.name, templateId: 'sysmlv2-template', libraryIds: [] } },
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

app.post('/api/projects/:id/invalidate', (req, res) => {
  invalidateProjectCache(req.params.id);
  res.json({ ok: true });
});

// Patch arbitrary element fields via a SysON DataVersion commit
app.patch('/api/projects/:id/elements/:eid', async (req, res) => {
  try {
    const { id: projectId, eid: elementId } = req.params;
    const updates = req.body;
    if (!updates || typeof updates !== 'object' || Array.isArray(updates)) {
      return res.status(400).json({ error: 'Request body must be a plain JSON object' });
    }
    const allElements = await getAllElements(projectId);
    const el = allElements.find(e => e['@id'] === elementId);
    if (!el) return res.status(404).json({ error: `Element not found: ${elementId}` });
    await sysonRestCommit(projectId, [{
      '@type': 'DataVersion',
      identity: { '@id': el['@id'], '@type': 'DataIdentity' },
      payload: { '@type': el['@type'], '@id': el['@id'], ...updates },
    }]);
    res.json({ success: true, element_id: el['@id'], updated: updates });
  } catch (e) {
    res.status(502).json({ error: e.message });
  }
});

// Create or update the Documentation element owned by a given element
app.post('/api/projects/:id/elements/:eid/documentation', async (req, res) => {
  try {
    const { id: projectId, eid: elementId } = req.params;
    const { body: docBody } = req.body;
    if (typeof docBody !== 'string') {
      return res.status(400).json({ error: '"body" must be a string' });
    }
    const allElements = await getAllElements(projectId);
    const el = allElements.find(e => e['@id'] === elementId);
    if (!el) return res.status(404).json({ error: `Element not found: ${elementId}` });

    // Find an existing Documentation element owned by this element
    const existing = allElements.find(
      e => e['@type'] === 'Documentation' && e.owner?.['@id'] === elementId
    );

    if (existing) {
      await sysonRestCommit(projectId, [{
        '@type': 'DataVersion',
        identity: { '@id': existing['@id'], '@type': 'DataIdentity' },
        payload: { '@type': 'Documentation', '@id': existing['@id'], body: docBody },
      }]);
      res.json({ success: true, doc_element_id: existing['@id'], created: false });
    } else {
      const docId = randomUUID();
      await sysonRestCommit(projectId, [{
        '@type': 'DataVersion',
        payload: { '@type': 'Documentation', '@id': docId, body: docBody, owner: { '@id': elementId } },
      }]);
      res.json({ success: true, doc_element_id: docId, created: true });
    }
  } catch (e) {
    res.status(502).json({ error: e.message });
  }
});

app.get('/api/projects/:id/export', async (req, res) => {
  try {
    const elements = await getAllElements(req.params.id);
    const projects = await sysonRest('/projects');
    const project = projects.find(p => p['@id'] === req.params.id);
    const name = project?.name ?? 'Model';
    const text = generateSysMLv2Text(elements, name);
    const filename = `${name.replace(/[^a-zA-Z0-9_-]/g, '_')}.sysml`;
    res.setHeader('Content-Type', 'text/plain; charset=utf-8');
    res.setHeader('Content-Disposition', `attachment; filename="${filename}"`);
    res.send(text);
  } catch (e) { res.status(502).json({ error: e.message }); }
});

// Returns SysON-persisted ConnectionUsage elements with connectorEnd data.
// The IBD viewer merges these with topology.json edges so both sources are visible.
app.get('/api/projects/:id/connections', async (req, res) => {
  try {
    const elements = await getAllElements(req.params.id);
    const CONNECTION_TYPES = new Set(['ConnectionUsage', 'FlowConnectionUsage', 'InterfaceUsage']);
    const connections = elements.filter(e => CONNECTION_TYPES.has(e['@type']));
    res.json(connections.map(c => ({
      id: c['@id'],
      type: c['@type'],
      name: c.declaredName ?? c.name ?? null,
      connectorEnd: (c.connectorEnd ?? []).map(ce => ({
        id: ce['@id'],
        connectedFeature: ce.connectedFeature?.['@id'] ?? null,
      })),
      source: (c.source ?? []).map(s => s['@id']),
      target: (c.target ?? []).map(t => t['@id']),
    })));
  } catch (e) { res.status(502).json({ error: e.message }); }
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

/mbse-build bdd:
  Use create_bdd_structure — creates PartDefinition + owned PartUsages + SysON General View.
  Then use create_relationship(FeatureTyping, partUsageId, partDefId) to type each PartUsage.
  Never satisfy BDD by dropping peer PartDefinitions onto a blank diagram.

/mbse-build ibd:
  STEP 1 — create_element("Port", name, partDefId) for each port on each PartDefinition.
  STEP 2 — create_connection(sourcePortId, targetPortId) for each interface.
            This creates ConnectionUsage + two ConnectorEnd elements in SysON (SysML v2 valid).
            Do NOT use create_ibd_connection (deprecated) or create_element("Connection").
  STEP 3 — validate_model() to confirm no broken connectorEnd refs or orphaned ports.
  STEP 4 — create_diagram(partDefId, "Interconnection View", name) to create SysON IBD view.

/mbse-build state:
  STEP 1 — create_element("State Definition", name, packageId) to define the state machine type.
  STEP 2 — create_element("State", stateName, stateDefId) for each state (creates StateUsage).
  STEP 3 — create_relationship("TransitionUsage", fromStateId, toStateId) for each transition.
  STEP 4 — create_diagram(stateDefId, "State Transition View", name) to create the SysON view.
  The Generated → State Machine tab picks up StateUsage + StateDefinition + TransitionUsage elements automatically.

/mbse-trace:
  Use create_relationship(SatisfyRequirementUsage, partId, reqId) and
      create_relationship(VerifyRequirementUsage, testCaseId, reqId).
  Use query_relationships to surface existing links. There is no get_traceability tool —
  build the traceability matrix from query_elements + query_relationships results.

/mbse-validate:
  Use validate_model() for automated checks (orphaned ports, broken connections, untyped parts,
  unsatisfied requirements). Then query_relationships to surface all relationship coverage.
  There is no .mbse-session.json mechanism — work entirely from live SysON state.

/mbse-diagram:
  Prefer create_diagram for SysON-rendered views. Use query_relationships (not get_traceability)
  to build Mermaid diagrams. There is no get_traceability tool.

All skills:
  The tools create_relationship, query_relationships, validate_model, get_project_state,
  update_element, and create_connection are fully available. Use them directly.
  Do not pretend any tool ran without actually calling it.
- Keep the workflow stateful inside the conversation: summarize phase, what was found, what changed, and what remains.
${activeWorkflow}`;
}

app.get('/api/projects/:id/topology', async (req, res) => {
  const topo = loadTopology();
  const projectTopo = topo[req.params.id];
  if (!projectTopo) {
    try {
      const sysonRes = await fetch(`${SYSON}/api/rest/projects`, { signal: AbortSignal.timeout(3000) });
      if (sysonRes.ok) {
        const projects = await sysonRes.json();
        const exists = Array.isArray(projects) && projects.some(p => p['@id'] === req.params.id);
        if (!exists) return res.status(404).json({ error: 'Project not found', edges: [] });
      }
    } catch { /* SysON unreachable — fall through to empty edges */ }
  }
  res.json(projectTopo ?? { edges: [] });
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

process.on('unhandledRejection', (reason) => {
  console.error('[unhandledRejection]', reason);
});
process.on('uncaughtException', (err) => {
  console.error('[uncaughtException]', err);
});

app.listen(PORT, '127.0.0.1', () => {
  console.log(`\n  sysml-bridge dashboard  →  http://localhost:${PORT}`);
  console.log(`  SysON endpoint          →  ${SYSON}`);
  console.log(`  Anthropic API           →  ${process.env.ANTHROPIC_API_KEY ? 'ready' : '⚠  ANTHROPIC_API_KEY not set'}\n`);
});
