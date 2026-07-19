/** assert-demo.ts — machine checks for spec §8 Phase 1. Exit non-zero on any failure. */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const out = (...p: string[]) => path.join(ROOT, "examples/angars/out", ...p);
const fail = (msg: string): never => { console.error(`DEMO ASSERT FAIL: ${msg}`); process.exit(1); };

if (!fs.existsSync(out("angars.sysml"))) fail("angars.sysml missing");

const audit = JSON.parse(fs.readFileSync(out("audit.json"), "utf8"));
if (audit.findings.length !== 0) fail(`Gate 1 findings present: ${audit.findings.length}`);

const baseline = JSON.parse(
  fs.readFileSync(path.join(ROOT, "examples/angars/fidelity-baseline.json"), "utf8")
);
if (audit.fidelity.matched < baseline.matched || audit.fidelity.tracePairs < baseline.tracePairs)
  fail(`fidelity ${audit.fidelity.matched}/${audit.fidelity.tracePairs} below baseline ${baseline.matched}/${baseline.tracePairs}`);

const renders = fs.existsSync(out("renders"))
  ? fs.readdirSync(out("renders")).filter((f) => f.endsWith(".pdf"))
  : [];
if (renders.length < 5) fail(`expected ≥5 rendered PDFs, got ${renders.length}`);
for (const f of renders)
  if (fs.statSync(out("renders", f)).size === 0) fail(`zero-byte render: ${f}`);

console.log(`demo:assert PASS — findings 0, fidelity ${audit.fidelity.matched}/${audit.fidelity.tracePairs}, renders ${renders.length}`);
