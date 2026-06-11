/**
 * gd-reject-proof.ts — Produce gd-reject.txt evidence for G-D closure.
 *
 * Demonstrates:
 *   1. recordRejection → persisted to prose-rejections.json
 *   2. isRejected → returns true for the rejected id
 *   3. Re-ingest loop: candidate skipped because isRejected = true
 *
 * Output: /tmp/rubric-anchored-recursion/prose-ingest/evidence/gd-reject.txt
 */

import { writeFile, readFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { recordRejection, isRejected, isApproved, type CandidateEntry } from "../packages/ir/src/approval-helpers.js";

const OUT_PATH = "/tmp/rubric-anchored-recursion/prose-ingest/evidence/gd-reject.txt";

async function main() {
  const lines: string[] = [];
  const log = (s: string) => { lines.push(s); console.log(s); };

  log("G-D REJECTION PROOF");
  log("====================");
  log(`Run at: ${new Date().toISOString()}`);
  log("");

  const dir = await mkdtemp(join(tmpdir(), "gd-reject-"));
  try {
    const approvedPath = join(dir, "prose-approved.json");
    const rejectionsPath = join(dir, "prose-rejections.json");

    const candidateId = "candidate-reject-gd-001";

    log("STEP 1 — recordRejection:");
    await recordRejection(candidateId, rejectionsPath);
    log(`  recordRejection("${candidateId}", rejectionsPath)  ← called`);
    log("");

    // Read back
    const raw = await readFile(rejectionsPath, "utf8");
    const parsed = JSON.parse(raw) as { rejectedIds: string[] };
    log("STEP 2 — prose-rejections.json on disk:");
    log(`  rejectedIds: ${JSON.stringify(parsed.rejectedIds)}`);
    log(`  contains "${candidateId}": ${parsed.rejectedIds.includes(candidateId)}`);
    log("");

    // isRejected
    const rejected = await isRejected(candidateId, rejectionsPath);
    log("STEP 3 — isRejected predicate:");
    log(`  isRejected("${candidateId}") = ${rejected}  (expected: true)`);
    log("");

    // isApproved returns false (never approved)
    const approved = await isApproved(candidateId, approvedPath);
    log("STEP 4 — isApproved predicate (file doesn't exist):");
    log(`  isApproved("${candidateId}") = ${approved}  (expected: false)`);
    log("");

    // Simulate re-ingest skip logic
    log("STEP 5 — Re-ingest skip simulation:");
    const candidates: CandidateEntry[] = [
      {
        id: candidateId,
        kind: "requirement",
        fields: { naturalKey: "GD-REJECT-1" },
        citation: {
          docId: "synthetic-doc", docSha256: "aa".repeat(32),
          chunkId: "chunk-001", sectionPath: "3.1", quote: "rejected candidate",
        },
      },
      {
        id: "candidate-pending-002",
        kind: "requirement",
        fields: { naturalKey: "GD-PENDING-2" },
        citation: {
          docId: "synthetic-doc", docSha256: "aa".repeat(32),
          chunkId: "chunk-002", sectionPath: "3.1", quote: "pending candidate",
        },
      },
    ];

    const pending: string[] = [];
    for (const c of candidates) {
      const skip =
        (await isApproved(c.id, approvedPath)) ||
        (await isRejected(c.id, rejectionsPath));
      log(`  candidate ${c.id}: skip=${skip}`);
      if (!skip) pending.push(c.id);
    }
    log(`  pending after skip: ${JSON.stringify(pending)}`);
    log("  (only candidate-pending-002 reaches the human gate)");
    log("");

    // Idempotence check
    await recordRejection(candidateId, rejectionsPath);
    const rawAfter = await readFile(rejectionsPath, "utf8");
    const parsedAfter = JSON.parse(rawAfter) as { rejectedIds: string[] };
    const dupCount = parsedAfter.rejectedIds.filter((id) => id === candidateId).length;
    log("STEP 6 — Idempotence (record same id twice):");
    log(`  count of "${candidateId}" in rejectedIds after 2nd record: ${dupCount}  (expected: 1)`);
    log("");

    // Assertions
    if (!rejected) throw new Error("isRejected returned false");
    if (approved) throw new Error("isApproved returned true for never-approved id");
    if (!parsed.rejectedIds.includes(candidateId)) throw new Error("id not in rejectedIds");
    if (pending.length !== 1 || pending[0] !== "candidate-pending-002") throw new Error("skip logic wrong");
    if (dupCount !== 1) throw new Error("idempotence failed");

    log("VERDICT: PASS — rejection persisted, re-ingest skips rejected ids, idempotent.");

  } finally {
    await rm(dir, { recursive: true, force: true });
  }

  await mkdir("/tmp/rubric-anchored-recursion/prose-ingest/evidence", { recursive: true });
  await writeFile(OUT_PATH, lines.join("\n") + "\n", "utf8");
  console.log(`\nWrote: ${OUT_PATH}`);
}

main().catch((err) => { console.error(err); process.exit(1); });
