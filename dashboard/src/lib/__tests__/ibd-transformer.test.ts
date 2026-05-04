import { describe, it, expect } from 'vitest';
import { transformToIBD } from '../ibd-transformer';
import type { SmapsElement, LocalElement } from '../../types/sysml';

const BLOCK_A: SmapsElement = { '@id': 'block-a', '@type': 'PartDefinition', declaredName: 'Battery' };
const BLOCK_B: SmapsElement = { '@id': 'block-b', '@type': 'PartDefinition', declaredName: 'Motor' };

const PORT_OUT: LocalElement = {
  '@id': 'port-out', '@type': 'ProxyPortUsage', declaredName: 'pwr_out',
  _local: true, owner: { '@id': 'block-a' },
};
const PORT_IN: LocalElement = {
  '@id': 'port-in', '@type': 'ProxyPortUsage', declaredName: 'pwr_in',
  _local: true, owner: { '@id': 'block-b' },
};
const CONN: LocalElement = {
  '@id': 'conn-1', '@type': 'ConnectionUsage', declaredName: 'powerLine',
  _local: true,
  connectorEnd: [
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-out' } },
    { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-in' } },
  ],
};

describe('transformToIBD', () => {
  it('creates one node per PartDefinition', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B], []);
    expect(nodes).toHaveLength(2);
    expect(nodes[0].data.name).toBe('Battery');
    expect(nodes[1].data.name).toBe('Motor');
  });

  it('ignores non-PartDefinition SMAPS elements', () => {
    const req: SmapsElement = { '@id': 'req-1', '@type': 'RequirementDefinition', declaredName: 'R1' };
    const { nodes } = transformToIBD([BLOCK_A, req], []);
    expect(nodes).toHaveLength(1);
  });

  it('assigns ProxyPortUsage to its owning block', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B], [PORT_OUT, PORT_IN]);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor   = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports).toHaveLength(1);
    expect(battery.data.ports[0].name).toBe('pwr_out');
    expect(motor.data.ports).toHaveLength(1);
    expect(motor.data.ports[0].name).toBe('pwr_in');
  });

  it('places source port on right, target port on left', () => {
    const { nodes } = transformToIBD([BLOCK_A, BLOCK_B], [PORT_OUT, PORT_IN, CONN]);
    const battery = nodes.find(n => n.id === 'block-a')!;
    const motor   = nodes.find(n => n.id === 'block-b')!;
    expect(battery.data.ports[0].position).toBe('right');
    expect(motor.data.ports[0].position).toBe('left');
  });

  it('creates one edge per ConnectionUsage', () => {
    const { edges } = transformToIBD([BLOCK_A, BLOCK_B], [PORT_OUT, PORT_IN, CONN]);
    expect(edges).toHaveLength(1);
    expect(edges[0].source).toBe('block-a');
    expect(edges[0].target).toBe('block-b');
    expect(edges[0].sourceHandle).toBe('port-out');
    expect(edges[0].targetHandle).toBe('port-in');
    expect(edges[0].label).toBe('powerLine');
  });

  it('drops edges with missing owner blocks', () => {
    const orphan: LocalElement = {
      '@id': 'orphan', '@type': 'ProxyPortUsage', declaredName: 'x',
      _local: true, owner: { '@id': 'nonexistent' },
    };
    const conn: LocalElement = {
      '@id': 'c2', '@type': 'ConnectionUsage', declaredName: 'c', _local: true,
      connectorEnd: [
        { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'orphan' } },
        { '@type': 'ConnectorEnd', connectedFeature: { '@id': 'port-in' } },
      ],
    };
    const { edges } = transformToIBD([BLOCK_A, BLOCK_B], [orphan, PORT_IN, conn]);
    expect(edges).toHaveLength(0);
  });

  it('returns node type sysmlBlock', () => {
    const { nodes } = transformToIBD([BLOCK_A], []);
    expect(nodes[0].type).toBe('sysmlBlock');
  });
});
