import { useEffect, useCallback } from 'react';
import {
  ReactFlow, Background, Controls, MiniMap,
  useNodesState, useEdgesState, useReactFlow, ReactFlowProvider,
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

function IBDViewerInner({ projectId }: IBDViewerProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<SysMLNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const { fitView } = useReactFlow();

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
      // fitView after React has painted the new positions
      setTimeout(() => fitView({ padding: 0.15, duration: 400 }), 50);
    } catch (err) {
      console.error('IBDViewer load error:', err);
    }
  }, [projectId, setNodes, setEdges, fitView]);

  useEffect(() => { load(); }, [load]);

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      colorMode="dark"
      proOptions={{ hideAttribution: true }}
      defaultEdgeOptions={defaultSysMLEdgeOptions}
    >
      <Background color="#1e2337" gap={24} size={1} />
      <Controls showInteractive={false} />
      <MiniMap nodeColor="#6366f1" maskColor="rgba(13,17,28,0.85)" style={{ background: '#141824', border: '1px solid #2a2f45' }} />
    </ReactFlow>
  );
}

export function IBDViewer({ projectId }: IBDViewerProps) {
  return (
    <div style={{ height: 520, borderRadius: 6, overflow: 'hidden', border: '1px solid var(--border)' }}>
      <ReactFlowProvider>
        <IBDViewerInner projectId={projectId} />
      </ReactFlowProvider>
    </div>
  );
}
