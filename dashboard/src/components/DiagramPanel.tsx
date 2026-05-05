import { useState, useEffect } from 'react';
import { IBDViewer } from './IBDViewer';
import { getRepresentations } from '../lib/api';
import { buildBDDModel, buildStateMachineModel, type BDDModel, type StateMachineModel } from '../lib/diagram-generators';
import type { SysONElement, Representation } from '../types/sysml';

interface DiagramPanelProps {
  projectId: string;
  elements: SysONElement[];
  refreshKey?: number;
  sysonElementId?: string | null;
  onClearElementNav?: () => void;
}

export function DiagramPanel({ projectId, elements, refreshKey, sysonElementId, onClearElementNav }: DiagramPanelProps) {
  const [activeSysOnTab, setActiveSysOnTab] = useState(0);
  const [activeGenTab, setActiveGenTab] = useState(0);
  const [source, setSource] = useState<'syson' | 'generated'>('syson');
  const [representations, setRepresentations] = useState<Representation[]>([]);

  useEffect(() => {
    setActiveSysOnTab(0);
    setActiveGenTab(0);
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

  type SysOnTab = { key: string; label: string; kind: 'syson'; repId: string; targetObjectId?: string };
  type GenTab =
    | { key: string; label: string; kind: 'bdd' }
    | { key: string; label: string; kind: 'state' }
    | { key: string; label: string; kind: 'ibd' };

  const sysonTabs: SysOnTab[] = visibleRepresentations.map(r => ({
    key: r.id, label: r.label, kind: 'syson', repId: r.id, targetObjectId: r.targetObjectId,
  }));

  const genTabs: GenTab[] = [
    ...(bddModel.blocks.length ? [{ key: 'gen-bdd', label: 'BDD', kind: 'bdd' as const }] : []),
    ...(hasStateRepresentation || hasStateModel ? [{ key: 'gen-state', label: 'State Machine', kind: 'state' as const }] : []),
    ...(hasPortUsage ? [{ key: 'gen-ibd', label: 'IBD', kind: 'ibd' as const }] : []),
  ];

  // When element nav is active, find the matching representation by targetObjectId
  const matchedSysOnIdx = sysonElementId
    ? sysonTabs.findIndex(t => t.targetObjectId === sysonElementId)
    : -1;

  // Auto-switch source if one side is empty; element nav forces syson
  const effectiveSource = sysonElementId
    ? 'syson'
    : (source === 'syson' && !sysonTabs.length && genTabs.length)
    ? 'generated'
    : (source === 'generated' && !genTabs.length && sysonTabs.length)
    ? 'syson'
    : source;

  const safeSysOn = sysonElementId && matchedSysOnIdx >= 0
    ? matchedSysOnIdx
    : Math.min(activeSysOnTab, Math.max(sysonTabs.length - 1, 0));
  const safeGen   = Math.min(activeGenTab, Math.max(genTabs.length - 1, 0));

  const activeSysOnTab_ = sysonTabs[safeSysOn];
  const activeGenTab_   = genTabs[safeGen];

  const noSysOn = !sysonTabs.length;
  const noGen   = !genTabs.length;

  const sysonBase = import.meta.env.VITE_SYSON_URL ?? 'http://localhost:8080';

  const tabBtn = (label: string, active: boolean, onClick: () => void) => (
    <button
      onClick={onClick}
      style={{
        padding: '3px 10px', borderRadius: 4,
        border: '1px solid', borderColor: active ? 'var(--primary)' : 'var(--border)',
        background: active ? 'var(--primary-dim)' : 'transparent',
        color: active ? 'var(--primary-text)' : 'var(--text3)',
        fontSize: 11, cursor: 'pointer',
      }}
    >
      {label}
    </button>
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      {/* Source toggle + tabs bar */}
      <div style={{ flexShrink: 0, padding: '8px 14px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 10, background: 'var(--surface)' }}>
        {/* SysON / Generated pill toggle */}
        <div style={{ display: 'flex', border: '1px solid var(--border2)', borderRadius: 6, overflow: 'hidden', flexShrink: 0 }}>
          {(['syson', 'generated'] as const).map(s => (
            <button
              key={s}
              onClick={() => { onClearElementNav?.(); setSource(s); }}
              style={{
                padding: '4px 10px', fontSize: 10, fontWeight: 700,
                letterSpacing: '0.07em', textTransform: 'uppercase',
                border: 'none', cursor: 'pointer',
                background: effectiveSource === s ? 'var(--primary)' : 'transparent',
                color: effectiveSource === s ? '#fff' : 'var(--text3)',
              }}
            >
              {s === 'syson' ? 'SysON' : 'Generated'}
            </button>
          ))}
        </div>

        <div style={{ width: 1, height: 18, background: 'var(--border2)' }} />

        {/* Tabs for current source */}
        <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap', flex: 1 }}>
          {effectiveSource === 'syson' && sysonTabs.map((t, i) => (
            <span key={t.key}>{tabBtn(t.label, i === safeSysOn, () => { onClearElementNav?.(); setActiveSysOnTab(i); })}</span>
          ))}
          {effectiveSource === 'generated' && genTabs.map((t, i) => (
            <span key={t.key}>{tabBtn(t.label, i === safeGen, () => { onClearElementNav?.(); setActiveGenTab(i); })}</span>
          ))}
        </div>
      </div>

      {/* Diagram content */}
      <div style={{ flex: 1, minHeight: 0, overflow: 'hidden', position: 'relative' }}>
        {effectiveSource === 'syson' && (
          sysonElementId && matchedSysOnIdx < 0 ? (
            <EmptyMsg>No SysON diagram for this element. Try creating one from the assistant.</EmptyMsg>
          ) : noSysOn ? (
            <EmptyMsg>No SysON diagrams yet. Ask the assistant to create a diagram view.</EmptyMsg>
          ) : activeSysOnTab_?.kind === 'syson' && (
            <iframe
              src={`${sysonBase}/projects/${projectId}/edit/${activeSysOnTab_.repId}`}
              style={{ width: '100%', height: '100%', border: 'none' }}
              title={activeSysOnTab_.label}
            />
          )
        )}

        {effectiveSource === 'generated' && (
          noGen ? (
            <EmptyMsg>No generated diagrams available for this model.</EmptyMsg>
          ) : activeGenTab_?.kind === 'ibd' ? (
            // ReactFlow needs overflow:hidden — not auto — to size itself
            <div style={{ height: '100%', overflow: 'hidden' }}>
              <IBDViewer projectId={projectId} elements={elements} />
            </div>
          ) : (
            <div style={{ height: '100%', overflow: 'auto' }}>
              {activeGenTab_?.kind === 'bdd'   && <GeneratedBDD model={bddModel} />}
              {activeGenTab_?.kind === 'state' && <GeneratedStateMachine model={stateModel} />}
            </div>
          )
        )}
      </div>
    </div>
  );
}

function EmptyMsg({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text4)', fontSize: 12, textAlign: 'center', padding: 24 }}>
      {children}
    </div>
  );
}

function GeneratedBDD({ model }: { model: BDDModel }) {
  const childBlocks = model.root
    ? model.root.childIds.map(id => model.blocks.find(b => b.id === id)).filter((b): b is BDDModel['blocks'][number] => Boolean(b))
    : model.blocks;

  // Fixed-height connector zone: bar sits at midpoint (ZONE/2 from top).
  // Child stubs extend upward by exactly ZONE/2 so they touch the bar.
  const ZONE = 52;
  const BAR_Y = ZONE / 2; // 26px from top of zone = bar y position

  return (
    <div style={{ padding: 20 }}>
      {model.root && (
        <>
          {/* Root block */}
          <div style={{ display: 'flex', justifyContent: 'center' }}>
            <div style={{ border: '1px solid var(--primary)', borderRadius: 6, background: 'var(--surface)', padding: '11px 18px', minWidth: 220, textAlign: 'center', boxShadow: '0 0 0 1px rgba(99,102,241,.25)' }}>
              <div style={{ fontSize: 10, color: 'var(--primary-text)' }}>«part def»</div>
              <div style={{ fontSize: 14, fontWeight: 800, color: '#f1f5f9', overflowWrap: 'anywhere' }}>{model.root.name}</div>
            </div>
          </div>

          {/* Connector zone: vertical stem + horizontal bar + label */}
          <div style={{ position: 'relative', height: ZONE }}>
            {/* Vertical from root center down to bar */}
            <div style={{ position: 'absolute', top: 0, left: '50%', width: 1, height: BAR_Y, background: 'var(--primary)' }} />
            {/* Horizontal bar */}
            <div style={{ position: 'absolute', top: BAR_Y, left: '8%', right: '8%', height: 1, background: 'var(--primary)' }} />
            {/* Label below bar */}
            <div style={{ position: 'absolute', top: BAR_Y + 5, left: 0, right: 0, textAlign: 'center', color: 'var(--primary-text)', fontSize: 10, fontFamily: 'monospace' }}>
              composition: owns subsystems
            </div>
          </div>
        </>
      )}

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(190px, 1fr))', gap: 12 }}>
        {childBlocks.map(block => (
          <div key={block.id} style={{ border: '1px solid var(--border2)', borderRadius: 6, background: 'var(--surface)', overflow: 'visible', position: 'relative' }}>
            {/* Stub extends upward from card top to the horizontal bar */}
            {model.root && (
              <div style={{ position: 'absolute', bottom: '100%', left: 'calc(50% - 0.5px)', width: 1, height: BAR_Y, background: 'var(--primary)' }} />
            )}
            <div style={{ borderRadius: 6, overflow: 'hidden' }}>
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
      <div style={{ padding: 24, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center' }}>
        <div>
          <div style={{ fontSize: 13, fontWeight: 700, color: '#f1f5f9', marginBottom: 6 }}>No state machine elements found</div>
          <div style={{ fontSize: 12, color: 'var(--text3)', maxWidth: 360, lineHeight: 1.5 }}>
            A SysON state representation exists, but this model does not contain StateUsage, StateDefinition, or TransitionUsage elements.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div style={{ padding: 18 }}>
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
