import { Fragment } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { Node } from '@xyflow/react';
import type { SysMLBlockNodeData } from '../../lib/ibd-transformer';

const PORT_H = 28;
const PAD    = 12;

type SysMLBlockNodeType = Node<SysMLBlockNodeData, 'sysmlBlock'>;

export function SysMLBlockNode({ data, selected }: NodeProps<SysMLBlockNodeType>) {
  const leftPorts  = data.ports.filter(p => p.position === 'left');
  const rightPorts = data.ports.filter(p => p.position === 'right');
  const maxPorts   = Math.max(leftPorts.length, rightPorts.length, 1);
  const bodyH      = maxPorts * PORT_H + PAD * 2;

  return (
    <div className={`sysml-block${selected ? ' selected' : ''}`}>
      <div className="sysml-accent-bar" />

      <div className="sysml-block-header">
        <div className="sysml-stereotype">«{data.stereotype}»</div>
        <div className="sysml-name">{data.name}</div>
      </div>

      <div className="sysml-block-body" style={{ height: bodyH }}>
        {leftPorts.map((port, i) => {
          const midY = PAD + i * PORT_H + PORT_H / 2;
          return (
            <Fragment key={port.id}>
              {/* invisible connection point at left edge */}
              <Handle
                type="target"
                id={port.id}
                position={Position.Left}
                style={{ top: midY, opacity: 0, width: 1, height: 1, border: 'none' }}
              />
              {/* visible port square */}
              <div className="sysml-port-sq left" style={{ top: midY - 5 }} />
              {/* label */}
              <span className="sysml-port-label left" style={{ top: midY - 8 }}>
                {port.name}
              </span>
            </Fragment>
          );
        })}

        {rightPorts.map((port, i) => {
          const midY = PAD + i * PORT_H + PORT_H / 2;
          return (
            <Fragment key={port.id}>
              <Handle
                type="source"
                id={port.id}
                position={Position.Right}
                style={{ top: midY, opacity: 0, width: 1, height: 1, border: 'none' }}
              />
              <div className="sysml-port-sq right" style={{ top: midY - 5 }} />
              <span className="sysml-port-label right" style={{ top: midY - 8 }}>
                {port.name}
              </span>
            </Fragment>
          );
        })}
      </div>
    </div>
  );
}
