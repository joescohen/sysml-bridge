import { useState } from 'react';
import type { Project } from '../types/sysml';
import { createProject, deleteProject } from '../lib/api';

interface SidebarProps {
  projects: Project[];
  currentProjectId: string | null;
  onSelect: (id: string) => void;
  onProjectsChanged: () => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
}

export function Sidebar({ projects, currentProjectId, onSelect, onProjectsChanged, collapsed, onToggleCollapsed }: SidebarProps) {
  const [showModal, setShowModal] = useState(false);
  const [newName, setNewName] = useState('');
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  async function handleCreate() {
    if (!newName.trim() || creating) return;
    setCreating(true);
    setCreateError(null);
    try {
      await createProject(newName.trim());
      setNewName('');
      setShowModal(false);
      onProjectsChanged();
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : 'Failed to create project');
    } finally {
      setCreating(false);
    }
  }

  async function handleDelete(e: React.MouseEvent, id: string, name: string) {
    e.stopPropagation();
    if (!confirm(`Delete "${name}"? This cannot be undone.`)) return;
    await deleteProject(id);
    onProjectsChanged();
  }

  const s = {
    sidebar: { borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column' as const, overflow: 'hidden', height: '100%' },
    header: { padding: collapsed ? '12px 8px 8px' : '14px 16px 10px', display: 'flex', alignItems: 'center', justifyContent: collapsed ? 'center' : 'space-between', gap: 7, flexShrink: 0 },
    label: { fontSize: 10, fontWeight: 700, letterSpacing: '0.1em', textTransform: 'uppercase' as const, color: 'var(--text3)' },
    addBtn: { width: 22, height: 22, borderRadius: 5, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 16, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' },
    collapseBtn: { width: 22, height: 22, borderRadius: 5, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' },
    list: { flex: 1, overflowY: 'auto' as const, padding: collapsed ? '4px 7px 16px' : '0 8px 16px' },
  };

  return (
    <aside style={s.sidebar}>
      <div style={s.header}>
        {!collapsed && <span style={s.label}>Projects</span>}
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          {!collapsed && <button style={s.addBtn} onClick={() => setShowModal(true)} title="New project">+</button>}
          <button style={s.collapseBtn} onClick={onToggleCollapsed} title={collapsed ? 'Expand project sidebar' : 'Collapse project sidebar'}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              {collapsed ? <path d="m9 18 6-6-6-6" /> : <path d="m15 18-6-6 6-6" />}
            </svg>
          </button>
        </div>
      </div>

      {collapsed && (
        <button style={{ ...s.addBtn, margin: '0 auto 8px', width: 30, height: 30 }} onClick={() => setShowModal(true)} title="New project">+</button>
      )}

      <div style={s.list}>
        {!projects.length && !collapsed && <div style={{ padding: '8px 12px', fontSize: 11.5, color: 'var(--text4)' }}>No projects</div>}
        {projects.map(p => {
          const active = p['@id'] === currentProjectId;
          return (
            <div
              key={p['@id']}
              onClick={() => onSelect(p['@id'])}
              title={collapsed ? p.name : undefined}
              style={{
                padding: collapsed ? 0 : '8px 10px', height: collapsed ? 34 : undefined,
                borderRadius: 7, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: collapsed ? 'center' : undefined, gap: 9,
                background: active ? 'var(--primary-dim)' : 'transparent', marginBottom: collapsed ? 5 : 2,
              }}
            >
              <div style={{ width: collapsed ? 12 : 8, height: collapsed ? 12 : 8, borderRadius: collapsed ? 4 : 2, flexShrink: 0, background: active ? 'var(--primary)' : 'var(--border2)' }} />
              {!collapsed && (
                <>
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
                </>
              )}
            </div>
          );
        })}
      </div>

      {showModal && (
        <div style={{ position: 'fixed', inset: 0, zIndex: 100, background: 'rgba(0,0,0,.6)', display: 'flex', alignItems: 'center', justifyContent: 'center', backdropFilter: 'blur(4px)' }} onClick={() => { setShowModal(false); setCreateError(null); }}>
          <div style={{ background: 'var(--surface)', border: '1px solid var(--border2)', borderRadius: 12, padding: 24, width: 340, maxWidth: '90vw' }} onClick={e => e.stopPropagation()}>
            <div style={{ fontSize: 15, fontWeight: 700, color: '#f1f5f9', marginBottom: 6 }}>New SysML Project</div>
            <div style={{ fontSize: 11.5, color: 'var(--text3)', marginBottom: 18, lineHeight: 1.5 }}>Creates a new SysML v2 project in SysON.</div>
            <input
              style={{ width: '100%', background: 'var(--bg)', border: `1px solid ${createError ? '#ef4444' : 'var(--border2)'}`, borderRadius: 8, padding: '9px 12px', color: 'var(--text)', fontSize: 13, fontFamily: 'inherit', outline: 'none', marginBottom: createError ? 8 : 14, display: 'block' }}
              placeholder="Project name, e.g. DroneSystem"
              value={newName}
              onChange={e => { setNewName(e.target.value); setCreateError(null); }}
              onKeyDown={e => { if (e.key === 'Enter') handleCreate(); if (e.key === 'Escape') { setShowModal(false); setCreateError(null); } }}
              autoFocus
              disabled={creating}
            />
            {createError && (
              <div style={{ fontSize: 11, color: '#ef4444', marginBottom: 12, lineHeight: 1.4 }}>{createError}</div>
            )}
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={() => { setShowModal(false); setCreateError(null); }} style={{ padding: '7px 14px', borderRadius: 7, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 12, cursor: 'pointer' }}>Cancel</button>
              <button onClick={handleCreate} disabled={!newName.trim() || creating} style={{ padding: '7px 14px', borderRadius: 7, border: 'none', background: 'var(--primary)', color: '#fff', fontSize: 12, fontWeight: 600, cursor: creating ? 'wait' : 'pointer', opacity: newName.trim() && !creating ? 1 : 0.35 }}>
                {creating ? 'Creating…' : 'Create Project'}
              </button>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}
