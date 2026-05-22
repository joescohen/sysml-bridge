import type { SysONElement } from '../types/sysml';

// ── Shared helpers ────────────────────────────────────────────────────────────

function elementName(el: SysONElement): string {
  return el.declaredName ?? el.name ?? el['@id'].slice(0, 8);
}

function refId(value: unknown): string | undefined {
  if (!value) return undefined;
  if (Array.isArray(value)) return refId(value[0]);
  if (typeof value === 'object' && '@id' in value) return String((value as { '@id': string })['@id']);
  return undefined;
}

function resolveLogicalOwner(el: SysONElement, byId: Map<string, SysONElement>): string | undefined {
  const visited = new Set<string>();
  let current = el.owner?.['@id'];
  while (current) {
    if (visited.has(current)) return undefined;
    visited.add(current);
    const owner = byId.get(current);
    if (!owner) return undefined;
    if (!owner['@type'].endsWith('Membership')) return owner['@id'];
    current = owner.owner?.['@id'];
  }
  return undefined;
}

// ── BDD model ─────────────────────────────────────────────────────────────────

export interface BDDBlock {
  id: string;
  name: string;
  stereotype: 'part def' | 'part';
  ports: string[];
  /** Owned PartUsage names with optional type annotation */
  parts: Array<{ name: string; typeName?: string }>;
  /** IDs of PartDefinitions this block specializes (parent in hierarchy) */
  parentIds: string[];
  /** IDs of PartDefinitions that specialize this block (children in hierarchy) */
  childIds: string[];
  /** BFS depth in specialization tree (0 = top-level, no specialization parents) */
  level: number;
}

export interface BDDModel {
  blocks: BDDBlock[];
  /** Legacy field: top-level root block for backward compat */
  root?: { id: string; name: string; childIds: string[] };
}

export function buildBDDModel(elements: SysONElement[]): BDDModel {
  const byId = new Map(elements.map(e => [e['@id'], e]));

  const partDefs = elements.filter(e => e['@type'] === 'PartDefinition');
  const partDefIds = new Set(partDefs.map(e => e['@id']));

  // Build specialization graph from Subclassification elements
  // Subclassification: source = child/subtype, target = parent/supertype
  const parentMap = new Map<string, string[]>();   // defId → parentDefIds
  const childMap  = new Map<string, string[]>();   // defId → childDefIds
  for (const e of elements) {
    if (e['@type'] !== 'Subclassification') continue;
    const srcs = Array.isArray(e.source) ? e.source : e.source ? [e.source] : [];
    const tgts = Array.isArray(e.target) ? e.target : e.target ? [e.target] : [];
    for (const src of srcs) {
      const srcId = typeof src === 'object' && '@id' in src ? src['@id'] : undefined;
      if (!srcId || !partDefIds.has(srcId)) continue;
      for (const tgt of tgts) {
        const tgtId = typeof tgt === 'object' && '@id' in tgt ? tgt['@id'] : undefined;
        if (!tgtId || !partDefIds.has(tgtId)) continue;
        if (!parentMap.has(srcId)) parentMap.set(srcId, []);
        if (!childMap.has(tgtId))  childMap.set(tgtId, []);
        parentMap.get(srcId)!.push(tgtId);
        childMap.get(tgtId)!.push(srcId);
      }
    }
  }

  // Compute BFS levels (0 = no PartDef parents among our set)
  const levelMap = new Map<string, number>();
  const queue: string[] = [];
  for (const d of partDefs) {
    if (!parentMap.get(d['@id'])?.length) {
      levelMap.set(d['@id'], 0);
      queue.push(d['@id']);
    }
  }
  while (queue.length) {
    const id = queue.shift()!;
    const lvl = levelMap.get(id)!;
    for (const childId of childMap.get(id) ?? []) {
      if (!levelMap.has(childId)) {
        levelMap.set(childId, lvl + 1);
        queue.push(childId);
      }
    }
  }
  // Any unreachable (circular?) defaults to 0
  for (const d of partDefs) {
    if (!levelMap.has(d['@id'])) levelMap.set(d['@id'], 0);
  }

  // Collect ports and parts by logical owner
  const portsByOwner = new Map<string, string[]>();
  const partsByOwner = new Map<string, Array<{ name: string; typeName?: string }>>();

  // FeatureTyping: maps a feature to its type (source = feature, target = type)
  const featureTypeMap = new Map<string, string>(); // featureId → typeId
  for (const e of elements) {
    if (e['@type'] !== 'FeatureTyping') continue;
    const srcs = Array.isArray(e.source) ? e.source : e.source ? [e.source] : [];
    const tgts = Array.isArray(e.target) ? e.target : e.target ? [e.target] : [];
    const srcId = refId(srcs[0]);
    const tgtId = refId(tgts[0]);
    if (srcId && tgtId) featureTypeMap.set(srcId, tgtId);
  }

  for (const el of elements) {
    if (el['@type'] === 'PortUsage') {
      const ownerId = resolveLogicalOwner(el, byId);
      if (!ownerId) continue;
      if (!portsByOwner.has(ownerId)) portsByOwner.set(ownerId, []);
      portsByOwner.get(ownerId)!.push(elementName(el));
    }
    if (el['@type'] === 'PartUsage') {
      const ownerId = resolveLogicalOwner(el, byId);
      if (!ownerId || !partDefIds.has(ownerId)) continue;
      if (!partsByOwner.has(ownerId)) partsByOwner.set(ownerId, []);
      const typeId = featureTypeMap.get(el['@id']);
      const typeEl  = typeId ? byId.get(typeId) : undefined;
      const typeName = typeEl ? elementName(typeEl) : undefined;
      partsByOwner.get(ownerId)!.push({ name: elementName(el), typeName });
    }
  }

  const blocks: BDDBlock[] = partDefs.map(d => ({
    id:         d['@id'],
    name:       elementName(d),
    stereotype: 'part def',
    ports:      portsByOwner.get(d['@id']) ?? [],
    parts:      partsByOwner.get(d['@id']) ?? [],
    parentIds:  parentMap.get(d['@id']) ?? [],
    childIds:   childMap.get(d['@id']) ?? [],
    level:      levelMap.get(d['@id']) ?? 0,
  }));

  // Sort: richest (most parts) blocks first within each level for visual prominence
  blocks.sort((a, b) => (a.level - b.level) || (b.parts.length - a.parts.length) || a.name.localeCompare(b.name));

  // Legacy root: block with no parents that has the most parts
  const legacy = [...blocks].filter(b => !b.parentIds.length).sort((a, b) => b.parts.length - a.parts.length)[0] ?? blocks[0];

  return {
    blocks,
    root: legacy ? { id: legacy.id, name: legacy.name, childIds: legacy.childIds } : undefined,
  };
}

// ── State machine model (unchanged) ──────────────────────────────────────────

export interface StateNode {
  id: string;
  name: string;
}

export interface StateTransition {
  id: string;
  name: string;
  sourceId?: string;
  targetId?: string;
}

export interface StateMachineModel {
  states: StateNode[];
  transitions: StateTransition[];
}

export function buildStateMachineModel(elements: SysONElement[]): StateMachineModel {
  const stateIds = new Set(
    elements
      .filter(el => el['@type'] === 'StateUsage' || el['@type'] === 'StateDefinition')
      .map(el => el['@id']),
  );

  // Include both TransitionUsage and SuccessionAsUsage — the latter is what
  // insertTextualSysMLv2 "succession first X then Y" creates, and it properly
  // persists source/target in SysON.  Only keep transitions whose endpoints
  // are known state elements so activity successions aren't mixed in.
  const transitions = elements
    .filter(el => el['@type'] === 'TransitionUsage' || el['@type'] === 'SuccessionAsUsage')
    .map(el => ({
      id:       el['@id'],
      name:     elementName(el),
      sourceId: refId(el.source),
      targetId: refId(el.target),
    }))
    .filter(t => t.sourceId != null && t.targetId != null &&
                 stateIds.has(t.sourceId!) && stateIds.has(t.targetId!));

  return {
    states: elements
      .filter(el => el['@type'] === 'StateUsage' || el['@type'] === 'StateDefinition')
      .map(el => ({ id: el['@id'], name: elementName(el) })),
    transitions,
  };
}

// ── Requirements model ────────────────────────────────────────────────────────

export interface RequirementNode {
  id: string;
  name: string;
  shortName: string | null;
  type: string;
  children: RequirementNode[];
  satisfiedBy: string[];
  verifiedBy: string[];
  docText: string | null;
  docElementId: string | null;  // ID of owned Documentation element, if any
}

export interface RequirementsModel {
  roots: RequirementNode[];
}

export function buildRequirementsModel(elements: SysONElement[]): RequirementsModel {
  const byId = new Map(elements.map(e => [e['@id'], e]));

  const REQ_TYPES = new Set(['RequirementDefinition', 'RequirementUsage']);

  const reqEls = elements.filter(e => REQ_TYPES.has(e['@type']));
  if (!reqEls.length) return { roots: [] };

  // Map: requirement ID → doc text + doc element ID (from owned Documentation elements)
  const docTextMap = new Map<string, string>();
  const docIdMap   = new Map<string, string>();
  for (const e of elements) {
    if (e['@type'] !== 'Documentation') continue;
    const ownerId = e.owner?.['@id'];
    if (!ownerId || !reqEls.find(r => r['@id'] === ownerId)) continue;
    if (e.body) {
      docTextMap.set(ownerId, String(e.body));
      docIdMap.set(ownerId, e['@id']);
    }
  }

  // Map: requirement ID → satisfied-by names / verified-by names
  const satisfiedBy = new Map<string, string[]>();
  const verifiedBy  = new Map<string, string[]>();
  for (const e of elements) {
    const isSatisfy = e['@type'] === 'SatisfyRequirementUsage';
    const isVerify  = e['@type'] === 'VerifyRequirementUsage';
    if (!isSatisfy && !isVerify) continue;
    const tgts = Array.isArray(e.target) ? e.target : e.target ? [e.target] : [];
    const srcs = Array.isArray(e.source) ? e.source : e.source ? [e.source] : [];
    for (const t of tgts) {
      const reqId = typeof t === 'object' && '@id' in t ? t['@id'] : undefined;
      if (!reqId) continue;
      const map = isSatisfy ? satisfiedBy : verifiedBy;
      if (!map.has(reqId)) map.set(reqId, []);
      const srcEl = srcs[0] && typeof srcs[0] === 'object' && '@id' in srcs[0]
        ? byId.get(srcs[0]['@id'] as string) : undefined;
      map.get(reqId)!.push(srcEl ? elementName(srcEl) : 'unknown');
    }
  }

  // Build tree using logical ownership
  const reqIds = new Set(reqEls.map(e => e['@id']));
  const allNodes = new Map<string, RequirementNode>();

  // Create all nodes first
  for (const e of reqEls) {
    const node: RequirementNode = {
      id:           e['@id'],
      name:         elementName(e),
      shortName:    e.declaredShortName ?? null,
      type:         e['@type'],
      children:     [],
      satisfiedBy:  satisfiedBy.get(e['@id']) ?? [],
      verifiedBy:   verifiedBy.get(e['@id']) ?? [],
      docText:      docTextMap.get(e['@id']) ?? null,
      docElementId: docIdMap.get(e['@id']) ?? null,
    };
    allNodes.set(e['@id'], node);
  }

  // Wire children via logical owner
  const roots: RequirementNode[] = [];
  for (const e of reqEls) {
    let ownerId = e.owner?.['@id'];
    // Walk through membership nodes
    const seen = new Set<string>();
    while (ownerId) {
      if (seen.has(ownerId)) { ownerId = undefined; break; }
      seen.add(ownerId);
      const owner = byId.get(ownerId);
      if (!owner) { ownerId = undefined; break; }
      if (!owner['@type'].endsWith('Membership')) break;
      ownerId = owner.owner?.['@id'];
    }
    if (ownerId && reqIds.has(ownerId)) {
      allNodes.get(ownerId)!.children.push(allNodes.get(e['@id'])!);
    }
    // else: top-level — collected below by finding nodes not a child of any other node
  }

  // Collect roots (not a child of another req node)
  const childIds = new Set<string>();
  for (const n of allNodes.values()) n.children.forEach(c => childIds.add(c.id));
  for (const n of allNodes.values()) {
    if (!childIds.has(n.id)) roots.push(n);
  }

  return { roots };
}

// ── Activity / Action model ───────────────────────────────────────────────────

export interface ActivityNode {
  id: string;
  name: string;
  kind: 'start' | 'end' | 'action' | 'decision' | 'merge';
}

export interface ActivityEdge {
  fromId: string;
  toId:   string;
  label?: string;
}

export interface ActivityFlow {
  id: string;
  name: string;
  nodes: ActivityNode[];
  edges: ActivityEdge[];
}

export interface ActivityModel {
  flows: ActivityFlow[];
}

export function buildActivityModel(elements: SysONElement[]): ActivityModel {
  const byId = new Map(elements.map(e => [e['@id'], e]));

  const actionDefs = elements.filter(e => e['@type'] === 'ActionDefinition');
  if (!actionDefs.length) return { flows: [] };

  function resolveLogical(el: SysONElement): string | undefined {
    const visited = new Set<string>();
    let cur = el.owner?.['@id'];
    while (cur) {
      if (visited.has(cur)) return undefined;
      visited.add(cur);
      const owner = byId.get(cur);
      if (!owner) return undefined;
      if (!owner['@type'].endsWith('Membership')) return owner['@id'];
      cur = owner.owner?.['@id'];
    }
    return undefined;
  }

  const flows: ActivityFlow[] = [];

  for (const def of actionDefs) {
    const defId = def['@id'];
    const nodes: ActivityNode[] = [];
    const edges: ActivityEdge[] = [];
    const nodeIds = new Set<string>();

    // Collect nodes owned by this ActionDefinition
    for (const e of elements) {
      const ownerId = resolveLogical(e);
      if (ownerId !== defId) continue;

      if (['ActionUsage', 'PerformActionUsage'].includes(e['@type'])) {
        const n: ActivityNode = { id: e['@id'], name: elementName(e), kind: 'action' };
        nodes.push(n);
        nodeIds.add(e['@id']);
      } else if (e['@type'] === 'DecisionNode') {
        nodes.push({ id: e['@id'], name: elementName(e) || '◇', kind: 'decision' });
        nodeIds.add(e['@id']);
      } else if (e['@type'] === 'MergeNode') {
        nodes.push({ id: e['@id'], name: elementName(e) || '◆', kind: 'merge' });
        nodeIds.add(e['@id']);
      }
    }

    // Sentinel start/end nodes keyed to the def itself
    const startId = `${defId}__start`;
    const endId   = `${defId}__end`;
    nodes.unshift({ id: startId, name: '', kind: 'start' });
    nodes.push({ id: endId, name: '', kind: 'end' });
    nodeIds.add(startId);
    nodeIds.add(endId);

    // Collect succession/transition edges
    for (const e of elements) {
      if (!['SuccessionAsUsage', 'TransitionUsage'].includes(e['@type'])) continue;
      const ownerId = resolveLogical(e);
      if (ownerId !== defId) continue;

      const srcs = Array.isArray(e.source) ? e.source : e.source ? [e.source] : [];
      const tgts = Array.isArray(e.target) ? e.target : e.target ? [e.target] : [];
      const fromId = srcs[0] && typeof srcs[0] === 'object' && '@id' in srcs[0]
        ? (srcs[0] as { '@id': string })['@id'] : undefined;
      const toId   = tgts[0] && typeof tgts[0] === 'object' && '@id' in tgts[0]
        ? (tgts[0] as { '@id': string })['@id'] : undefined;

      // Use start/end sentinels only when the ref explicitly points to the
      // owning ActionDefinition (SysML "first"/"then" succession endpoints).
      // Absent refs do NOT fall back to sentinels — that produced phantom edges.
      const from = fromId && nodeIds.has(fromId) ? fromId
                 : fromId === defId              ? startId
                 : undefined;
      const to   = toId   && nodeIds.has(toId)   ? toId
                 : toId   === defId              ? endId
                 : undefined;

      if (from && to) {
        edges.push({ fromId: from, toId: to, label: elementName(e) || undefined });
      }
    }

    // Deduplicate edges
    const seen = new Set<string>();
    const uniqueEdges = edges.filter(e => {
      const key = `${e.fromId}→${e.toId}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });

    if (nodes.length > 2) {   // more than just start + end
      flows.push({ id: defId, name: elementName(def), nodes, edges: uniqueEdges });
    }
  }

  return { flows };
}
