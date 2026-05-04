import ELK from 'elkjs/lib/elk.bundled.js';
import type { Node, Edge } from '@xyflow/react';
import type { SysMLBlockNodeData } from './ibd-transformer';

const elk = new ELK();

const BLOCK_WIDTH  = 180;
const BLOCK_HEIGHT = 120;

export async function applyELKLayout(
  nodes: Node<SysMLBlockNodeData>[],
  edges: Edge[],
): Promise<Node<SysMLBlockNodeData>[]> {
  if (nodes.length === 0) return nodes;

  const graph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.spacing.nodeNode': '60',
      'elk.layered.spacing.nodeNodeBetweenLayers': '80',
    },
    children: nodes.map(n => ({
      id: n.id,
      width: BLOCK_WIDTH,
      height: Math.max(BLOCK_HEIGHT, (n.data.ports.length + 1) * 24 + 24),
    })),
    edges: edges.map(e => ({
      id: e.id,
      sources: [e.source],
      targets: [e.target],
    })),
  };

  try {
    const layout = await elk.layout(graph);
    return nodes.map(n => {
      const laid = layout.children?.find(c => c.id === n.id);
      return laid
        ? { ...n, position: { x: laid.x ?? 0, y: laid.y ?? 0 } }
        : n;
    });
  } catch (err) {
    console.warn('ELK layout failed, using fallback positions:', err);
    return nodes.map((n, i) => ({
      ...n,
      position: { x: (i % 3) * (BLOCK_WIDTH + 80), y: Math.floor(i / 3) * (BLOCK_HEIGHT + 60) },
    }));
  }
}
