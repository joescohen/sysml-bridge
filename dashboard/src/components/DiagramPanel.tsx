import { useState, useEffect, useRef, useLayoutEffect } from 'react';
import { IBDViewer } from './IBDViewer';
import { getRepresentations, patchElement, putDocumentation } from '../lib/api';
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
  onRefresh?: () => void;
}

export function DiagramPanel({ projectId, elements, refreshKey, sysonElementId, onClearElementNav, onRefresh }: DiagramPanelProps) {
  const [activeSysOnTab, setActiveSysOnTab] = useState(0);
  const [activeGenTab, setActiveGenTab] = useState(0);
  const [source, setSource] = useState<'syson' | 'generated'>('syson');
  const [representations, setRepresentations] = useState<Representation[]>([]);

  useEffect(() => {
    let cancelled = false;
    setActiveSysOnTab(0);
    setActiveGenTab(0);
    getRepresentations(projectId)
      .then(r => { if (!cancelled) setRepresentations(r); })
      .catch(() => { if (!cancelled) setRepresentations([]); });
    return () => { cancelled = true; };
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

  function buildSysONUrl(base: string, pid: string, repId: string): string | null {
    try {
      const origin = new URL(base);
      if (origin.protocol !== 'http:' && origin.protocol !== 'https:') return null;
      const path = ['projects', encodeURIComponent(pid), 'edit', encodeURIComponent(repId)].join('/');
      return `${origin.origin}/${path}`;
    } catch {
      return null;
    }
  }

  const iframeSrc = activeSysOnTab_?.kind === 'syson'
    ? buildSysONUrl(sysonBase, projectId, activeSysOnTab_.repId)
    : null;

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
              onClick={() => { onClearElementNav?.(); setSource(s); setActiveSysOnTab(0); setActiveGenTab(0); }}
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
          noSysOn ? (
            <EmptyMsg>No SysON diagrams yet. Ask the assistant to create a diagram view.</EmptyMsg>
          ) : iframeSrc ? (
            <iframe
              src={iframeSrc}
              style={{ width: '100%', height: '100%', border: 'none' }}
              title={activeSysOnTab_?.label ?? ''}
            />
          ) : null
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
              {activeGenTab_?.kind === 'requirements' && <GeneratedRequirements model={reqModel} projectId={projectId} onRefresh={onRefresh} />}
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

type BDDEdge = { x1: number; y1: number; x2: number; y2: number };

function GeneratedBDD({ model }: { model: BDDModel }) {
  const byId = new Map(model.blocks.map(b => [b.id, b]));
  const containerRef = useRef<HTMLDivElement>(null);
  const blockRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const [edges, setEdges] = useState<BDDEdge[]>([]);
  const edgesKey = useRef('');

  const maxLevel = Math.max(0, ...model.blocks.map(b => b.level));
  const byLevel: BDDModel['blocks'][] = Array.from({ length: maxLevel + 1 }, () => []);
  for (const b of model.blocks) byLevel[b.level].push(b);

  // Measure block positions after layout and compute SVG edge coordinates.
  // Runs after every render; the key comparison prevents infinite re-renders.
  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const cRect = container.getBoundingClientRect();

    const newEdges: BDDEdge[] = [];
    for (const block of model.blocks) {
      const childEl = blockRefs.current.get(block.id);
      if (!childEl) continue;
      const cR = childEl.getBoundingClientRect();
      for (const parentId of block.parentIds) {
        const parentEl = blockRefs.current.get(parentId);
        if (!parentEl) continue;
        const pR = parentEl.getBoundingClientRect();
        // Arrow: child top-center → parent bottom-center (hollow triangle at parent end)
        newEdges.push({
          x1: Math.round(cR.left + cR.width / 2 - cRect.left),
          y1: Math.round(cR.top - cRect.top),
          x2: Math.round(pR.left + pR.width / 2 - cRect.left),
          y2: Math.round(pR.bottom - cRect.top),
        });
      }
    }

    const key = JSON.stringify(newEdges);
    if (key !== edgesKey.current) {
      edgesKey.current = key;
      setEdges(newEdges);
    }
  });

  if (!model.blocks.length) {
    return (
      <div style={{ padding: 24, textAlign: 'center', color: 'var(--text4)', fontSize: 12 }}>
        No PartDefinition elements found. Ask the assistant to run{' '}
        <code style={{ background: 'var(--surface)', padding: '1px 4px', borderRadius: 3 }}>/mbse-build bdd</code>.
      </div>
    );
  }

  return (
    <div ref={containerRef} style={{ position: 'relative', padding: 20, boxSizing: 'border-box' }}>
      {/* SVG connection layer — sits behind block cards */}
      {edges.length > 0 && (
        <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none', overflow: 'visible', zIndex: 0 }}>
          <defs>
            {/* Hollow triangle arrowhead (UML generalization) pointing in line direction */}
            <marker id="bdd-gen" markerWidth="14" markerHeight="14" refX="13" refY="7" orient="auto">
              <polygon points="1,1 13,7 1,13" fill="var(--surface)" stroke="#6366f1" strokeWidth="1.5" strokeLinejoin="round" />
            </marker>
          </defs>
          {edges.map((e, i) => {
            // Cubic bezier: control points keep x fixed per endpoint so lines fan gracefully
            const cpY1 = e.y1 + (e.y2 - e.y1) * 0.25;
            const cpY2 = e.y1 + (e.y2 - e.y1) * 0.75;
            return (
              <path
                key={i}
                d={`M ${e.x1} ${e.y1} C ${e.x1} ${cpY1}, ${e.x2} ${cpY2}, ${e.x2} ${e.y2}`}
                fill="none"
                stroke="#6366f1"
                strokeWidth="1.5"
                strokeOpacity="0.7"
                markerEnd="url(#bdd-gen)"
              />
            );
          })}
        </svg>
      )}

      {/* Block rows by specialization level */}
      <div style={{ position: 'relative', zIndex: 1 }}>
        {byLevel.map((levelBlocks, level) => (
          <div key={level}>
            {level > 0 && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '36px 0 16px', color: 'var(--text4)', fontSize: 10, letterSpacing: '0.06em' }}>
                <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
                <span style={{ fontFamily: 'monospace', textTransform: 'uppercase' }}>Level {level} — specialization</span>
                <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
              </div>
            )}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, justifyContent: 'center' }}>
              {levelBlocks.map(block => {
                const parents = block.parentIds.map(pid => byId.get(pid)?.name ?? pid);
                return (
                  <div
                    key={block.id}
                    ref={el => { if (el) blockRefs.current.set(block.id, el); else blockRefs.current.delete(block.id); }}
                    style={{
                      border: '1px solid',
                      borderColor: block.parentIds.length ? 'var(--border2)' : 'var(--primary)',
                      borderRadius: 6,
                      background: 'var(--surface)',
                      overflow: 'hidden',
                      boxShadow: block.parentIds.length ? 'none' : '0 0 0 1px rgba(99,102,241,.2)',
                      width: 200,
                      flexShrink: 0,
                    }}
                  >
                    <div style={{ padding: '7px 12px', borderBottom: '1px solid var(--border)', background: block.parentIds.length ? 'transparent' : 'rgba(99,102,241,.07)' }}>
                      <div style={{ fontSize: 9, color: 'var(--primary-text)', marginBottom: 1 }}>«part def»</div>
                      <div style={{ fontSize: 13, fontWeight: 800, color: '#f1f5f9', overflowWrap: 'anywhere' }}>{block.name}</div>
                      {parents.length > 0 && (
                        <div style={{ fontSize: 9, color: 'var(--text4)', fontFamily: 'monospace', marginTop: 2 }}>
                          :{'>'} {parents.join(', ')}
                        </div>
                      )}
                    </div>

                    {block.ports.length > 0 && (
                      <div style={{ padding: '5px 12px', borderBottom: '1px solid var(--border)' }}>
                        <div style={{ fontSize: 9, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 3 }}>Ports</div>
                        {block.ports.map(p => (
                          <div key={p} style={{ fontSize: 11, color: 'var(--text2)', fontFamily: 'monospace', padding: '1px 0' }}>⬡ {p}</div>
                        ))}
                      </div>
                    )}

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
    </div>
  );
}

// ── Requirements renderer ─────────────────────────────────────────────────────

function RequirementCard({
  node, depth, projectId, onRefresh,
}: {
  node: RequirementNode;
  depth: number;
  projectId: string;
  onRefresh?: () => void;
}) {
  const isDef = node.type === 'RequirementDefinition';
  const [expanded, setExpanded] = useState(false);
  const [editName, setEditName] = useState(node.name);
  const [editShortName, setEditShortName] = useState(node.shortName ?? '');
  const [editDoc, setEditDoc] = useState(node.docText ?? '');
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const handleToggle = () => {
    if (!expanded) {
      setEditName(node.name);
      setEditShortName(node.shortName ?? '');
      setEditDoc(node.docText ?? '');
      setSaveError(null);
    }
    setExpanded(v => !v);
  };

  const dirty =
    editName !== node.name ||
    editShortName !== (node.shortName ?? '') ||
    editDoc !== (node.docText ?? '');

  const handleSave = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      const updates: Record<string, unknown> = {};
      if (editName !== node.name) updates.declaredName = editName;
      if (editShortName !== (node.shortName ?? ''))
        updates.declaredShortName = editShortName || null;
      if (Object.keys(updates).length > 0)
        await patchElement(projectId, node.id, updates);
      if (editDoc !== (node.docText ?? ''))
        await putDocumentation(projectId, node.id, editDoc);
      setExpanded(false);
      onRefresh?.();
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const inputStyle: React.CSSProperties = {
    width: '100%', boxSizing: 'border-box',
    background: 'var(--bg)', border: '1px solid var(--border2)',
    borderRadius: 4, color: '#f1f5f9', fontSize: 12,
    padding: '5px 8px', fontFamily: 'inherit',
  };

  return (
    <div style={{ marginLeft: depth * 20 }}>
      <div style={{
        border: '1px solid',
        borderColor: isDef ? 'var(--primary)' : 'var(--border2)',
        borderRadius: 6,
        background: 'var(--surface)',
        marginBottom: 8,
        boxShadow: isDef ? '0 0 0 1px rgba(99,102,241,.2)' : 'none',
        overflow: 'hidden',
      }}>
        {/* Collapsed header — always visible, click to expand */}
        <div
          onClick={handleToggle}
          style={{ padding: '8px 14px', cursor: 'pointer', display: 'flex', alignItems: 'flex-start', gap: 8 }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 9, color: 'var(--primary-text)', marginBottom: 2 }}>
              «{isDef ? 'requirement def' : 'requirement'}»
            </div>
            <div style={{ fontSize: 13, fontWeight: isDef ? 800 : 600, color: '#f1f5f9' }}>{node.name}</div>
            {node.shortName && (
              <div style={{ fontSize: 10, color: 'var(--text4)', fontFamily: 'monospace', marginTop: 1 }}>{node.shortName}</div>
            )}
            {(node.satisfiedBy.length > 0 || node.verifiedBy.length > 0) && (
              <div style={{ marginTop: 5, display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {node.satisfiedBy.map((s, i) => (
                  <span key={i} style={{ fontSize: 10, background: 'rgba(34,197,94,.12)', color: '#4ade80', borderRadius: 3, padding: '1px 6px' }}>✓ {s}</span>
                ))}
                {node.verifiedBy.map((v, i) => (
                  <span key={i} style={{ fontSize: 10, background: 'rgba(96,165,250,.12)', color: '#60a5fa', borderRadius: 3, padding: '1px 6px' }}>⊛ {v}</span>
                ))}
              </div>
            )}
          </div>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"
            style={{ color: 'var(--text4)', flexShrink: 0, marginTop: 4, transition: 'transform 0.15s', transform: expanded ? 'rotate(180deg)' : 'none' }}>
            <path d="m6 9 6 6 6-6"/>
          </svg>
        </div>

        {/* Expanded detail + edit panel */}
        {expanded && (
          <div style={{ borderTop: '1px solid var(--border)', padding: '12px 14px', background: 'rgba(255,255,255,0.02)' }}>
            <label style={{ display: 'block', marginBottom: 10 }}>
              <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 4 }}>Name</div>
              <input value={editName} onChange={e => setEditName(e.target.value)} style={inputStyle} />
            </label>

            <label style={{ display: 'block', marginBottom: 10 }}>
              <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 4 }}>Short Name / ID</div>
              <input value={editShortName} onChange={e => setEditShortName(e.target.value)}
                placeholder="e.g. REQ-001" style={{ ...inputStyle, fontFamily: 'monospace' }} />
            </label>

            <label style={{ display: 'block', marginBottom: 10 }}>
              <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 4 }}>Documentation</div>
              <textarea value={editDoc} onChange={e => setEditDoc(e.target.value)}
                placeholder="Requirement statement…" rows={3}
                style={{ ...inputStyle, resize: 'vertical' }} />
            </label>

            {node.satisfiedBy.length > 0 && (
              <div style={{ marginBottom: 8 }}>
                <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 4 }}>Satisfied By</div>
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {node.satisfiedBy.map((s, i) => (
                    <span key={i} style={{ fontSize: 10, background: 'rgba(34,197,94,.12)', color: '#4ade80', borderRadius: 3, padding: '2px 6px' }}>✓ {s}</span>
                  ))}
                </div>
              </div>
            )}

            {node.verifiedBy.length > 0 && (
              <div style={{ marginBottom: 8 }}>
                <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 4 }}>Verified By</div>
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {node.verifiedBy.map((v, i) => (
                    <span key={i} style={{ fontSize: 10, background: 'rgba(96,165,250,.12)', color: '#60a5fa', borderRadius: 3, padding: '2px 6px' }}>⊛ {v}</span>
                  ))}
                </div>
              </div>
            )}

            <div style={{ marginBottom: 12 }}>
              <div style={{ fontSize: 10, color: 'var(--text4)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 2 }}>Element ID</div>
              <div style={{ fontSize: 10, fontFamily: 'monospace', color: 'var(--text4)', wordBreak: 'break-all' }}>{node.id}</div>
            </div>

            {saveError && (
              <div style={{ marginBottom: 8, fontSize: 11, color: '#f87171', background: 'rgba(248,113,113,.08)', borderRadius: 4, padding: '4px 8px' }}>
                {saveError}
              </div>
            )}

            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={() => { setExpanded(false); setSaveError(null); }} disabled={saving}
                style={{ background: 'none', border: '1px solid var(--border2)', borderRadius: 4, color: 'var(--text3)', cursor: 'pointer', fontSize: 11, padding: '4px 10px' }}>
                Cancel
              </button>
              <button onClick={handleSave} disabled={saving || !dirty}
                style={{
                  background: dirty && !saving ? 'var(--primary)' : 'transparent',
                  border: '1px solid', borderColor: dirty && !saving ? 'var(--primary)' : 'var(--border2)',
                  borderRadius: 4, color: dirty && !saving ? '#fff' : 'var(--text4)',
                  cursor: dirty && !saving ? 'pointer' : 'default',
                  fontSize: 11, padding: '4px 12px', fontWeight: 600,
                }}>
                {saving ? 'Saving…' : 'Save Changes'}
              </button>
            </div>
          </div>
        )}
      </div>

      {node.children.map(child => (
        <RequirementCard key={child.id} node={child} depth={depth + 1} projectId={projectId} onRefresh={onRefresh} />
      ))}
    </div>
  );
}

function GeneratedRequirements({ model, projectId, onRefresh }: { model: RequirementsModel; projectId: string; onRefresh?: () => void }) {
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
      {model.roots.map(r => <RequirementCard key={r.id} node={r} depth={0} projectId={projectId} onRefresh={onRefresh} />)}
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
