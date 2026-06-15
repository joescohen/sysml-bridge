/**
 * apply-human-dispositions.ts — Apply the HUMAN-approved dispositions for the 68
 * pending inference candidates.
 *
 * The HUMAN (Joe Cohen) approved explicit per-candidate disposition lists via the
 * mbse-infer gate (AskUserQuestion). This script applies them MECHANICALLY — it does
 * NOT re-judge. approvedBy is "Joe Cohen" because the human is the real approver here
 * (unlike the prior fixture-e2e-w3 fixture pass).
 *
 * Disposition lists below are keyed by id prefix; the full id is `infer-<prefix>…`.
 * Each prefix resolves to EXACTLY ONE queued record (verified before write).
 *
 *   APPROVE (29) → appendInferredApproval (approvedBy "Joe Cohen")
 *   REJECT  (34) → recordInferredRejection (reason stored verbatim here for audit;
 *                  the rejections file holds ids only — this script is the reason record)
 *   LEAVE   (5)  → untouched (remain queued for a later human pass)
 *
 * Run: pnpm tsx scripts/apply-human-dispositions.ts
 */
import { promises as fs } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  appendInferredApproval,
  recordInferredRejection,
  type InferenceCandidate,
} from "../packages/ir/src/inferred-approval-helpers.js";

const REPO_ROOT = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const CANDIDATES = join(REPO_ROOT, "examples/angars/model/inference-candidates.json");
const APPROVED = join(REPO_ROOT, "examples/angars/model/inferred-approved.json");
const REJECTIONS = join(REPO_ROOT, "examples/angars/model/inferred-rejections.json");
const APPROVED_BY = "Joe Cohen"; // the human approved via the mbse-infer gate

// --- APPROVE (29) — id prefixes by relation family --------------------------
const APPROVE: Record<string, string[]> = {
  allocation: [
    "0a02dc92", "0efec9cd", "b19ee37d", "24d0fb16", "24fc4249",
    "eb0fd07f", "05fac2aa", "2f7c7f1e", "471f2744", "748d97f1",
  ],
  controlJoin: [
    "0b46a1d6", "1f00d6c4", "27ac5eb4", "2da45c37", "438392d7",
    "54deac52", "66bcca2e", "66ee5214", "699b1fcd", "76e55d6d", "76f5a2a7",
  ],
  modeMembership: [
    "10f85a86", "116d9607", "12832365", "258b418d",
    "2a54e6de", "3587713f", "47bd4731", "6e8535d2",
  ],
};

// --- REJECT (34) — id prefix → verbatim human reason ------------------------
const REJECT: Record<string, string> = {
  // allocation
  "0e27f8a6": "haptic premises route to Haptic Alert Unit, wrong target",
  "2c235360": "console captures override input; panel is display sink",
  "326689c1": "sensor cited as actuator; actuation at pump/boom controller",
  "0685932e": "panel is alert sink; send role allocated to OCP",
  "0017af7b": "INS senses turbulence; stabilization is flight-control actuation",
  "47709710": "fusion consumes sensor outputs; engagement is ASCDPM control act",
  "5cf1a8dc": "already allocated to Data Storage & Logging; premises are display flows",
  "86a7ddc3": "display lives at HMI Panel (already allocated); OCP flows are transport",
  "8fe4c3f2": "same — already allocated to HMI Panel",
  "1d2f53e1": "console captures commands; execution is downstream",
  "1eae9434": "already allocated to boom controller; INS is sensing",
  "2a5cb54a": "radar senses; alignment actuated by boom/flight control",
  "2adc6376": "premises are internal nav-fusion flows, not aircraft telemetry reception",
  "3aa2c540": "GPS analysis belongs to GPS receiver/fusion, not INS",
  "422c2672": "proximity is LIDAR/radar domain, not GPS receiver",
  "126d17a9": "sensing supports alignment; actuation elsewhere",
  "9e67be4f": "premises are display/alert flows, nothing comms",
  // controlJoin
  "093fbbb9": "direction backwards: generation precedes dynamic updates",
  "3dee999d": "transitive shortcut would draw false bypass edge",
  "75391f17": "direction ambiguous vs containment ordering",
  "0a8e3c45": "cert validation is the mechanism of authentication; direction unsupported",
  "4971ea12": "ordering not stated; retraction is completion-stage",
  // modeMembership
  "3a6641e0": "compatibility validated at docking, not meet",
  "03bacfb2": "operator input is cross-mode; premise is only the mode itself",
  "0c11f98b": "alignment is contact-stage, not meet",
  "145d5811": "GPS analysis is continuous/GPS-denied, not handshake",
  "15a5a2e4": "prioritization belongs to Scheduling mode",
  "5b55b5ec": "already covered by greet + Docking memberships",
  "61ecf8c3": "no fueling at meet stage",
  "6326df9c": "schedule updates belong to Scheduling mode",
  "6565eab1": "comms is cross-mode; premises are standards reqs",
  "6915976a": "same — cross-mode",
  "1f032552": "boom adjustment too early at meet",
  "4d5bcb21": "premises are GPS-denied reqs — wrong mode",
};

// --- LEAVE PENDING (5) — recorded for clarity; not applied ------------------
const LEAVE = ["6961b93e", "7c84ed66", "3324bac9", "43423eae", "2e3af769"];

interface QueuedRecord {
  id: string;
  stage: string;
  relationFamily: string;
  sourceId: string;
  targetId: string;
  premises: string[];
  rationale: string;
  confidence: number;
  debate?: { verdict: string; advocate: number; challenger: number };
}

/** Resolve a prefix (+ optional expected family) to exactly one queued record. */
function resolveOne(
  prefix: string,
  queued: Map<string, QueuedRecord>,
  expectedFamily?: string,
): QueuedRecord {
  const full = `infer-${prefix}`;
  const matches = [...queued.values()].filter(
    (r) =>
      (r.id === full || r.id.startsWith(full)) &&
      (expectedFamily ? r.relationFamily === expectedFamily : true),
  );
  if (matches.length !== 1) {
    throw new Error(
      `prefix ${prefix}${expectedFamily ? ` (${expectedFamily})` : ""} resolved to ${matches.length} queued records — STOP`,
    );
  }
  return matches[0];
}

async function main(): Promise<void> {
  const file = JSON.parse(await fs.readFile(CANDIDATES, "utf8")) as {
    irHash: string;
    records: QueuedRecord[];
  };
  const runId = `run-${file.irHash}`;
  const queued = new Map(
    file.records.filter((r) => r.stage === "queued").map((r) => [r.id, r]),
  );

  console.log(
    `=== Applying human dispositions against ${queued.size} queued records (runId=${runId}, approvedBy="${APPROVED_BY}") ===\n`,
  );

  // Pre-resolve EVERYTHING first (fail fast before any write)
  const approveRecords: QueuedRecord[] = [];
  for (const [family, prefixes] of Object.entries(APPROVE)) {
    for (const p of prefixes) approveRecords.push(resolveOne(p, queued, family));
  }
  const rejectEntries: Array<{ rec: QueuedRecord; reason: string }> = [];
  for (const [p, reason] of Object.entries(REJECT)) {
    rejectEntries.push({ rec: resolveOne(p, queued), reason });
  }
  // Sanity: LEAVE prefixes resolve and are NOT in approve/reject
  const dispositioned = new Set([
    ...approveRecords.map((r) => r.id),
    ...rejectEntries.map((e) => e.rec.id),
  ]);
  for (const p of LEAVE) {
    const rec = resolveOne(p, queued);
    if (dispositioned.has(rec.id)) {
      throw new Error(`LEAVE prefix ${p} (${rec.id}) is also in approve/reject — STOP`);
    }
  }
  console.log(
    `Pre-resolved: ${approveRecords.length} approve, ${rejectEntries.length} reject, ${LEAVE.length} leave — all unique, no collisions.\n`,
  );

  // APPROVALS
  let approved = 0;
  for (const rec of approveRecords) {
    const candidate: InferenceCandidate = {
      id: rec.id,
      relationFamily: rec.relationFamily as InferenceCandidate["relationFamily"],
      sourceId: rec.sourceId,
      targetId: rec.targetId,
      premises: rec.premises,
      rationale: rec.rationale, // audit-only; never exported
      confidence: rec.confidence,
      inferenceRunId: runId,
      ...(rec.debate ? { debate: rec.debate as InferenceCandidate["debate"] } : {}),
    };
    await appendInferredApproval(candidate, APPROVED_BY, APPROVED, REJECTIONS);
    approved++;
    console.log(`APPROVE  ${rec.relationFamily.padEnd(15)} ${rec.id} conf=${rec.confidence}`);
  }

  // REJECTIONS (idempotent — already-rejected ids are safe no-ops)
  let rejected = 0;
  for (const { rec, reason } of rejectEntries) {
    await recordInferredRejection(rec.id, REJECTIONS);
    rejected++;
    console.log(`REJECT   ${rec.relationFamily.padEnd(15)} ${rec.id} conf=${rec.confidence}`);
    console.log(`         ${reason}`);
  }

  console.log(
    `\n=== summary: ${approved} approved, ${rejected} rejected, ${LEAVE.length} left queued ===`,
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
