import { describe, expect, it } from 'vitest';
import { buildBDDModel, buildStateMachineModel } from '../diagram-generators';
import type { SysONElement } from '../../types/sysml';

const elements: SysONElement[] = [
  { '@id': 'pkg', '@type': 'Package', declaredName: 'Vehicle' },
  { '@id': 'guidance', '@type': 'PartDefinition', declaredName: 'Guidance', owner: { '@id': 'pkg' } },
  { '@id': 'control', '@type': 'PartDefinition', declaredName: 'Control', owner: { '@id': 'pkg' } },
  { '@id': 'nav', '@type': 'PortUsage', declaredName: 'navData', owner: { '@id': 'guidance' } },
  { '@id': 'cmd', '@type': 'PortUsage', declaredName: 'command', owner: { '@id': 'control' } },
];

describe('buildBDDModel', () => {
  it('builds named blocks with their directly owned ports', () => {
    const model = buildBDDModel(elements, 'ANGARS');

    expect(model.root).toEqual({ id: 'root:ANGARS', name: 'ANGARS', childIds: ['guidance', 'control'] });
    expect(model.blocks).toEqual([
      { id: 'guidance', name: 'Guidance', ports: ['navData'] },
      { id: 'control', name: 'Control', ports: ['command'] },
    ]);
  });

  it('returns empty blocks when no part definitions exist', () => {
    expect(buildBDDModel([{ '@id': 'pkg', '@type': 'Package', declaredName: 'Empty' }]).blocks).toEqual([]);
  });
});

describe('buildStateMachineModel', () => {
  it('finds state usages and transition usages', () => {
    const model = buildStateMachineModel([
      { '@id': 's1', '@type': 'StateUsage', declaredName: 'Idle' },
      { '@id': 's2', '@type': 'StateUsage', declaredName: 'Refueling' },
      {
        '@id': 't1',
        '@type': 'TransitionUsage',
        declaredName: 'start',
        source: [{ '@id': 's1' }],
        target: [{ '@id': 's2' }],
      } as SysONElement,
    ]);

    expect(model.states.map(s => s.name)).toEqual(['Idle', 'Refueling']);
    expect(model.transitions).toEqual([{ id: 't1', name: 'start', sourceId: 's1', targetId: 's2' }]);
  });
});
