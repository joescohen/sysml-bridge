import { useState, useEffect, useCallback } from 'react';
import type { Project, SmapsElement, LocalElement } from '../types/sysml';
import { getElements, getLocalElements, deleteLocalElement } from '../lib/api';
import { DiagramPanel } from './DiagramPanel';

interface ProjectDetailProps {
  project: Project;
  onBack: () => void;
  refreshKey: number;
}

export function ProjectDetail({ project, onBack, refreshKey }: ProjectDetailProps) {
  const [smapsElements, setSmapsElements] = useState<SmapsElement[]>([]);
  const [localElements, setLocalElements] = useState<LocalElement[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [s, l] = await Promise.all([
        getElements(project['@id']),
        getLocalElements(project['@id']),
      ]);
      setSmapsElements(s);
      setLocalElements(l);
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => { load(); }, [load, refreshKey]);

  async function handleDeleteLocal(eid: string) {
    await deleteLocalElement(project['@id'], eid);
    load();
  }

  if (loading) return <div style={{ padding: 32, color: 'var(--text4)', fontSize: 12 }}>Loading model…</div>;

  const typeCounts: Record<string, number> = {};
  for (const e of [...smapsElements, ...localElements]) {
    typeCounts[e['@type']] = (typeCounts[e['@type']] ?? 0) + 1;
  }
  const partDefs = smapsElements.filter(e => e['@type'] === 'PartDefinition');
  const reqDefs  = smapsElements.filter(e => e['@type'] === 'RequirementDefinition');

  const escHtml = (s: string) => s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');

  return (
    <div style={{ padding: '24px 28px', overflowY: 'auto' as const, height: '100%' }}>
      {/* Header */}
      <div style={{ marginBottom: 20 }}>
        <button onClick={onBack} style={{ background: 'none', border: 'none', color: 'var(--text3)', cursor: 'pointer', fontSize: 11, display: 'flex', alignItems: 'center', gap: 5, padding: '4px 0', marginBottom: 6 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="m15 18-6-6 6-6"/></svg>
          All Projects
        </button>
        <div style={{ fontSize: 16, fontWeight: 700, color: '#f1f5f9' }}>{project.name}</div>
        <div style={{ fontSize: 10, fontFamily: 'monospace', color: 'var(--text4)', marginTop: 3 }}>{project['@id'].slice(0, 8)}…</div>
      </div>

      {/* Stats */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 10, marginBottom: 22 }}>
        {[
          { label: 'Elements', value: smapsElements.length, sub: 'in SMAPS' },
          { label: 'Part Defs', value: partDefs.length, sub: 'PartDefinition' },
          { label: 'Requirements', value: reqDefs.length, sub: 'RequirementDef' },
          { label: 'Local', value: localElements.length, sub: 'ports & connectors' },
        ].map(s => (
          <div key={s.label} style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 14 }}>
            <div style={{ fontSize: 10, color: 'var(--text3)', textTransform: 'uppercase', letterSpacing: '0.07em', marginBottom: 5 }}>{s.label}</div>
            <div style={{ fontSize: 26, fontWeight: 700, color: '#f1f5f9' }}>{s.value}</div>
            <div style={{ fontSize: 10, color: 'var(--text4)', marginTop: 3, fontFamily: 'monospace' }}>{s.sub}</div>
          </div>
        ))}
      </div>

      {/* Type chips */}
      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Element Types</div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))', gap: 7, marginBottom: 22 }}>
        {Object.entries(typeCounts).sort((a, b) => b[1] - a[1]).map(([type, count]) => {
          const isLocal = !smapsElements.some(e => e['@type'] === type);
          return (
            <div key={type} style={{ background: 'var(--surface)', border: `1px solid ${isLocal ? 'rgba(245,158,11,.3)' : 'var(--border)'}`, borderRadius: 6, padding: '8px 12px', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ color: isLocal ? '#f59e0b' : 'var(--text2)', fontSize: 11 }}>{type}</span>
              <span style={{ color: 'var(--primary-text)', fontWeight: 700, fontFamily: 'monospace', fontSize: 11 }}>{count}</span>
            </div>
          );
        })}
      </div>

      {/* Diagrams */}
      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>Diagrams</div>
      <DiagramPanel projectId={project['@id']} smapsElements={smapsElements} localElements={localElements} />

      {/* Elements table */}
      <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text3)', marginBottom: 10 }}>All Elements</div>
      <div style={{ background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden', marginBottom: 24 }}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 11.5 }}>
          <thead>
            <tr>{['Name', 'Type', 'ID'].map(h => <th key={h} style={{ textAlign: 'left', padding: '7px 12px', fontSize: 9.5, fontWeight: 700, letterSpacing: '0.09em', textTransform: 'uppercase', color: 'var(--text4)', borderBottom: '1px solid var(--border)' }}>{h}</th>)}</tr>
          </thead>
          <tbody>
            {smapsElements.map(e => (
              <tr key={e['@id']}>
                <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text)' }}>{escHtml(e.declaredName ?? e.name ?? '<unnamed>')}</td>
                <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: '#818cf8', fontFamily: 'monospace', fontSize: 10.5 }}>{e['@type']}</td>
                <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text4)', fontFamily: 'monospace', fontSize: 10 }}>{e['@id'].slice(0, 12)}…</td>
              </tr>
            ))}
            {localElements.map(e => {
              const owner = smapsElements.find(x => x['@id'] === e.owner?.['@id']);
              return (
                <tr key={e['@id']}>
                  <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text)' }}>
                    {escHtml(e.declaredName ?? '<unnamed>')}
                    {owner && <span style={{ color: 'var(--text3)', fontSize: 10, marginLeft: 5 }}>on {escHtml(owner.declaredName ?? '')}</span>}
                  </td>
                  <td style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)' }}>
                    <span style={{ color: '#818cf8', fontFamily: 'monospace', fontSize: 10.5 }}>{e['@type']}</span>
                    <span style={{ display: 'inline-block', fontSize: 9, fontWeight: 600, padding: '1px 5px', borderRadius: 4, marginLeft: 5, background: 'rgba(245,158,11,.12)', color: '#f59e0b', border: '1px solid rgba(245,158,11,.25)' }}>local</span>
                  </td>
                  <td
                    style={{ padding: '6px 12px', borderBottom: '1px solid var(--surface)', color: 'var(--text4)', fontFamily: 'monospace', fontSize: 10, cursor: 'pointer' }}
                    title="Delete local element"
                    onClick={() => handleDeleteLocal(e['@id'])}
                  >
                    {e['@id'].slice(0, 12)}… <span style={{ color: 'var(--text4)', fontSize: 10 }}>×</span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
