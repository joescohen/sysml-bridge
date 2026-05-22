import { useEffect, useCallback, useState } from 'react';
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
import { getTopology } from '../lib/api';
import type { SysONElement } from '../types/sysml';

type SysMLNode = Node<SysMLBlockNodeData>;

const nodeTypes = { sysmlBlock: SysMLBlockNode };
const edgeTypes = { sysmlEdge: SysMLEdge };

interface IBDViewerProps {
  projectId: string;
  elements: SysONElement[];
}

function IBDViewerInner({ projectId, elements }: IBDViewerProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<SysMLNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const { fitView } = useReactFlow();
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const topo = await getTopology(projectId);
      const { nodes: rawNodes, edges: rawEdges } = transformToIBD(elements, topo.edges);
      const laidOut = await applyELKLayout(rawNodes, rawEdges);
      setNodes(laidOut);
      setEdges(rawEdges);
    } catch (err) {
      console.error('IBDViewer load error:', err);
      setLoadError(err instanceof Error ? err.message : 'Failed to load IBD data');
    } finally {
      setLoading(false);
    }
  }, [projectId, elements, setNodes, setEdges]);

  useEffect(() => { load(); }, [load]);

  // When nodes arrive, schedule fitView. The cleanup cancels it if StrictMode
  // fires this effect twice — the second run re-schedules and actually fires.
  useEffect(() => {
    if (nodes.length === 0) return;
    const t = setTimeout(() => fitView({ padding: 0.15, duration: 400 }), 100);
    return () => clearTimeout(t);
  }, [nodes.length, fitView]);

  return (
    <div style={{ position: 'relative', height: '100%' }}>
      {loading && (
        <div style={{
          position: 'absolute', inset: 0, zIndex: 10,
          display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
          background: '#0d111c', gap: 12,
        }}>
          <div style={{ display: 'flex', gap: 6 }}>
            {[0, 1, 2].map(i => (
              <div key={i} style={{
                width: 8, height: 8, borderRadius: '50%', background: '#6366f1',
                animation: 'pulse 1.2s ease-in-out infinite',
                animationDelay: `${i * 0.2}s`,
              }} />
            ))}
          </div>
          <span style={{ fontSize: 11, color: 'var(--text3)' }}>Building IBD…</span>
        </div>
      )}
      {loadError && !loading && (
        <div style={{
          position: 'absolute', inset: 0, zIndex: 10,
          display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
          background: '#0d111c', gap: 8, padding: 24,
        }}>
          <span style={{ fontSize: 12, color: 'var(--red)', textAlign: 'center' }}>{loadError}</span>
          <button
            onClick={load}
            style={{ padding: '5px 12px', borderRadius: 6, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 11, cursor: 'pointer' }}
          >
            Retry
          </button>
        </div>
      )}
      {!loading && !loadError && nodes.length === 0 && (
        <div style={{
          position: 'absolute', inset: 0, zIndex: 10,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          background: '#0d111c',
        }}>
          <span style={{ fontSize: 12, color: 'var(--text4)' }}>No blocks with ports found in this project.</span>
        </div>
      )}
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
    </div>
  );
}

export function IBDViewer({ projectId, elements }: IBDViewerProps) {
  return (
    <div style={{ height: '100%', minHeight: 400, overflow: 'hidden' }}>
      <ReactFlowProvider>
        <IBDViewerInner projectId={projectId} elements={elements} />
      </ReactFlowProvider>
    </div>
  );
}
