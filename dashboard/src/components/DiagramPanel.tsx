import { useState, useEffect } from 'react';
import { IBDViewer } from './IBDViewer';
import { getRepresentations } from '../lib/api';
import { buildBDDModel, buildStateMachineModel, type BDDModel, type StateMachineModel } from '../lib/diagram-generators';
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
  const bddRootName = inferBDDRootName(representations);
  const bddModel = buildBDDModel(elements, bddRootName);
  const stateModel = buildStateMachineModel(elements);
  const hasStateRepresentation = representations.some(r => /state/i.test(r.label));
  const hasStateModel = stateModel.states.length > 0 || stateModel.transitions.length > 0;
  const visibleRepresentations = representations.filter((rep, idx, all) =>
    all.findIndex(other => other.label === rep.label) === idx
  );

  type Tab =
    | { key: string; label: string; kind: 'syson'; repId: string }
    | { key: string; label: string; kind: 'bdd' }
    | { key: string; label: string; kind: 'state' }
    | { key: string; label: string; kind: 'ibd' };

  const tabs: Tab[] = [
    ...visibleRepresentations.map(r => ({
      key: r.id,
      label: r.label,
      kind: 'syson' as const,
      repId: r.id,
    })),
    ...(bddModel.blocks.length ? [{ key: 'generated-bdd', label: 'BDD (Generated)', kind: 'bdd' as const }] : []),
    ...(hasStateRepresentation || hasStateModel ? [{ key: 'generated-state', label: 'State (Generated)', kind: 'state' as const }] : []),
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
      {activeTab_?.kind === 'bdd' && <GeneratedBDD model={bddModel} />}
      {activeTab_?.kind === 'state' && <GeneratedStateMachine model={stateModel} />}
      {activeTab_?.kind === 'ibd' && <IBDViewer projectId={projectId} />}
    </div>
  );
}

function GeneratedBDD({ model }: { model: BDDModel }) {
  const childBlocks = model.root
    ? model.root.childIds.map(id => model.blocks.find(block => block.id === id)).filter((block): block is BDDModel['blocks'][number] => Boolean(block))
    : model.blocks;

  return (
    <div style={{ minHeight: 360, border: '1px solid var(--border)', borderRadius: 6, background: 'var(--surface2)', padding: 18 }}>
      {model.root && (
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', marginBottom: 22 }}>
          <div style={{ border: '1px solid var(--primary)', borderRadius: 6, background: 'var(--surface)', padding: '11px 18px', minWidth: 220, textAlign: 'center', boxShadow: '0 0 0 1px rgba(99,102,241,.25)' }}>
            <div style={{ fontSize: 10, color: 'var(--primary-text)' }}>«part def»</div>
            <div style={{ fontSize: 14, fontWeight: 800, color: '#f1f5f9', overflowWrap: 'anywhere' }}>{model.root.name}</div>
          </div>
          <div style={{ width: 1, height: 22, background: 'var(--primary)' }} />
          <div style={{ height: 1, width: '78%', maxWidth: 620, background: 'var(--primary)' }} />
          <div style={{ color: 'var(--primary-text)', fontSize: 10, fontFamily: 'monospace', marginTop: 6 }}>composition: owns subsystems</div>
        </div>
      )}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(190px, 1fr))', gap: 12 }}>
        {childBlocks.map(block => (
          <div key={block.id} style={{ border: '1px solid var(--border2)', borderRadius: 6, background: 'var(--surface)', overflow: 'hidden', position: 'relative' }}>
            {model.root && <div style={{ height: 12, width: 1, background: 'var(--primary)', position: 'absolute', top: 0, left: '50%' }} />}
            <div style={{ padding: '10px 12px', borderBottom: '1px solid var(--border)' }}>
              <div style={{ fontSize: 10, color: 'var(--text3)' }}>«part def»</div>
              <div style={{ fontSize: 12, fontWeight: 700, color: '#f1f5f9', overflowWrap: 'anywhere' }}>{block.name}</div>
            </div>
            <div style={{ padding: 12, minHeight: 58 }}>
              {block.ports.length ? (
                block.ports.map(port => (
                  <div key={port} style={{ fontSize: 11, color: 'var(--text2)', fontFamily: 'monospace', padding: '3px 0', overflowWrap: 'anywhere' }}>
                    port {port}
                  </div>
                ))
              ) : (
                <div style={{ fontSize: 11, color: 'var(--text4)' }}>No ports</div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function inferBDDRootName(representations: Representation[]): string | undefined {
  const bdd = representations.find(r => /\bbdd\b/i.test(r.label));
  const label = bdd?.label ?? representations[0]?.label;
  const firstWord = label?.trim().split(/\s+/)[0];
  return firstWord && !/view\d*/i.test(firstWord) ? firstWord : undefined;
}

function GeneratedStateMachine({ model }: { model: StateMachineModel }) {
  if (!model.states.length && !model.transitions.length) {
    return (
      <div style={{ minHeight: 220, border: '1px solid var(--border)', borderRadius: 6, background: 'var(--surface2)', padding: 24, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center' }}>
        <div>
          <div style={{ fontSize: 13, fontWeight: 700, color: '#f1f5f9', marginBottom: 6 }}>No state machine elements found</div>
          <div style={{ fontSize: 12, color: 'var(--text3)', maxWidth: 360, lineHeight: 1.5 }}>
            A SysON state representation exists, but this model does not currently contain StateUsage, StateDefinition, or TransitionUsage elements.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div style={{ minHeight: 320, border: '1px solid var(--border)', borderRadius: 6, background: 'var(--surface2)', padding: 18 }}>
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', marginBottom: 18 }}>
        {model.states.map(state => (
          <div key={state.id} style={{ border: '1px solid var(--border2)', borderRadius: 6, background: 'var(--surface)', padding: '10px 14px', minWidth: 140 }}>
            <div style={{ fontSize: 10, color: 'var(--text3)' }}>«state»</div>
            <div style={{ fontSize: 13, color: '#f1f5f9', fontWeight: 700, overflowWrap: 'anywhere' }}>{state.name}</div>
          </div>
        ))}
      </div>
      {model.transitions.length > 0 && (
        <div style={{ borderTop: '1px solid var(--border)', paddingTop: 12 }}>
          {model.transitions.map(t => (
            <div key={t.id} style={{ fontSize: 11, color: 'var(--text2)', fontFamily: 'monospace', padding: '4px 0', overflowWrap: 'anywhere' }}>
              {t.sourceId ?? '?'} → {t.targetId ?? '?'} {t.name ? `: ${t.name}` : ''}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
