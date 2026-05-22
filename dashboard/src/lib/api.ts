import type { SysONElement, Project, Representation } from '../types/sysml';

async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, opts);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}: ${path}`);
  return res.json() as Promise<T>;
}

export function getProjects(): Promise<Project[]> {
  return apiFetch('/api/projects');
}

export function createProject(name: string): Promise<Project> {
  return apiFetch('/api/projects', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
}

export function deleteProject(id: string): Promise<unknown> {
  return apiFetch(`/api/projects/${id}`, { method: 'DELETE' });
}

export function getElements(projectId: string): Promise<SysONElement[]> {
  return apiFetch(`/api/projects/${projectId}/elements`);
}

export function getRepresentations(projectId: string): Promise<Representation[]> {
  return apiFetch(`/api/projects/${projectId}/representations`);
}

export interface TopologyEdge {
  id: string;
  label: string;
  sourcePort: string;
  targetPort: string;
}

export async function getTopology(projectId: string): Promise<{ edges: TopologyEdge[] }> {
  const res = await fetch(`/api/projects/${projectId}/topology`);
  if (res.status === 404) return { edges: [] };
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}: /api/projects/${projectId}/topology`);
  return res.json() as Promise<{ edges: TopologyEdge[] }>;
}

export function patchElement(projectId: string, elementId: string, updates: Record<string, unknown>): Promise<{ success: boolean }> {
  return apiFetch(`/api/projects/${projectId}/elements/${elementId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  });
}

export function putDocumentation(projectId: string, elementId: string, body: string): Promise<{ success: boolean; doc_element_id: string; created: boolean }> {
  return apiFetch(`/api/projects/${projectId}/elements/${elementId}/documentation`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ body }),
  });
}

export function invalidateCache(projectId: string): Promise<{ ok: boolean }> {
  return apiFetch(`/api/projects/${projectId}/invalidate`, { method: 'POST' });
}
