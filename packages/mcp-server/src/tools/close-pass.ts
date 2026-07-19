import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ModelStore } from "@sysml-bridge/model";
import { runWeaveCli } from "./weave-pass.js";

// ---------------------------------------------------------------------------
// close_pass — thin MCP cap over `weave --close-pass` (spec §5, §8 W5).
//
// Closes the pending weave pass opened by `weave_pass`: recompose → re-audit →
// write <project>/passes/pass-NNN.json → enforce the HARD convergence gate.
// The gate FAILS (non-zero exit) when error findings increased or the pass
// ends with any error finding; this tool surfaces that as isError so the
// session does NOT advance on a failed convergence.
//
// This tool, like weave_pass, is a THIN wrapper over the exact W3 CLI
// (scripts/weave.ts --close-pass) — same rationale (keeps the no-auto-approve
// ratchet's src-only scan intact; MCP and `pnpm weave` run identical code).
//
// LIFECYCLE MAPPING (see weave-pass.ts for the full rationale): a weave pass is
// an *enrichment* phase between `trace` and `validate`. close_pass maps to the
// `enrich` stage on a successful (convergent) close. `validate_model`'s
// stage-advance semantics are UNCHANGED.
//
// NO AUTO-APPROVE (spec §2): close_pass reads human dispositions from
// <project>/dispositions/ and records them into the pass record; it never
// writes a disposition itself.
// ---------------------------------------------------------------------------

export function registerClosePass(server: McpServer, _store: ModelStore) {
  server.tool(
    "close_pass",
    "Close the pending weave pass over a project directory: recompose → re-audit " +
      "→ write passes/pass-NNN.json → enforce the HARD convergence gate (fails if " +
      "error findings increased or remain). Reads human dispositions from " +
      "<project>/dispositions/ but writes none itself. Advances the session to the " +
      "`enrich` lifecycle stage on a convergent close.",
    {
      project: z
        .string()
        .describe(
          "Path to the weave project directory that has a pending open pass " +
            "(passes/pending-pass.json, written by weave_pass)."
        ),
    },
    async ({ project }) => {
      try {
        const { code, log } = await runWeaveCli(project, ["--close-pass"]);
        return {
          content: [
            {
              type: "text" as const,
              text: JSON.stringify({ pass: "close", exitCode: code, log }, null, 2),
            },
          ],
          // Non-zero = convergence gate failed (or CLI error). Surface as an
          // error so lifecycle tracking does NOT advance on a failed close.
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
