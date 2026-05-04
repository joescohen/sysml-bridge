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

export function getTopology(projectId: string): Promise<{ edges: TopologyEdge[] }> {
  return apiFetch(`/api/projects/${projectId}/topology`);
}
