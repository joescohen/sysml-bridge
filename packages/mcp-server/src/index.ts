import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import * as path from "node:path";
import { pathToFileURL } from "node:url";
import { z } from "zod";

import { FileStore, SysmlV2ApiStore } from "@sysml-bridge/model";
import type { ModelStore } from "@sysml-bridge/model";
import { SessionTracker, type LifecycleState } from "./session.js";
import { registerCreateElement } from "./tools/create-element.js";
import { registerQueryElements } from "./tools/query-elements.js";
import { registerCreateRelationship } from "./tools/create-relationship.js";
import { registerQueryRelationships } from "./tools/query-relationships.js";
import { registerValidateModel } from "./tools/validate-model.js";
import { registerExportSysml } from "./tools/export-sysml.js";
import { registerImportSysml } from "./tools/import-sysml.js";
import { registerGetProjectState } from "./tools/get-project-state.js";
import { registerUpdateElement } from "./tools/update-element.js";
import { registerDeleteElement } from "./tools/delete-element.js";
import { registerWeavePass } from "./tools/weave-pass.js";
import { registerClosePass } from "./tools/close-pass.js";

// ---------------------------------------------------------------------------
// Store
//
//   SYSML_FOUNDRY_BACKEND=file  (default) → file-native .sysml/JSON store,
//                                            no server (SYSML_FOUNDRY_MODEL_DIR,
//                                            default .sysml-bridge/models)
//   SYSML_FOUNDRY_BACKEND=api             → live SysML v2 REST API backend
//                                            (SYSML_FOUNDRY_API_ENDPOINT,
//                                            default http://localhost:9000)
//
// This is the ONLY file that instantiates a store — every tool depends only
// on the ModelStore interface, never on the concrete backend, so this is a
// one-line swap. Default stays `file`: nothing about behavior changes for
// anyone who doesn't set SYSML_FOUNDRY_BACKEND.
// ---------------------------------------------------------------------------

const MODEL_DIR =
  process.env.SYSML_FOUNDRY_MODEL_DIR ??
  path.join(process.cwd(), ".sysml-bridge/models");

const BACKEND = (process.env.SYSML_FOUNDRY_BACKEND ?? "file").toLowerCase();
const API_ENDPOINT = process.env.SYSML_FOUNDRY_API_ENDPOINT ?? "http://localhost:9000";

function createStore(): ModelStore {
  return BACKEND === "api" ? new SysmlV2ApiStore(API_ENDPOINT) : new FileStore(MODEL_DIR);
}

// ---------------------------------------------------------------------------
// Lifecycle session tracking
//
// A SessionTracker records how far the model has progressed through the MBSE
// lifecycle (init → ingest → build → trace → validate → render) into
// <MODEL_DIR>/.mbse/session.json, as a side effect of tool calls. The /mbse
// orchestrator skill reads that file to report position and route the next
// step.
//
// Wiring mechanism: we wrap `server.tool` so that, for the lifecycle-mapped
// tools, the registered handler is decorated to fire an `advance()` AFTER a
// successful (non-error) result. This is the LEAST invasive option — the tool
// files and their existing tests are untouched, and the public tool behavior
// is identical: the wrapper returns the tool's exact result and only performs
// a best-effort session write on the side. A rejected (backward) advance is
// swallowed (logged to stderr) so it can NEVER fail the tool call itself.
// ---------------------------------------------------------------------------

/**
 * Map from tool name → the lifecycle stage its successful call implies. Tools
 * not listed here do not move the session (e.g. query_elements is read-only).
 */
const LIFECYCLE_BY_TOOL: Record<string, LifecycleState> = {
  init_project: "init",
  import_sysml: "ingest",
  create_element: "build",
  create_relationship: "trace",
  // A weave pass is gap-driven enrichment that PROPOSES links to close audit
  // gaps — after build/trace, before validate. It maps to the dedicated
  // `enrich` stage (see tools/weave-pass.ts for the full rationale). Both the
  // open pass (weave_pass) and a convergent close (close_pass) advance here; a
  // failed close returns isError and does NOT advance.
  weave_pass: "enrich",
  close_pass: "enrich",
  validate_model: "validate",
  export_sysml: "render",
};

type ToolResult = { isError?: boolean; content?: unknown };

/** A tool result counts as clean iff it did not signal an error. */
function isClean(result: unknown): boolean {
  return !(result as ToolResult)?.isError;
}

/**
 * Wrap `server.tool` so registered handlers for lifecycle-mapped tools also
 * advance the session on success. The tool callback is always the LAST
 * argument across every `McpServer.tool` overload, so we decorate it in place
 * and forward the remaining arguments verbatim.
 */
function installSessionTracking(target: McpServer, session: SessionTracker): void {
  const originalTool = target.tool.bind(target) as (...args: unknown[]) => unknown;

  (target as unknown as { tool: (...args: unknown[]) => unknown }).tool = (
    ...args: unknown[]
  ) => {
    const name = args[0];
    const stage =
      typeof name === "string" ? LIFECYCLE_BY_TOOL[name] : undefined;

    if (stage === undefined || args.length < 2) {
      // Not a lifecycle tool (or an unexpected shape) — register untouched.
      return originalTool(...args);
    }

    const cbIndex = args.length - 1;
    const original = args[cbIndex];
    if (typeof original !== "function") {
      return originalTool(...args);
    }

    const wrapped = async (...cbArgs: unknown[]) => {
      const result = await (original as (...a: unknown[]) => unknown)(...cbArgs);

      // validate_model only advances to "validate" on a CLEAN run — i.e. the
      // tool returned findings with zero error-severity items AND no legacy
      // `issues`. Every other lifecycle tool advances whenever it did not
      // return isError.
      if (isClean(result) && shouldAdvance(name as string, result)) {
        try {
          await session.advance(stage);
        } catch (err) {
          // Backward / rejected transition — never fail the tool call.
          console.error(
            `[session] ${(err as Error).message} (tool ${String(name)})`
          );
        }
      }

      return result;
    };

    const forwarded = [...args];
    forwarded[cbIndex] = wrapped;
    return originalTool(...forwarded);
  };
}

/**
 * Extra gate for validate_model: only a clean validation advances the session.
 * A validation run is clean when its JSON payload reports no `issues` and no
 * error-severity `findings`. Any other tool always advances on a non-error
 * result.
 */
function shouldAdvance(name: string, result: unknown): boolean {
  if (name !== "validate_model") return true;

  const content = (result as ToolResult).content;
  if (!Array.isArray(content) || content.length === 0) return false;
  const first = content[0] as { text?: unknown };
  if (typeof first.text !== "string") return false;

  let parsed: {
    issues?: unknown;
    findings?: Array<{ severity?: unknown }>;
  };
  try {
    parsed = JSON.parse(first.text);
  } catch {
    return false;
  }

  const hasIssues = Array.isArray(parsed.issues) && parsed.issues.length > 0;
  const hasErrorFindings =
    Array.isArray(parsed.findings) &&
    parsed.findings.some((f) => f?.severity === "error");

  return !hasIssues && !hasErrorFindings;
}

// ---------------------------------------------------------------------------
// Server assembly
//
// Exposed as a factory so tests can build a fully-wired server (session
// tracking included) against a throwaway store + tracker, without triggering
// the stdio transport that `main()` starts.
// ---------------------------------------------------------------------------

export function buildServer(store: ModelStore, session: SessionTracker): McpServer {
  const server = new McpServer({
    name: "sysml-bridge",
    version: "0.1.0",
  });

  installSessionTracking(server, session);

  server.tool(
    "init_project",
    "Initialize or load a project. Must be called before using other tools.",
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
          const project = await store.createProject(name);
          return {
            content: [
              {
                type: "text" as const,
                text: JSON.stringify(
                  {
                    status: "created",
                    projectId: project["@id"],
                    branchId: store.branchId,
                  },
                  null,
                  2
                ),
              },
            ],
          };
        }

        const projects = await store.listProjects();
        const found = projects.find((p) => p.name === name);
        if (!found) {
          return {
            content: [{ type: "text" as const, text: `Project "${name}" not found` }],
            isError: true,
          };
        }

        const project = await store.loadProject(found["@id"]);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify(
                {
                  status: "loaded",
                  projectId: project["@id"],
                  branchId: store.branchId,
                  headCommitId: store.headCommitId,
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

  registerCreateElement(server, store);
  registerQueryElements(server, store);
  registerCreateRelationship(server, store);
  registerQueryRelationships(server, store);
  registerValidateModel(server, store);
  registerExportSysml(server, store);
  registerImportSysml(server, store);
  registerGetProjectState(server, store);
  registerUpdateElement(server, store);
  registerDeleteElement(server, store);
  registerWeavePass(server, store);
  registerClosePass(server, store);

  return server;
}

async function main() {
  const store: ModelStore = createStore();
  const session = new SessionTracker(MODEL_DIR);
  const server = buildServer(store, session);
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

// Only start the stdio server when run as the entry point. Importing this
// module (e.g. from a test that wants `buildServer`) must NOT connect a
// transport.
if (isEntryPoint()) {
  main().catch(console.error);
}

function isEntryPoint(): boolean {
  const invoked = process.argv[1];
  if (!invoked) return false;
  try {
    return import.meta.url === pathToFileURL(invoked).href;
  } catch {
    return false;
  }
}
