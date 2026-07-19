import { z } from "zod";
import { spawn } from "node:child_process";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";

// ---------------------------------------------------------------------------
// weave_pass — thin MCP cap over the W3 weave loop (spec §5, §8 W5).
//
// This tool is a THIN wrapper: it shells out to the exact W3 weave CLI
// (`scripts/weave.ts`), which owns all of the audit → queue → propose
// orchestration. Wrapping the CLI (rather than re-importing the pass runner
// into this package) is deliberate — it keeps the no-auto-approve source-scan
// ratchet's guarantees intact (the ratchet walks packages/<pkg>/src; the weave
// orchestration deliberately lives OUTSIDE package src) and means the MCP tool
// and `pnpm weave` run byte-identical code.
//
// LIFECYCLE MAPPING DECISION (documented here per the W5 brief):
//   A weave pass is gap-driven inference that PROPOSES additional links to
//   close audit gaps. It is neither a fresh `build` nor a `trace` — it does
//   not author model content directly (an open pass writes ZERO dispositions;
//   it only proposes to the review queue). It is an *enrichment* phase that
//   sits AFTER the initial build/trace and BEFORE validation. We therefore add
//   a dedicated `enrich` lifecycle stage (init → ingest → build → trace →
//   enrich → validate → render) rather than overloading `build`/`trace`, and
//   map BOTH weave tools to it. `validate_model`'s stage-advance semantics are
//   UNCHANGED — it still advances to `validate`, which now follows `enrich`.
//
// NO AUTO-APPROVE (spec §2): an open weave pass proposes candidates to the
// normal review queue and writes NO disposition. A human reviews/approves in
// the review UI; only then does `close_pass` recompose and re-audit.
// ---------------------------------------------------------------------------

const HERE = path.dirname(fileURLToPath(import.meta.url));
// dist/tools → dist → mcp-server → packages → repo root
const REPO_ROOT = path.resolve(HERE, "../../../..");
const WEAVE_SCRIPT = path.join(REPO_ROOT, "scripts", "weave.ts");
const TSX_BIN = path.join(REPO_ROOT, "node_modules", ".bin", "tsx");

export interface WeaveCliResult {
  code: number;
  /** Combined stderr+stdout log from the weave CLI (weave logs to stderr). */
  log: string;
}

/**
 * Run `scripts/weave.ts` as a subprocess and capture its exit code + log.
 * Exported so `close_pass` reuses the identical invocation path.
 */
export function runWeaveCli(
  project: string,
  extraArgs: string[],
): Promise<WeaveCliResult> {
  const args = [WEAVE_SCRIPT, "--project", path.resolve(project), ...extraArgs];
  return new Promise((resolve, reject) => {
    const child = spawn(TSX_BIN, args, {
      cwd: REPO_ROOT,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let log = "";
    child.stdout.on("data", (d: Buffer) => {
      log += d.toString();
    });
    child.stderr.on("data", (d: Buffer) => {
      log += d.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code: code ?? 1, log }));
  });
}

function optionalFlags(args: {
  budget_usd?: number;
  dry_run?: boolean;
  mock?: boolean;
}): string[] {
  const flags: string[] = [];
  if (typeof args.budget_usd === "number") flags.push("--budget", String(args.budget_usd));
  if (args.dry_run) flags.push("--dry-run");
  if (args.mock) flags.push("--mock");
  return flags;
}

export function registerWeavePass(server: McpServer, _store: ModelStore) {
  server.tool(
    "weave_pass",
    "Run one OPEN gap-driven weave pass over a project directory: audit → plan " +
      "gap queries → bounded targeted inference → propose candidates to the review " +
      "queue, then STOP. Writes NO disposition (no auto-approve — a human reviews " +
      "and approves before close_pass). Advances the session to the `enrich` " +
      "lifecycle stage (init → ingest → build → trace → enrich → validate → render).",
    {
      project: z
        .string()
        .describe(
          "Path to the weave project directory (expects <project>/model/<id>.json; " +
            "optional extracted.json / entities.json / mentions.json siblings)."
        ),
      budget_usd: z
        .number()
        .optional()
        .describe("Optional USD budget cap for targeted inference (logged when exceeded)."),
      dry_run: z
        .boolean()
        .optional()
        .describe("Plan queries and log intent without calling the provider."),
      mock: z
        .boolean()
        .optional()
        .describe(
          "Force the deterministic mock provider even when ANTHROPIC_API_KEY is set."
        ),
    },
    async ({ project, budget_usd, dry_run, mock }) => {
      try {
        const { code, log } = await runWeaveCli(project, optionalFlags({ budget_usd, dry_run, mock }));
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ pass: "open", exitCode: code, log }, null, 2),
            },
          ],
          isError: code !== 0,
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
