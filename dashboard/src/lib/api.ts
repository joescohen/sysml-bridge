import type { SmapsElement, LocalElement, Project, StoredDiagram } from '../types/sysml';

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
    body: JSON.stringify({ '@type': 'Project', name }),
  });
}

export function deleteProject(id: string): Promise<unknown> {
  return apiFetch(`/api/projects/${id}`, { method: 'DELETE' });
}

export function getElements(projectId: string): Promise<SmapsElement[]> {
  return apiFetch(`/api/projects/${projectId}/elements`);
}

export function getLocalElements(projectId: string): Promise<LocalElement[]> {
  return apiFetch(`/api/projects/${projectId}/local-elements`);
}

export function deleteLocalElement(projectId: string, elementId: string): Promise<unknown> {
  return apiFetch(`/api/projects/${projectId}/local-elements/${elementId}`, { method: 'DELETE' });
}

export function getDiagrams(projectId: string): Promise<StoredDiagram[]> {
  return apiFetch(`/api/projects/${projectId}/diagrams`);
}

export function deleteDiagram(projectId: string, idx: number): Promise<unknown> {
  return apiFetch(`/api/projects/${projectId}/diagrams/${idx}`, { method: 'DELETE' });
}
