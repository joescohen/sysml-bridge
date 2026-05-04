import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { Node } from '@xyflow/react';
import type { SysMLBlockNodeData } from '../../lib/ibd-transformer';

const PORT_ROW_HEIGHT = 24;
const BODY_PADDING    = 12;

type SysMLBlockNodeType = Node<SysMLBlockNodeData, 'sysmlBlock'>;

export function SysMLBlockNode({ data, selected }: NodeProps<SysMLBlockNodeType>) {
  const leftPorts  = data.ports.filter(p => p.position === 'left');
  const rightPorts = data.ports.filter(p => p.position === 'right');
  const bodyHeight = Math.max(leftPorts.length, rightPorts.length, 1) * PORT_ROW_HEIGHT + BODY_PADDING * 2;

  return (
    <div className={`sysml-block${selected ? ' selected' : ''}`} style={{ minWidth: 160 }}>
      <div className="sysml-block-header">
        <div className="sysml-stereotype">«{data.stereotype}»</div>
        <div className="sysml-name">{data.name}</div>
      </div>

      <div className="sysml-block-body" style={{ height: bodyHeight }}>
        {leftPorts.map((port, i) => {
          const top = BODY_PADDING + i * PORT_ROW_HEIGHT;
          return (
            <Handle
              key={port.id}
              type="target"
              id={port.id}
              position={Position.Left}
              style={{ top: top + 4 }}
            >
              <span
                className="sysml-port-label left"
                style={{ top: top - 1 }}
              >
                {port.name}
              </span>
            </Handle>
          );
        })}

        {rightPorts.map((port, i) => {
          const top = BODY_PADDING + i * PORT_ROW_HEIGHT;
          return (
            <Handle
              key={port.id}
              type="source"
              id={port.id}
              position={Position.Right}
              style={{ top: top + 4 }}
            >
              <span
                className="sysml-port-label right"
                style={{ top: top - 1, right: 14 }}
              >
                {port.name}
              </span>
            </Handle>
          );
        })}
      </div>
    </div>
  );
}
