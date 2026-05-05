import { describe, expect, it } from 'vitest';
import { buildBDDModel, buildStateMachineModel } from '../diagram-generators';
import type { SysONElement } from '../../types/sysml';

// Hierarchy: pkg → systemDef (PartDefinition) → [guidanceUsage, controlUsage] (PartUsage)
//            guidancePort (PortUsage owned by systemDef)
const elements: SysONElement[] = [
  { '@id': 'pkg', '@type': 'Package', declaredName: 'Vehicle' },
  { '@id': 'systemDef', '@type': 'PartDefinition', declaredName: 'VehicleSystem', owner: { '@id': 'pkg' } },
  { '@id': 'guidanceUsage', '@type': 'PartUsage', declaredName: 'guidance', owner: { '@id': 'systemDef' } },
  { '@id': 'controlUsage', '@type': 'PartUsage', declaredName: 'control', owner: { '@id': 'systemDef' } },
  { '@id': 'navPort', '@type': 'PortUsage', declaredName: 'navData', owner: { '@id': 'systemDef' } },
];

describe('buildBDDModel', () => {
  it('uses top-level PartDefinition as root, PartUsages as children', () => {
    const model = buildBDDModel(elements);

    expect(model.root).toEqual({ id: 'systemDef', name: 'VehicleSystem', childIds: ['guidanceUsage', 'controlUsage'] });
    expect(model.blocks.map(b => b.id)).toEqual(['systemDef', 'guidanceUsage', 'controlUsage']);
    expect(model.blocks.find(b => b.id === 'systemDef')?.stereotype).toBe('part def');
    expect(model.blocks.find(b => b.id === 'guidanceUsage')?.stereotype).toBe('part');
  });

  it('shows ports on the owning block', () => {
    const model = buildBDDModel(elements);
    expect(model.blocks.find(b => b.id === 'systemDef')?.ports).toEqual(['navData']);
  });

  it('returns empty when no PartDefinition elements exist', () => {
    const model = buildBDDModel([{ '@id': 'pkg', '@type': 'Package', declaredName: 'Empty' }]);
    expect(model.blocks).toEqual([]);
    expect(model.root).toBeUndefined();
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
