#!/usr/bin/env tsx
/**
 * w3-approve-prose.ts — Wave-3 T2 fixture-approval of control-flow + mode candidates.
 *
 * Reviewed by the Wave-3 integrator (see report). Approves ONLY candidates whose
 * quote genuinely states the flow/mode AND whose names resolve against the IR /
 * the approved-mode set. approvedBy = "fixture-e2e-w3".
 *
 * Approved (well-formed, genuinely-cited, name-resolvable docking substage chain
 * from CONOPS §3.3 scenario text):
 *   - mode: meet, greet, handshake            (each quote names it as a docking substage)
 *   - modeTransition: meet→greet, greet→handshake  (each quote states the trigger)
 *
 * Rejected (recorded as rejections with one-line reason):
 *   - 3 successions   (owningFunction "CONOPS Derivation" = authors' methodology,
 *                      not an ANGARS L3 function; actions don't match L3 names)
 *   - 1 parallel      (owningFunction "Approach and Docking" + branchActions are
 *                      descriptive prose, not L3 action names — unresolvable)
 *   - 1 modeTransition Normal Operations→GPS-denied mode (neither endpoint name
 *                      matches an approved mode — name-unresolvable)
 *
 * Appends to prose-approved-modes.json (the Pillar-6 input).
 */

import { readFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { appendApproval, recordRejection, type CandidateEntry } from "@sysml-bridge/ir";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const CANDIDATES = join(REPO_ROOT, "examples/angars/model/prose-candidates.json");
const APPROVED = join(REPO_ROOT, "examples/angars/model/prose-approved-modes.json");
const REJECTIONS = join(REPO_ROOT, "examples/angars/model/prose-rejections.json");

const APPROVED_BY = "fixture-e2e-w3";

// Candidate ids I reviewed and approved (resolvable docking-substage chain).
const APPROVE_IDS = new Set<string>([
  "prose-candidate-1f6c166f492f23a1", // mode: meet
  "prose-candidate-f6e6df4287b696c2", // mode: greet
  "prose-candidate-13ba4b94055724fc", // mode: handshake
  "prose-candidate-9c78a3b2ee330f57", // modeTransition: meet -> greet
  "prose-candidate-17e3b6af5a0b1589", // modeTransition: greet -> handshake
]);

// Candidate ids I reviewed and rejected, with reason.
const REJECT: Array<{ id: string; reason: string }> = [
  { id: "prose-candidate-018b26754a90acac", reason: "succession x3 (same id): owningFunction 'CONOPS Derivation' is the authors' derivation methodology, not an ANGARS L3 function; actions unresolvable" },
  { id: "prose-candidate-5e5a2369598e6204", reason: "parallel: owningFunction 'Approach and Docking' + branchActions are descriptive prose, not L3 action names — unresolvable" },
  { id: "prose-candidate-50ed100d497b6659", reason: "modeTransition Normal Operations->GPS-denied mode: neither endpoint name matches an approved mode — name-unresolvable" },
];

interface CandidatesFile {
  candidates: Array<{
    id: string;
    kind: CandidateEntry["kind"];
    fields: Record<string, unknown>;
    citation: CandidateEntry["citation"];
  }>;
}

async function main(): Promise<void> {
  const file: CandidatesFile = JSON.parse(await readFile(CANDIDATES, "utf8"));
  const byId = new Map(file.candidates.map((c) => [c.id, c]));

  let approved = 0;
  for (const id of APPROVE_IDS) {
    const cand = byId.get(id);
    if (!cand) {
      console.error(`MISSING candidate id (cannot approve): ${id}`);
      process.exitCode = 1;
      continue;
    }
    const entry: CandidateEntry = {
      id: cand.id,
      kind: cand.kind,
      fields: cand.fields,
      citation: cand.citation,
    };
    const result = await appendApproval(entry, APPROVED_BY, APPROVED, REJECTIONS);
    approved++;
    console.log(`APPROVED ${cand.kind.padEnd(15)} ${cand.id} -> entry ${result.id} | ${JSON.stringify(cand.fields)}`);
  }

  let rejected = 0;
  for (const { id, reason } of REJECT) {
    await recordRejection(id, REJECTIONS);
    rejected++;
    console.log(`REJECTED ${id} | ${reason}`);
  }

  console.log(`\n[T2] approved=${approved} rejected=${rejected} approvedBy=${APPROVED_BY}`);
  console.log(`[T2] approved file: ${APPROVED}`);
  console.log(`[T2] rejections file: ${REJECTIONS}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
