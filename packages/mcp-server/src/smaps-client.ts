import type {
  SmapsProject,
  SmapsDataVersion,
  SmapsCommitRequest,
  SmapsCommitResponse,
  SmapsElementResponse,
  SmapsQuery,
  SmapsBranch,
} from "./types/smaps.js";
import type {
  SysmlElement,
  SysmlRelationship,
  ProjectState,
} from "./types/sysml-elements.js";

// ---------------------------------------------------------------------------
// SmapsClient
//
// Wraps the SMAPS REST API which uses a git-like commit model.  All element
// mutations (create, update, delete) are submitted as atomic Commit objects
// containing one or more DataVersion change records.  There are NO direct
// PATCH or DELETE endpoints on elements.
// ---------------------------------------------------------------------------

export class SmapsClient {
  private endpoint: string;

  // Populated by createProject / loadProject
  public projectId: string | null = null;
  public branchId: string | null = null;
  public headCommitId: string | null = null;

  constructor(endpoint: string) {
    this.endpoint = endpoint.replace(/\/$/, "");
  }

  // -------------------------------------------------------------------------
  // Connection
  // -------------------------------------------------------------------------

  async checkConnection(): Promise<boolean> {
    try {
      const res = await fetch(`${this.endpoint}/projects`);
      return res.ok;
    } catch {
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // Project lifecycle
  // -------------------------------------------------------------------------

  /** Create a new SMAPS project and store its IDs. */
  async createProject(name: string): Promise<SmapsProject> {
    const res = await fetch(`${this.endpoint}/projects`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ "@type": "Project", name }),
    });
    if (!res.ok) {
      throw new Error(`Failed to create project: ${res.statusText}`);
    }
    const project = (await res.json()) as SmapsProject;
    await this._initFromProject(project);
    return project;
  }

  /** Load an existing project by ID and store its IDs. */
  async loadProject(projectId: string): Promise<SmapsProject> {
    const res = await fetch(`${this.endpoint}/projects/${projectId}`);
    if (!res.ok) {
      throw new Error(`Failed to load project ${projectId}: ${res.statusText}`);
    }
    const project = (await res.json()) as SmapsProject;
    await this._initFromProject(project);
    return project;
  }

  /** List all projects. */
  async listProjects(): Promise<SmapsProject[]> {
    const res = await fetch(`${this.endpoint}/projects`);
    if (!res.ok) {
      throw new Error(`Failed to list projects: ${res.statusText}`);
    }
    return (await res.json()) as SmapsProject[];
  }

  // -------------------------------------------------------------------------
  // Element CRUD — all mutations go through the commit endpoint
  // -------------------------------------------------------------------------

  /** Create a single element via a commit. */
  async createElement(
    type: string,
    name: string,
    attributes: Record<string, unknown> = {}
  ): Promise<SysmlElement> {
    this.assertInitialized();
    const commit = await this._postCommit([
      {
        "@type": "DataVersion",
        payload: { "@type": type, name, ...attributes },
      },
    ]);
    const changes = await this._fetchCommitChanges(commit["@id"]);
    return this._toSysmlElement(changes[0].payload as SmapsElementResponse);
  }

  /** Batch-create multiple elements in a single commit. */
  async createElements(
    elements: Array<{ type: string; name: string; attributes?: Record<string, unknown> }>
  ): Promise<SysmlElement[]> {
    this.assertInitialized();
    const changes = elements.map((el) => ({
      "@type": "DataVersion" as const,
      payload: { "@type": el.type, name: el.name, ...(el.attributes ?? {}) },
    }));
    const commit = await this._postCommit(changes);
    const committed = await this._fetchCommitChanges(commit["@id"]);
    return committed.map((c) =>
      this._toSysmlElement(c.payload as SmapsElementResponse)
    );
  }

  /** Update an existing element via a commit with identity set. */
  async updateElement(
    elementId: string,
    updates: Record<string, unknown>
  ): Promise<SysmlElement> {
    this.assertInitialized();

    const current = await this.getElement(elementId);

    const commit = await this._postCommit([
      {
        "@type": "DataVersion",
        identity: { "@id": elementId },
        payload: {
          "@type": current.type,
          ...current.raw,
          ...updates,
          identifier: current.elementId ?? undefined,
        },
      },
    ]);
    const changes = await this._fetchCommitChanges(commit["@id"]);
    return this._toSysmlElement(changes[0].payload as SmapsElementResponse);
  }

  /** Delete an element via a commit with null payload. */
  async deleteElement(elementId: string): Promise<void> {
    this.assertInitialized();
    await this._postCommit([
      {
        "@type": "DataVersion",
        identity: { "@id": elementId },
        payload: null,
      },
    ]);
  }

  // -------------------------------------------------------------------------
  // Queries
  // -------------------------------------------------------------------------

  /** Fetch a single element from the current commit. */
  async getElement(elementId: string): Promise<SysmlElement> {
    this.assertInitialized();
    const url = `${this.endpoint}/projects/${this.projectId!}/commits/${this.headCommitId!}/elements/${elementId}`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Element not found: ${elementId} — ${res.statusText}`);
    }
    const data = (await res.json()) as SmapsElementResponse;
    return this._toSysmlElement(data);
  }

  /**
   * Query elements using the SMAPS Query API.
   * POSTs to /query-results?commitId={commitId}.
   */
  async queryElements(type?: string, namePattern?: string): Promise<SysmlElement[]> {
    this.assertInitialized();

    const query: SmapsQuery = { "@type": "Query" };

    const constraints: unknown[] = [];

    if (type) {
      constraints.push({
        "@type": "PrimitiveConstraint",
        inverse: false,
        operator: "=",
        property: "@type",
        value: type,
      });
    }

    if (namePattern) {
      constraints.push({
        "@type": "PrimitiveConstraint",
        inverse: false,
        operator: "=",
        property: "name",
        value: namePattern,
      });
    }

    if (constraints.length === 1) {
      query.where = constraints[0] as SmapsQuery["where"];
    } else if (constraints.length > 1) {
      query.where = {
        "@type": "CompositeConstraint",
        operator: "and",
        constraint: constraints as SmapsQuery["where"] extends { constraint: infer C } ? C : never,
      };
    }

    const url = `${this.endpoint}/projects/${this.projectId!}/query-results?commitId=${this.headCommitId!}`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(query),
    });
    if (!res.ok) {
      throw new Error(`Query failed: ${res.statusText}`);
    }
    const data = (await res.json()) as SmapsElementResponse[];
    return data.map((d) => this._toSysmlElement(d));
  }

  /**
   * Query relationships.
   * If elementId is given, uses the element-specific relationships endpoint.
   * Otherwise, falls back to the query-results endpoint for all relationship types.
   */
  async queryRelationships(
    elementId?: string,
    direction: "in" | "out" | "both" = "both"
  ): Promise<SysmlRelationship[]> {
    this.assertInitialized();

    if (elementId) {
      const url =
        `${this.endpoint}/projects/${this.projectId!}/commits/${this.headCommitId!}` +
        `/elements/${elementId}/relationships?direction=${direction}`;
      const res = await fetch(url);
      if (!res.ok) {
        throw new Error(`Relationship query failed: ${res.statusText}`);
      }
      const data = (await res.json()) as SmapsElementResponse[];
      return data.map((d) => this._toSysmlRelationship(d));
    }

    // No elementId — query all relationship elements via query-results
    const query: SmapsQuery = {
      "@type": "Query",
      where: {
        "@type": "PrimitiveConstraint",
        inverse: false,
        operator: "instanceOf",
        property: "@type",
        value: "Relationship",
      },
    };
    const url = `${this.endpoint}/projects/${this.projectId!}/query-results?commitId=${this.headCommitId!}`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(query),
    });
    if (!res.ok) {
      throw new Error(`Relationship query failed: ${res.statusText}`);
    }
    const data = (await res.json()) as SmapsElementResponse[];
    return data.map((d) => this._toSysmlRelationship(d));
  }

  /** Aggregate element counts for the current project/commit. */
  async getProjectState(): Promise<ProjectState> {
    this.assertInitialized();
    const elements = await this.queryElements();
    const counts: Record<string, number> = {};
    for (const el of elements) {
      counts[el.type] = (counts[el.type] ?? 0) + 1;
    }
    return {
      projectId: this.projectId!,
      commitId: this.headCommitId!,
      branchId: this.branchId!,
      totalElements: elements.length,
      elementCountsByType: counts,
    };
  }

  // -------------------------------------------------------------------------
  // Private helpers
  // -------------------------------------------------------------------------

  private assertInitialized(): void {
    if (!this.projectId || !this.branchId) {
      throw new Error(
        "SmapsClient not initialized — call createProject() or loadProject() first"
      );
    }
  }

  /** Shared commit POST.  Updates headCommitId on success. */
  private async _postCommit(
    changes: SmapsCommitRequest["change"]
  ): Promise<SmapsCommitResponse> {
    const body: SmapsCommitRequest = {
      "@type": "Commit",
      change: changes,
      ...(this.headCommitId ? { previousCommit: { "@id": this.headCommitId } } : {}),
    };

    const url = `${this.endpoint}/projects/${this.projectId!}/commits?branchId=${this.branchId!}`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`Commit failed: ${res.statusText}`);
    }
    const commit = (await res.json()) as SmapsCommitResponse;
    this.headCommitId = commit["@id"];
    return commit;
  }

  /** Fetch the DataVersion changes for a commit (the API doesn't inline them). */
  private async _fetchCommitChanges(
    commitId: string
  ): Promise<SmapsDataVersion[]> {
    const url = `${this.endpoint}/projects/${this.projectId!}/commits/${commitId}/changes`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Failed to fetch commit changes: ${res.statusText}`);
    }
    return (await res.json()) as SmapsDataVersion[];
  }

  /** Load project IDs from a SmapsProject response. */
  private async _initFromProject(project: SmapsProject): Promise<void> {
    this.projectId = project["@id"];
    const defaultBranchId = project.defaultBranch?.["@id"];
    if (!defaultBranchId) {
      throw new Error(`Project ${this.projectId} has no defaultBranch`);
    }
    this.branchId = defaultBranchId;

    // Fetch branch to get the head commit
    const branchUrl = `${this.endpoint}/projects/${this.projectId}/branches/${this.branchId}`;
    const res = await fetch(branchUrl);
    if (!res.ok) {
      throw new Error(`Failed to fetch branch: ${res.statusText}`);
    }
    const branch = (await res.json()) as SmapsBranch;
    this.headCommitId = branch.head?.["@id"] ?? null;
  }

  private _toSysmlElement(data: SmapsElementResponse): SysmlElement {
    return {
      id: (data["@id"] as string) ?? "",
      elementId: (data.elementId as string) ?? (data["@id"] as string) ?? "",
      type: (data["@type"] as string) ?? "",
      name: (data.declaredName as string | null) ?? (data.name as string | null) ?? null,
      shortName: (data.declaredShortName as string | null) ?? null,
      qualifiedName: (data.qualifiedName as string | null) ?? null,
      ownerId: (data.owner as { "@id": string } | undefined)?.["@id"] ?? null,
      ownedElementIds: ((data.ownedElement as Array<{ "@id": string }>) ?? []).map(
        (e) => e["@id"]
      ),
      raw: data as Record<string, unknown>,
    };
  }

  private _toSysmlRelationship(data: SmapsElementResponse): SysmlRelationship {
    return {
      id: (data["@id"] as string) ?? "",
      type: (data["@type"] as string) ?? "",
      sourceIds: ((data.source as Array<{ "@id": string }>) ?? []).map((s) => s["@id"]),
      targetIds: ((data.target as Array<{ "@id": string }>) ?? []).map((t) => t["@id"]),
      raw: data as Record<string, unknown>,
    };
  }
}
