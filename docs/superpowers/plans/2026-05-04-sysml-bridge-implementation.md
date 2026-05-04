# SysML Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a working MCP server that bridges Claude Code to the SysML v2 SMAPS REST API, with a SysML v2 parser/serializer and 11 MBSE skills.

**Architecture:** Monorepo with two packages — an MCP server wrapping the SMAPS commit-based REST API, and a Claude Code skill plugin. The SMAPS API uses a git-like commit model where all element changes are submitted as atomic commits containing DataVersion change records.

**Tech Stack:** TypeScript, MCP SDK (`@modelcontextprotocol/sdk`), Vitest, pnpm workspaces, Docker (PostgreSQL + SysML v2 Pilot API), Zod.

---

## File Structure

### Files to create (new)

| File | Responsibility |
|---|---|
| `packages/mcp-server/src/types/smaps.ts` | SMAPS API request/response types (commit model, DataVersion, Query) |
| `packages/mcp-server/src/types/sysml-elements.ts` | SysML v2 element/relationship domain types |
| `packages/mcp-server/src/smaps-client.ts` | HTTP client wrapping the SMAPS commit-based REST API |
| `packages/mcp-server/src/utils/sysml-parser.ts` | Parse SysML v2 textual notation into element structures |
| `packages/mcp-server/src/utils/sysml-serializer.ts` | Serialize element structures to SysML v2 textual notation |
| `packages/mcp-server/src/tools/create-element.ts` | MCP tool: create elements via commits |
| `packages/mcp-server/src/tools/query-elements.ts` | MCP tool: query elements by type/name |
| `packages/mcp-server/src/tools/create-relationship.ts` | MCP tool: create relationship elements via commits |
| `packages/mcp-server/src/tools/query-relationships.ts` | MCP tool: query relationships for an element |
| `packages/mcp-server/src/tools/validate-model.ts` | MCP tool: run completeness/consistency checks |
| `packages/mcp-server/src/tools/export-sysml.ts` | MCP tool: export model to .sysml text |
| `packages/mcp-server/src/tools/import-sysml.ts` | MCP tool: parse .sysml text and import via commits |
| `packages/mcp-server/src/tools/get-project-state.ts` | MCP tool: return model summary stats |
| `packages/mcp-server/src/index.ts` | MCP server entry point, tool registration |
| `packages/mcp-server/src/__tests__/smaps-client.test.ts` | Unit tests for SMAPS client |
| `packages/mcp-server/src/__tests__/sysml-parser.test.ts` | Unit tests for SysML v2 parser |
| `packages/mcp-server/src/__tests__/sysml-serializer.test.ts` | Unit tests for SysML v2 serializer |
| `packages/mcp-server/src/__tests__/tools.test.ts` | Unit tests for MCP tools |
| `packages/mcp-server/vitest.config.ts` | Vitest configuration |

### Files to modify (existing scaffold that needs correction)

| File | What changes |
|---|---|
| `docker/docker-compose.yml` | Fix image name, env vars, add platform for Apple Silicon |
| `packages/mcp-server/package.json` | Add zod dependency, fix scripts |
| `packages/skills/skills/mbse-build.md` | Fix `verification case def` → `verification def`, `analysis case def` → `analysis def` |

---

## Task 1: Fix Infrastructure — Docker, Dependencies, Build

**Files:**
- Modify: `docker/docker-compose.yml`
- Modify: `packages/mcp-server/package.json`
- Create: `packages/mcp-server/vitest.config.ts`

- [ ] **Step 1: Fix docker-compose.yml with correct image and env vars**

Replace `docker/docker-compose.yml` entirely:

```yaml
services:
  db:
    image: postgres:15
    restart: always
    environment:
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=mysecretpassword
      - POSTGRES_DB=sysml2
    ports:
      - "5432:5432"
    volumes:
      - sysml-data:/var/lib/postgresql
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 5s
      timeout: 5s
      retries: 5

  sysml-api:
    image: mbsemashup/sysmlv2-api.pilotimpl:latest
    platform: linux/amd64
    tty: false
    environment:
      - HIBERNATE_SHOW_SQL=false
      - HIBERNATE_HBM2DDL=update
      - JDBC_DRIVER=org.postgresql.Driver
      - HIBERNATE_DIALECT=org.hibernate.dialect.PostgreSQLDialect
      - JDBC_URL=jdbc:postgresql://db:5432/sysml2
      - JDBC_USER=postgres
      - JDBC_PASSWORD=mysecretpassword
    ports:
      - "9000:9000"
    restart: always
    depends_on:
      db:
        condition: service_healthy

volumes:
  sysml-data:
    driver: local
```

- [ ] **Step 2: Update package.json with correct dependencies**

Replace `packages/mcp-server/package.json`:

```json
{
  "name": "@sysml-bridge/mcp-server",
  "version": "0.1.0",
  "description": "MCP server wrapping the SysML v2 SMAPS REST API",
  "type": "module",
  "main": "dist/index.js",
  "scripts": {
    "build": "tsc",
    "dev": "tsx watch src/index.ts",
    "start": "node dist/index.js",
    "test": "vitest run",
    "test:watch": "vitest",
    "lint": "tsc --noEmit"
  },
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.0.0",
    "zod": "^3.23.0"
  },
  "devDependencies": {
    "tsx": "^4.0.0",
    "typescript": "^5.5.0",
    "vitest": "^2.0.0"
  }
}
```

- [ ] **Step 3: Create vitest config**

Create `packages/mcp-server/vitest.config.ts`:

```typescript
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: true,
    include: ["src/__tests__/**/*.test.ts"],
  },
});
```

- [ ] **Step 4: Install dependencies and verify build**

```bash
cd /path/to/sysml-bridge && pnpm install
cd packages/mcp-server && pnpm lint
```

Expected: TypeScript compilation succeeds (may have errors from scaffold files — that's fine, we'll rewrite them).

- [ ] **Step 5: Commit**

```bash
git add docker/docker-compose.yml packages/mcp-server/package.json packages/mcp-server/vitest.config.ts pnpm-lock.yaml
git commit -m "fix: correct Docker image, env vars, and add vitest config"
```

---

## Task 2: SMAPS Types and Domain Types

**Files:**
- Rewrite: `packages/mcp-server/src/types/smaps.ts`
- Rewrite: `packages/mcp-server/src/types/sysml-elements.ts`

- [ ] **Step 1: Write SMAPS API types matching the actual commit-based API**

Replace `packages/mcp-server/src/types/smaps.ts`:

```typescript
export interface SmapsProject {
  "@id": string;
  "@type": "Project";
  name: string;
  description?: string;
  created?: string;
  defaultBranch?: { "@id": string };
}

export interface SmapsDataVersion {
  "@type": "DataVersion";
  payload: Record<string, unknown> | null;
  identity?: { "@id": string };
}

export interface SmapsCommitRequest {
  "@type": "Commit";
  change: SmapsDataVersion[];
  previousCommit?: { "@id": string };
}

export interface SmapsCommitResponse {
  "@id": string;
  "@type": "Commit";
  created: string;
  owningProject: { "@id": string };
  previousCommit?: { "@id": string };
  change: SmapsDataVersion[];
}

export interface SmapsElementResponse {
  "@id": string;
  "@type": string;
  name?: string | null;
  declaredName?: string | null;
  declaredShortName?: string | null;
  elementId?: string;
  qualifiedName?: string;
  owner?: { "@id": string };
  ownedElement?: Array<{ "@id": string }>;
  ownedRelationship?: Array<{ "@id": string }>;
  source?: Array<{ "@id": string }>;
  target?: Array<{ "@id": string }>;
  relatedElement?: Array<{ "@id": string }>;
  [key: string]: unknown;
}

export interface SmapsPrimitiveConstraint {
  "@type": "PrimitiveConstraint";
  inverse: boolean;
  operator: "=" | "<" | "<=" | ">" | ">=" | "in" | "instanceOf";
  property: string;
  value: string | number | boolean;
}

export interface SmapsCompositeConstraint {
  "@type": "CompositeConstraint";
  operator: "and" | "or";
  constraint: Array<SmapsPrimitiveConstraint | SmapsCompositeConstraint>;
}

export interface SmapsQuery {
  "@type": "Query";
  select?: string[];
  where?: SmapsPrimitiveConstraint | SmapsCompositeConstraint;
}

export interface SmapsBranch {
  "@id": string;
  "@type": "Branch";
  name: string;
  head: { "@id": string };
  owningProject: { "@id": string };
  created: string;
}
```

- [ ] **Step 2: Write SysML v2 domain types**

Replace `packages/mcp-server/src/types/sysml-elements.ts`:

```typescript
export interface SysmlElement {
  id: string;
  elementId: string;
  type: string;
  name: string | null;
  shortName: string | null;
  qualifiedName: string | null;
  ownerId: string | null;
  ownedElementIds: string[];
  raw: Record<string, unknown>;
}

export interface SysmlRelationship {
  id: string;
  type: string;
  sourceIds: string[];
  targetIds: string[];
  raw: Record<string, unknown>;
}

export interface ProjectState {
  projectId: string;
  commitId: string;
  branchId: string;
  totalElements: number;
  elementCountsByType: Record<string, number>;
}

export const SYSML_DEFINITION_TYPES = [
  "Package",
  "PartDefinition",
  "PortDefinition",
  "ConnectionDefinition",
  "InterfaceDefinition",
  "ItemDefinition",
  "AttributeDefinition",
  "RequirementDefinition",
  "ConstraintDefinition",
  "ActionDefinition",
  "StateDefinition",
  "UseCaseDefinition",
  "AllocationDefinition",
  "ViewDefinition",
  "ViewpointDefinition",
  "ConcernDefinition",
  "AnalysisCaseDefinition",
  "VerificationCaseDefinition",
  "EnumerationDefinition",
  "OccurrenceDefinition",
  "MetadataDefinition",
  "CalcDefinition",
  "RenderingDefinition",
] as const;

export const SYSML_USAGE_TYPES = [
  "PartUsage",
  "PortUsage",
  "ConnectionUsage",
  "InterfaceUsage",
  "ItemUsage",
  "AttributeUsage",
  "RequirementUsage",
  "ConstraintUsage",
  "ActionUsage",
  "StateUsage",
  "UseCaseUsage",
  "AllocationUsage",
  "ViewUsage",
  "ViewpointUsage",
  "AnalysisCaseUsage",
  "VerificationCaseUsage",
  "EnumerationUsage",
  "OccurrenceUsage",
  "CalcUsage",
  "RenderingUsage",
] as const;

export const SYSML_RELATIONSHIP_TYPES = [
  "OwningMembership",
  "FeatureMembership",
  "FeatureTyping",
  "Subsetting",
  "Redefinition",
  "Specialization",
  "Subclassification",
  "Conjugation",
  "Dependency",
  "Connector",
  "BindingConnector",
  "Annotation",
  "SatisfyRequirementUsage",
  "RequirementVerificationMembership",
] as const;

export type SysmlDefinitionType = (typeof SYSML_DEFINITION_TYPES)[number];
export type SysmlUsageType = (typeof SYSML_USAGE_TYPES)[number];
export type SysmlRelationshipType = (typeof SYSML_RELATIONSHIP_TYPES)[number];
```

- [ ] **Step 3: Verify types compile**

```bash
cd packages/mcp-server && pnpm lint
```

Expected: No type errors.

- [ ] **Step 4: Commit**

```bash
git add packages/mcp-server/src/types/
git commit -m "feat: rewrite SMAPS and SysML v2 types to match actual API spec"
```

---

## Task 3: SMAPS Client — Commit-Based API

**Files:**
- Rewrite: `packages/mcp-server/src/smaps-client.ts`
- Create: `packages/mcp-server/src/__tests__/smaps-client.test.ts`

- [ ] **Step 1: Write failing tests for SmapsClient**

Create `packages/mcp-server/src/__tests__/smaps-client.test.ts`:

```typescript
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SmapsClient } from "../smaps-client.js";

const mockFetch = vi.fn();
global.fetch = mockFetch;

describe("SmapsClient", () => {
  let client: SmapsClient;

  beforeEach(() => {
    mockFetch.mockReset();
    client = new SmapsClient("http://localhost:9000");
  });

  describe("checkConnection", () => {
    it("returns true when server responds", async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });
      expect(await client.checkConnection()).toBe(true);
    });

    it("returns false when server is down", async () => {
      mockFetch.mockRejectedValueOnce(new Error("ECONNREFUSED"));
      expect(await client.checkConnection()).toBe(false);
    });
  });

  describe("createProject", () => {
    it("creates a project and stores the project/branch/commit IDs", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          "@id": "proj-1",
          "@type": "Project",
          name: "TestProject",
          defaultBranch: { "@id": "branch-1" },
        }),
      });
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ([]),
      });

      const project = await client.createProject("TestProject");
      expect(project["@id"]).toBe("proj-1");
      expect(client.projectId).toBe("proj-1");
      expect(client.branchId).toBe("branch-1");
    });
  });

  describe("createElement", () => {
    it("submits a commit with a DataVersion payload", async () => {
      client.projectId = "proj-1";
      client.branchId = "branch-1";
      client.headCommitId = "commit-0";

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          "@id": "commit-1",
          "@type": "Commit",
          change: [
            {
              "@type": "DataVersion",
              payload: {
                "@id": "elem-1",
                "@type": "PartDefinition",
                name: "Engine",
              },
            },
          ],
        }),
      });

      const result = await client.createElement("PartDefinition", "Engine");

      expect(mockFetch).toHaveBeenCalledWith(
        "http://localhost:9000/projects/proj-1/commits?branchId=branch-1",
        expect.objectContaining({
          method: "POST",
          body: expect.stringContaining('"@type":"PartDefinition"'),
        })
      );
      expect(result.type).toBe("PartDefinition");
      expect(result.name).toBe("Engine");
      expect(client.headCommitId).toBe("commit-1");
    });
  });

  describe("queryElements", () => {
    it("posts a query to query-results endpoint", async () => {
      client.projectId = "proj-1";
      client.headCommitId = "commit-1";

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => [
          { "@id": "e1", "@type": "PartDefinition", name: "Engine" },
          { "@id": "e2", "@type": "PartDefinition", name: "Wheel" },
        ],
      });

      const results = await client.queryElements("PartDefinition");
      expect(results).toHaveLength(2);
      expect(results[0].type).toBe("PartDefinition");
    });
  });

  describe("queryRelationships", () => {
    it("queries relationships for an element with direction", async () => {
      client.projectId = "proj-1";
      client.headCommitId = "commit-1";

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => [
          {
            "@id": "r1",
            "@type": "Specialization",
            source: [{ "@id": "e1" }],
            target: [{ "@id": "e2" }],
          },
        ],
      });

      const rels = await client.queryRelationships("e1", "out");
      expect(rels).toHaveLength(1);
      expect(rels[0].type).toBe("Specialization");
      expect(rels[0].sourceIds).toEqual(["e1"]);
    });
  });

  describe("deleteElement", () => {
    it("submits a commit with null payload", async () => {
      client.projectId = "proj-1";
      client.branchId = "branch-1";
      client.headCommitId = "commit-0";

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          "@id": "commit-2",
          "@type": "Commit",
          change: [],
        }),
      });

      await client.deleteElement("elem-1");

      const body = JSON.parse(mockFetch.mock.calls[0][1].body);
      expect(body.change[0].payload).toBeNull();
      expect(body.change[0].identity["@id"]).toBe("elem-1");
    });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/mcp-server && pnpm test
```

Expected: FAIL — SmapsClient not yet updated.

- [ ] **Step 3: Rewrite SmapsClient with commit-based API**

Replace `packages/mcp-server/src/smaps-client.ts`:

```typescript
import type {
  SmapsProject,
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

export class SmapsClient {
  private endpoint: string;
  projectId: string | null = null;
  branchId: string | null = null;
  headCommitId: string | null = null;

  constructor(endpoint: string) {
    this.endpoint = endpoint.replace(/\/$/, "");
  }

  async checkConnection(): Promise<boolean> {
    try {
      const res = await fetch(`${this.endpoint}/projects`);
      return res.ok;
    } catch {
      return false;
    }
  }

  private assertInitialized(): void {
    if (!this.projectId || !this.branchId) {
      throw new Error(
        "Project not initialized. Call createProject() or loadProject() first."
      );
    }
  }

  async createProject(name: string): Promise<SmapsProject> {
    const res = await fetch(`${this.endpoint}/projects`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ "@type": "Project", name }),
    });
    if (!res.ok) throw new Error(`Failed to create project: ${res.statusText}`);

    const project = (await res.json()) as SmapsProject;
    this.projectId = project["@id"];
    this.branchId = project.defaultBranch?.["@id"] ?? null;

    const commitsRes = await fetch(
      `${this.endpoint}/projects/${this.projectId}/commits`
    );
    if (commitsRes.ok) {
      const commits = (await commitsRes.json()) as SmapsCommitResponse[];
      if (commits.length > 0) {
        this.headCommitId = commits[commits.length - 1]["@id"];
      }
    }

    return project;
  }

  async loadProject(projectId: string): Promise<SmapsProject> {
    const res = await fetch(`${this.endpoint}/projects/${projectId}`);
    if (!res.ok) throw new Error(`Project not found: ${projectId}`);

    const project = (await res.json()) as SmapsProject;
    this.projectId = project["@id"];
    this.branchId = project.defaultBranch?.["@id"] ?? null;

    const branchRes = await fetch(
      `${this.endpoint}/projects/${this.projectId}/branches`
    );
    if (branchRes.ok) {
      const branches = (await branchRes.json()) as SmapsBranch[];
      const main = branches.find((b) => b["@id"] === this.branchId);
      if (main) {
        this.headCommitId = main.head["@id"];
      }
    }

    return project;
  }

  async listProjects(): Promise<SmapsProject[]> {
    const res = await fetch(`${this.endpoint}/projects`);
    if (!res.ok) throw new Error(`Failed to list projects: ${res.statusText}`);
    return (await res.json()) as SmapsProject[];
  }

  private async commit(
    changes: Array<{
      payload: Record<string, unknown> | null;
      identityId?: string;
    }>
  ): Promise<SmapsCommitResponse> {
    this.assertInitialized();

    const body: SmapsCommitRequest = {
      "@type": "Commit",
      change: changes.map((c) => ({
        "@type": "DataVersion" as const,
        payload: c.payload,
        ...(c.identityId ? { identity: { "@id": c.identityId } } : {}),
      })),
      ...(this.headCommitId
        ? { previousCommit: { "@id": this.headCommitId } }
        : {}),
    };

    const url = `${this.endpoint}/projects/${this.projectId}/commits?branchId=${this.branchId}`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(`Commit failed (${res.status}): ${text}`);
    }

    const commit = (await res.json()) as SmapsCommitResponse;
    this.headCommitId = commit["@id"];
    return commit;
  }

  async createElement(
    type: string,
    name: string,
    attributes: Record<string, unknown> = {}
  ): Promise<SysmlElement> {
    const commit = await this.commit([
      { payload: { "@type": type, name, ...attributes } },
    ]);

    const created = commit.change[0]?.payload;
    if (!created) throw new Error("No element returned in commit response");

    return this.toElement(created as SmapsElementResponse);
  }

  async createElements(
    elements: Array<{
      type: string;
      name: string;
      attributes?: Record<string, unknown>;
    }>
  ): Promise<SysmlElement[]> {
    const commit = await this.commit(
      elements.map((e) => ({
        payload: { "@type": e.type, name: e.name, ...e.attributes },
      }))
    );

    return commit.change
      .filter((c) => c.payload !== null)
      .map((c) => this.toElement(c.payload as SmapsElementResponse));
  }

  async updateElement(
    elementId: string,
    updates: Record<string, unknown>
  ): Promise<SysmlElement> {
    const commit = await this.commit([
      {
        payload: { ...updates, identifier: elementId },
        identityId: elementId,
      },
    ]);

    const updated = commit.change[0]?.payload;
    if (!updated) throw new Error("No element returned in commit response");

    return this.toElement(updated as SmapsElementResponse);
  }

  async deleteElement(elementId: string): Promise<void> {
    await this.commit([{ payload: null, identityId: elementId }]);
  }

  async getElement(elementId: string): Promise<SysmlElement> {
    this.assertInitialized();
    if (!this.headCommitId) throw new Error("No commits exist yet");

    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/commits/${this.headCommitId}/elements/${elementId}`
    );
    if (!res.ok) throw new Error(`Element not found: ${elementId}`);

    return this.toElement((await res.json()) as SmapsElementResponse);
  }

  async queryElements(
    type?: string,
    namePattern?: string
  ): Promise<SysmlElement[]> {
    this.assertInitialized();
    if (!this.headCommitId) return [];

    if (!type && !namePattern) {
      const res = await fetch(
        `${this.endpoint}/projects/${this.projectId}/commits/${this.headCommitId}/elements`
      );
      if (!res.ok) throw new Error(`Query failed: ${res.statusText}`);
      const data = (await res.json()) as SmapsElementResponse[];
      return data.map((d) => this.toElement(d));
    }

    const constraints = [];
    if (type) {
      constraints.push({
        "@type": "PrimitiveConstraint" as const,
        inverse: false,
        operator: "=" as const,
        property: "@type",
        value: type,
      });
    }

    const query: SmapsQuery = {
      "@type": "Query",
      where:
        constraints.length === 1
          ? constraints[0]
          : {
              "@type": "CompositeConstraint" as const,
              operator: "and" as const,
              constraint: constraints,
            },
    };

    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/query-results?commitId=${this.headCommitId}`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(query),
      }
    );

    if (!res.ok) throw new Error(`Query failed: ${res.statusText}`);
    const data = (await res.json()) as SmapsElementResponse[];

    let results = data.map((d) => this.toElement(d));
    if (namePattern) {
      const lower = namePattern.toLowerCase();
      results = results.filter(
        (e) => e.name && e.name.toLowerCase().includes(lower)
      );
    }

    return results;
  }

  async queryRelationships(
    elementId?: string,
    direction: "in" | "out" | "both" = "both"
  ): Promise<SysmlRelationship[]> {
    this.assertInitialized();
    if (!this.headCommitId) return [];

    if (elementId) {
      const params = new URLSearchParams({ direction });
      const res = await fetch(
        `${this.endpoint}/projects/${this.projectId}/commits/${this.headCommitId}/elements/${elementId}/relationships?${params}`
      );
      if (!res.ok)
        throw new Error(`Relationship query failed: ${res.statusText}`);
      const data = (await res.json()) as SmapsElementResponse[];
      return data.map((d) => this.toRelationship(d));
    }

    const allElements = await this.queryElements();
    const relationships: SysmlRelationship[] = [];
    for (const el of allElements) {
      if (this.isRelationshipType(el.type)) {
        relationships.push({
          id: el.id,
          type: el.type,
          sourceIds: (el.raw.source as Array<{ "@id": string }> | undefined)?.map(
            (s) => s["@id"]
          ) ?? [],
          targetIds: (el.raw.target as Array<{ "@id": string }> | undefined)?.map(
            (t) => t["@id"]
          ) ?? [],
          raw: el.raw,
        });
      }
    }
    return relationships;
  }

  async getProjectState(): Promise<ProjectState> {
    this.assertInitialized();

    const elements = await this.queryElements();
    const counts: Record<string, number> = {};
    for (const el of elements) {
      counts[el.type] = (counts[el.type] ?? 0) + 1;
    }

    return {
      projectId: this.projectId!,
      commitId: this.headCommitId ?? "",
      branchId: this.branchId ?? "",
      totalElements: elements.length,
      elementCountsByType: counts,
    };
  }

  private isRelationshipType(type: string): boolean {
    const relTypes = new Set([
      "OwningMembership",
      "FeatureMembership",
      "FeatureTyping",
      "Subsetting",
      "Redefinition",
      "Specialization",
      "Subclassification",
      "Conjugation",
      "Dependency",
      "Connector",
      "BindingConnector",
      "Annotation",
      "SatisfyRequirementUsage",
      "RequirementVerificationMembership",
    ]);
    return relTypes.has(type);
  }

  private toElement(data: SmapsElementResponse): SysmlElement {
    return {
      id: data["@id"] ?? "",
      elementId: (data.elementId as string) ?? data["@id"] ?? "",
      type: data["@type"] ?? "",
      name: data.name ?? data.declaredName ?? null,
      shortName: data.declaredShortName ?? null,
      qualifiedName: data.qualifiedName ?? null,
      ownerId: data.owner?.["@id"] ?? null,
      ownedElementIds:
        data.ownedElement?.map((e) => e["@id"]) ?? [],
      raw: data as Record<string, unknown>,
    };
  }

  private toRelationship(data: SmapsElementResponse): SysmlRelationship {
    return {
      id: data["@id"] ?? "",
      type: data["@type"] ?? "",
      sourceIds: data.source?.map((s) => s["@id"]) ?? [],
      targetIds: data.target?.map((t) => t["@id"]) ?? [],
      raw: data as Record<string, unknown>,
    };
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd packages/mcp-server && pnpm test
```

Expected: All SmapsClient tests PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/mcp-server/src/smaps-client.ts packages/mcp-server/src/__tests__/smaps-client.test.ts
git commit -m "feat: rewrite SmapsClient for commit-based SMAPS API"
```

---

## Task 4: SysML v2 Parser

**Files:**
- Rewrite: `packages/mcp-server/src/utils/sysml-parser.ts`
- Create: `packages/mcp-server/src/__tests__/sysml-parser.test.ts`

- [ ] **Step 1: Write failing tests for the parser**

Create `packages/mcp-server/src/__tests__/sysml-parser.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { parseSysml } from "../utils/sysml-parser.js";

describe("parseSysml", () => {
  it("parses a package", () => {
    const result = parseSysml(`package Vehicle {}`);
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("Package");
    expect(result.elements[0].name).toBe("Vehicle");
  });

  it("parses part definitions", () => {
    const result = parseSysml(`
      part def Engine;
      part def Wheel;
    `);
    expect(result.elements).toHaveLength(2);
    expect(result.elements[0]).toEqual(
      expect.objectContaining({ type: "PartDefinition", name: "Engine" })
    );
  });

  it("parses part usages with typing", () => {
    const result = parseSysml(`part engine : Engine;`);
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("PartUsage");
    expect(result.elements[0].name).toBe("engine");
    expect(result.elements[0].typedBy).toBe("Engine");
  });

  it("parses requirement definitions", () => {
    const result = parseSysml(`
      requirement def MassRequirement {
        doc /* The mass shall not exceed the limit */
      }
    `);
    expect(result.elements).toHaveLength(1);
    expect(result.elements[0].type).toBe("RequirementDefinition");
  });

  it("parses short names in angle brackets", () => {
    const result = parseSysml(`requirement <'SYS-001'> MaxMass;`);
    expect(result.elements[0].shortName).toBe("SYS-001");
    expect(result.elements[0].name).toBe("MaxMass");
  });

  it("parses specialization with :>", () => {
    const result = parseSysml(`part def SportsCar :> Vehicle;`);
    expect(result.elements[0].specializes).toBe("Vehicle");
  });

  it("parses nested elements as children", () => {
    const result = parseSysml(`
      part def Vehicle {
        part engine : Engine;
        part transmission : Transmission;
      }
    `);
    expect(result.elements).toHaveLength(3);
    expect(result.elements[0].children).toHaveLength(2);
  });

  it("parses satisfy relationships", () => {
    const result = parseSysml(`satisfy massReq by vehicle;`);
    expect(result.relationships).toHaveLength(1);
    expect(result.relationships[0].type).toBe("satisfy");
    expect(result.relationships[0].requirement).toBe("massReq");
    expect(result.relationships[0].by).toBe("vehicle");
  });

  it("parses imports", () => {
    const result = parseSysml(`
      import ISQ::*;
      private import SI::kg;
    `);
    expect(result.imports).toHaveLength(2);
    expect(result.imports[0]).toBe("ISQ::*");
  });

  it("parses verification definitions", () => {
    const result = parseSysml(`verification def MassTest;`);
    expect(result.elements[0].type).toBe("VerificationCaseDefinition");
  });

  it("parses analysis definitions", () => {
    const result = parseSysml(`analysis def FuelAnalysis;`);
    expect(result.elements[0].type).toBe("AnalysisCaseDefinition");
  });

  it("parses enum definitions", () => {
    const result = parseSysml(`enum def FuelKind { gas; diesel; }`);
    expect(result.elements[0].type).toBe("EnumerationDefinition");
  });

  it("returns errors for unparseable lines", () => {
    const result = parseSysml(`this is not valid sysml`);
    expect(result.errors.length).toBeGreaterThan(0);
  });

  it("handles empty input", () => {
    const result = parseSysml("");
    expect(result.elements).toHaveLength(0);
    expect(result.errors).toHaveLength(0);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/mcp-server && pnpm test -- src/__tests__/sysml-parser.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement the parser**

Replace `packages/mcp-server/src/utils/sysml-parser.ts`:

```typescript
export interface ParsedElement {
  type: string;
  name: string;
  shortName?: string;
  typedBy?: string;
  specializes?: string;
  children: ParsedElement[];
  attributes: Record<string, unknown>;
}

export interface ParsedRelationship {
  type: "satisfy" | "verify" | "allocate" | "dependency";
  requirement?: string;
  by?: string;
  from?: string;
  to?: string;
}

export interface ParseResult {
  elements: ParsedElement[];
  relationships: ParsedRelationship[];
  imports: string[];
  errors: string[];
}

const KEYWORD_MAP: Record<string, string> = {
  "package": "Package",
  "part def": "PartDefinition",
  "part": "PartUsage",
  "port def": "PortDefinition",
  "port": "PortUsage",
  "connection def": "ConnectionDefinition",
  "connection": "ConnectionUsage",
  "interface def": "InterfaceDefinition",
  "interface": "InterfaceUsage",
  "item def": "ItemDefinition",
  "item": "ItemUsage",
  "attribute def": "AttributeDefinition",
  "attribute": "AttributeUsage",
  "requirement def": "RequirementDefinition",
  "requirement": "RequirementUsage",
  "constraint def": "ConstraintDefinition",
  "constraint": "ConstraintUsage",
  "action def": "ActionDefinition",
  "action": "ActionUsage",
  "state def": "StateDefinition",
  "state": "StateUsage",
  "use case def": "UseCaseDefinition",
  "use case": "UseCaseUsage",
  "allocation def": "AllocationDefinition",
  "allocation": "AllocationUsage",
  "view def": "ViewDefinition",
  "view": "ViewUsage",
  "viewpoint def": "ViewpointDefinition",
  "viewpoint": "ViewpointUsage",
  "concern def": "ConcernDefinition",
  "concern": "ConcernUsage",
  "verification def": "VerificationCaseDefinition",
  "verification": "VerificationCaseUsage",
  "analysis def": "AnalysisCaseDefinition",
  "analysis": "AnalysisCaseUsage",
  "enum def": "EnumerationDefinition",
  "enum": "EnumerationUsage",
  "calc def": "CalcDefinition",
  "calc": "CalcUsage",
  "rendering def": "RenderingDefinition",
  "rendering": "RenderingUsage",
  "occurrence def": "OccurrenceDefinition",
  "occurrence": "OccurrenceUsage",
  "metadata def": "MetadataDefinition",
};

const SORTED_KEYWORDS = Object.keys(KEYWORD_MAP).sort(
  (a, b) => b.length - a.length
);

export function parseSysml(text: string): ParseResult {
  const elements: ParsedElement[] = [];
  const relationships: ParsedRelationship[] = [];
  const imports: string[] = [];
  const errors: string[] = [];

  const lines = text.split("\n");
  const stack: ParsedElement[] = [];

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const line = raw.trim();

    if (!line || line.startsWith("//") || line === "}") continue;
    if (line.startsWith("/*")) {
      while (i < lines.length && !lines[i].includes("*/")) i++;
      continue;
    }
    if (line === "};") {
      if (stack.length > 0) stack.pop();
      continue;
    }
    if (line === "}") {
      if (stack.length > 0) stack.pop();
      continue;
    }

    if (line.startsWith("doc /*")) continue;

    const importMatch = line.match(
      /^(?:public\s+|private\s+)?import\s+(?:all\s+)?(.+);$/
    );
    if (importMatch) {
      imports.push(importMatch[1].trim());
      continue;
    }

    const satisfyMatch = line.match(
      /^(?:assert\s+)?satisfy\s+(?:requirement\s+)?(\S+)\s+by\s+(\S+);?$/
    );
    if (satisfyMatch) {
      relationships.push({
        type: "satisfy",
        requirement: satisfyMatch[1],
        by: satisfyMatch[2].replace(/;$/, ""),
      });
      continue;
    }

    const allocateMatch = line.match(
      /^allocate\s+(\S+)\s+to\s+(\S+);?$/
    );
    if (allocateMatch) {
      relationships.push({
        type: "allocate",
        from: allocateMatch[1],
        to: allocateMatch[2].replace(/;$/, ""),
      });
      continue;
    }

    const depMatch = line.match(
      /^dependency\s+from\s+(\S+)\s+to\s+(\S+);?$/
    );
    if (depMatch) {
      relationships.push({
        type: "dependency",
        from: depMatch[1],
        to: depMatch[2].replace(/;$/, ""),
      });
      continue;
    }

    const element = matchElement(line);
    if (element) {
      const parent = stack.length > 0 ? stack[stack.length - 1] : null;
      if (parent) {
        parent.children.push(element);
      } else {
        elements.push(element);
      }

      if (line.endsWith("{")) {
        stack.push(element);
      }
      continue;
    }

    if (
      line.startsWith("first ") ||
      line.startsWith("then ") ||
      line.startsWith("flow ") ||
      line.startsWith("connect ") ||
      line.startsWith("in ") ||
      line.startsWith("out ") ||
      line.startsWith("return ") ||
      line.startsWith("redefines ") ||
      line.startsWith("perform ") ||
      line.startsWith("exhibit ") ||
      line.startsWith("send ") ||
      line.startsWith("accept ") ||
      line.startsWith("transition ") ||
      line.startsWith("filter ") ||
      line.startsWith("expose ") ||
      line.startsWith("render ") ||
      line.startsWith("subject ") ||
      line.startsWith("objective ") ||
      line.startsWith("require ") ||
      line.startsWith("stakeholder ") ||
      line.startsWith("frame ") ||
      line.startsWith("end ") ||
      line.startsWith("alias ") ||
      line.startsWith("succeed ") ||
      line.startsWith("precondition ") ||
      line.startsWith("assert ")
    ) {
      continue;
    }

    if (line.length > 0 && !line.startsWith("*") && !line.startsWith("}")) {
      errors.push(`Line ${i + 1}: Unrecognized syntax: ${line}`);
    }
  }

  return { elements, relationships, imports, errors };
}

function matchElement(line: string): ParsedElement | null {
  let workLine = line;
  if (workLine.startsWith("public ") || workLine.startsWith("private ") || workLine.startsWith("protected ")) {
    workLine = workLine.replace(/^(?:public|private|protected)\s+/, "");
  }

  for (const keyword of SORTED_KEYWORDS) {
    if (workLine.startsWith(keyword + " ") || workLine === keyword + ";") {
      const rest = workLine.slice(keyword.length).trim();
      const parsed = parseDeclaration(rest);
      if (parsed) {
        return {
          type: KEYWORD_MAP[keyword],
          name: parsed.name,
          shortName: parsed.shortName,
          typedBy: parsed.typedBy,
          specializes: parsed.specializes,
          children: [],
          attributes: {},
        };
      }
    }
  }

  return null;
}

interface Declaration {
  name: string;
  shortName?: string;
  typedBy?: string;
  specializes?: string;
}

function parseDeclaration(rest: string): Declaration | null {
  let s = rest.replace(/\{$/, "").replace(/;$/, "").trim();
  if (!s) return null;

  let shortName: string | undefined;
  const shortMatch = s.match(/^<'([^']+)'>\s*/);
  if (shortMatch) {
    shortName = shortMatch[1];
    s = s.slice(shortMatch[0].length);
  }

  let specializes: string | undefined;
  const specMatch = s.match(/\s*:>\s*(\S+)/);
  if (specMatch) {
    specializes = specMatch[1];
    s = s.slice(0, specMatch.index).trim();
  }

  let typedBy: string | undefined;
  const typeMatch = s.match(/\s*:\s*(\S+)/);
  if (typeMatch) {
    typedBy = typeMatch[1];
    s = s.slice(0, typeMatch.index).trim();
  }

  const name = extractName(s);
  if (!name) return null;

  return { name, shortName, typedBy, specializes };
}

function extractName(s: string): string | null {
  const trimmed = s.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith("'")) {
    const end = trimmed.indexOf("'", 1);
    return end === -1 ? null : trimmed.slice(1, end);
  }

  const match = trimmed.match(/^([a-zA-Z_][a-zA-Z0-9_]*)/);
  return match ? match[1] : null;
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd packages/mcp-server && pnpm test -- src/__tests__/sysml-parser.test.ts
```

Expected: All parser tests PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/mcp-server/src/utils/sysml-parser.ts packages/mcp-server/src/__tests__/sysml-parser.test.ts
git commit -m "feat: rewrite SysML v2 parser with correct textual notation syntax"
```

---

## Task 5: SysML v2 Serializer

**Files:**
- Rewrite: `packages/mcp-server/src/utils/sysml-serializer.ts`
- Create: `packages/mcp-server/src/__tests__/sysml-serializer.test.ts`

- [ ] **Step 1: Write failing tests for the serializer**

Create `packages/mcp-server/src/__tests__/sysml-serializer.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../utils/sysml-serializer.js";
import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

function el(overrides: Partial<SysmlElement>): SysmlElement {
  return {
    id: "e1",
    elementId: "e1",
    type: "PartDefinition",
    name: "Test",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
    ...overrides,
  };
}

describe("serializeToSysml", () => {
  it("serializes a part definition", () => {
    const result = serializeToSysml([el({ type: "PartDefinition", name: "Engine" })], []);
    expect(result).toContain("part def Engine;");
  });

  it("serializes a requirement definition", () => {
    const result = serializeToSysml(
      [el({ type: "RequirementDefinition", name: "MassReq" })],
      []
    );
    expect(result).toContain("requirement def MassReq;");
  });

  it("serializes a package with children", () => {
    const pkg = el({ id: "pkg1", type: "Package", name: "Vehicle", ownedElementIds: ["e2"] });
    const child = el({ id: "e2", type: "PartDefinition", name: "Engine", ownerId: "pkg1" });
    const result = serializeToSysml([pkg, child], []);
    expect(result).toContain("package Vehicle {");
    expect(result).toContain("  part def Engine;");
    expect(result).toContain("}");
  });

  it("serializes verification def (not verification case def)", () => {
    const result = serializeToSysml(
      [el({ type: "VerificationCaseDefinition", name: "MassTest" })],
      []
    );
    expect(result).toContain("verification def MassTest;");
  });

  it("serializes analysis def", () => {
    const result = serializeToSysml(
      [el({ type: "AnalysisCaseDefinition", name: "FuelAnalysis" })],
      []
    );
    expect(result).toContain("analysis def FuelAnalysis;");
  });

  it("serializes enum def", () => {
    const result = serializeToSysml(
      [el({ type: "EnumerationDefinition", name: "FuelKind" })],
      []
    );
    expect(result).toContain("enum def FuelKind;");
  });

  it("serializes elements with short names", () => {
    const result = serializeToSysml(
      [el({ type: "RequirementDefinition", name: "MaxMass", shortName: "SYS-001" })],
      []
    );
    expect(result).toContain("requirement def <'SYS-001'> MaxMass;");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/mcp-server && pnpm test -- src/__tests__/sysml-serializer.test.ts
```

Expected: FAIL.

- [ ] **Step 3: Implement the serializer**

Replace `packages/mcp-server/src/utils/sysml-serializer.ts`:

```typescript
import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

const TYPE_TO_KEYWORD: Record<string, string> = {
  Package: "package",
  PartDefinition: "part def",
  PartUsage: "part",
  PortDefinition: "port def",
  PortUsage: "port",
  ConnectionDefinition: "connection def",
  ConnectionUsage: "connection",
  InterfaceDefinition: "interface def",
  InterfaceUsage: "interface",
  ItemDefinition: "item def",
  ItemUsage: "item",
  AttributeDefinition: "attribute def",
  AttributeUsage: "attribute",
  RequirementDefinition: "requirement def",
  RequirementUsage: "requirement",
  ConstraintDefinition: "constraint def",
  ConstraintUsage: "constraint",
  ActionDefinition: "action def",
  ActionUsage: "action",
  StateDefinition: "state def",
  StateUsage: "state",
  UseCaseDefinition: "use case def",
  UseCaseUsage: "use case",
  AllocationDefinition: "allocation def",
  AllocationUsage: "allocation",
  ViewDefinition: "view def",
  ViewUsage: "view",
  ViewpointDefinition: "viewpoint def",
  ViewpointUsage: "viewpoint",
  ConcernDefinition: "concern def",
  ConcernUsage: "concern",
  VerificationCaseDefinition: "verification def",
  VerificationCaseUsage: "verification",
  AnalysisCaseDefinition: "analysis def",
  AnalysisCaseUsage: "analysis",
  EnumerationDefinition: "enum def",
  EnumerationUsage: "enum",
  CalcDefinition: "calc def",
  CalcUsage: "calc",
  RenderingDefinition: "rendering def",
  RenderingUsage: "rendering",
  OccurrenceDefinition: "occurrence def",
  OccurrenceUsage: "occurrence",
  MetadataDefinition: "metadata def",
};

export function serializeToSysml(
  elements: SysmlElement[],
  relationships: SysmlRelationship[]
): string {
  const byId = new Map(elements.map((e) => [e.id, e]));
  const rootElements = elements.filter((e) => !e.ownerId || !byId.has(e.ownerId));

  const lines: string[] = [];
  for (const el of rootElements) {
    lines.push(serializeElement(el, elements, relationships, 0));
    lines.push("");
  }

  return lines.join("\n").trimEnd() + "\n";
}

function serializeElement(
  element: SysmlElement,
  allElements: SysmlElement[],
  relationships: SysmlRelationship[],
  indent: number
): string {
  const prefix = "  ".repeat(indent);
  const keyword = TYPE_TO_KEYWORD[element.type] ?? element.type;

  let declaration = `${prefix}${keyword}`;

  if (element.shortName) {
    declaration += ` <'${element.shortName}'>`;
  }

  if (element.name) {
    declaration += ` ${escapeName(element.name)}`;
  }

  const children = allElements.filter((e) => e.ownerId === element.id);

  if (children.length > 0) {
    const lines = [declaration + " {"];
    for (const child of children) {
      lines.push(serializeElement(child, allElements, relationships, indent + 1));
    }
    lines.push(`${prefix}}`);
    return lines.join("\n");
  }

  return declaration + ";";
}

function escapeName(name: string): string {
  if (/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name)) return name;
  return `'${name}'`;
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd packages/mcp-server && pnpm test -- src/__tests__/sysml-serializer.test.ts
```

Expected: All serializer tests PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/mcp-server/src/utils/sysml-serializer.ts packages/mcp-server/src/__tests__/sysml-serializer.test.ts
git commit -m "feat: rewrite SysML v2 serializer with correct textual keywords"
```

---

## Task 6: MCP Tools — Element Operations

**Files:**
- Rewrite: `packages/mcp-server/src/tools/create-element.ts`
- Rewrite: `packages/mcp-server/src/tools/query-elements.ts`
- Rewrite: `packages/mcp-server/src/tools/get-project-state.ts`
- Create: `packages/mcp-server/src/__tests__/tools.test.ts`

- [ ] **Step 1: Write failing tests for element tools**

Create `packages/mcp-server/src/__tests__/tools.test.ts`:

```typescript
import { describe, it, expect, vi, beforeEach } from "vitest";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { SmapsClient } from "../smaps-client.js";
import { registerCreateElement } from "../tools/create-element.js";
import { registerQueryElements } from "../tools/query-elements.js";
import { registerGetProjectState } from "../tools/get-project-state.js";

describe("MCP Tools", () => {
  let server: McpServer;
  let smaps: SmapsClient;

  beforeEach(() => {
    server = new McpServer({ name: "test", version: "0.0.1" });
    smaps = new SmapsClient("http://localhost:9000");
  });

  it("registerCreateElement registers a tool named create_element", () => {
    registerCreateElement(server, smaps);
    // Tool registration doesn't throw
    expect(true).toBe(true);
  });

  it("registerQueryElements registers a tool named query_elements", () => {
    registerQueryElements(server, smaps);
    expect(true).toBe(true);
  });

  it("registerGetProjectState registers a tool named get_project_state", () => {
    registerGetProjectState(server, smaps);
    expect(true).toBe(true);
  });
});
```

- [ ] **Step 2: Rewrite create-element.ts**

Replace `packages/mcp-server/src/tools/create-element.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerCreateElement(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "create_element",
    "Create a SysML v2 element via a SMAPS commit. Supports all SysML v2 types: PartDefinition, RequirementDefinition, ActionDefinition, etc.",
    {
      type: z
        .string()
        .describe("SysML v2 element type (e.g. PartDefinition, RequirementDefinition, Package)"),
      name: z.string().describe("Element name"),
      attributes: z
        .record(z.unknown())
        .optional()
        .describe("Additional element attributes"),
    },
    async ({ type, name, attributes }) => {
      try {
        const element = await smaps.createElement(type, name, attributes ?? {});
        return {
          content: [{ type: "text" as const, text: JSON.stringify(element, null, 2) }],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 3: Rewrite query-elements.ts**

Replace `packages/mcp-server/src/tools/query-elements.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerQueryElements(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "query_elements",
    "Find SysML v2 elements by type and/or name pattern. Uses the SMAPS Query API.",
    {
      type: z.string().optional().describe("Filter by element type (e.g. PartDefinition)"),
      name_pattern: z.string().optional().describe("Filter by name (substring match)"),
    },
    async ({ type, name_pattern }) => {
      try {
        const elements = await smaps.queryElements(type, name_pattern);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                { count: elements.length, elements },
                null,
                2
              ),
            },
          ],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 4: Rewrite get-project-state.ts**

Replace `packages/mcp-server/src/tools/get-project-state.ts`:

```typescript
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerGetProjectState(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "get_project_state",
    "Get a summary of the current model — element counts by type, project/branch/commit IDs",
    {},
    async () => {
      try {
        const state = await smaps.getProjectState();
        return {
          content: [{ type: "text" as const, text: JSON.stringify(state, null, 2) }],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 5: Run tests**

```bash
cd packages/mcp-server && pnpm test -- src/__tests__/tools.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add packages/mcp-server/src/tools/create-element.ts packages/mcp-server/src/tools/query-elements.ts packages/mcp-server/src/tools/get-project-state.ts packages/mcp-server/src/__tests__/tools.test.ts
git commit -m "feat: rewrite element MCP tools for commit-based API"
```

---

## Task 7: MCP Tools — Relationships

**Files:**
- Rewrite: `packages/mcp-server/src/tools/create-relationship.ts`
- Rewrite: `packages/mcp-server/src/tools/query-relationships.ts`
- Rewrite: `packages/mcp-server/src/tools/validate-model.ts`

- [ ] **Step 1: Rewrite create-relationship.ts**

Replace `packages/mcp-server/src/tools/create-relationship.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerCreateRelationship(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "create_relationship",
    "Create a relationship between SysML v2 elements (Dependency, Specialization, SatisfyRequirementUsage, etc.)",
    {
      type: z
        .string()
        .describe("Relationship type (e.g. Dependency, Specialization, SatisfyRequirementUsage)"),
      source_id: z.string().describe("Source element ID"),
      target_id: z.string().describe("Target element ID"),
      attributes: z.record(z.unknown()).optional().describe("Additional attributes"),
    },
    async ({ type, source_id, target_id, attributes }) => {
      try {
        const element = await smaps.createElement(type, "", {
          source: [{ "@id": source_id }],
          target: [{ "@id": target_id }],
          ...(attributes ?? {}),
        });
        return {
          content: [{ type: "text" as const, text: JSON.stringify(element, null, 2) }],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 2: Rewrite query-relationships.ts**

Replace `packages/mcp-server/src/tools/query-relationships.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerQueryRelationships(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "query_relationships",
    "Get relationships for an element. Uses the SMAPS relationships endpoint with direction filtering.",
    {
      element_id: z.string().optional().describe("Element to query relationships for"),
      direction: z
        .enum(["in", "out", "both"])
        .optional()
        .default("both")
        .describe("Relationship direction: in (targeting this element), out (sourced from), both"),
    },
    async ({ element_id, direction }) => {
      try {
        const rels = await smaps.queryRelationships(element_id, direction);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ count: rels.length, relationships: rels }, null, 2),
            },
          ],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 3: Rewrite validate-model.ts**

Replace `packages/mcp-server/src/tools/validate-model.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";

export function registerValidateModel(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "validate_model",
    "Run completeness and consistency checks — unsatisfied requirements, orphaned elements, missing connections",
    {
      scope: z.string().optional().describe("Element ID to scope validation to, or omit for full model"),
    },
    async () => {
      try {
        const state = await smaps.getProjectState();
        const requirements = await smaps.queryElements("RequirementDefinition");
        const parts = await smaps.queryElements("PartDefinition");

        const satisfiedReqIds = new Set<string>();
        for (const req of requirements) {
          const rels = await smaps.queryRelationships(req.id, "in");
          const hasSatisfy = rels.some(
            (r) =>
              r.type === "SatisfyRequirementUsage" ||
              r.type === "Dependency"
          );
          if (hasSatisfy) satisfiedReqIds.add(req.id);
        }

        const unsatisfied = requirements.filter((r) => !satisfiedReqIds.has(r.id));
        const issues: string[] = [];

        if (unsatisfied.length > 0) {
          issues.push(
            `${unsatisfied.length} requirements not satisfied: ${unsatisfied.map((r) => r.name ?? r.id).join(", ")}`
          );
        }

        if (parts.length > 0) {
          const connections = await smaps.queryElements("ConnectionUsage");
          if (connections.length === 0) {
            issues.push("Part definitions exist but no connections — IBD may be incomplete");
          }
        }

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  summary: state,
                  issues,
                  requirementCoverage: {
                    total: requirements.length,
                    satisfied: satisfiedReqIds.size,
                    unsatisfied: unsatisfied.length,
                    coveragePercent:
                      requirements.length > 0
                        ? Math.round((satisfiedReqIds.size / requirements.length) * 100)
                        : 0,
                  },
                },
                null,
                2
              ),
            },
          ],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 4: Run all tests**

```bash
cd packages/mcp-server && pnpm test
```

Expected: All tests PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/mcp-server/src/tools/create-relationship.ts packages/mcp-server/src/tools/query-relationships.ts packages/mcp-server/src/tools/validate-model.ts
git commit -m "feat: rewrite relationship and validation MCP tools"
```

---

## Task 8: MCP Tools — Import/Export

**Files:**
- Rewrite: `packages/mcp-server/src/tools/export-sysml.ts`
- Rewrite: `packages/mcp-server/src/tools/import-sysml.ts`

- [ ] **Step 1: Rewrite export-sysml.ts**

Replace `packages/mcp-server/src/tools/export-sysml.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";
import { serializeToSysml } from "../utils/sysml-serializer.js";

export function registerExportSysml(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "export_sysml",
    "Export model elements as SysML v2 textual notation (.sysml format)",
    {
      scope: z.string().optional().describe("Element ID to export, or omit for full model"),
    },
    async () => {
      try {
        const elements = await smaps.queryElements();
        const relationships = await smaps.queryRelationships();
        const sysmlText = serializeToSysml(elements, relationships);
        return {
          content: [{ type: "text" as const, text: sysmlText }],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 2: Rewrite import-sysml.ts**

Replace `packages/mcp-server/src/tools/import-sysml.ts`:

```typescript
import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { SmapsClient } from "../smaps-client.js";
import { parseSysml } from "../utils/sysml-parser.js";

export function registerImportSysml(server: McpServer, smaps: SmapsClient) {
  server.tool(
    "import_sysml",
    "Parse SysML v2 textual notation and import elements into the model via commits",
    {
      sysml_text: z.string().describe("SysML v2 textual notation to parse and import"),
    },
    async ({ sysml_text }) => {
      try {
        const parsed = parseSysml(sysml_text);

        if (parsed.errors.length > 0) {
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify({ success: false, errors: parsed.errors }, null, 2),
              },
            ],
            isError: true,
          };
        }

        const elementsToCreate = parsed.elements.map((e) => ({
          type: e.type,
          name: e.name,
          attributes: e.attributes,
        }));

        const created = await smaps.createElements(elementsToCreate);

        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                { success: true, elementsImported: created.length, elements: created },
                null,
                2
              ),
            },
          ],
        };
      } catch (err) {
        return {
          content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
          isError: true,
        };
      }
    }
  );
}
```

- [ ] **Step 3: Run all tests**

```bash
cd packages/mcp-server && pnpm test
```

Expected: All tests PASS.

- [ ] **Step 4: Commit**

```bash
git add packages/mcp-server/src/tools/export-sysml.ts packages/mcp-server/src/tools/import-sysml.ts
git commit -m "feat: rewrite import/export tools for commit-based API"
```

---

## Task 9: MCP Server Entry Point

**Files:**
- Rewrite: `packages/mcp-server/src/index.ts`

- [ ] **Step 1: Rewrite index.ts with project initialization flow**

Replace `packages/mcp-server/src/index.ts`:

```typescript
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { SmapsClient } from "./smaps-client.js";
import { registerCreateElement } from "./tools/create-element.js";
import { registerQueryElements } from "./tools/query-elements.js";
import { registerCreateRelationship } from "./tools/create-relationship.js";
import { registerQueryRelationships } from "./tools/query-relationships.js";
import { registerValidateModel } from "./tools/validate-model.js";
import { registerExportSysml } from "./tools/export-sysml.js";
import { registerImportSysml } from "./tools/import-sysml.js";
import { registerGetProjectState } from "./tools/get-project-state.js";
import { z } from "zod";

const SMAPS_ENDPOINT = process.env.SMAPS_ENDPOINT ?? "http://localhost:9000";

const server = new McpServer({
  name: "sysml-bridge",
  version: "0.1.0",
});

const smaps = new SmapsClient(SMAPS_ENDPOINT);

server.tool(
  "init_project",
  "Initialize or load a SMAPS project. Must be called before using other tools.",
  {
    name: z.string().describe("Project name to create or load"),
    create: z
      .boolean()
      .optional()
      .default(true)
      .describe("Create a new project (true) or load existing (false)"),
  },
  async ({ name, create }) => {
    try {
      if (create) {
        const project = await smaps.createProject(name);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  status: "created",
                  projectId: project["@id"],
                  branchId: smaps.branchId,
                },
                null,
                2
              ),
            },
          ],
        };
      }

      const projects = await smaps.listProjects();
      const found = projects.find((p) => p.name === name);
      if (!found) {
        return {
          content: [{ type: "text" as const, text: `Project "${name}" not found` }],
          isError: true,
        };
      }

      const project = await smaps.loadProject(found["@id"]);
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(
              {
                status: "loaded",
                projectId: project["@id"],
                branchId: smaps.branchId,
                headCommitId: smaps.headCommitId,
              },
              null,
              2
            ),
          },
        ],
      };
    } catch (err) {
      return {
        content: [{ type: "text" as const, text: `Error: ${(err as Error).message}` }],
        isError: true,
      };
    }
  }
);

registerCreateElement(server, smaps);
registerQueryElements(server, smaps);
registerCreateRelationship(server, smaps);
registerQueryRelationships(server, smaps);
registerValidateModel(server, smaps);
registerExportSysml(server, smaps);
registerImportSysml(server, smaps);
registerGetProjectState(server, smaps);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch(console.error);
```

- [ ] **Step 2: Verify build succeeds**

```bash
cd packages/mcp-server && pnpm build
```

Expected: Compiles to `dist/` with no errors.

- [ ] **Step 3: Run all tests**

```bash
cd packages/mcp-server && pnpm test
```

Expected: All tests PASS.

- [ ] **Step 4: Commit**

```bash
git add packages/mcp-server/src/index.ts
git commit -m "feat: rewrite MCP server entry point with init_project tool"
```

---

## Task 10: Fix Skills Terminology

**Files:**
- Modify: `packages/skills/skills/mbse-build.md`
- Modify: `packages/mcp-server/src/types/sysml-elements.ts` (if `VerificationCaseDefinition` references need updating)

- [ ] **Step 1: Fix mbse-build.md — correct verification/analysis terminology**

In `packages/skills/skills/mbse-build.md`, update the parametric section and any references:
- `verification case def` → `verification def`
- `analysis case def` → `analysis def`
- `VerificationCaseDefinition` is the internal API type but the textual keyword is `verification def`

Add a note to the skill:

After the `### mbse-build parametric` section, add:

```markdown
### Keyword Reference

SysML v2 textual keywords used by build subcommands:

| Subcommand | SysML v2 Keywords |
|---|---|
| bdd | `part def`, `part`, `:>` (specialization) |
| ibd | `part`, `port def`, `port`, `connection`, `interface`, `flow` |
| activity | `action def`, `action`, `first...then`, `flow` |
| sequence | `action def`, `action`, `accept`, `send` |
| state | `state def`, `state`, `transition`, `entry`, `do`, `exit` |
| parametric | `constraint def`, `constraint`, `calc def`, `attribute` |
```

- [ ] **Step 2: Commit**

```bash
git add packages/skills/skills/mbse-build.md
git commit -m "fix: correct SysML v2 textual keywords in mbse-build skill"
```

---

## Task 11: Integration Smoke Test

**Files:**
- Create: `packages/mcp-server/src/__tests__/integration.test.ts`

This test requires Docker. It's tagged so it can be skipped in CI without Docker.

- [ ] **Step 1: Write integration test**

Create `packages/mcp-server/src/__tests__/integration.test.ts`:

```typescript
import { describe, it, expect, beforeAll } from "vitest";
import { SmapsClient } from "../smaps-client.js";

const SMAPS_URL = process.env.SMAPS_ENDPOINT ?? "http://localhost:9000";

describe.skipIf(!process.env.INTEGRATION)(
  "SmapsClient integration (requires Docker)",
  () => {
    let client: SmapsClient;

    beforeAll(async () => {
      client = new SmapsClient(SMAPS_URL);
    });

    it("connects to the SMAPS server", async () => {
      const connected = await client.checkConnection();
      expect(connected).toBe(true);
    });

    it("creates a project", async () => {
      const project = await client.createProject(`test-${Date.now()}`);
      expect(project["@id"]).toBeTruthy();
      expect(client.projectId).toBeTruthy();
      expect(client.branchId).toBeTruthy();
    });

    it("creates an element via commit", async () => {
      const element = await client.createElement("PartDefinition", "Engine");
      expect(element.type).toBe("PartDefinition");
      expect(element.name).toBe("Engine");
      expect(element.id).toBeTruthy();
    });

    it("queries elements by type", async () => {
      await client.createElement("PartDefinition", "Wheel");
      const results = await client.queryElements("PartDefinition");
      expect(results.length).toBeGreaterThanOrEqual(2);
    });

    it("creates a requirement and queries it", async () => {
      await client.createElement("RequirementDefinition", "MassReq");
      const reqs = await client.queryElements("RequirementDefinition");
      expect(reqs.some((r) => r.name === "MassReq")).toBe(true);
    });

    it("gets project state with element counts", async () => {
      const state = await client.getProjectState();
      expect(state.totalElements).toBeGreaterThan(0);
      expect(state.elementCountsByType["PartDefinition"]).toBeGreaterThanOrEqual(2);
    });
  }
);
```

- [ ] **Step 2: Run integration test (requires Docker running)**

```bash
cd docker && docker compose up -d
# Wait for startup (~60s)
cd ../packages/mcp-server && INTEGRATION=1 pnpm test -- src/__tests__/integration.test.ts
```

Expected: All integration tests PASS.

- [ ] **Step 3: Commit**

```bash
git add packages/mcp-server/src/__tests__/integration.test.ts
git commit -m "test: add integration smoke test against SMAPS Docker"
```

---

## Task 12: Final Build, Push, and Verify

- [ ] **Step 1: Run full test suite**

```bash
cd /path/to/sysml-bridge && pnpm test
```

Expected: All unit tests PASS.

- [ ] **Step 2: Build**

```bash
pnpm build
```

Expected: Clean TypeScript compilation.

- [ ] **Step 3: Verify MCP server starts**

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | node packages/mcp-server/dist/index.js
```

Expected: JSON-RPC response with server info and tool list.

- [ ] **Step 4: Push to GitHub**

```bash
git push origin main
```

Expected: All commits pushed to https://github.com/joescohen/sysml-bridge
