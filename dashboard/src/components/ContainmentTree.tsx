import { useState } from 'react';
import type { TreeNode } from '../lib/containment';

const TYPE_ICONS: Record<string, string> = {
  Package: '📦',
  PartDefinition: '🔧',
  PartUsage: '🔩',
  PortUsage: '⚡',
  ActionDefinition: '🎬',
  ActionUsage: '▶',
  StateDefinition: '🔄',
  StateUsage: '🔄',
  RequirementDefinition: '📋',
  RequirementUsage: '📝',
  ConnectionUsage: '🔗',
  ConnectionDefinition: '🔗',
  InterfaceDefinition: '🔌',
  InterfaceUsage: '🔌',
  ItemDefinition: '📎',
  ItemUsage: '📎',
  FlowConnectionUsage: '➡',
  AllocationUsage: '📐',
  ConstraintUsage: '⛓',
  DecisionNode: '◆',
  ForkNode: '⑂',
  JoinNode: '⑃',
  MergeNode: '◇',
  Documentation: '📝',
};

function getIcon(type: string): string {
  return TYPE_ICONS[type] ?? '○';
}

function TreeNodeRow({ node, depth }: { node: TreeNode; depth: number }) {
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
          gap: 6,
          padding: '3px 8px',
          paddingLeft: depth * 18 + 8,
          cursor: hasChildren ? 'pointer' : 'default',
          borderRadius: 4,
          fontSize: 12,
        }}
        onClick={() => hasChildren && setExpanded(e => !e)}
      >
        <span style={{ width: 14, textAlign: 'center', fontSize: 10, color: 'var(--text4)', flexShrink: 0 }}>
          {hasChildren ? (expanded ? '▼' : '▶') : ''}
        </span>
        <span style={{ fontSize: 13, flexShrink: 0 }}>{getIcon(type)}</span>
        <span style={{ color: 'var(--text4)', fontSize: 10, fontFamily: 'monospace', flexShrink: 0 }}>{type}</span>
        <span style={{ color: 'var(--text)', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</span>
      </div>
      {expanded && node.children.map(child => (
        <TreeNodeRow key={child.element['@id']} node={child} depth={depth + 1} />
      ))}
    </>
  );
}

interface ContainmentTreeProps {
  roots: TreeNode[];
}

export function ContainmentTree({ roots }: ContainmentTreeProps) {
  if (!roots.length) {
    return (
      <div style={{ padding: 16, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
        No elements in this project yet.
      </div>
    );
  }

  return (
    <div style={{
      background: 'var(--bg)',
      border: '1px solid var(--border)',
      borderRadius: 8,
      padding: '8px 0',
      marginBottom: 24,
      maxHeight: 400,
      overflowY: 'auto',
    }}>
      {roots.map(root => (
        <TreeNodeRow key={root.element['@id']} node={root} depth={0} />
      ))}
    </div>
  );
}
