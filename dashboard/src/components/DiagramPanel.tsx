import { useState, useEffect, useRef } from 'react';
import mermaid from 'mermaid';
import { IBDViewer } from './IBDViewer';
import { getDiagrams, deleteDiagram } from '../lib/api';
import type { SmapsElement, LocalElement, StoredDiagram } from '../types/sysml';

mermaid.initialize({
  startOnLoad: false, theme: 'dark',
  themeVariables: { background: '#161622', primaryColor: '#1e2030', primaryTextColor: '#e2e8f0', lineColor: '#475569', fontSize: '12px' },
});

function MermaidViewer({ code, id }: { code: string; id: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const uid = `mermaid-${id}-${Date.now()}`;
    mermaid.render(uid, code)
      .then(({ svg }) => { if (ref.current) ref.current.innerHTML = svg; })
      .catch(err => { if (ref.current) ref.current.innerHTML = `<div style="color:#ef4444;font-size:11px;padding:8px">Render error: ${String(err.message)}</div>`; });
  }, [code, id]);
  return <div ref={ref} />;
}

interface DiagramPanelProps {
  projectId: string;
  smapsElements: SmapsElement[];
  localElements: LocalElement[];
}

export function DiagramPanel({ projectId, smapsElements, localElements }: DiagramPanelProps) {
  const [activeTab, setActiveTab] = useState(0);
  const [stored, setStored] = useState<StoredDiagram[]>([]);

  useEffect(() => {
    getDiagrams(projectId).then(setStored).catch(() => setStored([]));
  }, [projectId]);

  const hasProxyPorts = localElements.some(e => e['@type'] === 'ProxyPortUsage');
  const partDefs = smapsElements.filter(e => e['@type'] === 'PartDefinition');

  const bddCode = partDefs.length
    ? ['classDiagram', ...partDefs.map(p => `    class ${(p.declaredName ?? 'Unknown').replace(/\s+/g, '_').replace(/[^a-zA-Z0-9_]/g, '')} {\n        <<part def>>\n    }`)].join('\n')
    : null;

  type Tab = { key: string; label: string; kind: 'ibd' | 'bdd' | 'stored'; storedIdx?: number };

  const tabs: Tab[] = [
    ...(hasProxyPorts ? [{ key: 'ibd', label: 'IBD', kind: 'ibd' as const }] : []),
    ...(bddCode ? [{ key: 'bdd', label: 'BDD', kind: 'bdd' as const }] : []),
    ...stored.map((d, i) => ({ key: `s${i}`, label: d.type, kind: 'stored' as const, storedIdx: i })),
  ];

  const safeIdx = Math.min(activeTab, tabs.length - 1);
  const activeTab_ = tabs[safeIdx];

  async function handleDeleteStored(e: React.MouseEvent, idx: number) {
    e.stopPropagation();
    await deleteDiagram(projectId, idx);
    setStored(prev => prev.filter((_, i) => i !== idx));
    setActiveTab(0);
  }

  if (!tabs.length) {
    return (
      <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No diagrams yet. Ask the assistant to build an IBD.
      </div>
    );
  }

  return (
    <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22 }}>
      <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap' as const, marginBottom: 12 }}>
        {tabs.map((t, i) => (
          <button
            key={t.key}
            onClick={() => setActiveTab(i)}
            style={{ padding: '4px 11px', borderRadius: 5, border: '1px solid', borderColor: i === safeIdx ? 'var(--primary)' : 'var(--border)', background: i === safeIdx ? 'var(--primary-dim)' : 'transparent', color: i === safeIdx ? 'var(--primary-text)' : 'var(--text3)', fontSize: 11, cursor: 'pointer', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          >
            {t.label}
            {t.kind === 'stored' && (
              <span onClick={e => handleDeleteStored(e, t.storedIdx!)} style={{ fontSize: 13, color: 'var(--text4)', cursor: 'pointer', lineHeight: 1 }}>×</span>
            )}
          </button>
        ))}
      </div>

      {activeTab_?.kind === 'ibd' && <IBDViewer projectId={projectId} />}
      {activeTab_?.kind === 'bdd' && bddCode && <MermaidViewer code={bddCode} id={`bdd-${projectId}`} />}
      {activeTab_?.kind === 'stored' && activeTab_.storedIdx !== undefined && stored[activeTab_.storedIdx] && (
        <MermaidViewer code={stored[activeTab_.storedIdx].mermaid} id={`stored-${activeTab_.storedIdx}`} />
      )}
    </div>
  );
}
