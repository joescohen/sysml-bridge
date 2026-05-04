import { useState } from 'react';
import type { Project } from '../types/sysml';
import { createProject, deleteProject } from '../lib/api';

interface SidebarProps {
  projects: Project[];
  currentProjectId: string | null;
  onSelect: (id: string) => void;
  onProjectsChanged: () => void;
}

export function Sidebar({ projects, currentProjectId, onSelect, onProjectsChanged }: SidebarProps) {
  const [showModal, setShowModal] = useState(false);
  const [newName, setNewName] = useState('');

  async function handleCreate() {
    if (!newName.trim()) return;
    await createProject(newName.trim());
    setNewName('');
    setShowModal(false);
    onProjectsChanged();
  }

  async function handleDelete(e: React.MouseEvent, id: string, name: string) {
    e.stopPropagation();
    if (!confirm(`Delete "${name}"? This cannot be undone.`)) return;
    await deleteProject(id);
    onProjectsChanged();
  }

  const s = {
    sidebar: { borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column' as const, overflow: 'hidden', height: '100%' },
    header: { padding: '14px 16px 10px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0 },
    label: { fontSize: 10, fontWeight: 700, letterSpacing: '0.1em', textTransform: 'uppercase' as const, color: 'var(--text3)' },
    addBtn: { width: 22, height: 22, borderRadius: 5, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 16, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' },
    list: { flex: 1, overflowY: 'auto' as const, padding: '0 8px 16px' },
  };

  return (
    <aside style={s.sidebar}>
      <div style={s.header}>
        <span style={s.label}>Projects</span>
        <button style={s.addBtn} onClick={() => setShowModal(true)} title="New project">+</button>
      </div>

      <div style={s.list}>
        {!projects.length && <div style={{ padding: '8px 12px', fontSize: 11.5, color: 'var(--text4)' }}>No projects</div>}
        {projects.map(p => {
          const active = p['@id'] === currentProjectId;
          return (
            <div
              key={p['@id']}
              onClick={() => onSelect(p['@id'])}
              style={{ padding: '8px 10px', borderRadius: 7, cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 9, background: active ? 'var(--primary-dim)' : 'transparent', marginBottom: 2 }}
            >
              <div style={{ width: 8, height: 8, borderRadius: 2, flexShrink: 0, background: active ? 'var(--primary)' : 'var(--border2)' }} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12.5, color: active ? 'var(--primary-text)' : 'var(--text2)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</div>
                <div style={{ fontSize: 10, color: 'var(--text4)', fontFamily: 'monospace' }}>{p['@id'].slice(0, 8)}</div>
              </div>
              <button
                onClick={e => handleDelete(e, p['@id'], p.name)}
                style={{ width: 18, height: 18, borderRadius: 4, border: 'none', background: 'transparent', color: 'var(--text4)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
              >
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </button>
            </div>
          );
        })}
      </div>

      {showModal && (
        <div style={{ position: 'fixed', inset: 0, zIndex: 100, background: 'rgba(0,0,0,.6)', display: 'flex', alignItems: 'center', justifyContent: 'center', backdropFilter: 'blur(4px)' }} onClick={() => setShowModal(false)}>
          <div style={{ background: 'var(--surface)', border: '1px solid var(--border2)', borderRadius: 12, padding: 24, width: 340, maxWidth: '90vw' }} onClick={e => e.stopPropagation()}>
            <div style={{ fontSize: 15, fontWeight: 700, color: '#f1f5f9', marginBottom: 6 }}>New SysML Project</div>
            <div style={{ fontSize: 11.5, color: 'var(--text3)', marginBottom: 18, lineHeight: 1.5 }}>Creates a new project in the SMAPS repository.</div>
            <input
              style={{ width: '100%', background: 'var(--bg)', border: '1px solid var(--border2)', borderRadius: 8, padding: '9px 12px', color: 'var(--text)', fontSize: 13, fontFamily: 'inherit', outline: 'none', marginBottom: 14, display: 'block' }}
              placeholder="Project name, e.g. DroneSystem"
              value={newName}
              onChange={e => setNewName(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleCreate(); if (e.key === 'Escape') setShowModal(false); }}
              autoFocus
            />
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={() => setShowModal(false)} style={{ padding: '7px 14px', borderRadius: 7, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 12, cursor: 'pointer' }}>Cancel</button>
              <button onClick={handleCreate} disabled={!newName.trim()} style={{ padding: '7px 14px', borderRadius: 7, border: 'none', background: 'var(--primary)', color: '#fff', fontSize: 12, fontWeight: 600, cursor: 'pointer', opacity: newName.trim() ? 1 : 0.35 }}>Create Project</button>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}
