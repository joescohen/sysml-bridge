import type { ModelStore } from "./store.js";
import { isRelationship, toRelationship } from "./file-store.js";
import type {
  ProjectDescriptor,
  SysmlElement,
  SysmlRelationship,
  ProjectState,
} from "./types.js";

// ---------------------------------------------------------------------------
// SysmlV2ApiStore
//
// A ModelStore backed by a live OMG SysML v2 API & Services server (e.g. the
// pilot implementation, or eventually Cameo/SysON). Ported from the
// sysml-bridge SmapsClient (packages/mcp-server/src/smaps-client.ts there),
// reconciled with foundry's current SysmlElement/ProjectDescriptor types and
// with the wire contract as VERIFIED LIVE against a running pilot server —
// see docs/superpowers/specs/2026-07-11-repository-substrate-design.md.
//
// The API is a git-like commit graph: Projects -> Branches -> Commits ->
// immutable DataVersion deltas -> Elements. There are NO direct PATCH/DELETE
// endpoints on elements — every mutation (create, update, delete) is a
// Commit containing one or more DataVersion changes. A DataVersion with a
// non-null `payload` creates/updates an element; one with `identity` set and
// `payload: null` deletes it.
//
// Live-verified deviations from the sysml-bridge SmapsClient this was ported
// from (do not "fix" these back without re-verifying against the server):
//
//   1. KerML naming: the wire property for an element's display name is
//      `declaredName`, never `name` (`name` always comes back null). The
//      bridge's original client sent `name` in the payload — that was wrong
//      against this server and is corrected here.
//   2. PrimitiveConstraint.value MUST be an array, even for scalar equality
//      (`{"operator":"=","property":"@type","value":["PartDefinition"]}`).
//      A bare scalar throws a Jackson MismatchedInputException server-side
//      (`Cannot deserialize instance of java.util.ArrayList out of
//      VALUE_STRING token`).
//   3. The `instanceOf` PrimitiveConstraint operator (used by the bridge to
//      fetch "all Relationship-typed elements" server-side) is NOT
//      implemented by the pilot — it throws
//      `UnsupportedOperationException: Unsupported primitive constraint
//      operator: INSTANCE_OF`. queryRelationships() without an elementId
//      falls back to fetching all elements and classifying relationships
//      client-side (same heuristic as FileStore) instead.
//   4. A DataVersion payload is a FULL REPLACEMENT of the element's state,
//      not a patch: sending only the changed field(s) silently resets every
//      other field to null/default (verified: isAbstract:true became null
//      after an update payload that omitted it). updateElement() therefore
//      reads the current element and merges `updates` into its full raw
//      snapshot before committing, exactly as the bridge already did.
//   5. Setting `owner: {"@id": parentId}` on create stores that reference on
//      the child, but does NOT populate the parent's `ownedElement` inverse
//      — real SysML v2 containment needs an explicit OwningMembership
//      relationship element, which this store does not create. Best-effort
//      owner passthrough is implemented for parity with FileStore's
//      ownerId field, but containment is NOT reliably established this way
//      against the pilot. Documented, not solved, in this milestone.
//   6. There is no merge/rebase/diff endpoint anywhere in the API — branch +
//      commit only. Merge is a client-side concern (Milestone 3, parked).
// ---------------------------------------------------------------------------

// --- Wire types (SysML v2 API / KerML JSON shape) --------------------------

interface WireRef {
  "@id": string;
}

interface WireProject {
  "@id": string;
  "@type": "Project";
  name: string;
  description?: string | null;
  created?: string;
  defaultBranch?: WireRef;
}

interface WireBranch {
  "@id": string;
  "@type": "Branch";
  name: string;
  head: WireRef | null;
  owningProject?: WireRef;
  referencedCommit?: WireRef | null;
  created?: string;
}

interface WireDataVersion {
  "@type": "DataVersion";
  payload: Record<string, unknown> | null;
  identity?: WireRef;
}

interface WireCommitRequest {
  "@type": "Commit";
  change: WireDataVersion[];
  previousCommit?: WireRef;
}

interface WireCommitResponse {
  "@id": string;
  "@type": "Commit";
  created?: string;
  owningProject?: WireRef;
  previousCommit?: WireRef | null;
}

interface WireElement {
  "@id": string;
  "@type": string;
  elementId?: string;
  name?: string | null;
  declaredName?: string | null;
  declaredShortName?: string | null;
  qualifiedName?: string | null;
  aliasIds?: unknown[];
  owner?: WireRef | null;
  ownedElement?: WireRef[];
  [key: string]: unknown;
}

/** Live-verified: `value` must be an array, even for scalar equality. */
interface WirePrimitiveConstraint {
  "@type": "PrimitiveConstraint";
  inverse: boolean;
  operator: "=" | "<" | "<=" | ">" | ">=" | "in" | "instanceOf";
  property: string;
  value: unknown[];
}

interface WireCompositeConstraint {
  "@type": "CompositeConstraint";
  operator: "and" | "or";
  constraint: Array<WirePrimitiveConstraint | WireCompositeConstraint>;
}

interface WireQuery {
  "@type": "Query";
  select?: string[];
  where?: WirePrimitiveConstraint | WireCompositeConstraint;
}

// ---------------------------------------------------------------------------

export class SysmlV2ApiStore implements ModelStore {
  private endpoint: string;

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

  async createProject(name: string): Promise<ProjectDescriptor> {
    const res = await fetch(`${this.endpoint}/projects`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ "@type": "Project", name }),
    });
    if (!res.ok) {
      throw new Error(`Failed to create project: ${res.statusText}`);
    }
    const project = (await res.json()) as WireProject;
    await this._initFromProject(project);
    return this._toProjectDescriptor(project);
  }

  async loadProject(projectId: string): Promise<ProjectDescriptor> {
    const res = await fetch(`${this.endpoint}/projects/${projectId}`);
    if (!res.ok) {
      throw new Error(`Failed to load project ${projectId}: ${res.statusText}`);
    }
    const project = (await res.json()) as WireProject;
    await this._initFromProject(project);
    return this._toProjectDescriptor(project);
  }

  async listProjects(): Promise<ProjectDescriptor[]> {
    const res = await fetch(`${this.endpoint}/projects`);
    if (!res.ok) {
      throw new Error(`Failed to list projects: ${res.statusText}`);
    }
    const projects = (await res.json()) as WireProject[];
    return projects.map((p) => this._toProjectDescriptor(p));
  }

  // -------------------------------------------------------------------------
  // Element CRUD — all mutations go through the commit endpoint
  // -------------------------------------------------------------------------

  async createElement(
    type: string,
    name: string,
    attributes: Record<string, unknown> = {}
  ): Promise<SysmlElement> {
    this.assertInitialized();
    const commit = await this._postCommit([
      { "@type": "DataVersion", payload: toWirePayload(type, name, attributes) },
    ]);
    const changes = await this._fetchCommitChanges(commit["@id"]);
    return this._toSysmlElement(changes[0].payload as WireElement);
  }

  async createElements(
    elements: Array<{ type: string; name: string; attributes?: Record<string, unknown> }>
  ): Promise<SysmlElement[]> {
    this.assertInitialized();
    const changes: WireDataVersion[] = elements.map((el) => ({
      "@type": "DataVersion",
      payload: toWirePayload(el.type, el.name, el.attributes ?? {}),
    }));
    const commit = await this._postCommit(changes);
    const committed = await this._fetchCommitChanges(commit["@id"]);
    return committed.map((c) => this._toSysmlElement(c.payload as WireElement));
  }

  async updateElement(
    elementId: string,
    updates: Record<string, unknown>
  ): Promise<SysmlElement> {
    this.assertInitialized();

    // Live-verified: a DataVersion payload fully replaces the element's
    // state — omitted fields are reset, not left alone. So we must read the
    // current full snapshot and merge `updates` into it before committing,
    // rather than sending `updates` alone.
    const current = await this.getElement(elementId);
    const merged: Record<string, unknown> = { ...current.raw, ...updates };
    delete merged["@id"];
    delete merged["@type"];
    delete merged.elementId;
    delete merged.ownedElement; // server-derived inverse, not settable here

    const resolvedName =
      typeof updates.declaredName === "string"
        ? updates.declaredName
        : typeof updates.name === "string"
          ? updates.name
          : (current.name ?? "");
    // `merged.declaredName` may still hold the STALE value carried over from
    // current.raw (e.g. when `updates` only sets `name`, not `declaredName`)
    // — toWirePayload prefers an explicit attributes.declaredName over its
    // `name` parameter, which would silently undo the rename. Clear it so
    // `resolvedName` (the single source of truth for this update) wins.
    delete merged.declaredName;
    delete merged.name;

    const commit = await this._postCommit([
      {
        "@type": "DataVersion",
        identity: { "@id": elementId },
        payload: toWirePayload(current.type, resolvedName, merged),
      },
    ]);
    const changes = await this._fetchCommitChanges(commit["@id"]);
    return this._toSysmlElement(changes[0].payload as WireElement);
  }

  async deleteElement(elementId: string): Promise<void> {
    this.assertInitialized();
    await this._postCommit([
      { "@type": "DataVersion", identity: { "@id": elementId }, payload: null },
    ]);
  }

  // -------------------------------------------------------------------------
  // Queries
  // -------------------------------------------------------------------------

  async getElement(elementId: string): Promise<SysmlElement> {
    this.assertInitialized();
    // A brand-new project/branch has no commits yet (branch.head is null,
    // live-verified) — there is nothing to fetch, and interpolating a null
    // headCommitId into the URL would literally send `.../commits/null/...`,
    // which the server 400s on. Fail with the same "not found" contract the
    // caller already expects instead.
    if (this.headCommitId === null) {
      throw new Error(`Element not found: ${elementId} — no commits yet`);
    }
    const url = `${this.endpoint}/projects/${this.projectId!}/commits/${this.headCommitId}/elements/${elementId}`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Element not found: ${elementId} — ${res.statusText}`);
    }
    const data = (await res.json()) as WireElement;
    return this._toSysmlElement(data);
  }

  /**
   * Query elements using the SysML v2 API Query endpoint.
   * POSTs to /query-results?commitId={commitId}.
   *
   * The pilot only supports exact-match constraints server-side (no
   * substring/wildcard operator), so `namePattern` is applied client-side —
   * same substring, case-insensitive semantics as FileStore — for parity
   * across backends.
   */
  async queryElements(type?: string, namePattern?: string): Promise<SysmlElement[]> {
    this.assertInitialized();

    // No commits yet on this branch => no elements exist yet. (Live-verified
    // bug found via the real MCP tool surface: create_element's GATE-05
    // pre-check calls queryElements() before anything has ever been
    // committed, which would otherwise send `commitId=null` and 400.)
    if (this.headCommitId === null) {
      return [];
    }

    const query: WireQuery = type
      ? {
          "@type": "Query",
          where: {
            "@type": "PrimitiveConstraint",
            inverse: false,
            operator: "=",
            property: "@type",
            value: [type],
          },
        }
      : { "@type": "Query" };

    let elements = await this._postQuery(query);

    if (namePattern) {
      const needle = namePattern.toLowerCase();
      elements = elements.filter((e) => (e.name ?? "").toLowerCase().includes(needle));
    }

    return elements;
  }

  /**
   * Query relationships.
   * If elementId is given, uses the element-specific relationships endpoint.
   * Otherwise, fetches all elements and classifies relationships
   * client-side (see class-level doc comment, deviation #3).
   */
  async queryRelationships(
    elementId?: string,
    direction: "in" | "out" | "both" = "both"
  ): Promise<SysmlRelationship[]> {
    this.assertInitialized();

    if (elementId) {
      // No commits yet => the element (and any relationship touching it)
      // cannot exist. Same null-headCommitId guard as getElement/queryElements.
      if (this.headCommitId === null) return [];

      const url =
        `${this.endpoint}/projects/${this.projectId!}/commits/${this.headCommitId}` +
        `/elements/${elementId}/relationships?direction=${direction}`;
      const res = await fetch(url);
      if (!res.ok) {
        throw new Error(`Relationship query failed: ${res.statusText}`);
      }
      const data = (await res.json()) as WireElement[];
      return data.map((d) => toRelationship(this._toSysmlElement(d)));
    }

    const all = await this.queryElements();
    return all.filter(isRelationship).map(toRelationship);
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
      // No commits yet => no commit id; ProjectState.commitId is typed as a
      // required string, so represent "none yet" as "" rather than the
      // literal string "null".
      commitId: this.headCommitId ?? "",
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
        "SysmlV2ApiStore not initialized — call createProject() or loadProject() first"
      );
    }
  }

  private async _postQuery(query: WireQuery): Promise<SysmlElement[]> {
    const url = `${this.endpoint}/projects/${this.projectId!}/query-results?commitId=${this.headCommitId!}`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(query),
    });
    if (!res.ok) {
      throw new Error(`Query failed: ${res.statusText}`);
    }
    const data = (await res.json()) as WireElement[];
    return data.map((d) => this._toSysmlElement(d));
  }

  /** Shared commit POST. Updates headCommitId on success. */
  private async _postCommit(changes: WireDataVersion[]): Promise<WireCommitResponse> {
    const body: WireCommitRequest = {
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
    const commit = (await res.json()) as WireCommitResponse;
    this.headCommitId = commit["@id"];
    return commit;
  }

  /** Fetch the DataVersion changes for a commit (the commit response doesn't inline them). */
  private async _fetchCommitChanges(commitId: string): Promise<WireDataVersion[]> {
    const url = `${this.endpoint}/projects/${this.projectId!}/commits/${commitId}/changes`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Failed to fetch commit changes: ${res.statusText}`);
    }
    return (await res.json()) as WireDataVersion[];
  }

  /** Load project/branch/head-commit ids from a WireProject response. */
  private async _initFromProject(project: WireProject): Promise<void> {
    this.projectId = project["@id"];
    const defaultBranchId = project.defaultBranch?.["@id"];
    if (!defaultBranchId) {
      throw new Error(`Project ${this.projectId} has no defaultBranch`);
    }
    this.branchId = defaultBranchId;

    const branchUrl = `${this.endpoint}/projects/${this.projectId}/branches/${this.branchId}`;
    const res = await fetch(branchUrl);
    if (!res.ok) {
      throw new Error(`Failed to fetch branch: ${res.statusText}`);
    }
    const branch = (await res.json()) as WireBranch;
    this.headCommitId = branch.head?.["@id"] ?? null;
  }

  private _toProjectDescriptor(project: WireProject): ProjectDescriptor {
    return {
      "@id": project["@id"],
      "@type": "Project",
      name: project.name,
      defaultBranch: { "@id": project.defaultBranch?.["@id"] ?? this.branchId ?? "" },
    };
  }

  private _toSysmlElement(data: WireElement): SysmlElement {
    const serverId = data["@id"] ?? "";
    const aliasIds = Array.isArray(data.aliasIds) ? data.aliasIds : [];
    const aliasId = typeof aliasIds[0] === "string" ? (aliasIds[0] as string) : undefined;

    return {
      id: serverId,
      elementId: data.elementId ?? serverId,
      type: data["@type"] ?? "",
      name: data.declaredName ?? data.name ?? null,
      shortName: data.declaredShortName ?? null,
      qualifiedName: data.qualifiedName ?? null,
      ownerId: data.owner?.["@id"] ?? null,
      ownedElementIds: (data.ownedElement ?? []).map((e) => e["@id"]),
      ...(aliasId !== undefined ? { aliasId } : {}),
      raw: data as Record<string, unknown>,
    };
  }
}

// ---------------------------------------------------------------------------
// Module helpers
// ---------------------------------------------------------------------------

/**
 * Translate foundry-side createElement/updateElement arguments into a
 * SysML v2 API wire payload:
 *   - `name` (positional, foundry convention) -> `declaredName`
 *   - `attributes.shortName` -> `declaredShortName`
 *   - `attributes.owner` / `attributes.ownerId` -> `owner: {"@id": ...}`
 *   - `attributes.aliasId` (a foundry-local id to preserve) is folded into
 *     `aliasIds` alongside any explicitly-provided `aliasIds`.
 * Everything else in `attributes` passes through unchanged.
 */
function toWirePayload(
  type: string,
  name: string,
  attributes: Record<string, unknown>
): Record<string, unknown> {
  const {
    aliasId,
    aliasIds: existingAliasIds,
    name: _foundryName,
    declaredName: explicitDeclaredName,
    shortName,
    declaredShortName: explicitDeclaredShortName,
    owner,
    ownerId,
    ...rest
  } = attributes;

  const declaredName =
    typeof explicitDeclaredName === "string"
      ? explicitDeclaredName
      : name !== ""
        ? name
        : null;

  const payload: Record<string, unknown> = { ...rest, "@type": type, declaredName };

  const declaredShortName =
    typeof explicitDeclaredShortName === "string"
      ? explicitDeclaredShortName
      : typeof shortName === "string"
        ? shortName
        : undefined;
  if (declaredShortName !== undefined) payload.declaredShortName = declaredShortName;

  const ownerRef = extractOwnerRef(owner, ownerId);
  if (ownerRef) payload.owner = ownerRef;

  const aliasIdList = Array.isArray(existingAliasIds) ? [...existingAliasIds] : [];
  if (typeof aliasId === "string" && !aliasIdList.includes(aliasId)) {
    aliasIdList.push(aliasId);
  }
  if (aliasIdList.length > 0) payload.aliasIds = aliasIdList;

  return payload;
}

function extractOwnerRef(owner: unknown, ownerId: unknown): WireRef | undefined {
  if (typeof owner === "string") return { "@id": owner };
  if (owner && typeof owner === "object" && "@id" in (owner as object)) {
    const id = (owner as { "@id"?: unknown })["@id"];
    if (typeof id === "string") return { "@id": id };
  }
  if (typeof ownerId === "string") return { "@id": ownerId };
  return undefined;
}
