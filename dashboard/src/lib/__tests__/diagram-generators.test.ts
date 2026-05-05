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
  it('uses top-level PartDefinition as root, PartUsages listed as parts', () => {
    const model = buildBDDModel(elements);

    // root is a legacy field — only check id/name, not childIds (which is for backward compat only)
    expect(model.root?.id).toBe('systemDef');
    expect(model.root?.name).toBe('VehicleSystem');
    // blocks contains only PartDefinitions; PartUsages appear in block.parts
    expect(model.blocks.map(b => b.id)).toEqual(['systemDef']);
    expect(model.blocks.find(b => b.id === 'systemDef')?.stereotype).toBe('part def');
    expect(model.blocks.find(b => b.id === 'systemDef')?.parts.map(p => p.name)).toEqual(['guidance', 'control']);
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
  it('finds state usages and transition usages (TransitionUsage)', () => {
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

  it('finds transitions created via SuccessionAsUsage (insertTextualSysMLv2 path)', () => {
    // SysON's REST commit API cannot create new elements; insertTextualSysMLv2 with
    // "succession first X then Y" creates SuccessionAsUsage with persisted source/target.
    const model = buildStateMachineModel([
      { '@id': 's1', '@type': 'StateUsage', declaredName: 'Idle' },
      { '@id': 's2', '@type': 'StateUsage', declaredName: 'Flying' },
      { '@id': 's3', '@type': 'StateUsage', declaredName: 'Landing' },
      {
        '@id': 'su1',
        '@type': 'SuccessionAsUsage',
        declaredName: 'idle_to_flying',
        source: [{ '@id': 's1' }],
        target: [{ '@id': 's2' }],
      } as SysONElement,
      {
        '@id': 'su2',
        '@type': 'SuccessionAsUsage',
        declaredName: 'flying_to_landing',
        source: [{ '@id': 's2' }],
        target: [{ '@id': 's3' }],
      } as SysONElement,
    ]);

    expect(model.states.map(s => s.name)).toEqual(['Idle', 'Flying', 'Landing']);
    expect(model.transitions).toHaveLength(2);
    expect(model.transitions[0]).toEqual({ id: 'su1', name: 'idle_to_flying', sourceId: 's1', targetId: 's2' });
    expect(model.transitions[1]).toEqual({ id: 'su2', name: 'flying_to_landing', sourceId: 's2', targetId: 's3' });
  });

  it('excludes SuccessionAsUsage whose endpoints are not states (activity successions)', () => {
    const model = buildStateMachineModel([
      { '@id': 'a1', '@type': 'ActionUsage', declaredName: 'Compute' },
      { '@id': 'a2', '@type': 'ActionUsage', declaredName: 'Actuate' },
      {
        '@id': 'su1',
        '@type': 'SuccessionAsUsage',
        declaredName: 'compute_to_actuate',
        source: [{ '@id': 'a1' }],
        target: [{ '@id': 'a2' }],
      } as SysONElement,
    ]);

    expect(model.transitions).toHaveLength(0);
  });
});
