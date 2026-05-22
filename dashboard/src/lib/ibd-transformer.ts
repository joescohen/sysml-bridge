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

function refId(val: unknown): string | undefined {
  if (!val) return undefined;
  if (Array.isArray(val)) {
    const first = val[0];
    return first && typeof first === 'object' && '@id' in first
      ? String((first as { '@id': string })['@id'])
      : undefined;
  }
  if (typeof val === 'object' && '@id' in (val as object))
    return String((val as { '@id': string })['@id']);
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

export function transformToIBD(
  elements: SysONElement[],
  explicitEdges: TopologyEdge[] = [],
): { nodes: Node<SysMLBlockNodeData>[]; edges: Edge[] } {
  const byId = new Map<string, SysONElement>();
  for (const el of elements) byId.set(el['@id'], el);

  const partUsages = elements.filter(e => e['@type'] === 'PartUsage');
  const portUsages = elements.filter(e => e['@type'] === 'PortUsage');
  const connections = elements.filter(
    e => e['@type'] === 'ConnectionUsage' || e['@type'] === 'InterfaceUsage',
  );

  // Map each PartUsage to its logical owning PartDefinition
  const partUsageOwner = new Map<string, string>(); // partUsageId → owning partDefId
  for (const pu of partUsages) {
    const ownerId = resolveLogicalOwner(pu, byId);
    if (!ownerId) continue;
    const ownerEl = byId.get(ownerId);
    if (ownerEl?.['@type'] === 'PartDefinition') {
      partUsageOwner.set(pu['@id'], ownerId);
    }
  }

  // Map each PortUsage to its owning PartUsage by walking the ownership chain directly
  // This avoids FeatureTyping dependency (which may be absent for some PartUsages)
  const portToPartUsage = new Map<string, string>(); // portId → partUsageId
  for (const p of portUsages) {
    const ownerId = resolveLogicalOwner(p, byId);
    if (!ownerId) continue;
    const ownerEl = byId.get(ownerId);
    if (ownerEl?.['@type'] === 'PartUsage') {
      portToPartUsage.set(p['@id'], ownerId);
    }
  }

  // Collect all port IDs referenced by connections (from ConnectionUsage source/target + topology)
  const connectedPortIds = new Set<string>();
  for (const c of connections) {
    const src = refId(c.source) ?? c.connectorEnd?.[0]?.connectedFeature?.['@id'];
    const tgt = refId(c.target) ?? c.connectorEnd?.[1]?.connectedFeature?.['@id'];
    if (src) connectedPortIds.add(src);
    if (tgt) connectedPortIds.add(tgt);
  }
  for (const e of explicitEdges) {
    connectedPortIds.add(e.sourcePort);
    connectedPortIds.add(e.targetPort);
  }

  // Score each PartDefinition: count of its owned PartUsage children that own connected ports
  // Highest score = context block (the IBD frame whose children become nodes)
  const contextScores = new Map<string, number>();
  for (const portId of connectedPortIds) {
    const puId = portToPartUsage.get(portId);
    if (!puId) continue;
    const defId = partUsageOwner.get(puId);
    if (!defId) continue;
    contextScores.set(defId, (contextScores.get(defId) ?? 0) + 1);
  }

  // Fallback: PartDef with most PartUsage children when no connections are wired yet
  let contextBlockId: string | undefined;
  if (contextScores.size > 0) {
    contextBlockId = [...contextScores.entries()].sort((a, b) => b[1] - a[1])[0][0];
  } else {
    const childCounts = new Map<string, number>();
    for (const defId of partUsageOwner.values()) {
      childCounts.set(defId, (childCounts.get(defId) ?? 0) + 1);
    }
    if (childCounts.size > 0) {
      contextBlockId = [...childCounts.entries()].sort((a, b) => b[1] - a[1])[0][0];
    }
  }

  if (!contextBlockId) return { nodes: [], edges: [] };

  // IBD nodes = PartUsage instances directly owned by the context block
  const contextPartUsages = partUsages.filter(
    pu => partUsageOwner.get(pu['@id']) === contextBlockId,
  );
  const contextPartUsageIds = new Set(contextPartUsages.map(pu => pu['@id']));

  // portToNode: portId → contextPartUsage @id (only ports on IBD nodes)
  const portToNode = new Map<string, string>();
  for (const p of portUsages) {
    const puId = portToPartUsage.get(p['@id']);
    if (puId && contextPartUsageIds.has(puId)) {
      portToNode.set(p['@id'], puId);
    }
  }

  // Classify source vs target ports for handle side positioning
  const sourcePorts = new Set<string>();
  const targetPorts = new Set<string>();
  for (const c of connections) {
    const src = refId(c.source) ?? c.connectorEnd?.[0]?.connectedFeature?.['@id'];
    const tgt = refId(c.target) ?? c.connectorEnd?.[1]?.connectedFeature?.['@id'];
    if (src) sourcePorts.add(src);
    if (tgt) targetPorts.add(tgt);
  }
  for (const e of explicitEdges) {
    sourcePorts.add(e.sourcePort);
    targetPorts.add(e.targetPort);
  }

  // Build nodes from context PartUsages
  const nodes: Node<SysMLBlockNodeData>[] = contextPartUsages.map(pu => {
    const blockPorts = portUsages.filter(p => portToNode.get(p['@id']) === pu['@id']);
    const ports: PortHandle[] = blockPorts.map(p => {
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
      id: pu['@id'],
      type: 'sysmlBlock',
      position: { x: 0, y: 0 },
      data: {
        stereotype: 'part',
        name: pu.declaredName ?? pu.name ?? pu['@id'].slice(0, 8),
        ports,
      },
    };
  });

  // Build edges from ConnectionUsage elements
  // Primary path: use source[]/target[] (SysON REST API populates these correctly)
  // Fallback: connectorEnd.connectedFeature (always null in SysON — kept for other SysML servers)
  const edges: Edge[] = [];
  for (const c of connections) {
    const srcPortId = refId(c.source) ?? c.connectorEnd?.[0]?.connectedFeature?.['@id'];
    const tgtPortId = refId(c.target) ?? c.connectorEnd?.[1]?.connectedFeature?.['@id'];
    if (!srcPortId || !tgtPortId) continue;
    const srcNode = portToNode.get(srcPortId);
    const tgtNode = portToNode.get(tgtPortId);
    if (!srcNode || !tgtNode) continue;
    edges.push({
      id: c['@id'],
      source: srcNode,
      sourceHandle: srcPortId,
      target: tgtNode,
      targetHandle: tgtPortId,
      type: 'sysmlEdge',
      label: c.declaredName ?? '',
      data: { label: c.declaredName ?? '' },
    });
  }

  // Merge explicit topology edges (deduplicate by id)
  const edgeIds = new Set(edges.map(e => e.id));
  for (const te of explicitEdges) {
    if (edgeIds.has(te.id)) continue;
    const srcNode = portToNode.get(te.sourcePort);
    const tgtNode = portToNode.get(te.targetPort);
    if (!srcNode || !tgtNode) continue;
    edges.push({
      id: te.id,
      source: srcNode,
      sourceHandle: te.sourcePort,
      target: tgtNode,
      targetHandle: te.targetPort,
      type: 'sysmlEdge',
      label: te.label,
      data: { label: te.label },
    });
  }

  return { nodes, edges };
}
