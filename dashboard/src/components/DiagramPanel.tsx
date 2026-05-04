import type { SysONElement } from '../types/sysml';

interface DiagramPanelProps {
  projectId: string;
  elements: SysONElement[];
}

export function DiagramPanel({ projectId: _projectId, elements: _elements }: DiagramPanelProps) {
  return (
    <div style={{ background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: 20, marginBottom: 22, color: 'var(--text4)', fontSize: 12, textAlign: 'center' }}>
      No diagrams yet. Ask the assistant to create a diagram view.
    </div>
  );
}
