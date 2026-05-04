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

export interface TopologyEdge {
  id: string;
  label: string;
  sourcePort: string;
  targetPort: string;
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
  explicitEdges: TopologyEdge[] = [],
): { nodes: Node<SysMLBlockNodeData>[]; edges: Edge[] } {
  const byId = new Map<string, SysONElement>();
  for (const el of elements) byId.set(el['@id'], el);

  const partDefs = elements.filter(e => e['@type'] === 'PartDefinition');
  const portUsages = elements.filter(e => e['@type'] === 'PortUsage');
  const connections = elements.filter(e => e['@type'] === 'ConnectionUsage');

  // Build port→block lookup for determining left/right sides
  const portToBlock = new Map<string, string>();
  for (const p of portUsages) {
    const owner = resolveLogicalOwner(p, byId);
    if (owner) portToBlock.set(p['@id'], owner);
  }

  // Determine source vs target ports from both connectorEnd data and explicit topology edges
  const sourcePorts = new Set<string>();
  const targetPorts = new Set<string>();

  // From connectorEnd (if present)
  for (const c of connections) {
    const src = c.connectorEnd?.[0]?.connectedFeature?.['@id'];
    const tgt = c.connectorEnd?.[1]?.connectedFeature?.['@id'];
    if (src) sourcePorts.add(src);
    if (tgt) targetPorts.add(tgt);
  }

  // From explicit topology edges
  for (const e of explicitEdges) {
    sourcePorts.add(e.sourcePort);
    targetPorts.add(e.targetPort);
  }

  const blockIds = new Set(partDefs.map(b => b['@id']));

  const nodes: Node<SysMLBlockNodeData>[] = partDefs.map(block => {
    const blockPorts = portUsages.filter(p => resolveLogicalOwner(p, byId) === block['@id']);
    const ports: PortHandle[] = blockPorts.map(p => {
      // Classify: if it appears as source-only → right, target-only → left, both → right
      const isSource = sourcePorts.has(p['@id']);
      const isTarget = targetPorts.has(p['@id']);
      const position: 'left' | 'right' = isTarget && !isSource ? 'left' : 'right';
      return {
        id: p['@id'],
        name: p.declaredName ?? p.name ?? p['@id'].slice(0, 8),
        position,
      };
    });
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

  // Build edges from connectorEnd data
  const edges: Edge[] = [];
  for (const c of connections) {
    if (!c.connectorEnd?.[0] || !c.connectorEnd?.[1]) continue;
    const srcPortId = c.connectorEnd[0].connectedFeature?.['@id'];
    const tgtPortId = c.connectorEnd[1].connectedFeature?.['@id'];
    if (!srcPortId || !tgtPortId) continue;
    const srcBlock = portToBlock.get(srcPortId);
    const tgtBlock = portToBlock.get(tgtPortId);
    if (!srcBlock || !tgtBlock || !blockIds.has(srcBlock) || !blockIds.has(tgtBlock)) continue;
    edges.push({
      id: c['@id'],
      source: srcBlock,
      sourceHandle: srcPortId,
      target: tgtBlock,
      targetHandle: tgtPortId,
      type: 'sysmlEdge',
      label: c.declaredName ?? '',
      data: { label: c.declaredName ?? '' },
    });
  }

  // Add explicit topology edges (deduplicate by id)
  const edgeIds = new Set(edges.map(e => e.id));
  for (const te of explicitEdges) {
    if (edgeIds.has(te.id)) continue;
    const srcBlock = portToBlock.get(te.sourcePort);
    const tgtBlock = portToBlock.get(te.targetPort);
    if (!srcBlock || !tgtBlock || !blockIds.has(srcBlock) || !blockIds.has(tgtBlock)) continue;
    edges.push({
      id: te.id,
      source: srcBlock,
      sourceHandle: te.sourcePort,
      target: tgtBlock,
      targetHandle: te.targetPort,
      type: 'sysmlEdge',
      label: te.label,
      data: { label: te.label },
    });
  }

  return { nodes, edges };
}
