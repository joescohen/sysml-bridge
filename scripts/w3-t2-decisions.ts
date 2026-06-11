/**
 * w3-t2-decisions.ts — Wave-3 T2: judgment review of prose control-flow + mode candidates.
 *
 * Reviews every succession / parallel / decision / modeTransition / mode candidate in
 * prose-candidates.json. Approves via appendApproval (approvedBy "fixture-e2e-w3") ONLY if
 * the quote genuinely states it AND owningFunction/action names resolve against the IR's L3
 * action names, deduped against already-approved modes. Rejects the rest via recordRejection
 * with a one-line reason (logged to stdout).
 *
 * Run: pnpm tsx scripts/w3-t2-decisions.ts
 */
import { promises as fs } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  appendApproval,
  recordRejection,
  type CandidateEntry,
} from "../packages/ir/src/approval-helpers.js";

const REPO_ROOT = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const CANDIDATES = join(REPO_ROOT, "examples/angars/model/prose-candidates.json");
const APPROVED = join(REPO_ROOT, "examples/angars/model/prose-approved-modes.json");
const REJECTIONS = join(REPO_ROOT, "examples/angars/model/prose-rejections.json");
const APPROVED_BY = "fixture-e2e-w3";

const CONTROL_KINDS = new Set(["succession", "parallel", "decision", "modeTransition", "mode"]);

interface RawCandidate {
  id: string;
  kind: string;
  fields: Record<string, unknown>;
  citation: { docId: string; docSha256: string; chunkId: string; sectionPath: string; quote: string };
  confidence?: number;
}

// Decision = approve | reject with a reason. Built from explicit per-candidate judgment below.
type Decision = { action: "approve"; reason: string } | { action: "reject"; reason: string };

/**
 * Already-approved modes/transitions (from prose-approved-modes.json) used for dedup.
 * meet/greet/handshake/Docking and Fueling + GPS-Denied Operation are present; their
 * transitions meet->greet and greet->handshake are present too.
 */
const APPROVED_MODE_NAMES = new Set([
  "scheduling and prioritization",
  "docking and fueling",
  "gps-denied operation",
  "meet",
  "greet",
  "handshake",
]);

// IR L3 action names (from extracted.json) — control-flow operands must resolve here.
// The succession/parallel candidates reference "CONOPS Derivation" / "Approach and Docking"
// owning functions and prose-phrase actions that do NOT appear in this set.

/** Explicit per-candidate decision keyed by candidate id. Every control-flow/mode
 * candidate gets an entry — approve only when genuinely grounded and resolvable. */
const DECISIONS: Record<string, Decision> = {
  // ── succession (single candidate id, 3 edges) — owningFunction "CONOPS Derivation"
  //    and actions ("Analyze stakeholder needs" etc.) describe the document's authoring
  //    methodology, not ANGARS system behavior; none resolve to IR L3 action names.
  //    Also: 'succession' is not an approvable prose kind (schema enum excludes it).
  "prose-candidate-018b26754a90acac": {
    action: "reject",
    reason:
      "succession: owningFunction 'CONOPS Derivation' + actions describe doc authoring method, not system behavior; no IR L3 action match; 'succession' not an approvable prose kind",
  },
  // ── parallel — owningFunction "Approach and Docking" not an IR function; branchActions
  //    are prose phrases, not IR action names. 'parallel' also not an approvable kind.
  "prose-candidate-5e5a2369598e6204": {
    action: "reject",
    reason:
      "parallel: owningFunction 'Approach and Docking' absent from IR; branchActions are prose phrases not L3 actions; 'parallel' not an approvable prose kind",
  },
  // ── modeTransition meet->greet — already approved (prose-191f3d815cecf38c). Duplicate.
  "prose-candidate-9c78a3b2ee330f57": {
    action: "reject",
    reason: "modeTransition meet->greet duplicate of already-approved prose-191f3d815cecf38c",
  },
  // ── modeTransition greet->handshake — already approved (prose-1468413ad26d6844). Duplicate.
  "prose-candidate-17e3b6af5a0b1589": {
    action: "reject",
    reason: "modeTransition greet->handshake duplicate of already-approved prose-1468413ad26d6844",
  },
  // ── modeTransition Normal Operations->GPS-denied mode — 'Normal Operations' is not an
  //    approved mode; the approved GPS transition is Scheduling and Prioritization->GPS-Denied
  //    Operation. Dangling fromMode.
  "prose-candidate-50ed100d497b6659": {
    action: "reject",
    reason:
      "modeTransition fromMode 'Normal Operations' is not an approved mode; approved GPS transition originates from 'Scheduling and Prioritization'",
  },
  // ── mode Mission Operation Area — quote describes a geographic region the aircraft is
  //    'within', not an operating mode; the behavior it begins is the approved 'Scheduling
  //    and Prioritization' mode.
  "prose-candidate-f70960aac621592f": {
    action: "reject",
    reason:
      "mode 'Mission Operation Area' is a geographic region in the quote, not an operating mode; behavior covered by approved 'Scheduling and Prioritization'",
  },
  // ── mode Docking and Fueling — already approved (prose-7b808179ddbec422). Duplicate.
  "prose-candidate-1464ad7941f237db": {
    action: "reject",
    reason: "mode 'Docking and Fueling' duplicate of already-approved prose-7b808179ddbec422",
  },
  // ── mode meet — already approved. Duplicate.
  "prose-candidate-1f6c166f492f23a1": {
    action: "reject",
    reason: "mode 'meet' duplicate of already-approved substage mode",
  },
  // ── mode greet — already approved. Duplicate.
  "prose-candidate-f6e6df4287b696c2": {
    action: "reject",
    reason: "mode 'greet' duplicate of already-approved substage mode",
  },
  // ── mode handshake — already approved. Duplicate.
  "prose-candidate-13ba4b94055724fc": {
    action: "reject",
    reason: "mode 'handshake' duplicate of already-approved substage mode",
  },
  // ── mode GPS-denied mode — semantic duplicate of approved 'GPS-Denied Operation'
  //    (same source concept, variant name).
  "prose-candidate-8cd3307f60dd53c7": {
    action: "reject",
    reason: "mode 'GPS-denied mode' variant-name duplicate of approved 'GPS-Denied Operation'",
  },
  // ── mode Authenticated Connection — bare FAR list fragment ('Receive: Authenticated
  //    Connection'); not a stated operating mode.
  "prose-candidate-8b4a7394891000f0": {
    action: "reject",
    reason: "mode 'Authenticated Connection' is a FAR list-item fragment, not a stated operating mode",
  },
  // ── mode Secure Handshake — bare FAR heading fragment; not a stated operating mode.
  "prose-candidate-134570b354324db2": {
    action: "reject",
    reason: "mode 'Secure Handshake' is a bare FAR fragment, not a stated operating mode",
  },
  // ── mode Threat Environment — FAR section heading ('9. Threat Environment'); not a mode.
  "prose-candidate-70a402fa88c94268": {
    action: "reject",
    reason: "mode 'Threat Environment' is a FAR section heading ('9. Threat Environment'), not an operating mode",
  },
};

async function main(): Promise<void> {
  const raw = JSON.parse(await fs.readFile(CANDIDATES, "utf8")) as
    | RawCandidate[]
    | { candidates?: RawCandidate[] };
  const items: RawCandidate[] = Array.isArray(raw) ? raw : raw.candidates ?? [];
  const controlFlow = items.filter((it) => CONTROL_KINDS.has(it.kind));

  console.log(`=== T2 review: ${controlFlow.length} control-flow/mode candidates ===\n`);

  let approved = 0;
  let rejected = 0;
  const unhandled: string[] = [];

  for (const it of controlFlow) {
    const decision = DECISIONS[it.id];
    if (!decision) {
      unhandled.push(`${it.kind} ${it.id} :: ${JSON.stringify(it.fields)}`);
      continue;
    }
    if (decision.action === "approve") {
      const candidate: CandidateEntry = {
        id: it.id,
        kind: it.kind as CandidateEntry["kind"],
        fields: it.fields,
        citation: it.citation,
      };
      const entry = await appendApproval(candidate, APPROVED_BY, APPROVED, REJECTIONS);
      approved++;
      console.log(`APPROVE  ${it.kind.padEnd(15)} ${it.id}  -> ${entry.id}`);
      console.log(`         ${decision.reason}`);
    } else {
      await recordRejection(it.id, REJECTIONS);
      rejected++;
      console.log(`REJECT   ${it.kind.padEnd(15)} ${it.id}`);
      console.log(`         ${decision.reason}`);
    }
  }

  console.log(`\n=== T2 summary: ${approved} approved, ${rejected} rejected ===`);
  if (unhandled.length > 0) {
    console.error(`\n!! ${unhandled.length} UNHANDLED candidates (no explicit decision):`);
    for (const u of unhandled) console.error(`   ${u}`);
    process.exit(1);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
