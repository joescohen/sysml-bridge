/**
 * demo.ts — Tier-1 demo: corpus → IR → model → Gate 1 → Gate 2 → renders.
 * Deterministic; requires no API key. Any gate failure exits non-zero.
 */
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "../../..");
const OUT = path.join(ROOT, "examples/angars/out");

function run(title: string, cmd: string, args: string[]) {
  console.log(`\n=== ${title} ===`);
  execFileSync(cmd, args, { cwd: ROOT, stdio: "inherit" });
}

run("1/4 extract corpus → IR", "pnpm", ["demo:extract"]);
run("2/4 build model + Gate 1 + serialize + Gate 2", "pnpm", ["demo:build"]);
fs.mkdirSync(path.join(OUT, "renders"), { recursive: true });
run("3/4 render views", path.join(ROOT, "tools/viewer/render.sh"), [
  path.join(OUT, "angars.sysml"),
  path.join(OUT, "renders"),
  "--spec",
  path.join(ROOT, "examples/angars/views.json"),
  "--png",
]);
run("4/4 assert outputs", "pnpm", ["demo:assert"]);
console.log("\nDemo complete. Outputs in examples/angars/out/");
