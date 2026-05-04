import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { SmapsClient } from "../smaps-client.js";

// ---------------------------------------------------------------------------
// Fetch mock helpers
// ---------------------------------------------------------------------------

function mockFetch(responses: Array<{ ok: boolean; status?: number; body?: unknown }>) {
  const queue = [...responses];
  return vi.fn().mockImplementation(() => {
    const next = queue.shift();
    if (!next) throw new Error("Unexpected fetch call — queue empty");
    return Promise.resolve({
      ok: next.ok,
      status: next.status ?? (next.ok ? 200 : 500),
      statusText: next.ok ? "OK" : "Internal Server Error",
      json: () => Promise.resolve(next.body ?? {}),
    });
  });
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const ENDPOINT = "http://localhost:9000/sysml-v2";

const FAKE_PROJECT = {
  "@id": "proj-uuid-1",
  "@type": "Project",
  name: "TestProject",
  defaultBranch: { "@id": "branch-uuid-1" },
};

const FAKE_BRANCH = {
  "@id": "branch-uuid-1",
  "@type": "Branch",
  name: "main",
  head: { "@id": "commit-uuid-0" },
  owningProject: { "@id": "proj-uuid-1" },
  created: "2024-01-01T00:00:00Z",
};

const FAKE_COMMIT_RESPONSE = {
  "@id": "commit-uuid-1",
  "@type": "Commit",
  created: "2024-01-02T00:00:00Z",
  owningProject: { "@id": "proj-uuid-1" },
  change: [
    {
      "@type": "DataVersion",
      payload: {
        "@id": "elem-uuid-1",
        "@type": "PartDefinition",
        name: "Engine",
        declaredName: "Engine",
        declaredShortName: null,
        qualifiedName: "Engine",
        owner: { "@id": "proj-uuid-1" },
        ownedElement: [],
      },
    },
  ],
};

const FAKE_ELEMENT_RESPONSE = {
  "@id": "elem-uuid-1",
  "@type": "PartDefinition",
  name: "Engine",
  declaredName: "Engine",
  declaredShortName: null,
  qualifiedName: "Engine",
  owner: { "@id": "proj-uuid-1" },
  ownedElement: [],
};

const FAKE_RELATIONSHIP_RESPONSE = {
  "@id": "rel-uuid-1",
  "@type": "FeatureTyping",
  source: [{ "@id": "elem-uuid-1" }],
  target: [{ "@id": "elem-uuid-2" }],
  relatedElement: [{ "@id": "elem-uuid-1" }, { "@id": "elem-uuid-2" }],
  owner: { "@id": "proj-uuid-1" },
  ownedElement: [],
  name: null,
  declaredName: null,
  declaredShortName: null,
  qualifiedName: null,
};

// ---------------------------------------------------------------------------
// Helpers to build an initialized SmapsClient
// ---------------------------------------------------------------------------

async function buildInitializedClient(fetchMock: ReturnType<typeof vi.fn>) {
  vi.stubGlobal("fetch", fetchMock);
  const client = new SmapsClient(ENDPOINT);
  await client.loadProject("proj-uuid-1");
  return client;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("SmapsClient", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  // -------------------------------------------------------------------------
  describe("checkConnection", () => {
    it("returns true when the server responds OK", async () => {
      vi.stubGlobal(
        "fetch",
        mockFetch([{ ok: true, body: [] }])
      );
      const client = new SmapsClient(ENDPOINT);
      expect(await client.checkConnection()).toBe(true);
    });

    it("returns false when the server responds with an error", async () => {
      vi.stubGlobal(
        "fetch",
        mockFetch([{ ok: false, status: 503 }])
      );
      const client = new SmapsClient(ENDPOINT);
      expect(await client.checkConnection()).toBe(false);
    });

    it("returns false when fetch throws (network error)", async () => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockRejectedValue(new Error("ECONNREFUSED"))
      );
      const client = new SmapsClient(ENDPOINT);
      expect(await client.checkConnection()).toBe(false);
    });
  });

  // -------------------------------------------------------------------------
  describe("createProject", () => {
    it("POSTs to /projects, fetches branch, and stores projectId/branchId/headCommitId", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },          // POST /projects
        { ok: true, body: FAKE_BRANCH },           // GET /projects/{id}/branches/{branchId}
      ]);
      vi.stubGlobal("fetch", fetch);

      const client = new SmapsClient(ENDPOINT);
      const project = await client.createProject("TestProject");

      // Verify the POST call
      const [url, init] = fetch.mock.calls[0] as [string, RequestInit];
      expect(url).toBe(`${ENDPOINT}/projects`);
      expect(init.method).toBe("POST");
      const body = JSON.parse(init.body as string);
      expect(body["@type"]).toBe("Project");
      expect(body.name).toBe("TestProject");

      // Verify stored state
      expect(client.projectId).toBe("proj-uuid-1");
      expect(client.branchId).toBe("branch-uuid-1");
      expect(client.headCommitId).toBe("commit-uuid-0");

      // Verify returned value
      expect(project["@id"]).toBe("proj-uuid-1");
      expect(project.name).toBe("TestProject");
    });
  });

  // -------------------------------------------------------------------------
  describe("loadProject", () => {
    it("GETs the project and its branch, then stores state", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },   // GET /projects/{id}
        { ok: true, body: FAKE_BRANCH },    // GET /projects/{id}/branches/{branchId}
      ]);
      vi.stubGlobal("fetch", fetch);

      const client = new SmapsClient(ENDPOINT);
      await client.loadProject("proj-uuid-1");

      expect(client.projectId).toBe("proj-uuid-1");
      expect(client.branchId).toBe("branch-uuid-1");
      expect(client.headCommitId).toBe("commit-uuid-0");
    });

    it("throws when the project is not found", async () => {
      vi.stubGlobal(
        "fetch",
        mockFetch([{ ok: false, status: 404 }])
      );
      const client = new SmapsClient(ENDPOINT);
      await expect(client.loadProject("bad-id")).rejects.toThrow();
    });
  });

  // -------------------------------------------------------------------------
  describe("listProjects", () => {
    it("GETs /projects and returns the array", async () => {
      const fetch = mockFetch([{ ok: true, body: [FAKE_PROJECT] }]);
      vi.stubGlobal("fetch", fetch);

      const client = new SmapsClient(ENDPOINT);
      const projects = await client.listProjects();

      expect(projects).toHaveLength(1);
      expect(projects[0]["@id"]).toBe("proj-uuid-1");

      const [url] = fetch.mock.calls[0] as [string];
      expect(url).toBe(`${ENDPOINT}/projects`);
    });
  });

  // -------------------------------------------------------------------------
  describe("createElement", () => {
    it("POSTs a commit with correct body structure", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },         // loadProject GET
        { ok: true, body: FAKE_BRANCH },          // loadProject branch GET
        { ok: true, body: FAKE_COMMIT_RESPONSE }, // createElement POST
        { ok: true, body: FAKE_COMMIT_RESPONSE.change }, // fetchCommitChanges GET
      ]);
      const client = await buildInitializedClient(fetch);

      const el = await client.createElement("PartDefinition", "Engine");

      // Verify commit POST
      const commitCall = fetch.mock.calls[2] as [string, RequestInit];
      expect(commitCall[0]).toBe(`${ENDPOINT}/projects/proj-uuid-1/commits?branchId=branch-uuid-1`);
      expect(commitCall[1].method).toBe("POST");

      const body = JSON.parse(commitCall[1].body as string);
      expect(body["@type"]).toBe("Commit");
      expect(body.previousCommit["@id"]).toBe("commit-uuid-0");
      expect(body.change).toHaveLength(1);
      expect(body.change[0]["@type"]).toBe("DataVersion");
      expect(body.change[0].payload["@type"]).toBe("PartDefinition");
      expect(body.change[0].payload.name).toBe("Engine");

      // headCommitId updated
      expect(client.headCommitId).toBe("commit-uuid-1");

      // Returned element shape
      expect(el.id).toBe("elem-uuid-1");
      expect(el.type).toBe("PartDefinition");
      expect(el.name).toBe("Engine");
    });

    it("includes extra attributes in the payload", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: FAKE_COMMIT_RESPONSE },
        { ok: true, body: FAKE_COMMIT_RESPONSE.change },
      ]);
      const client = await buildInitializedClient(fetch);

      await client.createElement("PartDefinition", "Engine", { isAbstract: true });

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body.change[0].payload.isAbstract).toBe(true);
    });

    it("throws if project not loaded", async () => {
      vi.stubGlobal("fetch", vi.fn());
      const client = new SmapsClient(ENDPOINT);
      await expect(client.createElement("PartDefinition", "X")).rejects.toThrow(/not initialized/i);
    });
  });

  // -------------------------------------------------------------------------
  describe("createElements (batch)", () => {
    it("sends a single commit with multiple DataVersion changes", async () => {
      const batchCommitResponse = {
        "@id": "commit-uuid-2",
        "@type": "Commit",
        created: "2024-01-02T00:00:00Z",
        owningProject: { "@id": "proj-uuid-1" },
        change: [
          {
            "@type": "DataVersion",
            payload: {
              "@id": "elem-uuid-A",
              "@type": "PartDefinition",
              name: "Engine",
              declaredName: "Engine",
              declaredShortName: null,
              qualifiedName: "Engine",
              owner: { "@id": "proj-uuid-1" },
              ownedElement: [],
            },
          },
          {
            "@type": "DataVersion",
            payload: {
              "@id": "elem-uuid-B",
              "@type": "PortDefinition",
              name: "FuelPort",
              declaredName: "FuelPort",
              declaredShortName: null,
              qualifiedName: "FuelPort",
              owner: { "@id": "proj-uuid-1" },
              ownedElement: [],
            },
          },
        ],
      };

      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: batchCommitResponse },
        { ok: true, body: batchCommitResponse.change },
      ]);
      const client = await buildInitializedClient(fetch);

      const elements = await client.createElements([
        { type: "PartDefinition", name: "Engine" },
        { type: "PortDefinition", name: "FuelPort" },
      ]);

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body.change).toHaveLength(2);
      expect(elements).toHaveLength(2);
      expect(client.headCommitId).toBe("commit-uuid-2");
    });
  });

  // -------------------------------------------------------------------------
  describe("updateElement", () => {
    it("sends a commit with identity set to existing elementId", async () => {
      const updateCommitResponse = {
        "@id": "commit-uuid-3",
        "@type": "Commit",
        created: "2024-01-03T00:00:00Z",
        owningProject: { "@id": "proj-uuid-1" },
        change: [
          {
            "@type": "DataVersion",
            identity: { "@id": "elem-uuid-1" },
            payload: {
              "@id": "elem-uuid-1",
              "@type": "PartDefinition",
              name: "Engine v2",
              declaredName: "Engine v2",
              declaredShortName: null,
              qualifiedName: "Engine v2",
              owner: { "@id": "proj-uuid-1" },
              ownedElement: [],
            },
          },
        ],
      };

      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: FAKE_ELEMENT_RESPONSE },          // getElement
        { ok: true, body: updateCommitResponse },            // updateElement commit
        { ok: true, body: updateCommitResponse.change },    // fetchCommitChanges
      ]);
      const client = await buildInitializedClient(fetch);

      const updated = await client.updateElement("elem-uuid-1", { name: "Engine v2" });

      const body = JSON.parse((fetch.mock.calls[3] as [string, RequestInit])[1].body as string);
      expect(body["@type"]).toBe("Commit");
      expect(body.change[0]["@type"]).toBe("DataVersion");
      expect(body.change[0].identity["@id"]).toBe("elem-uuid-1");
      expect(body.change[0].payload.name).toBe("Engine v2");

      expect(updated.id).toBe("elem-uuid-1");
      expect(client.headCommitId).toBe("commit-uuid-3");
    });
  });

  // -------------------------------------------------------------------------
  describe("deleteElement", () => {
    it("sends a commit with null payload and identity pointing to element", async () => {
      const deleteCommitResponse = {
        "@id": "commit-uuid-4",
        "@type": "Commit",
        created: "2024-01-04T00:00:00Z",
        owningProject: { "@id": "proj-uuid-1" },
        change: [
          {
            "@type": "DataVersion",
            identity: { "@id": "elem-uuid-1" },
            payload: null,
          },
        ],
      };

      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: deleteCommitResponse },
      ]);
      const client = await buildInitializedClient(fetch);

      await client.deleteElement("elem-uuid-1");

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body["@type"]).toBe("Commit");
      expect(body.change[0]["@type"]).toBe("DataVersion");
      expect(body.change[0].identity["@id"]).toBe("elem-uuid-1");
      expect(body.change[0].payload).toBeNull();

      expect(client.headCommitId).toBe("commit-uuid-4");
    });

    it("throws if project not loaded", async () => {
      vi.stubGlobal("fetch", vi.fn());
      const client = new SmapsClient(ENDPOINT);
      await expect(client.deleteElement("some-id")).rejects.toThrow(/not initialized/i);
    });
  });

  // -------------------------------------------------------------------------
  describe("getElement", () => {
    it("GETs the element at the current commit", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: FAKE_ELEMENT_RESPONSE },
      ]);
      const client = await buildInitializedClient(fetch);

      const el = await client.getElement("elem-uuid-1");

      const [url] = fetch.mock.calls[2] as [string];
      expect(url).toContain("/elements/elem-uuid-1");
      expect(el.id).toBe("elem-uuid-1");
      expect(el.type).toBe("PartDefinition");
      expect(el.name).toBe("Engine");
    });
  });

  // -------------------------------------------------------------------------
  describe("queryElements", () => {
    it("POSTs to query-results with a Query body", async () => {
      const queryResultsResponse = [FAKE_ELEMENT_RESPONSE];
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: queryResultsResponse },
      ]);
      const client = await buildInitializedClient(fetch);

      const elements = await client.queryElements("PartDefinition");

      const [url, init] = fetch.mock.calls[2] as [string, RequestInit];
      expect(url).toContain("/query-results");
      expect(url).toContain(`commitId=${client.headCommitId}`);
      expect(init.method).toBe("POST");

      const body = JSON.parse(init.body as string);
      expect(body["@type"]).toBe("Query");

      expect(elements).toHaveLength(1);
      expect(elements[0].type).toBe("PartDefinition");
    });

    it("returns all elements when no filter given", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [FAKE_ELEMENT_RESPONSE] },
      ]);
      const client = await buildInitializedClient(fetch);

      const elements = await client.queryElements();
      expect(elements).toHaveLength(1);
    });
  });

  // -------------------------------------------------------------------------
  describe("queryRelationships", () => {
    it("GETs relationships for an element with direction parameter", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [FAKE_RELATIONSHIP_RESPONSE] },
      ]);
      const client = await buildInitializedClient(fetch);

      const rels = await client.queryRelationships("elem-uuid-1", "both");

      const [url] = fetch.mock.calls[2] as [string];
      expect(url).toContain(`/elements/elem-uuid-1/relationships`);
      expect(url).toContain("direction=both");

      expect(rels).toHaveLength(1);
      expect(rels[0].sourceIds).toContain("elem-uuid-1");
      expect(rels[0].targetIds).toContain("elem-uuid-2");
    });

    it("queries all relationships when no elementId given", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [FAKE_RELATIONSHIP_RESPONSE] },
      ]);
      const client = await buildInitializedClient(fetch);

      const rels = await client.queryRelationships();

      const [url] = fetch.mock.calls[2] as [string];
      // Should query the elements endpoint filtered to relationship types
      expect(url).toContain("/query-results");
      expect(rels).toHaveLength(1);
    });
  });

  // -------------------------------------------------------------------------
  describe("getProjectState", () => {
    it("returns aggregate counts from current commit", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [FAKE_ELEMENT_RESPONSE] },
      ]);
      const client = await buildInitializedClient(fetch);

      const state = await client.getProjectState();

      expect(state.projectId).toBe("proj-uuid-1");
      expect(state.commitId).toBe("commit-uuid-0");
      expect(state.branchId).toBe("branch-uuid-1");
      expect(state.totalElements).toBe(1);
      expect(state.elementCountsByType["PartDefinition"]).toBe(1);
    });
  });
});
