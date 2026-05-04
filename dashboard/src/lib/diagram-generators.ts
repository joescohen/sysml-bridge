import type { SysONElement } from '../types/sysml';

export interface BDDBlock {
  id: string;
  name: string;
  ports: string[];
}

export interface BDDModel {
  root?: {
    id: string;
    name: string;
    childIds: string[];
  };
  blocks: BDDBlock[];
}

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

function elementName(el: SysONElement): string {
  return el.declaredName ?? el.name ?? el['@id'].slice(0, 8);
}

function refId(value: unknown): string | undefined {
  if (!value) return undefined;
  if (Array.isArray(value)) return refId(value[0]);
  if (typeof value === 'object' && '@id' in value) return String((value as { '@id': string })['@id']);
  return undefined;
}

export function buildBDDModel(elements: SysONElement[], rootName?: string): BDDModel {
  const portsByOwner = new Map<string, string[]>();
  for (const el of elements) {
    if (el['@type'] !== 'PortUsage') continue;
    const ownerId = el.owner?.['@id'];
    if (!ownerId) continue;
    const ports = portsByOwner.get(ownerId) ?? [];
    ports.push(elementName(el));
    portsByOwner.set(ownerId, ports);
  }

  const partDefinitions = elements.filter(el => el['@type'] === 'PartDefinition');
  const partIds = new Set(partDefinitions.map(el => el['@id']));
  const blocks = partDefinitions.map(el => ({
        id: el['@id'],
        name: elementName(el),
        ports: portsByOwner.get(el['@id']) ?? [],
      }));
  const topLevelBlocks = partDefinitions.filter(el => !partIds.has(el.owner?.['@id'] ?? ''));

  return {
    root: rootName && topLevelBlocks.length
      ? {
        id: `root:${rootName}`,
        name: rootName,
        childIds: topLevelBlocks.map(el => el['@id']),
      }
      : undefined,
    blocks,
  };
}

export function buildStateMachineModel(elements: SysONElement[]): StateMachineModel {
  return {
    states: elements
      .filter(el => el['@type'] === 'StateUsage' || el['@type'] === 'StateDefinition')
      .map(el => ({ id: el['@id'], name: elementName(el) })),
    transitions: elements
      .filter(el => el['@type'] === 'TransitionUsage')
      .map(el => ({
        id: el['@id'],
        name: elementName(el),
        sourceId: refId(el.source),
        targetId: refId(el.target),
      })),
  };
}
