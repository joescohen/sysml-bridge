import { useState, useEffect, useCallback } from 'react';
import type { Project, SysONElement } from '../types/sysml';
import { getElements } from '../lib/api';
import { buildContainmentTree } from '../lib/containment';
import { ContainmentTree } from './ContainmentTree';
import { DiagramPanel } from './DiagramPanel';

interface ProjectDetailProps {
  project: Project;
  onBack: () => void;
  refreshKey: number;
}

export function ProjectDetail({ project, onBack, refreshKey }: ProjectDetailProps) {
  const [elements, setElements] = useState<SysONElement[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [treeCollapsed, setTreeCollapsed] = useState(false);

  const projectId = project['@id'];
  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setElements(await getElements(projectId));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load elements');
      setElements([]);
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => { load(); }, [load, refreshKey]);

  const roots = !loading && !error ? buildContainmentTree(elements) : [];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      {/* ── Header strip ─────────────────────────────────────────────────── */}
      <div style={{
        flexShrink: 0,
        padding: '0 16px',
        height: 44,
        borderBottom: '1px solid var(--border)',
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        background: 'var(--surface)',
      }}>
        <button
          onClick={onBack}
          style={{ background: 'none', border: 'none', color: 'var(--text3)', cursor: 'pointer', fontSize: 11, display: 'flex', alignItems: 'center', gap: 4, padding: '4px 0', flexShrink: 0 }}
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="m15 18-6-6 6-6"/></svg>
          Projects
        </button>
        <div style={{ width: 1, height: 16, background: 'var(--border2)' }} />
        <div style={{ fontSize: 13, fontWeight: 700, color: '#f1f5f9', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{project.name}</div>
        <div style={{ fontSize: 10, fontFamily: 'monospace', color: 'var(--text4)' }}>{project['@id'].slice(0, 8)}</div>
        <div style={{ flex: 1 }} />
        {!loading && !error && (
          <div style={{ fontSize: 10, color: 'var(--text4)', fontFamily: 'monospace' }}>
            {elements.filter(e => !e['@type'].endsWith('Membership')).length} elements
          </div>
        )}
      </div>

      {/* ── Loading / error states ────────────────────────────────────────── */}
      {loading && (
        <div style={{ padding: 32, color: 'var(--text4)', fontSize: 12 }}>Loading model…</div>
      )}
      {error && (
        <div style={{ padding: 32, color: '#ef4444', fontSize: 12 }}>Error: {error}</div>
      )}

      {/* ── Split body ───────────────────────────────────────────────────── */}
      {!loading && !error && (
        <div style={{ display: 'flex', flex: 1, minHeight: 0, overflow: 'hidden' }}>

          {/* Containment tree panel */}
          <div style={{
            width: treeCollapsed ? 36 : 260,
            flexShrink: 0,
            borderRight: '1px solid var(--border)',
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
            transition: 'width 0.18s ease',
            background: 'var(--bg)',
          }}>
            {/* Tree header */}
            <div style={{
              height: 36,
              display: 'flex',
              alignItems: 'center',
              justifyContent: treeCollapsed ? 'center' : 'space-between',
              padding: treeCollapsed ? '0 8px' : '0 10px 0 14px',
              borderBottom: '1px solid var(--border)',
              flexShrink: 0,
              gap: 6,
            }}>
              {!treeCollapsed && (
                <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)' }}>
                  Containment Tree
                </span>
              )}
              <button
                onClick={() => setTreeCollapsed(v => !v)}
                title={treeCollapsed ? 'Expand tree' : 'Collapse tree'}
                style={{
                  width: 22, height: 22, borderRadius: 5,
                  border: '1px solid var(--border2)',
                  background: 'transparent', color: 'var(--text3)',
                  cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
                  flexShrink: 0,
                }}
              >
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  {treeCollapsed
                    ? <path d="m9 18 6-6-6-6"/>
                    : <path d="m15 18-6-6 6-6"/>}
                </svg>
              </button>
            </div>

            {/* Tree content — scrollable */}
            {!treeCollapsed && (
              <div style={{ flex: 1, overflowY: 'auto', overflowX: 'hidden' }}>
                <ContainmentTree roots={roots} />
              </div>
            )}
          </div>

          {/* Diagram area — fills remaining space */}
          <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
            <DiagramPanel projectId={projectId} elements={elements} refreshKey={refreshKey} />
          </div>

        </div>
      )}
    </div>
  );
}
