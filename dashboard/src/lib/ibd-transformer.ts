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
