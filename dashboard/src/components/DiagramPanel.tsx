import { useState, useEffect } from 'react';
import { IBDViewer } from './IBDViewer';
import { getRepresentations } from '../lib/api';
import {
  buildBDDModel, buildStateMachineModel, buildRequirementsModel, buildActivityModel,
  type BDDModel, type StateMachineModel, type RequirementsModel, type ActivityModel,
  type RequirementNode,
} from '../lib/diagram-generators';
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

  const hasPortUsage    = elements.some(e => e['@type'] === 'PortUsage');
  const bddModel        = buildBDDModel(elements);
  const stateModel      = buildStateMachineModel(elements);
  const reqModel        = buildRequirementsModel(elements);
  const activityModel   = buildActivityModel(elements);

  const hasStateRepresentation = representations.some(r => /state/i.test(r.label));
  const hasStateModel          = stateModel.states.length > 0 || stateModel.transitions.length > 0;

  const visibleRepresentations = representations.filter((rep, idx, all) =>
    all.findIndex(other => other.label === rep.label) === idx
  );

  type SysOnTab = { key: string; label: string; kind: 'syson'; repId: string; targetObjectId?: string };
  type GenTab =
    | { key: string; label: string; kind: 'bdd' }
    | { key: string; label: string; kind: 'state' }
    | { key: string; label: string; kind: 'ibd' }
    | { key: string; label: string; kind: 'requirements' }
    | { key: string; label: string; kind: 'activity' };

  const sysonTabs: SysOnTab[] = visibleRepresentations
    .map(r => ({ key: r.id, label: r.label, kind: 'syson' as const, repId: r.id, targetObjectId: r.targetObjectId }))
    .sort((a, b) => {
      const score = (label: string) => /bdd/i.test(label) ? 1 : 0;
      return score(a.label) - score(b.label);
    });

  const genTabs: GenTab[] = [
    ...(bddModel.blocks.length ? [{ key: 'gen-bdd', label: 'BDD', kind: 'bdd' as const }] : []),
    ...(reqModel.roots.length  ? [{ key: 'gen-req', label: 'Requirements', kind: 'requirements' as const }] : []),
    ...(activityModel.flows.length ? [{ key: 'gen-act', label: 'Activity', kind: 'activity' as const }] : []),
    ...(hasStateRepresentation || hasStateModel ? [{ key: 'gen-state', label: 'State', kind: 'state' as const }] : []),
    ...(hasPortUsage ? [{ key: 'gen-ibd', label: 'IBD', kind: 'ibd' as const }] : []),
  ];

  const matchedSysOnIdx = sysonElementId
    ? sysonTabs.findIndex(t => t.targetObjectId === sysonElementId)
    : -1;

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
            <div style={{ height: '100%', overflow: 'hidden' }}>
              <IBDViewer projectId={projectId} elements={elements} />
            </div>
          ) : (
            <div style={{ height: '100%', overflow: 'auto' }}>
              {activeGenTab_?.kind === 'bdd'          && <GeneratedBDD model={bddModel} />}
              {activeGenTab_?.kind === 'state'        && <GeneratedStateMachine model={stateModel} />}
              {activeGenTab_?.kind === 'requirements' && <GeneratedRequirements model={reqModel} />}
              {activeGenTab_?.kind === 'activity'     && <GeneratedActivity model={activityModel} />}
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

// ── BDD renderer ──────────────────────────────────────────────────────────────

function GeneratedBDD({ model }: { model: BDDModel }) {
  const byId = new Map(model.blocks.map(b => [b.id, b]));

  // Group blocks by level for hierarchical display
  const maxLevel = Math.max(0, ...model.blocks.map(b => b.level));
  const byLevel: BDDModel['blocks'][] = Array.from({ length: maxLevel + 1 }, () => []);
  for (const b of model.blocks) byLevel[b.level].push(b);

  if (!model.blocks.length) {
    return (
      <div style={{ padding: 24, textAlign: 'center', color: 'var(--text4)', fontSize: 12 }}>
        No PartDefinition elements found. Ask the assistant to run{' '}
        <code style={{ background: 'var(--surface)', padding: '1px 4px', borderRadius: 3 }}>/mbse-build bdd</code>.
      </div>
    );
  }

  return (
    <div style={{ padding: 20, boxSizing: 'border-box' }}>
      {byLevel.map((levelBlocks, level) => (
        <div key={level}>
          {level > 0 && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '12px 0 8px', color: 'var(--text4)', fontSize: 10, letterSpacing: '0.06em' }}>
              <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
              <span style={{ fontFamily: 'monospace', textTransform: 'uppercase' }}>Level {level} — specialization</span>
              <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
            </div>
          )}
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 10 }}>
            {levelBlocks.map(block => {
              const parents = block.parentIds.map(pid => byId.get(pid)?.name ?? pid);
              return (
                <div key={block.id} style={{
                  border: '1px solid',
                  borderColor: block.parentIds.length ? 'var(--border2)' : 'var(--primary)',
                  borderRadius: 6,
                  background: 'var(--surface)',
                  overflow: 'hidden',
                  boxShadow: block.parentIds.length ? 'none' : '0 0 0 1px rgba(99,102,241,.2)',
                }}>
                  {/* Header */}
                  <div style={{ padding: '7px 12px', borderBottom: '1px solid var(--border)', background: block.parentIds.length ? 'transparent' : 'rgba(99,102,241,.07)' }}>
                    <div style={{ fontSize: 9, color: 'var(--primary-text)', marginBottom: 1 }}>«part def»</div>
                    <div style={{ fontSize: 13, fontWeight: 800, color: '#f1f5f9', overflowWrap: 'anywhere' }}>{block.name}</div>
                    {parents.length > 0 && (
                      <div style={{ fontSize: 9, color: 'var(--text4)', fontFamily: 'monospace', marginTop: 2 }}>
                        :{'>'} {parents.join(', ')}
                      </div>
                    )}
                  </div>

                  {/* Ports section */}
                  {block.ports.length > 0 && (
                    <div style={{ padding: '5px 12px', borderBottom: '1px solid var(--border)' }}>
                      <div style={{ fontSize: 9, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 3 }}>Ports</div>
                      {block.ports.map(p => (
                        <div key={p} style={{ fontSize: 11, color: 'var(--text2)', fontFamily: 'monospace', padding: '1px 0' }}>⬡ {p}</div>
                      ))}
                    </div>
                  )}

                  {/* Parts section */}
                  {block.parts.length > 0 && (
                    <div style={{ padding: '5px 12px' }}>
                      <div style={{ fontSize: 9, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 3 }}>Parts</div>
                      {block.parts.map((p, i) => (
                        <div key={i} style={{ fontSize: 11, color: 'var(--text3)', fontFamily: 'monospace', padding: '1px 0', overflowWrap: 'anywhere' }}>
                          □ {p.name}{p.typeName ? ` : ${p.typeName}` : ''}
                        </div>
                      ))}
                    </div>
                  )}

                  {!block.ports.length && !block.parts.length && (
                    <div style={{ padding: '5px 12px' }}>
                      <div style={{ fontSize: 11, color: 'var(--text4)' }}>—</div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Requirements renderer ─────────────────────────────────────────────────────

function RequirementCard({ node, depth }: { node: RequirementNode; depth: number }) {
  const isDef = node.type === 'RequirementDefinition';
  return (
    <div style={{ marginLeft: depth * 20 }}>
      <div style={{
        border: '1px solid',
        borderColor: isDef ? 'var(--primary)' : 'var(--border2)',
        borderRadius: 6,
        background: 'var(--surface)',
        padding: '8px 14px',
        marginBottom: 8,
        boxShadow: isDef ? '0 0 0 1px rgba(99,102,241,.2)' : 'none',
      }}>
        <div style={{ fontSize: 9, color: 'var(--primary-text)', marginBottom: 2 }}>
          «{isDef ? 'requirement def' : 'requirement'}»
        </div>
        <div style={{ fontSize: 13, fontWeight: isDef ? 800 : 600, color: '#f1f5f9' }}>{node.name}</div>
        {node.satisfiedBy.length > 0 && (
          <div style={{ marginTop: 5, display: 'flex', gap: 4, flexWrap: 'wrap' }}>
            {node.satisfiedBy.map((s, i) => (
              <span key={i} style={{ fontSize: 10, background: 'rgba(34,197,94,.12)', color: '#4ade80', borderRadius: 3, padding: '1px 6px' }}>
                ✓ {s}
              </span>
            ))}
          </div>
        )}
      </div>
      {node.children.map(child => (
        <RequirementCard key={child.id} node={child} depth={1} />
      ))}
    </div>
  );
}

function GeneratedRequirements({ model }: { model: RequirementsModel }) {
  if (!model.roots.length) {
    return (
      <div style={{ padding: 24, textAlign: 'center', color: 'var(--text4)', fontSize: 12 }}>
        No requirement elements found.
      </div>
    );
  }
  return (
    <div style={{ padding: 20 }}>
      <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 14 }}>
        Requirements — {model.roots.length} root(s)
      </div>
      {model.roots.map(r => <RequirementCard key={r.id} node={r} depth={0} />)}
    </div>
  );
}

// ── Activity / Action renderer ────────────────────────────────────────────────

function ActivityFlowDiagram({ flow }: { flow: ActivityModel['flows'][0] }) {
  const nodeById = new Map(flow.nodes.map(n => [n.id, n]));

  // Topological sort: BFS from start node
  const adj = new Map<string, string[]>();
  for (const e of flow.edges) {
    if (!adj.has(e.fromId)) adj.set(e.fromId, []);
    adj.get(e.fromId)!.push(e.toId);
  }
  const startNode = flow.nodes.find(n => n.kind === 'start');
  const ordered: typeof flow.nodes = [];
  const visited = new Set<string>();
  const bfsQ = startNode ? [startNode.id] : [];
  while (bfsQ.length) {
    const id = bfsQ.shift()!;
    if (visited.has(id)) continue;
    visited.add(id);
    const node = nodeById.get(id);
    if (node) ordered.push(node);
    for (const next of adj.get(id) ?? []) bfsQ.push(next);
  }
  // Add any orphaned nodes not reached
  for (const n of flow.nodes) if (!visited.has(n.id)) ordered.push(n);

  const nodeStyle = (kind: string): React.CSSProperties => {
    if (kind === 'start') return { width: 18, height: 18, borderRadius: '50%', background: 'var(--primary)', flexShrink: 0 };
    if (kind === 'end')   return { width: 18, height: 18, borderRadius: '50%', border: '3px solid var(--primary)', background: 'var(--bg)', flexShrink: 0 };
    if (kind === 'decision') return {
      width: 28, height: 28, transform: 'rotate(45deg)',
      border: '1.5px solid var(--primary)', background: 'var(--surface)', flexShrink: 0,
    };
    if (kind === 'merge') return {
      width: 28, height: 28, transform: 'rotate(45deg)',
      border: '1.5px solid var(--primary)', background: 'rgba(99,102,241,.18)', flexShrink: 0,
    };
    return {
      border: '1px solid var(--primary)', borderRadius: 5,
      background: 'var(--surface)', padding: '6px 12px',
      fontSize: 11, color: '#f1f5f9', fontWeight: 600, whiteSpace: 'nowrap',
    };
  };

  const arrow = (
    <svg width="24" height="16" viewBox="0 0 24 16" style={{ flexShrink: 0 }}>
      <line x1="0" y1="8" x2="18" y2="8" stroke="var(--primary)" strokeWidth="1.5"/>
      <polygon points="18,4 24,8 18,12" fill="var(--primary)"/>
    </svg>
  );

  return (
    <div style={{ marginBottom: 24 }}>
      <div style={{ fontSize: 11, fontWeight: 700, color: 'var(--primary-text)', marginBottom: 12, fontFamily: 'monospace' }}>
        «action def» {flow.name}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
        {ordered.map((node, i) => (
          <div key={node.id} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            {i > 0 && arrow}
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', ...nodeStyle(node.kind) }}>
              {node.kind === 'action' && node.name}
              {node.kind === 'decision' && (
                <span style={{ transform: 'rotate(-45deg)', fontSize: 12 }}>◇</span>
              )}
              {node.kind === 'merge' && (
                <span style={{ transform: 'rotate(-45deg)', fontSize: 12 }}>◆</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function GeneratedActivity({ model }: { model: ActivityModel }) {
  if (!model.flows.length) {
    return (
      <div style={{ padding: 24, textAlign: 'center', color: 'var(--text4)', fontSize: 12 }}>
        No ActionDefinition elements found.
      </div>
    );
  }
  return (
    <div style={{ padding: 20 }}>
      {model.flows.map(flow => <ActivityFlowDiagram key={flow.id} flow={flow} />)}
    </div>
  );
}

// ── State machine renderer (unchanged) ───────────────────────────────────────

function GeneratedStateMachine({ model }: { model: StateMachineModel }) {
  if (!model.states.length && !model.transitions.length) {
    return (
      <div style={{ padding: 24, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center' }}>
        <div>
          <div style={{ fontSize: 13, fontWeight: 700, color: '#f1f5f9', marginBottom: 6 }}>No state machine elements found</div>
          <div style={{ fontSize: 12, color: 'var(--text3)', maxWidth: 360, lineHeight: 1.5 }}>
            Ask the assistant to run{' '}
            <code style={{ background: 'var(--surface)', padding: '1px 4px', borderRadius: 3 }}>/mbse-build state</code>{' '}
            to create StateUsage and TransitionUsage elements.
          </div>
        </div>
      </div>
    );
  }

  const nameById = new Map(model.states.map(s => [s.id, s.name]));

  return (
    <div style={{ padding: 18 }}>
      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', marginBottom: 20 }}>
        {model.states.map(state => (
          <div key={state.id} style={{
            border: '1px solid var(--primary)', borderRadius: 8, background: 'var(--surface)',
            padding: '10px 16px', minWidth: 130, textAlign: 'center',
            boxShadow: '0 0 0 1px rgba(99,102,241,.2)',
          }}>
            <div style={{ fontSize: 9, color: 'var(--primary-text)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 3 }}>«state»</div>
            <div style={{ fontSize: 13, color: '#f1f5f9', fontWeight: 700, overflowWrap: 'anywhere' }}>{state.name}</div>
          </div>
        ))}
      </div>
      {model.transitions.length > 0 && (
        <div style={{ borderTop: '1px solid var(--border)', paddingTop: 14 }}>
          <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 8 }}>Transitions</div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {model.transitions.map(t => {
              const srcName = nameById.get(t.sourceId ?? '') ?? t.sourceId ?? '?';
              const tgtName = nameById.get(t.targetId ?? '') ?? t.targetId ?? '?';
              return (
                <div key={t.id} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span style={{ fontSize: 11, color: 'var(--text2)', background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 4, padding: '3px 8px' }}>{srcName}</span>
                  <svg width="28" height="14" viewBox="0 0 28 14" style={{ flexShrink: 0 }}>
                    <line x1="0" y1="7" x2="22" y2="7" stroke="var(--primary)" strokeWidth="1.5"/>
                    <polygon points="22,3 28,7 22,11" fill="var(--primary)"/>
                  </svg>
                  <span style={{ fontSize: 11, color: 'var(--text2)', background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 4, padding: '3px 8px' }}>{tgtName}</span>
                  {t.name && <span style={{ fontSize: 10, color: 'var(--text4)', fontStyle: 'italic' }}>{t.name}</span>}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
