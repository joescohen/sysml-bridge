/**
 * assert-weave-model.ts — machine checks for the weave-mini Cameo-import
 * artifact (docs/verification/cameo-import-runbook.md). Exit non-zero on any
 * failure. Mirrors examples/angars/pipeline/assert-demo.ts's style.
 *
 * This does NOT read a stale audit.json off disk — it RE-DERIVES the model
 * from the committed corpus + fixtures by calling the SAME `generate()` the
 * builder's CLI entry point calls (ingest -> build -> serialize -> Gate 1 ->
 * Gate 2, throwing on any gate failure), then additionally asserts:
 *
 *   1. Both gates passed (generate() didn't throw -- 0 audit errors, grammar-
 *      valid).
 *   2. The freshly-regenerated .sysml text is BYTE-IDENTICAL to the
 *      COMMITTED examples/weave-mini/weave-model.sysml -- so if someone edits
 *      the committed artifact by hand (or the corpus/fixtures/generator drift
 *      out of sync) without regenerating, this fails loudly instead of the
 *      artifact silently rotting.
 *   3. The containment-fed allocation chain resolved to the SAME id pinned in
 *      examples/weave-mini/answer-key.json's `chain.id` — an independent
 *      cross-check against the corpus's own recorded ground truth.
 *
 * Usage:
 *   pnpm tsx examples/weave-mini/assert-weave-model.ts
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { generate } from "./build-weave-model.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const COMMITTED_SYSML = path.join(HERE, "weave-model.sysml");
const ANSWER_KEY = path.join(HERE, "answer-key.json");

const fail = (msg: string): never => {
  console.error(`ASSERT-WEAVE-MODEL FAIL: ${msg}`);
  process.exit(1);
};

async function main(): Promise<void> {
  if (!fs.existsSync(COMMITTED_SYSML)) {
    fail(`committed artifact missing: ${COMMITTED_SYSML} (run: pnpm tsx examples/weave-mini/build-weave-model.ts)`);
  }
  const before = fs.readFileSync(COMMITTED_SYSML, "utf8");

  // generate() throws on ANY gate failure (Gate 1 audit errors, Gate 2
  // grammar errors) — reaching the line after this call IS the "both gates
  // pass" assertion.
  const result = await generate();

  const errorFindings = result.auditResult.findings.filter((f) => f.severity === "error");
  if (errorFindings.length !== 0) {
    // Unreachable in practice (generate() would have thrown first) — kept as
    // an explicit, self-contained assertion so this script never silently
    // trusts generate()'s internal throw alone.
    fail(`Gate 1 reported ${errorFindings.length} error finding(s) after generate() returned`);
  }

  const after = fs.readFileSync(COMMITTED_SYSML, "utf8");
  if (before !== after) {
    fail(
      `committed weave-model.sysml was NOT up to date before this run — regeneration changed it. ` +
        `Commit the regenerated file.`
    );
  }
  if (result.sysmlText !== after) {
    fail(`generate()'s in-memory sysmlText does not match what it wrote to disk (should be identical).`);
  }

  if (!fs.existsSync(ANSWER_KEY)) {
    fail(`answer-key.json missing: ${ANSWER_KEY}`);
  }
  const answerKey = JSON.parse(fs.readFileSync(ANSWER_KEY, "utf8")) as {
    chain: { id: string };
  };
  if (result.chainCandidate.id !== answerKey.chain.id) {
    fail(
      `chain id mismatch: generator produced ${result.chainCandidate.id}, ` +
        `answer-key.json pins ${answerKey.chain.id}`
    );
  }

  console.log(
    `assert-weave-model PASS — Gate 1: 0 errors, Gate 2: grammar-valid, ` +
      `artifact byte-identical, chain=${result.chainCandidate.id} matches answer-key.json`
  );
}

main().catch((err) => {
  console.error("ASSERT-WEAVE-MODEL FAIL (exception):", err);
  process.exit(1);
});
