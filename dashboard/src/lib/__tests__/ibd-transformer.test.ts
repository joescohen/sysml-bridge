import { describe, it, expect } from 'vitest';
import { transformToIBD } from '../ibd-transformer';
import type { SysONElement } from '../../types/sysml';

const BLOCK_A: SysONElement = {
  '@id': 'block-a', '@type': 'PartDefinition', declaredName: 'Battery',
  ownedElement: [{ '@id': 'mem-port-out' }], owner: { '@id': 'pkg' },
};
const MEM_PORT_OUT: SysONElement = {
  '@id': 'mem-port-out', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'port-out' }], owner: { '@id': 'block-a' },
};
const PORT_OUT: SysONElement = {
  '@id': 'port-out', '@type': 'PortUsage', declaredName: 'pwr_out',
  ownedElement: [], owner: { '@id': 'mem-port-out' },
};

const BLOCK_B: SysONElement = {
  '@id': 'block-b', '@type': 'PartDefinition', declaredName: 'Motor',
  ownedElement: [{ '@id': 'mem-port-in' }], owner: { '@id': 'pkg' },
};
const MEM_PORT_IN: SysONElement = {
  '@id': 'mem-port-in', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'port-in' }], owner: { '@id': 'block-b' },
};
const PORT_IN: SysONElement = {
  '@id': 'port-in', '@type': 'PortUsage', declaredName: 'pwr_in',
  ownedElement: [], owner: { '@id': 'mem-port-in' },
};

const MEM_CONN: SysONElement = {
  '@id': 'mem-conn', '@type': 'OwningMembership', declaredName: null,
  ownedElement: [{ '@id': 'conn-1' }], owner: { '@id': 'block-a' },
};
const CONN: SysONElement = {
  '@id': 'conn-1', '@type': 'ConnectionUsage', declaredName: 'powerLine',
  ownedElement: [], owner: { '@id': 'mem-conn' },
  connectorEnd: [
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-out' } },
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-in' } },
  ],
} as SysONElement & { connectorEnd: Array<{ '@type': string; connectedFeature: { '@id': string } }> };

const ALL = [BLOCK_A, MEM_PORT_OUT, PORT_OUT, BLOCK_B, MEM_PORT_IN, PORT_IN, MEM_CONN, CONN];

describe('transformToIBD', () => {
  it('creates one node per PartDefinition', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B] as any);
    expect(nodes).toHaveLength(2);
    expect(nodes[0].data.name).toBe('Battery');
    expect(nodes[1].data.name).toBe('Motor');
  });

  it('ignores non-PartDefinition elements', () => {
    const req: SysONElement = { '@id': 'req-1', '@type': 'RequirementDefinition', declaredName: 'R1' };
    const { nodes } = transformToIBD([BLOCK_A, req] as any);
    expect(nodes).toHaveLength(1);
  });

  it('assigns PortUsage to its owning block (resolving through membership)', () => {
    const { nodes } = transformToIBD(ALL as any);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports).toHaveLength(1);
    expect(battery.data.ports[0].name).toBe('pwr_out');
    expect(motor.data.ports).toHaveLength(1);
    expect(motor.data.ports[0].name).toBe('pwr_in');
  });

  it('places source port on right, target port on left', () => {
    const { nodes } = transformToIBD(ALL as any);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports[0].position).toBe('right');
    expect(motor.data.ports[0].position).toBe('left');
  });

  it('creates one edge per ConnectionUsage', () => {
    const { edges } = transformToIBD(ALL as any);
    expect(edges).toHaveLength(1);
    expect(edges[0].source).toBe('block-a');
    expect(edges[0].target).toBe('block-b');
    expect(edges[0].sourceHandle).toBe('port-out');
    expect(edges[0].targetHandle).toBe('port-in');
    expect(edges[0].label).toBe('powerLine');
  });

  it('returns node type sysmlBlock', () => {
    const { nodes } = transformToIBD([BLOCK_A] as any);
    expect(nodes[0].type).toBe('sysmlBlock');
  });

  it('drops edge when port owner is not a known block', () => {
    const orphanBlock: SysONElement = {
      '@id': 'orphan', '@type': 'PartDefinition', declaredName: 'Orphan',
      ownedElement: [], owner: { '@id': 'unknown-pkg' },
    };
    const orphanMem: SysONElement = {
      '@id': 'orphan-mem', '@type': 'OwningMembership', declaredName: null,
      ownedElement: [{ '@id': 'orphan-port' }], owner: { '@id': 'orphan' },
    };
    const orphanPort: SysONElement = {
      '@id': 'orphan-port', '@type': 'PortUsage', declaredName: 'p',
      ownedElement: [], owner: { '@id': 'orphan-mem' },
    };
    const badConn: SysONElement = {
      '@id': 'bad-conn', '@type': 'ConnectionUsage', declaredName: 'bad',
      ownedElement: [], owner: { '@id': 'orphan-mem' },
      connectorEnd: [
        { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'orphan-port' } },
        { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'nonexistent-port' } },
      ],
    } as SysONElement & { connectorEnd: Array<{ '@type': string; connectedFeature: { '@id': string } }> };

    const { edges } = transformToIBD([orphanBlock, orphanMem, orphanPort, badConn] as any);
    expect(edges).toHaveLength(0);
  });
});
