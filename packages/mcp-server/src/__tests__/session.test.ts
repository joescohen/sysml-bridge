/**
 * session.test.ts
 *
 * (a) Unit tests on SessionTracker — the forward-only lifecycle state machine
 *     persisted to <dir>/.mbse/session.json (atomic temp+rename).
 * (b) E2E tests via callTool — drive the fully-wired server (buildServer, which
 *     installs the same session tracking main() uses) through the lifecycle and
 *     assert session.json advances to "render", reading the file between calls
 *     so the intermediate states are observed.
 *
 * The unit atomic-write test mirrors
 * packages/model/src/store/__tests__/atomic-persist.test.ts.
 * The e2e buildTestPair mirrors create-relationship.test.ts.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { promises as fsp } from "node:fs";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { FileStore } from "@sysml-bridge/model";
import { clearCorpusCache } from "@sysml-bridge/gates";
import { SessionTracker, LIFECYCLE, type LifecycleState } from "../session.js";
import { buildServer } from "../index.js";

// ---------------------------------------------------------------------------
// (a) Unit — SessionTracker
// ---------------------------------------------------------------------------

describe("SessionTracker — lifecycle state machine", () => {
  let dir: string;
  let tracker: SessionTracker;

  beforeEach(async () => {
    dir = await fsp.mkdtemp(path.join(os.tmpdir(), "sysml-session-unit-"));
    tracker = new SessionTracker(dir);
  });

  afterEach(async () => {
    vi.restoreAllMocks();
    await fsp.rm(dir, { recursive: true, force: true });
  });

  it("LIFECYCLE is the seven ordered stages", () => {
    expect([...LIFECYCLE]).toEqual([
      "init",
      "ingest",
      "build",
      "trace",
      "enrich",
      "validate",
      "render",
    ]);
  });

  it("state() is null before any advance (file absent)", () => {
    expect(tracker.state()).toBeNull();
  });

  it("a full ordered walk hits exactly the seven states in order", async () => {
    const observed: (LifecycleState | null)[] = [];
    for (const stage of LIFECYCLE) {
      await tracker.advance(stage);
      observed.push(tracker.state());
    }
    expect(observed).toEqual([
      "init",
      "ingest",
      "build",
      "trace",
      "enrich",
      "validate",
      "render",
    ]);
  });

  it("forward skips are allowed (init → build → render)", async () => {
    await tracker.advance("init");
    expect(tracker.state()).toBe("init");
    await tracker.advance("build"); // skips ingest
    expect(tracker.state()).toBe("build");
    await tracker.advance("render"); // skips trace, validate
    expect(tracker.state()).toBe("render");
  });

  it("advancing to the current state is an idempotent no-op (no write)", async () => {
    await tracker.advance("build");
    const file = path.join(dir, ".mbse", "session.json");
    const before = await fsp.readFile(file, "utf8");

    const writeSpy = vi.spyOn(fs.promises, "writeFile");
    await tracker.advance("build"); // same state
    expect(writeSpy).not.toHaveBeenCalled();

    const after = await fsp.readFile(file, "utf8");
    expect(after).toBe(before);
    expect(tracker.state()).toBe("build");
  });

  it("a backward advance throws with the exact error message shape", async () => {
    await tracker.advance("validate");
    await expect(tracker.advance("build")).rejects.toThrow(
      "invalid lifecycle transition validate → build"
    );
    // State must not have regressed.
    expect(tracker.state()).toBe("validate");
  });

  it("an unknown target state throws", async () => {
    await expect(
      tracker.advance("bogus" as unknown as LifecycleState)
    ).rejects.toThrow("unknown lifecycle state bogus");
  });

  it("persist is atomic — writes a temp file then renames onto session.json", async () => {
    // Mirror atomic-persist.test.ts: no writeFile may target the real file;
    // the final rename must land on it; no .tmp- leftover remains.
    const writeSpy = vi.spyOn(fs.promises, "writeFile");
    const renameSpy = vi.spyOn(fs.promises, "rename");

    await tracker.advance("init");

    const sessionFile = path.join(dir, ".mbse", "session.json");
    for (const call of writeSpy.mock.calls) {
      expect(String(call[0])).not.toBe(sessionFile);
    }
    expect(renameSpy).toHaveBeenCalled();
    const lastRename = renameSpy.mock.calls.at(-1)!;
    expect(String(lastRename[1])).toBe(sessionFile);

    const doc = JSON.parse(await fsp.readFile(sessionFile, "utf8"));
    expect(doc.state).toBe("init");
    expect(typeof doc.updatedAt).toBe("string");

    const leftovers = (await fsp.readdir(path.join(dir, ".mbse"))).filter((f) =>
      f.includes(".tmp-")
    );
    expect(leftovers).toEqual([]);
  });

  it("session.json is human-readable JSON (2-space indent, {state, updatedAt})", async () => {
    await tracker.advance("trace");
    const raw = await fsp.readFile(path.join(dir, ".mbse", "session.json"), "utf8");
    // 2-space indent leaves a `\n  "state"` line.
    expect(raw).toContain('\n  "state": "trace"');
    const doc = JSON.parse(raw);
    expect(Object.keys(doc).sort()).toEqual(["state", "updatedAt"]);
  });
});

// ---------------------------------------------------------------------------
// (b) E2E — session tracking wired to real tool calls via buildServer
// ---------------------------------------------------------------------------

async function connectWiredClient(
  dir: string
): Promise<{ client: Client; cleanup: () => Promise<void> }> {
  const store = new FileStore(dir);
  const session = new SessionTracker(dir);
  const server = buildServer(store, session);

  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: "test-client", version: "0.0.1" });

  await server.connect(serverTransport);
  await client.connect(clientTransport);

  return {
    client,
    cleanup: async () => {
      await client.close();
    },
  };
}

function readSessionState(dir: string): LifecycleState | null {
  try {
    const raw = fs.readFileSync(path.join(dir, ".mbse", "session.json"), "utf8");
    return JSON.parse(raw).state as LifecycleState;
  } catch {
    return null;
  }
}

describe("session tracking — e2e via callTool", () => {
  let dir: string;
  let client: Client;
  let cleanup: () => Promise<void>;

  beforeEach(async () => {
    dir = await fsp.mkdtemp(path.join(os.tmpdir(), "sysml-session-e2e-"));
    ({ client, cleanup } = await connectWiredClient(dir));
  });

  afterEach(async () => {
    await cleanup();
    await fsp.rm(dir, { recursive: true, force: true });
  });

  it("drives init → build → trace → validate → render and lands at render, passing through the states", async () => {
    const seen: (LifecycleState | null)[] = [];

    // init_project → init
    const init = await client.callTool({
      name: "init_project",
      arguments: { name: "Session E2E", create: true },
    });
    expect(init.isError).toBeFalsy();
    seen.push(readSessionState(dir));
    expect(readSessionState(dir)).toBe("init");

    // create_element (a Usage part) → build.
    // R4: use a Usage so a later relationship has a valid Feature operand.
    const partRes = await client.callTool({
      name: "create_element",
      arguments: { type: "PartUsage", name: "Widget" },
    });
    expect(partRes.isError).toBeFalsy();
    const part = JSON.parse(
      (partRes.content as Array<{ text: string }>)[0].text
    );
    const partId = part.element?.id ?? part.id;
    expect(typeof partId).toBe("string");
    seen.push(readSessionState(dir));
    expect(readSessionState(dir)).toBe("build");

    // A requirement usage to be the trace target.
    const reqRes = await client.callTool({
      name: "create_element",
      arguments: { type: "RequirementUsage", name: "MassReq" },
    });
    expect(reqRes.isError).toBeFalsy();
    const req = JSON.parse((reqRes.content as Array<{ text: string }>)[0].text);
    const reqId = req.element?.id ?? req.id;

    // create_relationship → trace
    const rel = await client.callTool({
      name: "create_relationship",
      arguments: {
        type: "SatisfyRequirementUsage",
        source_id: partId,
        target_id: reqId,
      },
    });
    expect(rel.isError).toBeFalsy();
    seen.push(readSessionState(dir));
    expect(readSessionState(dir)).toBe("trace");

    // validate_model — this minimal model has open issues, so a CLEAN-run gate
    // means the session may or may not reach "validate". Either way it must NOT
    // regress below "trace". (We assert the non-regression invariant, not a
    // specific advance, because "validate" only fires on a clean run.)
    const validate = await client.callTool({
      name: "validate_model",
      arguments: {},
    });
    expect(validate.isError).toBeFalsy();
    const afterValidate = readSessionState(dir);
    seen.push(afterValidate);
    expect(LIFECYCLE.indexOf(afterValidate!)).toBeGreaterThanOrEqual(
      LIFECYCLE.indexOf("trace")
    );

    // export_sysml → render
    const exp = await client.callTool({
      name: "export_sysml",
      arguments: {},
    });
    expect(exp.isError).toBeFalsy();
    seen.push(readSessionState(dir));

    // Final state is render.
    expect(readSessionState(dir)).toBe("render");

    // The walk passed through init, build, and trace on the way to render.
    expect(seen).toContain("init");
    expect(seen).toContain("build");
    expect(seen).toContain("trace");
  });

  it("import_sysml advances the session to ingest", async () => {
    await client.callTool({
      name: "init_project",
      arguments: { name: "Ingest E2E", create: true },
    });
    expect(readSessionState(dir)).toBe("init");

    const imp = await client.callTool({
      name: "import_sysml",
      arguments: { sysml_text: "package P { part def Widget; }" },
    });
    expect(imp.isError).toBeFalsy();
    expect(readSessionState(dir)).toBe("ingest");
  });

  it("a backward-implying tool call never fails the tool and never regresses the session", async () => {
    // Drive to render, then call create_element (which maps to the earlier
    // 'build' stage). The advance is a backward move → rejected internally, but
    // the tool call itself must still succeed and the session must stay at render.
    await client.callTool({
      name: "init_project",
      arguments: { name: "Regress E2E", create: true },
    });
    await client.callTool({
      name: "export_sysml",
      arguments: {},
    });
    expect(readSessionState(dir)).toBe("render");

    const late = await client.callTool({
      name: "create_element",
      arguments: { type: "PartUsage", name: "LatePart" },
    });
    // Tool call itself succeeds...
    expect(late.isError).toBeFalsy();
    // ...and the session did not regress.
    expect(readSessionState(dir)).toBe("render");
  });

  it("a CLEAN validate_model run advances the session to validate", async () => {
    // Minimal model that satisfies every validate_model issue category:
    // a covered stakeholder need, a system requirement that is forward-traced
    // (satisfy), verified (verify), and backward-traced (derive), a non-orphan
    // part, provenance on everything, no dangling endpoints. Provenance ids
    // resolve against a minimal corpus fixture (GATE-05 runs on create_element,
    // and the resolution set is id ∪ naturalKey ∪ name).
    const corpusPath = path.join(dir, "session-clean-corpus.json");
    fs.writeFileSync(
      corpusPath,
      JSON.stringify({
        schema_version: "1.0.0",
        subsystem: "TEST",
        needs: [{ id: "corpus-need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
        requirements: [
          {
            id: "corpus-req-001",
            kind: "requirement",
            naturalKey: "R1",
            name: "Test Requirement",
            statement: "The system shall do something.",
            needIds: ["corpus-need-001"],
          },
        ],
        functions: [],
        components: [
          { id: "corpus-comp-001", kind: "component", naturalKey: "C1", name: "Component C1" },
        ],
        satisfies: [],
        allocations: [],
      })
    );
    const savedCorpusPath = process.env.SYSML_BRIDGE_CORPUS_PATH;
    process.env.SYSML_BRIDGE_CORPUS_PATH = corpusPath;
    clearCorpusCache();
    try {
      await client.callTool({
        name: "init_project",
        arguments: { name: "Clean Validate E2E", create: true },
      });

      // create_element returns the bare element when there are zero findings,
      // or {element, findings} when any findings exist — handle both.
      const mk = async (type: string, name: string, attributes: Record<string, unknown>) => {
        const res = await client.callTool({
          name: "create_element",
          arguments: { type, name, attributes },
        });
        expect(res.isError).toBeFalsy();
        const parsed = JSON.parse(
          (res as { content: Array<{ text: string }> }).content[0].text
        );
        return parsed.element ?? parsed;
      };

      // R4: trace operands must be Usages — the Need is a RequirementUsage so
      // the Derive edge below has usage operands on both ends.
      const need = await mk("RequirementUsage", "Need1", {
        provenanceSourceId: "N1",
        stakeholderNeed: true,
      });
      const req = await mk("RequirementUsage", "Req1", { provenanceSourceId: "R1" });
      const part = await mk("PartUsage", "Part1", { provenanceSourceId: "C1" });
      await mk("DeriveRequirementUsage", "", {
        provenanceSourceId: "R1",
        source: [{ "@id": req.id }],
        target: [{ "@id": need.id }],
      });
      await mk("SatisfyRequirementUsage", "", {
        provenanceSourceId: "R1",
        source: [{ "@id": part.id }],
        target: [{ "@id": req.id }],
      });
      await mk("VerifyRequirementUsage", "", {
        provenanceSourceId: "R1",
        source: [{ "@id": req.id }],
        target: [{ "@id": req.id }],
      });
      expect(readSessionState(dir)).toBe("build");

      const validate = await client.callTool({
        name: "validate_model",
        arguments: {},
      });
      expect(validate.isError).toBeFalsy();
      const payload = JSON.parse(
        (validate as { content: Array<{ text: string }> }).content[0].text
      );
      // The model is genuinely clean — this test exercises the clean branch of
      // the session wiring, which the mixed walk above cannot.
      expect(payload.issues).toEqual([]);
      // Clean means clean across BOTH channels: legacy issues AND Gate-1 findings.
      expect(
        (payload.findings ?? []).filter((f: { severity: string }) => f.severity === "error")
      ).toEqual([]);
      // Forward skip build → validate is allowed and MUST have fired.
      expect(readSessionState(dir)).toBe("validate");
    } finally {
      if (savedCorpusPath === undefined) delete process.env.SYSML_BRIDGE_CORPUS_PATH;
      else process.env.SYSML_BRIDGE_CORPUS_PATH = savedCorpusPath;
      clearCorpusCache();
    }
  });
});
