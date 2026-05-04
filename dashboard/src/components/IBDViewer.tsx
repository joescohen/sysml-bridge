import { useEffect, useCallback } from 'react';
import {
  ReactFlow, Background, Controls, MiniMap,
  useNodesState, useEdgesState,
  type Node, type Edge,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { SysMLBlockNode } from './sysml/SysMLBlockNode';
import { SysMLEdge, defaultSysMLEdgeOptions } from './sysml/SysMLEdge';
import { transformToIBD, type SysMLBlockNodeData } from '../lib/ibd-transformer';
import { applyELKLayout } from '../lib/ibd-layout';
import { getElements, getTopology } from '../lib/api';

type SysMLNode = Node<SysMLBlockNodeData>;

const nodeTypes = { sysmlBlock: SysMLBlockNode };
const edgeTypes = { sysmlEdge: SysMLEdge };

interface IBDViewerProps {
  projectId: string;
}

export function IBDViewer({ projectId }: IBDViewerProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<SysMLNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  const load = useCallback(async () => {
    try {
      const [elements, topo] = await Promise.all([
        getElements(projectId),
        getTopology(projectId),
      ]);
      const { nodes: rawNodes, edges: rawEdges } = transformToIBD(elements, topo.edges);
      const laidOut = await applyELKLayout(rawNodes, rawEdges);
      setNodes(laidOut);
      setEdges(rawEdges);
    } catch (err) {
      console.error('IBDViewer load error:', err);
    }
  }, [projectId, setNodes, setEdges]);

  useEffect(() => { load(); }, [load]);

  return (
    <div style={{ height: 520, borderRadius: 6, overflow: 'hidden', border: '1px solid var(--border)' }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        colorMode="dark"
        proOptions={{ hideAttribution: true }}
        defaultEdgeOptions={defaultSysMLEdgeOptions}
      >
        <Background color="#334155" gap={20} size={1} />
        <Controls showInteractive={false} />
        <MiniMap nodeColor="#475569" maskColor="rgba(15,23,42,0.8)" style={{ background: 'var(--surface2)' }} />
      </ReactFlow>
    </div>
  );
}
