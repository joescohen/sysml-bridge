import { promises as fs } from "node:fs";
import * as path from "node:path";
import { randomUUID } from "node:crypto";

import type { ModelStore } from "./store.js";
import type { SmapsProject } from "./types/smaps.js";
import {
  SYSML_RELATIONSHIP_TYPES,
  type SysmlElement,
  type SysmlRelationship,
  type ProjectState,
} from "./types/sysml-elements.js";

// ---------------------------------------------------------------------------
// FileStore
//
// File-native implementation of ModelStore. The persistent source of truth is
// a JSON model document on disk (full fidelity, diffable, no server). The
// `.sysml` textual notation is produced on demand by the serializer (export
// path) for Cameo import — that is the interchange artifact, not the store.
//
// This is the research consensus (RESEARCH.md): file-native, no SMAPS, no
// SysON. The 11 mbse-* skills and all 8 MCP tools are unchanged — they call
// the same ModelStore interface; only what is behind it changed.
// ---------------------------------------------------------------------------

const BRANCH_ID = "main";
const HEAD_COMMIT_ID = "head";

const RELATIONSHIP_TYPE_SET = new Set<string>(SYSML_RELATIONSHIP_TYPES);

interface FileModelDoc {
  "@type": "FileModel";
  id: string;
  name: string;
  branchId: string;
  headCommitId: string;
  elements: SysmlElement[];
}

export class FileStore implements ModelStore {
  private modelDir: string;

  public projectId: string | null = null;
  public branchId: string | null = null;
  public headCommitId: string | null = null;

  private projectName: string | null = null;
  private elements: SysmlElement[] = [];

  constructor(modelDir: string) {
    this.modelDir = modelDir;
  }

  async checkConnection(): Promise<boolean> {
    // The file backend is always reachable.
    return true;
  }

  // -------------------------------------------------------------------------
  // Project lifecycle
  // -------------------------------------------------------------------------

  async createProject(name: string): Promise<SmapsProject> {
    this.projectId = slugify(name);
    this.projectName = name;
    this.branchId = BRANCH_ID;
    this.headCommitId = HEAD_COMMIT_ID;
    this.elements = [];
    await this.persist();
    return this.toSmapsProject();
  }

  async loadProject(projectId: string): Promise<SmapsProject> {
    const doc = await this.readDoc(projectId);
    this.projectId = doc.id;
    this.projectName = doc.name;
    this.branchId = doc.branchId ?? BRANCH_ID;
    this.headCommitId = doc.headCommitId ?? HEAD_COMMIT_ID;
    this.elements = doc.elements ?? [];
    return this.toSmapsProject();
  }

  async listProjects(): Promise<SmapsProject[]> {
    let files: string[];
    try {
      files = await fs.readdir(this.modelDir);
    } catch {
      return [];
    }
    const projects: SmapsProject[] = [];
    for (const f of files) {
      if (!f.endsWith(".json")) continue;
      try {
        const raw = await fs.readFile(path.join(this.modelDir, f), "utf8");
        const doc = JSON.parse(raw) as FileModelDoc;
        if (doc["@type"] !== "FileModel") continue;
        projects.push({
          "@id": doc.id,
          "@type": "Project",
          name: doc.name,
          defaultBranch: { "@id": doc.branchId ?? BRANCH_ID },
        });
      } catch {
        // Skip unreadable / non-model JSON files.
      }
    }
    return projects;
  }

  // -------------------------------------------------------------------------
  // Element CRUD
  // -------------------------------------------------------------------------

  async createElement(
    type: string,
    name: string,
    attributes: Record<string, unknown> = {}
  ): Promise<SysmlElement> {
    this.assertInitialized();
    const el = this.buildElement(type, name, attributes);
    this.insert(el);
    await this.persist();
    return clone(el);
  }

  async createElements(
    inputs: Array<{ type: string; name: string; attributes?: Record<string, unknown> }>
  ): Promise<SysmlElement[]> {
    this.assertInitialized();
    const created = inputs.map((i) =>
      this.buildElement(i.type, i.name, i.attributes ?? {})
    );
    for (const el of created) this.insert(el);
    await this.persist();
    return created.map(clone);
  }

  async updateElement(
    elementId: string,
    updates: Record<string, unknown>
  ): Promise<SysmlElement> {
    this.assertInitialized();
    const el = this.find(elementId);
    if (!el) throw new Error(`Element not found: ${elementId}`);

    el.raw = { ...el.raw, ...updates };
    if (typeof updates.name === "string") el.name = updates.name;
    if (typeof updates.shortName === "string") el.shortName = updates.shortName;
    if (typeof updates.qualifiedName === "string") {
      el.qualifiedName = updates.qualifiedName;
    }

    if ("owner" in updates || "ownerId" in updates) {
      this.detachFromOwner(el);
      el.ownerId = extractOwner(updates);
      this.attachToOwner(el);
    }

    await this.persist();
    return clone(el);
  }

  async deleteElement(elementId: string): Promise<void> {
    this.assertInitialized();
    const el = this.find(elementId);
    if (!el) return;
    this.detachFromOwner(el);
    this.elements = this.elements.filter((e) => e.id !== el.id);
    await this.persist();
  }

  // -------------------------------------------------------------------------
  // Queries
  // -------------------------------------------------------------------------

  async getElement(elementId: string): Promise<SysmlElement> {
    this.assertInitialized();
    const el = this.find(elementId);
    if (!el) throw new Error(`Element not found: ${elementId}`);
    return clone(el);
  }

  async queryElements(type?: string, namePattern?: string): Promise<SysmlElement[]> {
    this.assertInitialized();
    let result = this.elements;
    if (type) {
      result = result.filter((e) => e.type === type);
    }
    if (namePattern) {
      const needle = namePattern.toLowerCase();
      result = result.filter((e) => (e.name ?? "").toLowerCase().includes(needle));
    }
    return result.map(clone);
  }

  async queryRelationships(
    elementId?: string,
    direction: "in" | "out" | "both" = "both"
  ): Promise<SysmlRelationship[]> {
    this.assertInitialized();
    let rels = this.elements.filter(isRelationship).map(toRelationship);
    if (elementId) {
      rels = rels.filter((r) => {
        const isOut = r.sourceIds.includes(elementId);
        const isIn = r.targetIds.includes(elementId);
        if (direction === "in") return isIn;
        if (direction === "out") return isOut;
        return isIn || isOut;
      });
    }
    return rels;
  }

  async getProjectState(): Promise<ProjectState> {
    this.assertInitialized();
    const counts: Record<string, number> = {};
    for (const el of this.elements) {
      counts[el.type] = (counts[el.type] ?? 0) + 1;
    }
    return {
      projectId: this.projectId!,
      commitId: this.headCommitId!,
      branchId: this.branchId!,
      totalElements: this.elements.length,
      elementCountsByType: counts,
    };
  }

  // -------------------------------------------------------------------------
  // Internals
  // -------------------------------------------------------------------------

  private assertInitialized(): void {
    if (!this.projectId) {
      throw new Error(
        "FileStore not initialized — call createProject() or loadProject() first"
      );
    }
  }

  private buildElement(
    type: string,
    name: string,
    attributes: Record<string, unknown>
  ): SysmlElement {
    const providedId =
      typeof attributes["@id"] === "string" ? (attributes["@id"] as string) : null;
    const id = providedId ?? randomUUID();

    const resolvedName =
      name !== ""
        ? name
        : typeof attributes.name === "string"
          ? (attributes.name as string)
          : null;

    return {
      id,
      elementId: id,
      type,
      name: resolvedName,
      shortName:
        asString(attributes.shortName) ?? asString(attributes.declaredShortName) ?? null,
      qualifiedName: asString(attributes.qualifiedName) ?? null,
      ownerId: extractOwner(attributes),
      ownedElementIds: [],
      raw: { "@id": id, "@type": type, name: resolvedName, ...attributes },
    };
  }

  private insert(el: SysmlElement): void {
    this.elements.push(el);
    this.attachToOwner(el);
  }

  private attachToOwner(el: SysmlElement): void {
    if (!el.ownerId) return;
    const owner = this.find(el.ownerId);
    if (owner && !owner.ownedElementIds.includes(el.id)) {
      owner.ownedElementIds.push(el.id);
    }
  }

  private detachFromOwner(el: SysmlElement): void {
    if (!el.ownerId) return;
    const owner = this.find(el.ownerId);
    if (owner) {
      owner.ownedElementIds = owner.ownedElementIds.filter((x) => x !== el.id);
    }
  }

  private find(elementId: string): SysmlElement | undefined {
    return this.elements.find(
      (e) => e.id === elementId || e.elementId === elementId
    );
  }

  private toSmapsProject(): SmapsProject {
    return {
      "@id": this.projectId!,
      "@type": "Project",
      name: this.projectName ?? this.projectId!,
      defaultBranch: { "@id": this.branchId! },
    };
  }

  private filePath(projectId: string): string {
    return path.join(this.modelDir, `${projectId}.json`);
  }

  private async readDoc(projectId: string): Promise<FileModelDoc> {
    let raw: string;
    try {
      raw = await fs.readFile(this.filePath(projectId), "utf8");
    } catch {
      throw new Error(`Project "${projectId}" not found in ${this.modelDir}`);
    }
    return JSON.parse(raw) as FileModelDoc;
  }

  private async persist(): Promise<void> {
    await fs.mkdir(this.modelDir, { recursive: true });
    const doc: FileModelDoc = {
      "@type": "FileModel",
      id: this.projectId!,
      name: this.projectName ?? this.projectId!,
      branchId: this.branchId!,
      headCommitId: this.headCommitId!,
      elements: this.elements,
    };
    await fs.writeFile(
      this.filePath(this.projectId!),
      JSON.stringify(doc, null, 2),
      "utf8"
    );
  }
}

// ---------------------------------------------------------------------------
// Module helpers
// ---------------------------------------------------------------------------

function slugify(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
  return slug || randomUUID();
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function clone(el: SysmlElement): SysmlElement {
  return {
    ...el,
    ownedElementIds: [...el.ownedElementIds],
    raw: { ...el.raw },
  };
}

function extractOwner(attributes: Record<string, unknown>): string | null {
  const owner = attributes.owner;
  if (typeof owner === "string") return owner;
  if (owner && typeof owner === "object" && "@id" in (owner as object)) {
    const id = (owner as { "@id"?: unknown })["@id"];
    if (typeof id === "string") return id;
  }
  if (typeof attributes.ownerId === "string") return attributes.ownerId as string;
  return null;
}

function isRelationship(el: SysmlElement): boolean {
  if (Array.isArray(el.raw.source) || Array.isArray(el.raw.target)) return true;
  if (RELATIONSHIP_TYPE_SET.has(el.type)) return true;
  return (
    el.type.endsWith("Membership") ||
    el.type.endsWith("Connector") ||
    el.type === "Dependency" ||
    el.type === "Specialization" ||
    el.type === "Subclassification" ||
    el.type === "Subsetting" ||
    el.type === "Redefinition"
  );
}

function idsFrom(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    if (typeof item === "string") {
      out.push(item);
    } else if (item && typeof item === "object" && "@id" in (item as object)) {
      const id = (item as { "@id"?: unknown })["@id"];
      if (typeof id === "string") out.push(id);
    }
  }
  return out;
}

function toRelationship(el: SysmlElement): SysmlRelationship {
  return {
    id: el.id,
    type: el.type,
    sourceIds: idsFrom(el.raw.source),
    targetIds: idsFrom(el.raw.target),
    raw: el.raw,
  };
}
