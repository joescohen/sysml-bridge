import { useEffect, useCallback } from 'react';
import {
  ReactFlow, Background, Controls,
  useNodesState, useEdgesState,
  type Node, type Edge,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { SysMLBlockNode } from './sysml/SysMLBlockNode';
import { SysMLEdge } from './sysml/SysMLEdge';
import { transformToIBD, type SysMLBlockNodeData } from '../lib/ibd-transformer';
import { applyELKLayout } from '../lib/ibd-layout';
import { getElements, getLocalElements } from '../lib/api';

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
      const [smapsElements, localElements] = await Promise.all([
        getElements(projectId),
        getLocalElements(projectId),
      ]);
      const { nodes: rawNodes, edges: rawEdges } = transformToIBD(smapsElements, localElements);
      const laidOut = await applyELKLayout(rawNodes, rawEdges);
      setNodes(laidOut);
      setEdges(rawEdges);
    } catch (err) {
      console.error('IBDViewer load error:', err);
    }
  }, [projectId, setNodes, setEdges]);

  useEffect(() => { load(); }, [load]);

  return (
    <div style={{ height: 420 }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        fitView
        colorMode="dark"
        proOptions={{ hideAttribution: true }}
      >
        <Background />
        <Controls />
      </ReactFlow>
    </div>
  );
}
