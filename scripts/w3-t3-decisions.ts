/**
 * w3-t3-decisions.ts — Wave-3 T3: judgment review of queued inferred-allocation proposals.
 *
 * The integrator read the top-10 queued allocation proposals (by confidence) and the
 * top modeMembership proposals from inference-candidates.json, resolving every premise
 * id against the composed IR (requirement text, N2 flow endpoints/labels, components,
 * functions, prose modes) and reading the audit-only rationale. Decisions below approve
 * via appendInferredApproval (approvedBy "fixture-e2e-w3") ONLY where the premises
 * genuinely support the link; junk is rejected via recordInferredRejection. Proposals
 * not listed here remain queued (read-later, not junk).
 *
 * inferenceRunId: queued records carry no run id; we derive `run-<irHash>` from the
 * candidates file header — PROV-traceable to the exact inference run output.
 *
 * Run: pnpm tsx scripts/w3-t3-decisions.ts
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
const APPROVED_BY = "fixture-e2e-w3";

type Decision = { action: "approve"; reason: string } | { action: "reject"; reason: string };

/** Per-proposal judgments. Reasons reference premise ids only (no corpus quotes). */
const DECISIONS: Record<string, Decision> = {
  // ── APPROVALS — premises genuinely support function→component performance ──
  "infer-9e815104194f629e": {
    action: "approve",
    reason:
      "Display-update function on the display component: requirement-0e41a37bfdd150ac + requirement-ac18632551d3b670 mandate display/update rates; n2-240cf…/2b2f…/353a…/bc75… are display in-flows terminating at the target.",
  },
  "infer-03dd646f4f7e3b5a": {
    action: "approve",
    reason:
      "Mission-data display on the display component: requirement-0b901460aa322dfb mandates real-time mission display; same four N2 display in-flows terminate at the target.",
  },
  "infer-50c9ab07aeef239b": {
    action: "approve",
    reason:
      "Operator-input reception on the console: n2-977fb2813fe838eb (external→target: Operator Commands) is precisely this function's interface.",
  },
  "infer-093f04743b54200c": {
    action: "approve",
    reason:
      "Logging function on the logging unit: requirement-af535729d43de097 mandates audit logging; n2-0105148a2a80e141/n2-df27baf534260ddf show Stored Data flows through the target.",
  },
  "infer-0f7b51576a8b0d7d": {
    action: "approve",
    reason:
      "Notification-sending on the alert hub: requirement-7227c0f5ceb810d8 mandates operator notification; five N2 OUT-flows from the target (to display + haptic sinks) show it is the alert source.",
  },
  "infer-cc6f49e52b9d47ec": {
    action: "approve",
    reason:
      "Boom adjustment on the boom controller: requirement-efc8fdab05c98560 + requirement-407a3a584abb3562 are exactly boom-length/altitude adjustment; n2-faf9ca…/79c173… show the target's control interfaces.",
  },
  // modeMembership (clearly supported)
  "infer-3d2e1051f1008799": {
    action: "approve",
    reason:
      "Probe retraction is in-mode for Docking and Fueling: requirement-08ae046bf2cce65c ties retraction to refueling completion, which occurs within prose-7b808179ddbec422.",
  },
  "infer-171c79578b37a9ec": {
    action: "approve",
    reason:
      "Proximity monitoring is in-mode for greet: requirement-61246fbce9f75742 mandates proximity-bound aborts; prose-a0ae8cb5f1f2cd0b is the close-proximity alignment substage.",
  },

  // ── REJECTIONS — premises do not support the stated target ──
  "infer-0e27f8a6f5957fb3": {
    action: "reject",
    reason:
      "Premise reqs (requirement-15eba9d3428ddf42, requirement-9adb050e723e224c) are tactile/haptic feedback, which N2 flows route to the dedicated Haptic Alert Unit — wrong target component.",
  },
  "infer-326689c186033110": {
    action: "reject",
    reason:
      "Target is a sensor; N2 premises (n2-38258089c9ad0477, n2-c39deb2bd9730eca) are sense-signal flows — flow ADJUSTMENT actuation resides with Fuel Pump/Refueling Boom Controller, not the meter.",
  },
  "infer-0685932ec1f5fe4f": {
    action: "reject",
    reason:
      "N2 premises (n2-2b2f3799a3d913fd, n2-353a4602ddbba402) show the target as alert SINK; the sending role belongs to Operator Control Plane (approved as infer-0f7b51576a8b0d7d).",
  },
  "infer-2c23536006e0eaae": {
    action: "reject",
    reason:
      "Premise reqs (requirement-b3b04188490ecaa9, requirement-f38aca42efea67cf) establish override as HMI-domain but do not settle the display panel as the override PROCESSOR — override input arrives via the console per N2 Operator Commands.",
  },
};

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

async function main(): Promise<void> {
  const file = JSON.parse(await fs.readFile(CANDIDATES, "utf8")) as {
    irHash: string;
    records: QueuedRecord[];
  };
  const runId = `run-${file.irHash}`;
  const queued = new Map(
    file.records.filter((r) => r.stage === "queued").map((r) => [r.id, r]),
  );

  console.log(`=== T3 decisions against ${queued.size} queued proposals (runId=${runId}) ===\n`);

  let approved = 0;
  let rejected = 0;
  for (const [id, decision] of Object.entries(DECISIONS)) {
    const rec = queued.get(id);
    if (!rec) {
      console.error(`!! ${id} not found among queued records — STOP`);
      process.exit(1);
    }
    if (decision.action === "approve") {
      const candidate: InferenceCandidate = {
        id: rec.id,
        relationFamily: rec.relationFamily as InferenceCandidate["relationFamily"],
        sourceId: rec.sourceId,
        targetId: rec.targetId,
        premises: rec.premises,
        rationale: rec.rationale, // audit-only; never exported
        confidence: rec.confidence,
        inferenceRunId: runId,
        ...(rec.debate
          ? { debate: rec.debate as InferenceCandidate["debate"] }
          : {}),
      };
      await appendInferredApproval(candidate, APPROVED_BY, APPROVED, REJECTIONS);
      approved++;
      console.log(`APPROVE  ${rec.relationFamily.padEnd(15)} ${id} conf=${rec.confidence}`);
      console.log(`         ${decision.reason}`);
    } else {
      await recordInferredRejection(id, REJECTIONS);
      rejected++;
      console.log(`REJECT   ${rec.relationFamily.padEnd(15)} ${id} conf=${rec.confidence}`);
      console.log(`         ${decision.reason}`);
    }
  }

  const remaining = queued.size - approved - rejected;
  console.log(
    `\n=== T3 summary: ${approved} approved, ${rejected} rejected, ${remaining} left queued (unreviewed — pending human pass) ===`,
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
