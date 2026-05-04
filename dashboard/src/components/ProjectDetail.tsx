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

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setElements(await getElements(project['@id']));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load elements');
      setElements([]);
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => { load(); }, [load, refreshKey]);

  if (loading) return <div style={{ padding: 32, color: 'var(--text4)', fontSize: 12 }}>Loading model…</div>;
  if (error) return <div style={{ padding: 32, color: '#ef4444', fontSize: 12 }}>Error: {error}</div>;

  const nonMembership = elements.filter(e => !e['@type'].endsWith('Membership'));
  const typeCounts: Record<string, number> = {};
  for (const e of nonMembership) {
    typeCounts[e['@type']] = (typeCounts[e['@type']] ?? 0) + 1;
  }

  const roots = buildContainmentTree(elements);

  return (
    <div style={{ padding: '24px 28px', overflowY: 'auto', height: '100%' }}>
      <div style={{ marginBottom: 20 }}>
        <button onClick={onBack} style={{ background: 'none', border: 'none', color: 'var(--text3)', cursor: 'pointer', fontSize: 11, display: 'flex', alignItems: 'center', gap: 5, padding: '4px 0', marginBottom: 6 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="m15 18-6-6 6-6"/></svg>
          All Projects
        </button>
        <div style={{ fontSize: 16, fontWeight: 700, color: '#f1f5f9' }}>{project.name}</div>
        <div style={{ fontSize: 10, fontFamily: 'monospace', color: 'var(--text4)', marginTop: 3 }}>{project['@id'].slice(0, 8)}…</div>
      </div>

      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Diagrams</div>
      <DiagramPanel projectId={project['@id']} elements={elements} refreshKey={refreshKey} />

      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Containment Tree</div>
      <ContainmentTree roots={roots} />

      <details style={{ marginTop: 22 }}>
        <summary style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', cursor: 'pointer', userSelect: 'none', marginBottom: 10 }}>
          Model Stats
        </summary>
        <div style={{ marginTop: 10 }}>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 10, marginBottom: 16 }}>
            {[
              { label: 'Elements', value: nonMembership.length, sub: 'in SysON' },
              { label: 'Types', value: Object.keys(typeCounts).length, sub: 'unique @types' },
              { label: 'Roots', value: roots.length, sub: 'root nodes' },
            ].map(s => (
              <div key={s.label} style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 14 }}>
                <div style={{ fontSize: 10, color: 'var(--text3)', textTransform: 'uppercase', letterSpacing: '0.07em', marginBottom: 5 }}>{s.label}</div>
                <div style={{ fontSize: 26, fontWeight: 700, color: '#f1f5f9' }}>{s.value}</div>
                <div style={{ fontSize: 10, color: 'var(--text4)', marginTop: 3, fontFamily: 'monospace' }}>{s.sub}</div>
              </div>
            ))}
          </div>
          <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 8 }}>Element Types</div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))', gap: 7 }}>
            {Object.entries(typeCounts).sort((a, b) => b[1] - a[1]).map(([type, count]) => (
              <div key={type} style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 6, padding: '8px 12px', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ color: 'var(--text2)', fontSize: 11 }}>{type}</span>
                <span style={{ color: 'var(--primary-text)', fontWeight: 700, fontFamily: 'monospace', fontSize: 11 }}>{count}</span>
              </div>
            ))}
          </div>
        </div>
      </details>
    </div>
  );
}
