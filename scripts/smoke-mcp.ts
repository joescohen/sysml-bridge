/**
 * smoke-mcp.ts — stdio smoke client for the built MCP server.
 *
 * Spawns `node packages/mcp-server/dist/index.js` over stdio (the exact wiring
 * .mcp.json gives Claude Code), drives a full lifecycle against a throwaway
 * project dir, and exits non-zero on any failure:
 *
 *   init_project → create_element ×2 → create_relationship → weave_pass
 *   → validate_model → export_sysml → get_project_state
 *
 * The MCP model dir is <project>/model, which is exactly the layout the W3
 * weave loop expects (a FileModel JSON under <project>/model), so after the
 * model is built we exercise the W5 weave_pass tool against <project> itself:
 * it runs an OPEN gap-driven pass (audit → propose to the review queue → STOP),
 * writing NO disposition (no auto-approve).
 *
 * Assertions: every call non-isError; weave_pass opens a pass and proposes to
 * the review queue; the exported SysML contains the created part;
 * <modelDir>/.mbse/session.json ends at "render".
 *
 * Uses the committed ANGARS corpus for GATE-05 provenance resolution so the
 * smoke exercises the REAL write gate (no allow_invalid bypass): "C&C" is a
 * resolvable provenance id (subsystem), and component names resolve too.
 */
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function fail(msg: string): never {
  console.error(`SMOKE FAIL: ${msg}`);
  process.exit(1);
}

async function main(): Promise<void> {
  // The weave loop expects a project dir with a FileModel under <project>/model,
  // so point the MCP server's model dir at <project>/model. The MCP-built model
  // then IS a valid weave project we can run weave_pass against.
  const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), "foundry-smoke-"));
  const modelDir = path.join(projectDir, "model");
  fs.mkdirSync(modelDir, { recursive: true });

  const transport = new StdioClientTransport({
    command: "node",
    args: [path.join(ROOT, "packages/mcp-server/dist/index.js")],
    env: {
      ...process.env,
      SYSML_FOUNDRY_MODEL_DIR: modelDir,
      SYSML_BRIDGE_CORPUS_PATH: path.join(ROOT, "examples/angars/extracted.json"),
    },
  });
  const client = new Client({ name: "smoke-mcp", version: "0.1.0" });
  await client.connect(transport);

  const call = async (name: string, args: Record<string, unknown>) => {
    const res = (await client.callTool({ name, arguments: args })) as {
      isError?: boolean;
      content: Array<{ text: string }>;
    };
    if (res.isError) fail(`${name} returned isError: ${res.content?.[0]?.text?.slice(0, 300)}`);
    console.log(`ok ${name}`);
    return res.content[0]?.text ?? "";
  };

  await call("init_project", { name: "Smoke Project", create: true });

  // Provenance ids resolve against the committed ANGARS corpus
  // (resolution set = id ∪ naturalKey ∪ name; "C&C Power Module" is a component name).
  const partText = await call("create_element", {
    type: "PartUsage",
    name: "SmokePart",
    attributes: { provenanceSourceId: "C&C Power Module" },
  });
  const part = JSON.parse(partText);
  const partId: string = part.element?.id ?? part.id;
  if (!partId) fail("create_element returned no id");

  const reqText = await call("create_element", {
    type: "RequirementUsage",
    name: "SmokeReq",
    attributes: { provenanceSourceId: "C&C Power Module" },
  });
  const req = JSON.parse(reqText);
  const reqId: string = req.element?.id ?? req.id;

  await call("create_relationship", {
    type: "SatisfyRequirementUsage",
    source_id: partId,
    target_id: reqId,
  });

  // W5 weave tool: run an OPEN gap-driven pass over the project dir. Uses the
  // deterministic mock provider (no ANTHROPIC_API_KEY needed) and proposes to
  // the review queue without writing any disposition.
  const weaveText = await call("weave_pass", { project: projectDir, mock: true });
  const weave = JSON.parse(weaveText) as { pass?: string; exitCode?: number; log?: string };
  if (weave.exitCode !== 0) fail(`weave_pass exited ${weave.exitCode}: ${weave.log?.slice(0, 300)}`);
  if (!/pass \d+ OPEN/.test(weave.log ?? "")) {
    fail(`weave_pass did not report an open pass; log: ${weave.log?.slice(0, 300)}`);
  }
  const candidatesFile = path.join(projectDir, "candidates", "inference-candidates.json");
  if (!fs.existsSync(candidatesFile)) fail(`weave_pass wrote no review queue at ${candidatesFile}`);
  console.log(`ok weave_pass opened a pass, proposed to ${candidatesFile}`);

  await call("validate_model", {});

  const sysml = await call("export_sysml", {});
  if (!sysml.includes("SmokePart")) fail("export_sysml output does not contain SmokePart");

  const state = await call("get_project_state", {});
  if (!state.includes("Smoke Project") && !state.includes("smoke-project")) {
    fail("get_project_state does not mention the project");
  }

  const sessionFile = path.join(modelDir, ".mbse", "session.json");
  if (!fs.existsSync(sessionFile)) fail(`session file missing at ${sessionFile}`);
  const session = JSON.parse(fs.readFileSync(sessionFile, "utf8"));
  if (session.state !== "render") fail(`session state is '${session.state}', expected 'render'`);

  console.log(`ok session.json state=render (${sessionFile})`);
  console.log("SMOKE PASS — full lifecycle over stdio (incl. weave_pass)");
  await client.close();
  fs.rmSync(projectDir, { recursive: true, force: true });
}

main().catch((e) => {
  console.error("SMOKE FAIL (exception):", e);
  process.exit(1);
});
