import { describe, it, expect } from 'vitest';
import { buildContainmentTree, type TreeNode } from '../containment';

const PKG = {
  '@id': 'pkg-1', '@type': 'Package', declaredName: 'DroneSystem',
  ownedElement: [{ '@id': 'mem-1' }], owner: null,
};
const MEM = {
  '@id': 'mem-1', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'part-1' }], owner: { '@id': 'pkg-1' },
};
const PART = {
  '@id': 'part-1', '@type': 'PartDefinition', declaredName: 'FlightController',
  ownedElement: [{ '@id': 'mem-2' }], owner: { '@id': 'mem-1' },
};
const MEM2 = {
  '@id': 'mem-2', '@type': 'FeatureMembership', declaredName: null,
  ownedElement: [{ '@id': 'port-1' }], owner: { '@id': 'part-1' },
};
const PORT = {
  '@id': 'port-1', '@type': 'PortUsage', declaredName: 'pwr_in',
  ownedElement: [], owner: { '@id': 'mem-2' },
};

const elements = [PKG, MEM, PART, MEM2, PORT];

describe('buildContainmentTree', () => {
  it('skips membership wrappers', () => {
    const roots = buildContainmentTree(elements as any);
    expect(roots).toHaveLength(1);
    expect(roots[0].element['@type']).toBe('Package');
    expect(roots[0].children).toHaveLength(1);
    expect(roots[0].children[0].element['@type']).toBe('PartDefinition');
  });

  it('nests ports under their logical parent', () => {
    const roots = buildContainmentTree(elements as any);
    const part = roots[0].children[0];
    expect(part.children).toHaveLength(1);
    expect(part.children[0].element['@type']).toBe('PortUsage');
    expect(part.children[0].element.declaredName).toBe('pwr_in');
  });

  it('returns empty array for empty input', () => {
    expect(buildContainmentTree([])).toEqual([]);
  });

  it('handles elements with no ownedElement field', () => {
    const lone = { '@id': 'x', '@type': 'PartDefinition', declaredName: 'X' };
    const roots = buildContainmentTree([lone as any]);
    expect(roots).toHaveLength(1);
    expect(roots[0].children).toEqual([]);
  });
});
