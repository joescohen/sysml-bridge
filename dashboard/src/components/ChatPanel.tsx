import { useState, useEffect, useRef, useCallback } from 'react';
import type { Project } from '../types/sysml';

interface Message {
  role: 'user' | 'assistant';
  content: string;
  tools?: ToolCall[];
}

interface ToolCall {
  id: string;
  name: string;
  status: 'pending' | 'running' | 'done';
}

interface Conversation {
  id: string;
  title: string;
  messages: Message[];
}

function genId() { return Math.random().toString(36).slice(2) + Date.now().toString(36); }

function loadConvs(): Record<string, Conversation[]> {
  try { return JSON.parse(localStorage.getItem('sysml_convs') ?? '{}'); } catch { return {}; }
}
function saveConvs(c: Record<string, Conversation[]>) {
  localStorage.setItem('sysml_convs', JSON.stringify(c));
}

const TOOL_LABELS: Record<string, string> = {
  query_elements: 'Queried elements',
  create_element: 'Created element',
  delete_element: 'Deleted element',
  create_diagram: 'Created diagram',
  export_sysml: 'Exported SysML',
  create_project: 'Created project',
};

function escHtml(s: string) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function renderMarkdown(text: string) {
  return escHtml(text)
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/`([^`]+)`/g, '<code style="font-family:monospace;background:var(--surface2);padding:1px 4px;border-radius:3px">$1</code>');
}

interface ChatPanelProps {
  project: Project | null;
  onModelChanged: () => void;
}

export function ChatPanel({ project, onModelChanged }: ChatPanelProps) {
  const [convs, setConvs] = useState<Record<string, Conversation[]>>(loadConvs);
  const [activeId, setActiveId] = useState<Record<string, string>>({});
  const [input, setInput] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [showConvList, setShowConvList] = useState(false);
  const [streamText, setStreamText] = useState('');
  const [streamTools, setStreamTools] = useState<ToolCall[]>([]);
  const readerRef = useRef<ReadableStreamDefaultReader<Uint8Array> | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const pid = project?.['@id'] ?? null;

  useEffect(() => {
    if (!pid) return;
    setConvs(prev => {
      if (prev[pid]?.length) return prev;
      const conv = { id: genId(), title: 'New conversation', messages: [] };
      const updated = { ...prev, [pid]: [conv] };
      saveConvs(updated);
      return updated;
    });
  }, [pid]);

  useEffect(() => {
    if (!pid) return;
    setActiveId(prev => {
      if (prev[pid]) return prev;
      const first = convs[pid]?.[0]?.id;
      return first ? { ...prev, [pid]: first } : prev;
    });
  }, [pid, convs]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [streamText, streamTools, convs, activeId]);

  const activeConv = pid ? (convs[pid] ?? []).find(c => c.id === activeId[pid]) : null;

  const updateConv = useCallback((pid: string, id: string, fn: (c: Conversation) => Conversation) => {
    setConvs(prev => {
      const updated = {
        ...prev,
        [pid]: (prev[pid] ?? []).map(c => c.id === id ? fn(c) : c),
      };
      saveConvs(updated);
      return updated;
    });
  }, []);

  async function sendMessage(text: string) {
    if (!pid || !text.trim() || isStreaming) return;
    if (!activeConv) return;

    const convId = activeConv.id;
    updateConv(pid, convId, c => ({
      ...c,
      title: c.title === 'New conversation' ? text.slice(0, 40) + (text.length > 40 ? '…' : '') : c.title,
      messages: [...c.messages, { role: 'user', content: text }],
    }));

    setInput('');
    setIsStreaming(true);
    setStreamText('');
    setStreamTools([]);

    const apiMessages = [...activeConv.messages, { role: 'user' as const, content: text }]
      .map(m => ({ role: m.role, content: m.content }));

    try {
      const response = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ messages: apiMessages, projectId: pid }),
      });
      if (!response.ok) {
        const err = await response.json().catch(() => ({ error: response.statusText }));
        throw new Error((err as { error: string }).error ?? response.statusText);
      }

      const reader = response.body!.getReader();
      readerRef.current = reader;
      const decoder = new TextDecoder();
      let buffer = '';
      let accText = '';
      const tools: ToolCall[] = [];
      const toolMap: Record<string, ToolCall> = {};

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop()!;
        for (const line of lines) {
          if (!line.startsWith('data: ')) continue;
          const event = JSON.parse(line.slice(6)) as Record<string, string>;
          if (event.type === 'text') {
            accText += event.text;
            setStreamText(accText);
          } else if (event.type === 'tool_start') {
            const t: ToolCall = { id: event.id, name: event.name, status: 'pending' };
            tools.push(t); toolMap[event.id] = t;
            setStreamTools([...tools]);
          } else if (event.type === 'tool_running') {
            if (toolMap[event.id]) { toolMap[event.id].status = 'running'; setStreamTools([...tools]); }
          } else if (event.type === 'tool_done') {
            if (toolMap[event.id]) { toolMap[event.id].status = 'done'; setStreamTools([...tools]); }
          } else if (event.type === 'done') {
            break;
          } else if (event.type === 'error') {
            throw new Error(event.message);
          }
        }
      }

      updateConv(pid, convId, c => ({
        ...c,
        messages: [...c.messages, { role: 'assistant', content: accText, tools: [...tools] }],
      }));

      const mutating = ['create_element', 'create_project', 'delete_element', 'create_diagram'];
      if (tools.some(t => mutating.includes(t.name) && t.status === 'done')) {
        onModelChanged();
      }
    } catch (err) {
      if (err instanceof Error && err.name !== 'AbortError') {
        setStreamText(`Error: ${err.message}`);
      }
    } finally {
      readerRef.current = null;
      setIsStreaming(false);
      setStreamText('');
      setStreamTools([]);
    }
  }

  function newConversation() {
    if (!pid) return;
    const conv: Conversation = { id: genId(), title: 'New conversation', messages: [] };
    setConvs(prev => {
      const updated = { ...prev, [pid]: [conv, ...(prev[pid] ?? [])] };
      saveConvs(updated);
      return updated;
    });
    setActiveId(prev => ({ ...prev, [pid]: conv.id }));
    setShowConvList(false);
  }

  function deleteConv(id: string) {
    if (!pid) return;
    setConvs(prev => {
      const filtered = (prev[pid] ?? []).filter(c => c.id !== id);
      const next = filtered.length ? filtered : [{ id: genId(), title: 'New conversation', messages: [] }];
      const updated = { ...prev, [pid]: next };
      saveConvs(updated);
      return updated;
    });
    setActiveId(prev => {
      const newActive = (convs[pid] ?? []).find(c => c.id !== id)?.id ?? genId();
      return { ...prev, [pid]: newActive };
    });
  }

  const messages = activeConv?.messages ?? [];

  const s = {
    panel: { borderLeft: '1px solid var(--border)', display: 'flex', flexDirection: 'column' as const, minHeight: 0, background: 'var(--surface)', height: '100%' },
    topbar: { borderBottom: '1px solid var(--border)', padding: '0 12px', height: 44, display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 },
    avatar: { width: 26, height: 26, borderRadius: 8, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    title: { fontSize: 12, fontWeight: 600, flex: 1 },
    convBtn: { fontSize: 11, color: 'var(--text3)', background: 'none', border: 'none', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4, padding: '3px 6px', borderRadius: 5, maxWidth: 120, overflow: 'hidden' as const },
    newBtn: { width: 24, height: 24, borderRadius: 5, border: '1px solid var(--border2)', background: 'transparent', color: 'var(--text3)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' },
    convList: { borderBottom: '1px solid var(--border)', background: 'var(--bg)', maxHeight: 160, overflowY: 'auto' as const, flexShrink: 0 },
    msgArea: { flex: 1, overflowY: 'auto' as const, padding: '16px 12px', display: 'flex', flexDirection: 'column' as const, gap: 14 },
    inputArea: { borderTop: '1px solid var(--border)', padding: '10px', flexShrink: 0, display: 'flex', gap: 7, alignItems: 'flex-end', background: 'var(--surface)' },
    textarea: { flex: 1, resize: 'none' as const, overflow: 'hidden' as const, background: 'var(--bg)', border: '1px solid var(--border2)', borderRadius: 9, padding: '8px 11px', color: 'var(--text)', fontSize: 12.5, fontFamily: 'inherit', lineHeight: 1.5, minHeight: 36, maxHeight: 120, outline: 'none' },
    sendBtn: { width: 34, height: 34, borderRadius: 8, border: 'none', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, background: 'var(--primary)', color: '#fff', opacity: isStreaming || !input.trim() ? 0.35 : 1 },
    stopBtn: { width: 34, height: 34, borderRadius: 8, border: '1px solid var(--border2)', background: 'var(--surface2)', color: 'var(--text2)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
  };

  return (
    <div style={s.panel}>
      <div style={s.topbar}>
        <div style={s.avatar}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
        </div>
        <span style={s.title}>MBSE Assistant</span>
        <button style={s.convBtn} onClick={() => setShowConvList(v => !v)}>
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{activeConv?.title ?? 'New conversation'}</span>
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="m6 9 6 6 6-6"/></svg>
        </button>
        <button style={s.newBtn} onClick={newConversation} title="New conversation">+</button>
      </div>

      {showConvList && pid && (
        <div style={s.convList}>
          {(convs[pid] ?? []).map(c => (
            <div key={c.id}
              onClick={() => { setActiveId(prev => ({ ...prev, [pid]: c.id })); setShowConvList(false); }}
              style={{ padding: '8px 12px', fontSize: 11.5, color: c.id === activeId[pid] ? 'var(--primary-text)' : 'var(--text2)', cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, background: c.id === activeId[pid] ? 'var(--primary-dim)' : 'transparent' }}
            >
              <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.title}</span>
              <button onClick={e => { e.stopPropagation(); deleteConv(c.id); }} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text4)', fontSize: 14 }}>×</button>
            </div>
          ))}
        </div>
      )}

      <div style={s.msgArea}>
        {!messages.length && !isStreaming && (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', textAlign: 'center', padding: '20px 8px', gap: 12 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: '#f1f5f9' }}>MBSE Assistant</div>
            <div style={{ fontSize: 11, color: 'var(--text3)', lineHeight: 1.6 }}>Ask me to query, create, or analyze your SysML model.</div>
            {pid && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6, width: '100%', marginTop: 4 }}>
                {['List all elements in this project', 'Create a PartDefinition called Sensor', 'Create a General View diagram for this project'].map(q => (
                  <button key={q} onClick={() => sendMessage(q)} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 11px', borderRadius: 8, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text3)', fontSize: 11, cursor: 'pointer', textAlign: 'left' }}>
                    {q}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        {messages.map((msg, i) => msg.role === 'user' ? (
          <div key={i} style={{ display: 'flex', justifyContent: 'flex-end' }}>
            <div style={{ maxWidth: '85%', background: 'var(--primary)', color: '#fff', borderRadius: '14px 14px 4px 14px', padding: '9px 13px', fontSize: 12.5, lineHeight: 1.55, whiteSpace: 'pre-wrap' }}>{msg.content}</div>
          </div>
        ) : (
          <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
            <div style={{ width: 26, height: 26, borderRadius: 8, flexShrink: 0, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: 1 }}>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
            </div>
            <div style={{ maxWidth: '88%', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: '4px 14px 14px 14px', overflow: 'hidden' }}>
              {msg.content && <div style={{ padding: '9px 13px', fontSize: 12.5, lineHeight: 1.6, color: 'var(--text)' }} dangerouslySetInnerHTML={{ __html: renderMarkdown(msg.content) }} />}
              {(msg.tools ?? []).length > 0 && (
                <div style={{ padding: '8px 13px 9px', display: 'flex', flexDirection: 'column', gap: 5, borderTop: msg.content ? '1px solid var(--border)' : undefined }}>
                  {(msg.tools ?? []).map(t => (
                    <div key={t.id} style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
                      {t.status === 'done'
                        ? <div style={{ width: 13, height: 13, borderRadius: '50%', background: 'rgba(34,197,94,.15)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}><svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="#22c55e" strokeWidth="3" strokeLinecap="round"><polyline points="20 6 9 17 4 12"/></svg></div>
                        : <div style={{ width: 13, height: 13, borderRadius: '50%', border: '1.5px solid var(--primary-dim)', borderTopColor: 'var(--primary)', animation: 'spin 0.7s linear infinite', flexShrink: 0 }} />}
                      <span style={{ fontSize: 10.5, color: 'var(--text3)' }}>{(TOOL_LABELS[t.name] ?? t.name.replace(/_/g, ' ')) + (t.status === 'done' ? '' : '…')}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        ))}

        {isStreaming && (
          <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
            <div style={{ width: 26, height: 26, borderRadius: 8, flexShrink: 0, background: 'var(--primary-dim)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: 1 }}>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#818cf8" strokeWidth="2" strokeLinecap="round"><path d="m3 21 1.9-5.7a8.5 8.5 0 1 1 3.8 3.8z"/></svg>
            </div>
            <div style={{ maxWidth: '88%', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: '4px 14px 14px 14px', overflow: 'hidden' }}>
              {streamText && <div style={{ padding: '9px 13px', fontSize: 12.5, lineHeight: 1.6, color: 'var(--text)' }} dangerouslySetInnerHTML={{ __html: renderMarkdown(streamText) }} />}
              {streamTools.length > 0 && (
                <div style={{ padding: '8px 13px 9px', display: 'flex', flexDirection: 'column', gap: 5, borderTop: streamText ? '1px solid var(--border)' : undefined }}>
                  {streamTools.map(t => (
                    <div key={t.id} style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
                      {t.status === 'done'
                        ? <div style={{ width: 13, height: 13, borderRadius: '50%', background: 'rgba(34,197,94,.15)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}><svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="#22c55e" strokeWidth="3" strokeLinecap="round"><polyline points="20 6 9 17 4 12"/></svg></div>
                        : <div style={{ width: 13, height: 13, borderRadius: '50%', border: '1.5px solid var(--primary-dim)', borderTopColor: 'var(--primary)', animation: 'spin 0.7s linear infinite', flexShrink: 0 }} />}
                      <span style={{ fontSize: 10.5, color: 'var(--text3)' }}>{(TOOL_LABELS[t.name] ?? t.name.replace(/_/g, ' ')) + (t.status === 'done' ? '' : '…')}</span>
                    </div>
                  ))}
                </div>
              )}
              {!streamText && !streamTools.length && (
                <div style={{ display: 'flex', gap: 4, alignItems: 'center', padding: '10px 13px' }}>
                  {[0, 200, 400].map(delay => (
                    <div key={delay} style={{ width: 5, height: 5, borderRadius: '50%', background: 'var(--text4)', animation: `pulse 1.2s ease-in-out ${delay}ms infinite` }} />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      <div style={s.inputArea}>
        <textarea
          style={s.textarea}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(input); } }}
          placeholder={pid ? 'Ask about your model…' : 'Select a project first'}
          disabled={!pid}
          rows={1}
        />
        {isStreaming
          ? <button style={s.stopBtn} onClick={() => readerRef.current?.cancel()}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><rect x="4" y="4" width="16" height="16" rx="2"/></svg>
            </button>
          : <button style={s.sendBtn} disabled={!input.trim() || !pid || isStreaming} onClick={() => sendMessage(input)}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
            </button>
        }
      </div>
    </div>
  );
}
