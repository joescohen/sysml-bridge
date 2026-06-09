import type { SmapsProject } from "./types/smaps.js";
import type {
  SysmlElement,
  SysmlRelationship,
  ProjectState,
} from "./types/sysml-elements.js";

// ---------------------------------------------------------------------------
// ModelStore
//
// Backend-agnostic interface for the SysML model store. Tools depend ONLY on
// this interface, never on a concrete backend — so swapping backends is a
// one-line change in index.ts.
//
//   SmapsClient  → live SMAPS REST backend (Pilot / Cameo server)
//   FileStore    → file-native .sysml / JSON on disk, no server (research default)
//
// Per RESEARCH.md the consensus is file-native; SMAPS stays available behind
// the same interface for the live-model / Cameo-server path.
// ---------------------------------------------------------------------------

export interface ModelStore {
  projectId: string | null;
  branchId: string | null;
  headCommitId: string | null;

  checkConnection(): Promise<boolean>;

  createProject(name: string): Promise<SmapsProject>;
  loadProject(projectId: string): Promise<SmapsProject>;
  listProjects(): Promise<SmapsProject[]>;

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
