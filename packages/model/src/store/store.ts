import type {
  ProjectDescriptor,
  SysmlElement,
  SysmlRelationship,
  ProjectState,
} from "./types.js";

// ---------------------------------------------------------------------------
// ModelStore
//
// Backend-agnostic interface for the SysML model store. Tools depend ONLY on
// this interface, never on a concrete backend — so a different backend (e.g.
// a live SysML v2 REST API such as SMAPS on Pilot/Cameo) is a one-line swap
// at server wiring time. This repo ships the file-native FileStore; the
// interface is the documented portability seam.
// ---------------------------------------------------------------------------

export interface ModelStore {
  projectId: string | null;
  branchId: string | null;
  headCommitId: string | null;

  checkConnection(): Promise<boolean>;

  createProject(name: string): Promise<ProjectDescriptor>;
  loadProject(projectId: string): Promise<ProjectDescriptor>;
  listProjects(): Promise<ProjectDescriptor[]>;

  createElement(
    type: string,
    name: string,
    attributes?: Record<string, unknown>
  ): Promise<SysmlElement>;
  createElements(
    elements: Array<{ type: string; name: string; attributes?: Record<string, unknown> }>
  ): Promise<SysmlElement[]>;
  updateElement(
    elementId: string,
    updates: Record<string, unknown>
  ): Promise<SysmlElement>;
  deleteElement(elementId: string): Promise<void>;

  getElement(elementId: string): Promise<SysmlElement>;
  queryElements(type?: string, namePattern?: string): Promise<SysmlElement[]>;
  queryRelationships(
    elementId?: string,
    direction?: "in" | "out" | "both"
  ): Promise<SysmlRelationship[]>;
  getProjectState(): Promise<ProjectState>;
}
