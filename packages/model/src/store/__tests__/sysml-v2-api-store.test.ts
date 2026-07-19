import { describe, it, expect, vi, afterEach } from "vitest";
import { SysmlV2ApiStore } from "../sysml-v2-api-store.js";

// ---------------------------------------------------------------------------
// Fetch mock helpers — a queue-based fake fetch, ported from the
// sysml-bridge SmapsClient test fixtures (packages/mcp-server/src/__tests__/
// smaps-client.test.ts there), reconciled with the wire shapes VERIFIED
// LIVE against the pilot server at http://localhost:9000 (KerML
// `declaredName`, not `name`; PrimitiveConstraint.value as an array).
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

const ENDPOINT = "http://localhost:9000";

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

function elementResponse(overrides: Record<string, unknown> = {}) {
  return {
    "@id": "elem-uuid-1",
    "@type": "PartDefinition",
    name: null,
    declaredName: "Engine",
    declaredShortName: null,
    elementId: "elem-uuid-1",
    qualifiedName: null,
    aliasIds: [],
    owner: null,
    ownedElement: [],
    ...overrides,
  };
}

const FAKE_COMMIT_RESPONSE = {
  "@id": "commit-uuid-1",
  "@type": "Commit",
  created: "2024-01-02T00:00:00Z",
  owningProject: { "@id": "proj-uuid-1" },
};

function dataVersion(payload: Record<string, unknown> | null, identity?: { "@id": string }) {
  return {
    "@type": "DataVersion",
    ...(identity ? { identity } : {}),
    payload,
  };
}

// ---------------------------------------------------------------------------
// Helpers to build an initialized SysmlV2ApiStore
// ---------------------------------------------------------------------------

async function buildInitializedStore(fetchMock: ReturnType<typeof vi.fn>) {
  vi.stubGlobal("fetch", fetchMock);
  const store = new SysmlV2ApiStore(ENDPOINT);
  await store.loadProject("proj-uuid-1");
  return store;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("SysmlV2ApiStore", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  // -------------------------------------------------------------------------
  describe("checkConnection", () => {
    it("returns true when the server responds OK", async () => {
      vi.stubGlobal("fetch", mockFetch([{ ok: true, body: [] }]));
      const store = new SysmlV2ApiStore(ENDPOINT);
      expect(await store.checkConnection()).toBe(true);
    });

    it("returns false when the server responds with an error", async () => {
      vi.stubGlobal("fetch", mockFetch([{ ok: false, status: 503 }]));
      const store = new SysmlV2ApiStore(ENDPOINT);
      expect(await store.checkConnection()).toBe(false);
    });

    it("returns false when fetch throws (network error)", async () => {
      vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("ECONNREFUSED")));
      const store = new SysmlV2ApiStore(ENDPOINT);
      expect(await store.checkConnection()).toBe(false);
    });
  });

  // -------------------------------------------------------------------------
  describe("createProject", () => {
    it("POSTs to /projects, fetches the branch, and stores project/branch/head ids", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT }, // POST /projects
        { ok: true, body: FAKE_BRANCH }, // GET  /projects/{id}/branches/{branchId}
      ]);
      vi.stubGlobal("fetch", fetch);

      const store = new SysmlV2ApiStore(ENDPOINT);
      const project = await store.createProject("TestProject");

      const [url, init] = fetch.mock.calls[0] as [string, RequestInit];
      expect(url).toBe(`${ENDPOINT}/projects`);
      expect(init.method).toBe("POST");
      const body = JSON.parse(init.body as string);
      expect(body["@type"]).toBe("Project");
      expect(body.name).toBe("TestProject");

      expect(store.projectId).toBe("proj-uuid-1");
      expect(store.branchId).toBe("branch-uuid-1");
      expect(store.headCommitId).toBe("commit-uuid-0");

      expect(project["@id"]).toBe("proj-uuid-1");
      expect(project.name).toBe("TestProject");
    });
  });

  // -------------------------------------------------------------------------
  describe("loadProject", () => {
    it("GETs the project and its branch, then stores state", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
      ]);
      vi.stubGlobal("fetch", fetch);

      const store = new SysmlV2ApiStore(ENDPOINT);
      await store.loadProject("proj-uuid-1");

      expect(store.projectId).toBe("proj-uuid-1");
      expect(store.branchId).toBe("branch-uuid-1");
      expect(store.headCommitId).toBe("commit-uuid-0");
    });

    it("throws when the project is not found", async () => {
      vi.stubGlobal("fetch", mockFetch([{ ok: false, status: 404 }]));
      const store = new SysmlV2ApiStore(ENDPOINT);
      await expect(store.loadProject("bad-id")).rejects.toThrow();
    });
  });

  // -------------------------------------------------------------------------
  describe("listProjects", () => {
    it("GETs /projects and returns the array", async () => {
      const fetch = mockFetch([{ ok: true, body: [FAKE_PROJECT] }]);
      vi.stubGlobal("fetch", fetch);

      const store = new SysmlV2ApiStore(ENDPOINT);
      const projects = await store.listProjects();

      expect(projects).toHaveLength(1);
      expect(projects[0]["@id"]).toBe("proj-uuid-1");

      const [url] = fetch.mock.calls[0] as [string];
      expect(url).toBe(`${ENDPOINT}/projects`);
    });
  });

  // -------------------------------------------------------------------------
  describe("createElement", () => {
    it("POSTs a commit whose payload uses declaredName (not name)", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT }, // loadProject GET
        { ok: true, body: FAKE_BRANCH }, // loadProject branch GET
        { ok: true, body: FAKE_COMMIT_RESPONSE }, // createElement commit POST
        { ok: true, body: [dataVersion(elementResponse())] }, // fetchCommitChanges GET
      ]);
      const store = await buildInitializedStore(fetch);

      const el = await store.createElement("PartDefinition", "Engine");

      const commitCall = fetch.mock.calls[2] as [string, RequestInit];
      expect(commitCall[0]).toBe(`${ENDPOINT}/projects/proj-uuid-1/commits?branchId=branch-uuid-1`);
      expect(commitCall[1].method).toBe("POST");

      const body = JSON.parse(commitCall[1].body as string);
      expect(body["@type"]).toBe("Commit");
      expect(body.previousCommit["@id"]).toBe("commit-uuid-0");
      expect(body.change).toHaveLength(1);
      expect(body.change[0]["@type"]).toBe("DataVersion");
      expect(body.change[0].payload["@type"]).toBe("PartDefinition");
      expect(body.change[0].payload.declaredName).toBe("Engine");
      expect(body.change[0].payload.name).toBeUndefined();

      expect(store.headCommitId).toBe("commit-uuid-1");

      expect(el.id).toBe("elem-uuid-1");
      expect(el.elementId).toBe("elem-uuid-1");
      expect(el.type).toBe("PartDefinition");
      expect(el.name).toBe("Engine");
    });

    it("includes extra attributes in the payload", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: FAKE_COMMIT_RESPONSE },
        { ok: true, body: [dataVersion(elementResponse({ isAbstract: true }))] },
      ]);
      const store = await buildInitializedStore(fetch);

      await store.createElement("PartDefinition", "Engine", { isAbstract: true });

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body.change[0].payload.isAbstract).toBe(true);
    });

    it("folds a foundry-side aliasId into the payload's aliasIds array", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: FAKE_COMMIT_RESPONSE },
        {
          ok: true,
          body: [dataVersion(elementResponse({ aliasIds: ["local-foundry-id-42"] }))],
        },
      ]);
      const store = await buildInitializedStore(fetch);

      const el = await store.createElement("PartDefinition", "Engine", {
        aliasId: "local-foundry-id-42",
      });

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body.change[0].payload.aliasIds).toEqual(["local-foundry-id-42"]);
      // Server-assigned identity still lands in id/elementId; the foundry id
      // is recoverable separately via aliasId.
      expect(el.id).toBe("elem-uuid-1");
      expect(el.elementId).toBe("elem-uuid-1");
      expect(el.aliasId).toBe("local-foundry-id-42");
    });

    it("throws if project not loaded", async () => {
      vi.stubGlobal("fetch", vi.fn());
      const store = new SysmlV2ApiStore(ENDPOINT);
      await expect(store.createElement("PartDefinition", "X")).rejects.toThrow(/not initialized/i);
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
      };
      const changes = [
        dataVersion(
          elementResponse({ "@id": "elem-uuid-A", elementId: "elem-uuid-A", declaredName: "Engine" })
        ),
        dataVersion(
          elementResponse({
            "@id": "elem-uuid-B",
            elementId: "elem-uuid-B",
            "@type": "PortDefinition",
            declaredName: "FuelPort",
          })
        ),
      ];

      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: batchCommitResponse },
        { ok: true, body: changes },
      ]);
      const store = await buildInitializedStore(fetch);

      const elements = await store.createElements([
        { type: "PartDefinition", name: "Engine" },
        { type: "PortDefinition", name: "FuelPort" },
      ]);

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body.change).toHaveLength(2);
      expect(body.change[0].payload.declaredName).toBe("Engine");
      expect(body.change[1].payload.declaredName).toBe("FuelPort");
      expect(elements).toHaveLength(2);
      expect(elements[0].id).toBe("elem-uuid-A");
      expect(elements[1].id).toBe("elem-uuid-B");
      expect(store.headCommitId).toBe("commit-uuid-2");
    });
  });

  // -------------------------------------------------------------------------
  describe("updateElement", () => {
    it("reads the current element, merges updates, and commits a full-replacement payload", async () => {
      const updateCommitResponse = {
        "@id": "commit-uuid-3",
        "@type": "Commit",
        created: "2024-01-03T00:00:00Z",
        owningProject: { "@id": "proj-uuid-1" },
      };

      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: elementResponse({ isAbstract: true }) }, // getElement (current)
        { ok: true, body: updateCommitResponse }, // updateElement commit
        {
          ok: true,
          body: [dataVersion(elementResponse({ declaredName: "Engine v2", isAbstract: true }))],
        }, // fetchCommitChanges
      ]);
      const store = await buildInitializedStore(fetch);

      const updated = await store.updateElement("elem-uuid-1", { name: "Engine v2" });

      const body = JSON.parse((fetch.mock.calls[3] as [string, RequestInit])[1].body as string);
      expect(body["@type"]).toBe("Commit");
      expect(body.change[0]["@type"]).toBe("DataVersion");
      expect(body.change[0].identity["@id"]).toBe("elem-uuid-1");
      expect(body.change[0].payload.declaredName).toBe("Engine v2");
      // The field NOT touched by this update (isAbstract) must still be
      // carried forward from the current snapshot — the API replaces the
      // whole element state per commit, so dropping it would silently
      // reset it server-side (live-verified).
      expect(body.change[0].payload.isAbstract).toBe(true);

      expect(updated.id).toBe("elem-uuid-1");
      expect(updated.name).toBe("Engine v2");
      expect(store.headCommitId).toBe("commit-uuid-3");
    });
  });

  // -------------------------------------------------------------------------
  describe("deleteElement", () => {
    it("sends a commit with null payload and identity pointing to the element", async () => {
      const deleteCommitResponse = {
        "@id": "commit-uuid-4",
        "@type": "Commit",
        created: "2024-01-04T00:00:00Z",
        owningProject: { "@id": "proj-uuid-1" },
      };

      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: deleteCommitResponse },
      ]);
      const store = await buildInitializedStore(fetch);

      await store.deleteElement("elem-uuid-1");

      const body = JSON.parse((fetch.mock.calls[2] as [string, RequestInit])[1].body as string);
      expect(body["@type"]).toBe("Commit");
      expect(body.change[0]["@type"]).toBe("DataVersion");
      expect(body.change[0].identity["@id"]).toBe("elem-uuid-1");
      expect(body.change[0].payload).toBeNull();

      expect(store.headCommitId).toBe("commit-uuid-4");
    });

    it("throws if project not loaded", async () => {
      vi.stubGlobal("fetch", vi.fn());
      const store = new SysmlV2ApiStore(ENDPOINT);
      await expect(store.deleteElement("some-id")).rejects.toThrow(/not initialized/i);
    });
  });

  // -------------------------------------------------------------------------
  describe("getElement", () => {
    it("GETs the element at the current commit", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: elementResponse() },
      ]);
      const store = await buildInitializedStore(fetch);

      const el = await store.getElement("elem-uuid-1");

      const [url] = fetch.mock.calls[2] as [string];
      expect(url).toContain("/elements/elem-uuid-1");
      expect(el.id).toBe("elem-uuid-1");
      expect(el.type).toBe("PartDefinition");
      expect(el.name).toBe("Engine");
    });
  });

  // -------------------------------------------------------------------------
  describe("queryElements", () => {
    it("POSTs to query-results with a Query body whose constraint value is an array", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [elementResponse()] },
      ]);
      const store = await buildInitializedStore(fetch);

      const elements = await store.queryElements("PartDefinition");

      const [url, init] = fetch.mock.calls[2] as [string, RequestInit];
      expect(url).toContain("/query-results");
      expect(url).toContain(`commitId=${store.headCommitId}`);
      expect(init.method).toBe("POST");

      const body = JSON.parse(init.body as string);
      expect(body["@type"]).toBe("Query");
      // Live-verified requirement: the pilot server 500s on a bare scalar
      // value ("Cannot deserialize instance of java.util.ArrayList out of
      // VALUE_STRING token") — it must be wrapped in an array.
      expect(Array.isArray(body.where.value)).toBe(true);
      expect(body.where.value).toEqual(["PartDefinition"]);

      expect(elements).toHaveLength(1);
      expect(elements[0].type).toBe("PartDefinition");
    });

    it("returns all elements when no filter given (a bare {\"@type\":\"Query\"})", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [elementResponse()] },
      ]);
      const store = await buildInitializedStore(fetch);

      const elements = await store.queryElements();

      const [, init] = fetch.mock.calls[2] as [string, RequestInit];
      const body = JSON.parse(init.body as string);
      expect(body.where).toBeUndefined();
      expect(elements).toHaveLength(1);
    });

    it("applies namePattern as a client-side substring filter (server has no wildcard op)", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        {
          ok: true,
          body: [
            elementResponse({ declaredName: "Engine" }),
            elementResponse({ "@id": "elem-uuid-2", elementId: "elem-uuid-2", declaredName: "Wheel" }),
          ],
        },
      ]);
      const store = await buildInitializedStore(fetch);

      const elements = await store.queryElements(undefined, "eng");
      expect(elements).toHaveLength(1);
      expect(elements[0].name).toBe("Engine");
    });
  });

  // -------------------------------------------------------------------------
  // Regression coverage for a live bug found by driving the real MCP tool
  // surface end-to-end (create_element's GATE-05 pre-check calls
  // queryElements() before anything has ever been committed): a brand-new
  // project/branch has `head: null` (live-verified), so headCommitId is
  // null. Before the fix, that null was interpolated straight into the
  // query/element URL as the literal text "null", and the pilot server
  // 400ed. These reads must short-circuit instead of ever hitting fetch.
  describe("reads on a fresh project with no commits yet (headCommitId === null)", () => {
    const FRESH_BRANCH = { ...FAKE_BRANCH, head: null };

    it("queryElements returns [] without calling fetch again", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FRESH_BRANCH },
      ]);
      const store = await buildInitializedStore(fetch);
      expect(store.headCommitId).toBeNull();

      const calls = fetch.mock.calls.length;
      const elements = await store.queryElements();
      expect(elements).toEqual([]);
      expect(fetch.mock.calls.length).toBe(calls); // no extra (bad) fetch fired
    });

    it("getElement throws a not-found error without calling fetch again", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FRESH_BRANCH },
      ]);
      const store = await buildInitializedStore(fetch);

      const calls = fetch.mock.calls.length;
      await expect(store.getElement("elem-uuid-1")).rejects.toThrow(/not found/i);
      expect(fetch.mock.calls.length).toBe(calls);
    });

    it("queryRelationships(elementId) returns [] without calling fetch again", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FRESH_BRANCH },
      ]);
      const store = await buildInitializedStore(fetch);

      const calls = fetch.mock.calls.length;
      const rels = await store.queryRelationships("elem-uuid-1");
      expect(rels).toEqual([]);
      expect(fetch.mock.calls.length).toBe(calls);
    });

    it("getProjectState reports zero elements and commitId \"\" (not the string \"null\")", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FRESH_BRANCH },
      ]);
      const store = await buildInitializedStore(fetch);

      const state = await store.getProjectState();
      expect(state.totalElements).toBe(0);
      expect(state.commitId).toBe("");
    });
  });

  // -------------------------------------------------------------------------
  describe("queryRelationships", () => {
    const FAKE_RELATIONSHIP_RESPONSE = {
      "@id": "rel-uuid-1",
      "@type": "FeatureTyping",
      source: [{ "@id": "elem-uuid-1" }],
      target: [{ "@id": "elem-uuid-2" }],
      owner: null,
      ownedElement: [],
      name: null,
      declaredName: null,
      declaredShortName: null,
      qualifiedName: null,
    };

    it("GETs relationships for an element with a direction parameter", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [FAKE_RELATIONSHIP_RESPONSE] },
      ]);
      const store = await buildInitializedStore(fetch);

      const rels = await store.queryRelationships("elem-uuid-1", "both");

      const [url] = fetch.mock.calls[2] as [string];
      expect(url).toContain("/elements/elem-uuid-1/relationships");
      expect(url).toContain("direction=both");

      expect(rels).toHaveLength(1);
      expect(rels[0].sourceIds).toContain("elem-uuid-1");
      expect(rels[0].targetIds).toContain("elem-uuid-2");
    });

    it("falls back to fetching + classifying all elements when no elementId is given (no instanceOf op)", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        // queryElements() (no filter) -> query-results with a bare Query,
        // returning a mix of a plain element and a relationship element.
        { ok: true, body: [elementResponse(), FAKE_RELATIONSHIP_RESPONSE] },
      ]);
      const store = await buildInitializedStore(fetch);

      const rels = await store.queryRelationships();

      const [url, init] = fetch.mock.calls[2] as [string, RequestInit];
      expect(url).toContain("/query-results");
      const body = JSON.parse(init.body as string);
      // Must NOT use the unsupported instanceOf operator against @type.
      expect(body.where).toBeUndefined();

      expect(rels).toHaveLength(1);
      expect(rels[0].type).toBe("FeatureTyping");
    });
  });

  // -------------------------------------------------------------------------
  describe("getProjectState", () => {
    it("returns aggregate counts from the current commit", async () => {
      const fetch = mockFetch([
        { ok: true, body: FAKE_PROJECT },
        { ok: true, body: FAKE_BRANCH },
        { ok: true, body: [elementResponse()] },
      ]);
      const store = await buildInitializedStore(fetch);

      const state = await store.getProjectState();

      expect(state.projectId).toBe("proj-uuid-1");
      expect(state.commitId).toBe("commit-uuid-0");
      expect(state.branchId).toBe("branch-uuid-1");
      expect(state.totalElements).toBe(1);
      expect(state.elementCountsByType["PartDefinition"]).toBe(1);
    });
  });
});
