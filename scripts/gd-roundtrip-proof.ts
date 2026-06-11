/**
 * gd-roundtrip-proof.ts — Produce gd-roundtrip.txt evidence for G-D closure.
 *
 * Demonstrates the full C6/C8 round-trip:
 *   1. Build a synthetic candidate
 *   2. appendApproval → ProseApprovedEntry
 *   3. Validate entry against ProseApprovedEntrySchema
 *   4. composeIR → approved id in approvedProseIds set
 *   5. Confirm the approved id could satisfy GATE03-unresolvable-provenance
 *
 * Output: /tmp/rubric-anchored-recursion/prose-ingest/evidence/gd-roundtrip.txt
 */

import { writeFile, readFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { appendApproval, type CandidateEntry } from "../packages/ir/src/approval-helpers.js";
import { ProseApprovedEntrySchema, composeIR } from "../packages/ir/src/prose-approved.js";
import { SCHEMA_VERSION } from "../packages/ir/src/schema.js";

const OUT_PATH = "/tmp/rubric-anchored-recursion/prose-ingest/evidence/gd-roundtrip.txt";

const MINIMAL_EXTRACTED = {
  schema_version: SCHEMA_VERSION,
  subsystem: "TestSub",
  needs: [{ id: "need-001", kind: "need", naturalKey: "N1", name: "Test Need" }],
  requirements: [
    {
      id: "requirement-abc",
      kind: "requirement",
      naturalKey: "CC-1",
      name: "Do Thing",
      statement: "The system shall do a thing.",
      needIds: ["need-001"],
    },
  ],
  functions: [
    { id: "function-xyz", kind: "function", naturalKey: "F1", name: "Func", level: "L1", owner: "TestSub" },
  ],
  components: [{ id: "component-111", kind: "component", naturalKey: "COMP-1", name: "Widget" }],
  satisfies: [{ reqId: "requirement-abc", functionId: "function-xyz" }],
  allocations: [{ functionId: "function-xyz", componentId: "component-111" }],
};

async function main() {
  const lines: string[] = [];
  const log = (s: string) => { lines.push(s); console.log(s); };

  log("G-D ROUND-TRIP PROOF");
  log("=====================");
  log(`Run at: ${new Date().toISOString()}`);
  log("");

  const dir = await mkdtemp(join(tmpdir(), "gd-roundtrip-"));
  try {
    const approvedPath = join(dir, "prose-approved.json");
    const rejectionsPath = join(dir, "prose-rejections.json");
    const extractedPath = join(dir, "extracted.json");
    await writeFile(extractedPath, JSON.stringify(MINIMAL_EXTRACTED));

    // STEP 1: synthetic candidate
    const candidate: CandidateEntry = {
      id: "candidate-roundtrip-001",
      kind: "requirement",
      fields: { naturalKey: "GD-1", name: "Round-Trip Req", statement: "The system shall round-trip." },
      citation: {
        docId: "synthetic-doc-gd",
        docSha256: "aabbcc00112233440000000000000000aabbcc00112233440000000000000000",
        chunkId: "chunk-gd-roundtrip-001",
        sectionPath: "3.1 Round-Trip Requirements",
        quote: "The system shall round-trip.",
      },
    };

    log("STEP 1 — Candidate:");
    log(`  id:       ${candidate.id}`);
    log(`  kind:     ${candidate.kind}`);
    log(`  chunkId:  ${candidate.citation.chunkId}`);
    log(`  quote:    "${candidate.citation.quote}"`);
    log("");

    // STEP 2: appendApproval
    const entry = await appendApproval(candidate, "proof-runner", approvedPath, rejectionsPath);

    log("STEP 2 — appendApproval result:");
    log(`  entry.id:          ${entry.id}`);
    log(`  entry.status:      ${entry.status}`);
    log(`  entry.approvedBy:  ${entry.approvedBy}`);
    log(`  entry.approvedAt:  ${entry.approvedAt}`);
    log(`  entry.candidateId: ${entry.candidateId}`);
    log("");

    // STEP 3: schema validation
    const schemaResult = ProseApprovedEntrySchema.safeParse(entry);
    log("STEP 3 — ProseApprovedEntrySchema.safeParse(entry):");
    log(`  success: ${schemaResult.success}`);
    if (!schemaResult.success) {
      log(`  ERRORS: ${JSON.stringify(schemaResult.error.issues)}`);
      throw new Error("Schema validation failed!");
    }
    log("");

    // Read back the file to verify it grew
    const raw = await readFile(approvedPath, "utf8");
    const parsed = JSON.parse(raw) as { entries: Array<{ id: string }> };
    log("STEP 4 — prose-approved.json on disk:");
    log(`  entries.length: ${parsed.entries.length}  (expected: 1)`);
    log(`  entries[0].id:  ${parsed.entries[0]?.id}`);
    log("");

    // STEP 5: composeIR
    const ir = await composeIR(extractedPath, approvedPath);
    log("STEP 5 — composeIR(extractedPath, approvedPath):");
    log(`  ir.proseEntries.length:            ${ir.proseEntries.length}  (expected: 1)`);
    log(`  ir.approvedProseIds.has(entry.id): ${ir.approvedProseIds.has(entry.id)}  (expected: true)`);
    log(`  ir.proseEntries[0].status:         ${ir.proseEntries[0]?.status}  (expected: approved)`);
    log("");

    // STEP 6: GATE03 tie-in
    log("STEP 6 — GATE03 provenance resolution:");
    log(`  If a model element has provenanceSourceId = "${entry.id}"`);
    log(`  and composeIR is called with this prose-approved.json,`);
    log(`  then ir.approvedProseIds.has("${entry.id}") = ${ir.approvedProseIds.has(entry.id)}`);
    log(`  => GATE03-unresolvable-provenance is SATISFIED.`);
    log("");

    // Assertions
    if (parsed.entries.length !== 1) throw new Error("File did not grow to 1 entry");
    if (!ir.approvedProseIds.has(entry.id)) throw new Error("id not in approvedProseIds");
    if (ir.proseEntries[0]?.status !== "approved") throw new Error("status is not approved");

    log("VERDICT: PASS — full C6/C8 round-trip demonstrated.");

  } finally {
    await rm(dir, { recursive: true, force: true });
  }

  await mkdir("/tmp/rubric-anchored-recursion/prose-ingest/evidence", { recursive: true });
  await writeFile(OUT_PATH, lines.join("\n") + "\n", "utf8");
  console.log(`\nWrote: ${OUT_PATH}`);
}

main().catch((err) => { console.error(err); process.exit(1); });
