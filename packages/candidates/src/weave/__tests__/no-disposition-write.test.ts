/**
 * no-disposition-write.test.ts — weave PROPOSES, never disposes (spec §2, §8 W3).
 *
 * The repo-wide no-auto-approve ratchet (packages/candidates/src/__tests__/
 * no-auto-approve.test.ts) already scans every packages/<pkg>/src for the
 * approval writers, so the weave MODULE is covered there. This test adds the
 * explicit W3 proof that neither the weave module NOR its CLI orchestration
 * (scripts/weave.ts, which the ratchet does not scan) calls any disposition
 * writer — weave is not an allowlisted writer and never becomes one.
 */
import { describe, it, expect } from "vitest";
import { promises as fsp } from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../../../..");
const WEAVE_DIR = path.resolve(HERE, "..");

/** Every function that writes a human disposition to disk. */
const DISPOSITION_WRITERS = [
  "appendApproval",
  "appendInferredApproval",
  "appendEntityMerge",
  "recordRejection",
  "recordInferredRejection",
  "recordEntityRejection",
];

async function sourceFiles(dir: string): Promise<string[]> {
  const out: string[] = [];
  for (const entry of await fsp.readdir(dir, { withFileTypes: true })) {
    if (entry.name === "__tests__" || entry.name === "node_modules") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await sourceFiles(full)));
    else if (entry.name.endsWith(".ts")) out.push(full);
  }
  return out;
}

describe("weave writes no disposition", () => {
  it("neither the weave module nor scripts/weave.ts calls any disposition writer", async () => {
    const files = [...(await sourceFiles(WEAVE_DIR)), path.join(REPO_ROOT, "scripts/weave.ts")];
    const offenders: string[] = [];
    for (const file of files) {
      const lines = (await fsp.readFile(file, "utf8")).split("\n");
      for (const [i, line] of lines.entries()) {
        // Ignore comments — we are looking for CALL SITES, not prose.
        const code = line.replace(/\/\/.*$/, "").replace(/\/\*.*?\*\//g, "");
        for (const token of DISPOSITION_WRITERS) {
          if (new RegExp(`\\b${token}\\s*\\(`).test(code)) {
            offenders.push(`${path.relative(REPO_ROOT, file)}:${i + 1}: ${line.trim()}`);
          }
        }
      }
    }
    expect(offenders, `weave must not call a disposition writer:\n${offenders.join("\n")}`).toEqual(
      [],
    );
  });
});
