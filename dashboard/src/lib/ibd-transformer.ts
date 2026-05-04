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
