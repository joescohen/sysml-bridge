/**
 * check-state-endpoints.ts
 *
 * State-transition endpoint regression guard for the notation-parity loop.
 *
 * Rubric S6 requires every `transition first X then Y;` in a state machine to
 * render as an arrow with BOTH endpoints present. A regression that dropped a
 * transition's source or target would still produce a plausible-looking diagram
 * (one fewer arrow among many) — the kind of silent loss a screenshot review
 * misses. This guard closes that gap deterministically:
 *
 *   1. render `probes/state-machine-control.sysml` with the exporter's `--stats`
 *      flag, which prints one `STATS ... connector_routes=N ...` line per view.
 *      A `connector_route` is a drawn polyline linking two boxes, so it exists
 *      ONLY when both of a transition's endpoints resolved to placed nodes;
 *   2. count the `transition first ... then ...;` statements in the probe source;
 *   3. assert the two counts are equal.
 *
 * If a future viewer change drops an endpoint (route not built) or the parser
 * stops resolving a transition, `connector_routes` falls below the transition
 * count and this exits non-zero. Both signals are derived from live artifacts —
 * the rendered layout and the probe text — never a hardcoded number.
 *
 * Usage: tsx scripts/check-state-endpoints.ts   (run by `pnpm check:parity`)
 */

import { spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const BINARY = path.join(REPO_ROOT, "tools/viewer/target/release/export_figures");
const PROBE = path.join(REPO_ROOT, "probes/state-machine-control.sysml");
const SPEC = path.join(REPO_ROOT, "probes/views/state-machine-control.json");

function fail(msg: string): never {
  console.error(`check-state-endpoints — FAIL: ${msg}`);
  process.exit(1);
}

if (!fs.existsSync(BINARY)) {
  fail(
    `viewer binary not built at ${path.relative(REPO_ROOT, BINARY)}. ` +
      `Build it first: (cd tools/viewer && cargo build --release --bin export_figures)`,
  );
}
if (!fs.existsSync(PROBE)) fail(`state probe not found at ${path.relative(REPO_ROOT, PROBE)}`);

// (2) Count transitions in the probe source (the ground-truth expectation).
const src = fs.readFileSync(PROBE, "utf8");
const transitionCount = (src.match(/\btransition\s+first\b/g) ?? []).length;
if (transitionCount === 0) {
  fail(`no \`transition first ...\` statements found in ${path.relative(REPO_ROOT, PROBE)}`);
}

// (1) Render with --stats; STATS lines go to stderr. spawnSync captures stderr
// regardless of exit code.
const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "state-endpoints-"));
try {
  const r = spawnSync(BINARY, [PROBE, outDir, "--spec", SPEC, "--stats"], { encoding: "utf8" });
  const stderr = r.stderr ?? "";
  const statsLine = stderr.split("\n").find((l) => l.includes("kind=StateTransition"));
  if (!statsLine) {
    fail(`no StateTransition STATS line in exporter output:\n${stderr}\n${r.stdout ?? ""}`);
  }
  const m = statsLine.match(/connector_routes=(\d+)/);
  if (!m) fail(`could not parse connector_routes from STATS line: ${statsLine}`);
  const drawnRoutes = Number(m[1]);

  // (3) Assert every transition drew a two-endpoint route.
  if (drawnRoutes !== transitionCount) {
    fail(
      `state probe has ${transitionCount} \`transition first\` statement(s) but the render ` +
        `drew ${drawnRoutes} transition route(s). A transition lost an endpoint (S6 regression).`,
    );
  }

  console.log(
    `check-state-endpoints — OK: ${transitionCount} transition(s), ${drawnRoutes} drawn route(s) ` +
      `(every transition renders with both endpoints).`,
  );
} finally {
  fs.rmSync(outDir, { recursive: true, force: true });
}
