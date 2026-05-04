import type { SysONElement } from '../types/sysml';

export interface TreeNode {
  element: SysONElement;
  children: TreeNode[];
}

function isMembership(type: string): boolean {
  return type.endsWith('Membership');
}

export function buildContainmentTree(elements: SysONElement[]): TreeNode[] {
  const byId = new Map<string, SysONElement>();
  for (const el of elements) byId.set(el['@id'], el);

  const childrenOf = new Map<string, SysONElement[]>();
  for (const el of elements) {
    const ownerId = el.owner?.['@id'];
    if (!ownerId) continue;
    if (!childrenOf.has(ownerId)) childrenOf.set(ownerId, []);
    childrenOf.get(ownerId)!.push(el);
  }

  function buildNode(el: SysONElement): TreeNode | null {
    if (isMembership(el['@type'])) return null;
    const directChildren = childrenOf.get(el['@id']) ?? [];
    const logicalChildren: TreeNode[] = [];
    for (const child of directChildren) {
      if (isMembership(child['@type'])) {
        const grandchildren = childrenOf.get(child['@id']) ?? [];
        for (const gc of grandchildren) {
          const node = buildNode(gc);
          if (node) logicalChildren.push(node);
        }
      } else {
        const node = buildNode(child);
        if (node) logicalChildren.push(node);
      }
    }
    return { element: el, children: logicalChildren };
  }

  const roots: TreeNode[] = [];
  for (const el of elements) {
    if (el.owner === null || el.owner === undefined) {
      if (!isMembership(el['@type'])) {
        const node = buildNode(el);
        if (node) roots.push(node);
      }
    }
  }

  if (roots.length === 0) {
    const hasOwner = new Set(elements.filter(e => e.owner).map(e => e['@id']));
    for (const el of elements) {
      const ownerId = el.owner?.['@id'];
      if (ownerId && !byId.has(ownerId) && !isMembership(el['@type'])) {
        const node = buildNode(el);
        if (node) roots.push(node);
      }
    }
  }

  return roots;
}
