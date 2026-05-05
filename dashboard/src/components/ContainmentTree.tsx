import { useState } from 'react';
import type { TreeNode } from '../lib/containment';

// ── Cameo-style SVG icons ─────────────────────────────────────────────────────
type IconProps = { color: string };

const icons: Record<string, (p: IconProps) => JSX.Element> = {
  Package: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M1 5h12v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V5z"/>
      <path d="M1 5V4a1 1 0 0 1 1-1h2.5L6 4.5h7a.5.5 0 0 1 .5.5"/>
    </svg>
  ),
  PartDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="1" y="1.5" width="12" height="11" rx="1"/>
      <line x1="1" y1="5.5" x2="13" y2="5.5"/>
    </svg>
  ),
  PartUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="2" y="2" width="10" height="10" rx="1"/>
      <line x1="2" y1="5.5" x2="12" y2="5.5"/>
      <line x1="5" y1="5.5" x2="5" y2="12"/>
    </svg>
  ),
  PortUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="3" y="3" width="8" height="8" rx="1"/>
      <line x1="7" y1="3" x2="7" y2="1"/>
      <line x1="7" y1="11" x2="7" y2="13"/>
    </svg>
  ),
  PortDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="3" y="3" width="8" height="8" rx="1"/>
      <line x1="7" y1="3" x2="7" y2="1"/>
      <line x1="7" y1="11" x2="7" y2="13"/>
    </svg>
  ),
  ActionDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="1" y="3" width="12" height="8" rx="3"/>
      <path d="m6 5.5 2.5 1.5-2.5 1.5"/>
    </svg>
  ),
  ActionUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="1.5" y="3.5" width="11" height="7" rx="2.5"/>
      <path d="m5.5 5.5 2.5 1.5-2.5 1.5"/>
    </svg>
  ),
  StateDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="1" y="3" width="12" height="8" rx="4"/>
    </svg>
  ),
  StateUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="1.5" y="3.5" width="11" height="7" rx="3.5"/>
      <circle cx="7" cy="7" r="1.5" fill={color} stroke="none"/>
    </svg>
  ),
  ConnectionUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <line x1="1" y1="7" x2="10" y2="7" strokeDasharray="2 1.5"/>
      <path d="m8 5 3 2-3 2"/>
    </svg>
  ),
  ConnectionDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <path d="m3 5 3 2-3 2"/>
      <line x1="3" y1="7" x2="11" y2="7" strokeDasharray="2 1.5"/>
      <path d="m11 5-3 2 3 2"/>
    </svg>
  ),
  InterfaceDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="4" y="2" width="6" height="7" rx="1"/>
      <line x1="7" y1="9" x2="7" y2="12"/>
      <line x1="5" y1="12" x2="9" y2="12"/>
      <line x1="5.5" y1="2" x2="5.5" y2="0.5"/>
      <line x1="8.5" y1="2" x2="8.5" y2="0.5"/>
    </svg>
  ),
  InterfaceUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="4.5" y="2.5" width="5" height="6" rx="1"/>
      <line x1="7" y1="8.5" x2="7" y2="12"/>
      <line x1="5" y1="12" x2="9" y2="12"/>
      <line x1="5.5" y1="2.5" x2="5.5" y2="1"/>
      <line x1="8.5" y1="2.5" x2="8.5" y2="1"/>
    </svg>
  ),
  ItemDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M7 1.5 12.5 7 7 12.5 1.5 7z"/>
    </svg>
  ),
  ItemUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M7 2.5 11.5 7 7 11.5 2.5 7z"/>
    </svg>
  ),
  FlowConnectionUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <line x1="1" y1="7" x2="11" y2="7"/>
      <path d="m9 4.5 3 2.5-3 2.5"/>
    </svg>
  ),
  RequirementDefinition: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="2" y="1.5" width="10" height="11" rx="1"/>
      <line x1="4.5" y1="5.5" x2="9.5" y2="5.5"/>
      <line x1="4.5" y1="8" x2="9.5" y2="8"/>
      <line x1="4.5" y1="10.5" x2="7.5" y2="10.5"/>
    </svg>
  ),
  RequirementUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="2.5" y="2" width="9" height="10" rx="1"/>
      <line x1="4.5" y1="6" x2="9.5" y2="6"/>
      <line x1="4.5" y1="8.5" x2="9.5" y2="8.5"/>
    </svg>
  ),
  AllocationUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <line x1="2" y1="5" x2="12" y2="5"/>
      <line x1="2" y1="9" x2="12" y2="9"/>
      <path d="m10 3 2 2-2 2"/>
      <path d="m4 7-2 2 2 2"/>
    </svg>
  ),
  ConstraintUsage: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <path d="M5.5 2c-1.5 0-2.5.7-2.5 1.5V6.5c0 .6-.5 1-1 .5"/>
      <path d="M8.5 2c1.5 0 2.5.7 2.5 1.5V6.5c0 .6.5 1 1 .5"/>
      <path d="M5.5 12c-1.5 0-2.5-.7-2.5-1.5V7.5c0-.6-.5-1-1-.5"/>
      <path d="M8.5 12c1.5 0 2.5-.7 2.5-1.5V7.5c0-.6.5-1 1-.5"/>
    </svg>
  ),
  DecisionNode: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M7 2 12 7 7 12 2 7z"/>
    </svg>
  ),
  ForkNode: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="2.5" strokeLinecap="round">
      <line x1="1" y1="7" x2="13" y2="7"/>
    </svg>
  ),
  JoinNode: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="2.5" strokeLinecap="round">
      <line x1="1" y1="7" x2="13" y2="7"/>
    </svg>
  ),
  MergeNode: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M7 2 12 7 7 12 2 7z"/>
    </svg>
  ),
  Documentation: ({ color }) => (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <path d="M3 1h5.5L11 3.5V13a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1z"/>
      <path d="M8.5 1v3H11"/>
      <line x1="4" y1="7.5" x2="10" y2="7.5"/>
      <line x1="4" y1="10" x2="8" y2="10"/>
    </svg>
  ),
};

const TYPE_COLORS: Record<string, string> = {
  Package: '#f59e0b',
  PartDefinition: '#6366f1',
  PartUsage: '#818cf8',
  PortUsage: '#14b8a6',
  PortDefinition: '#14b8a6',
  ActionDefinition: '#f97316',
  ActionUsage: '#fb923c',
  StateDefinition: '#a855f7',
  StateUsage: '#c084fc',
  ConnectionUsage: '#22c55e',
  ConnectionDefinition: '#4ade80',
  InterfaceDefinition: '#3b82f6',
  InterfaceUsage: '#60a5fa',
  ItemDefinition: '#eab308',
  ItemUsage: '#fbbf24',
  FlowConnectionUsage: '#06b6d4',
  RequirementDefinition: '#f43f5e',
  RequirementUsage: '#fb7185',
  AllocationUsage: '#64748b',
  ConstraintUsage: '#94a3b8',
  DecisionNode: '#f59e0b',
  ForkNode: '#94a3b8',
  JoinNode: '#94a3b8',
  MergeNode: '#f59e0b',
  Documentation: '#64748b',
};

function TypeIcon({ type }: { type: string }) {
  const color = TYPE_COLORS[type] ?? '#475569';
  const Icon = icons[type];
  if (Icon) return <Icon color={color} />;
  // Generic fallback: small rounded rect
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={color} strokeWidth="1.4" strokeLinecap="round">
      <rect x="2" y="3" width="10" height="8" rx="1.5"/>
    </svg>
  );
}

function TreeNodeRow({ node, depth, onElementClick }: { node: TreeNode; depth: number; onElementClick?: (elementId: string) => void }) {
  const [expanded, setExpanded] = useState(depth < 2);
  const hasChildren = node.children.length > 0;
  const name = node.element.declaredName ?? node.element.name ?? '<unnamed>';
  const type = node.element['@type'];

  return (
    <>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 5,
          padding: '2.5px 8px',
          paddingLeft: depth * 16 + 8,
          cursor: 'pointer',
          borderRadius: 3,
          fontSize: 12,
        }}
        className="tree-row"
        title="Open in SysON"
        onClick={() => onElementClick?.(node.element['@id'])}
      >
        <span
          style={{ width: 12, textAlign: 'center', fontSize: 9, color: 'var(--text4)', flexShrink: 0 }}
          onClick={e => { e.stopPropagation(); hasChildren && setExpanded(ex => !ex); }}
        >
          {hasChildren ? (expanded ? '▾' : '▸') : ''}
        </span>
        <span style={{ flexShrink: 0, display: 'flex', alignItems: 'center' }}>
          <TypeIcon type={type} />
        </span>
        <span style={{ color: 'var(--text)', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>{name}</span>
        <span style={{ color: 'var(--text4)', fontSize: 9, fontFamily: 'monospace', flexShrink: 0, opacity: 0.6 }}>{type.replace(/(Definition|Usage)$/, m => m === 'Definition' ? ' def' : '')}</span>
      </div>
      {expanded && node.children.map(child => (
        <TreeNodeRow key={child.element['@id']} node={child} depth={depth + 1} onElementClick={onElementClick} />
      ))}
    </>
  );
}

interface ContainmentTreeProps {
  roots: TreeNode[];
  onElementClick?: (elementId: string) => void;
}

export function ContainmentTree({ roots, onElementClick }: ContainmentTreeProps) {
  if (!roots.length) {
    return (
      <div style={{ padding: 16, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No elements in this project yet.
      </div>
    );
  }

  return (
    <div style={{ padding: '4px 0' }}>
      {roots.map(root => (
        <TreeNodeRow key={root.element['@id']} node={root} depth={0} onElementClick={onElementClick} />
      ))}
    </div>
  );
}
