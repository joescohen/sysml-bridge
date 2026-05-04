import { useState, useEffect, useCallback } from 'react';
import type { Project } from './types/sysml';
import { getProjects } from './lib/api';
import { Sidebar } from './components/Sidebar';
import { ProjectDetail } from './components/ProjectDetail';
import { ChatPanel } from './components/ChatPanel';

const GRADS = [
  ['#1e3a5f','#0f2040'], ['#1a3a2a','#0d2015'], ['#3a1a3a','#200d20'],
  ['#3a2a1a','#201508'], ['#1a2a3a','#0a1520'], ['#2a1a3a','#150a20'],
];
function gradFor(name: string) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xfffff;
  const [a, b] = GRADS[h % GRADS.length];
  return `linear-gradient(135deg, ${a} 0%, ${b} 100%)`;
}

export function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [currentProjectId, setCurrentProjectId] = useState<string | null>(null);
  const [sysonOnline, setSysonOnline] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [isCompact, setIsCompact] = useState(() => window.innerWidth < 1100);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);

  const loadProjects = useCallback(async () => {
    try {
      const list = await getProjects();
      setProjects(list);
      setSysonOnline(true);
    } catch {
      setSysonOnline(false);
    }
  }, []);

  useEffect(() => { loadProjects(); }, [loadProjects]);
  useEffect(() => {
    const id = setInterval(loadProjects, 30_000);
    return () => clearInterval(id);
  }, [loadProjects]);
  useEffect(() => {
    function onResize() {
      setIsCompact(window.innerWidth < 1100);
    }
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const currentProject = projects.find(p => p['@id'] === currentProjectId) ?? null;

  function handleModelChanged() {
    setRefreshKey(k => k + 1);
  }

  const layout = {
    wrapper: { display: 'flex', flexDirection: 'column' as const, height: '100vh' },
    header: { height: 52, padding: '0 20px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 10, flexShrink: 0, background: 'var(--bg)', zIndex: 10 },
    body: { display: 'grid', gridTemplateColumns: `${sidebarCollapsed ? 54 : isCompact ? 220 : 248}px minmax(0, 1fr)`, flex: 1, minHeight: 0, overflow: 'hidden' },
    main: { overflowY: 'auto' as const, display: 'flex', flexDirection: 'column' as const },
  };

  return (
    <div style={layout.wrapper}>
      <header style={layout.header}>
        <div style={{ width: 7, height: 7, borderRadius: '50%', background: sysonOnline ? 'var(--green)' : 'var(--red)', flexShrink: 0, transition: 'background 0.3s' }} />
        <h1 style={{ fontSize: 14, fontWeight: 600, color: '#f1f5f9', letterSpacing: '0.02em' }}>sysml-bridge</h1>
        <span style={{ color: 'var(--border2)' }}>·</span>
        <span style={{ fontSize: 11, color: 'var(--text3)', fontFamily: 'monospace' }}>SysON :8080</span>
        <div style={{ flex: 1 }} />
        <button onClick={loadProjects} style={{ padding: '5px 8px', borderRadius: 6, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', fontSize: 11, cursor: 'pointer' }}>↻ refresh</button>
      </header>

      <div style={layout.body}>
        <Sidebar
          projects={projects}
          currentProjectId={currentProjectId}
          onSelect={setCurrentProjectId}
          onProjectsChanged={loadProjects}
          collapsed={sidebarCollapsed}
          onToggleCollapsed={() => setSidebarCollapsed(v => !v)}
        />

        <main style={layout.main}>
          {!currentProject && (
            projects.length === 0
              ? (
                <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 16, textAlign: 'center', padding: 40 }}>
                  <div style={{ width: 56, height: 56, borderRadius: 14, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="1.5" strokeLinecap="round"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="m8 21 4-4 4 4"/><path d="M12 17v4"/></svg>
                  </div>
                  <div style={{ fontSize: 17, fontWeight: 700, color: '#f1f5f9' }}>No projects yet</div>
                  <div style={{ fontSize: 12, color: 'var(--text3)', maxWidth: 260, lineHeight: 1.6 }}>Create your first SysML v2 project to start modeling.</div>
                </div>
              )
              : (
                <div style={{ padding: '28px 28px 0', flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 20 }}>
                    <div>
                      <div style={{ fontSize: 18, fontWeight: 700, color: '#f1f5f9' }}>Your Models</div>
                      <div style={{ fontSize: 12, color: 'var(--text3)', marginTop: 2 }}>SysML v2 projects in SysON</div>
                    </div>
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: 14 }}>
                    {projects.map(p => (
                      <div key={p['@id']} onClick={() => setCurrentProjectId(p['@id'])} style={{ border: '1px solid var(--border)', borderRadius: 10, overflow: 'hidden', cursor: 'pointer', background: 'var(--surface)', transition: 'border-color 0.15s, transform 0.15s' }}>
                        <div style={{ height: 80, background: gradFor(p.name), position: 'relative', display: 'flex', alignItems: 'flex-end', padding: '10px 12px' }}>
                          <div style={{ position: 'absolute', top: 14, left: 12, width: 32, height: 32, borderRadius: 8, background: 'rgba(255,255,255,.1)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,.7)" strokeWidth="1.5" strokeLinecap="round"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/></svg>
                          </div>
                          <span style={{ marginLeft: 'auto', fontSize: 11, fontWeight: 700, fontFamily: 'monospace', color: 'rgba(255,255,255,.75)', background: 'rgba(0,0,0,.25)', padding: '2px 7px', borderRadius: 99 }}>{p['@id'].slice(0, 8)}</span>
                        </div>
                        <div style={{ padding: '12px 14px' }}>
                          <div style={{ fontSize: 13, fontWeight: 600, color: '#f1f5f9', marginBottom: 4 }}>{p.name}</div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )
          )}

          {currentProject && (
            <ProjectDetail
              key={currentProject['@id']}
              project={currentProject}
              onBack={() => setCurrentProjectId(null)}
              refreshKey={refreshKey}
            />
          )}
        </main>

      </div>

      <button
        onClick={() => setChatOpen(v => !v)}
        style={{
          position: 'fixed', right: 22, bottom: 22, zIndex: 70,
          height: 42, padding: '0 14px', borderRadius: 10,
          border: '1px solid var(--border2)', background: chatOpen ? 'var(--primary)' : 'var(--surface)',
          color: chatOpen ? '#fff' : 'var(--text)', boxShadow: '0 18px 50px rgba(0,0,0,.38)',
          display: 'flex', alignItems: 'center', gap: 9, cursor: 'pointer', fontSize: 12, fontWeight: 700,
        }}
        title={chatOpen ? 'Close assistant' : 'Open assistant'}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
        {chatOpen ? 'Close Assistant' : 'MBSE Assistant'}
      </button>

      {chatOpen && (
        <div
          style={{
            position: 'fixed', right: 22, top: 68, bottom: 76, zIndex: 60,
            width: 'min(440px, calc(100vw - 44px))',
            border: '1px solid var(--border2)', borderRadius: 12, overflow: 'hidden',
            background: 'var(--surface)', boxShadow: '0 24px 80px rgba(0,0,0,.52)',
          }}
        >
          <ChatPanel project={currentProject} onModelChanged={handleModelChanged} />
        </div>
      )}
    </div>
  );
}
