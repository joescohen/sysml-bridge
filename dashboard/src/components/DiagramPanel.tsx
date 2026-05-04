import { useState, useEffect } from 'react';
import { IBDViewer } from './IBDViewer';
import { getRepresentations } from '../lib/api';
import type { SysONElement, Representation } from '../types/sysml';

interface DiagramPanelProps {
  projectId: string;
  elements: SysONElement[];
  refreshKey?: number;
}

export function DiagramPanel({ projectId, elements, refreshKey }: DiagramPanelProps) {
  const [activeTab, setActiveTab] = useState(0);
  const [representations, setRepresentations] = useState<Representation[]>([]);

  useEffect(() => {
    setActiveTab(0);
    getRepresentations(projectId).then(setRepresentations).catch(() => setRepresentations([]));
  }, [projectId, refreshKey]);

  const hasPortUsage = elements.some(e => e['@type'] === 'PortUsage');

  type Tab =
    | { key: string; label: string; kind: 'syson'; repId: string }
    | { key: string; label: string; kind: 'ibd' };

  const tabs: Tab[] = [
    ...representations.map(r => ({
      key: r.id,
      label: r.label,
      kind: 'syson' as const,
      repId: r.id,
    })),
    ...(hasPortUsage ? [{ key: 'ibd-rf', label: 'IBD (React Flow)', kind: 'ibd' as const }] : []),
  ];

  const safeIdx = Math.min(activeTab, Math.max(tabs.length - 1, 0));
  const activeTab_ = tabs[safeIdx];

  if (!tabs.length) {
    return (
      <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No diagrams yet. Ask the assistant to create a diagram view.
      </div>
    );
  }

  const sysonBase = import.meta.env.VITE_SYSON_URL ?? 'http://localhost:8080';

  return (
    <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22 }}>
      <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap', marginBottom: 12 }}>
        {tabs.map((t, i) => (
          <button
            key={t.key}
            onClick={() => setActiveTab(i)}
            style={{
              padding: '4px 11px', borderRadius: 5,
              border: '1px solid', borderColor: i === safeIdx ? 'var(--primary)' : 'var(--border)',
              background: i === safeIdx ? 'var(--primary-dim)' : 'transparent',
              color: i === safeIdx ? 'var(--primary-text)' : 'var(--text3)',
              fontSize: 11, cursor: 'pointer',
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      {activeTab_?.kind === 'syson' && (
        // No sandbox: SysON editor requires scripts + same-origin access. Only served on localhost.
        <iframe
          src={`${sysonBase}/projects/${projectId}/edit/${(activeTab_ as Extract<Tab, { kind: 'syson' }>).repId}`}
          style={{
            width: '100%', height: 480, border: '1px solid var(--border)',
            borderRadius: 6, background: '#1a1a2e',
          }}
          title={activeTab_.label}
        />
      )}
      {activeTab_?.kind === 'ibd' && <IBDViewer projectId={projectId} />}
    </div>
  );
}
