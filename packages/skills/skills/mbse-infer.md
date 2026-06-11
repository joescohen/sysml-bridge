---
name: mbse-infer
description: Human-approval inference skill — present pending inferred-link candidates from inference-candidates.json grouped by relation family and target subsystem; human approve/reject each; approve calls appendInferredApproval, reject calls recordInferredRejection. NO auto-approve path exists.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Infer

Drive inference-layer candidates through the human-approval gate. Every candidate in
`inference-candidates.json` with stage `queued` (including those with `uncertain`
debate verdict) is presented to the human reviewer grouped by **relation family** →
**target subsystem**. The human decides approve or reject for each item (or each group).
Approvals are appended to `inferred-approved.json` via `appendInferredApproval`;
rejections are recorded in `inferred-rejections.json` via `recordInferredRejection`.
Already-decided candidates (approved or rejected) are silently skipped.

---

## Hard constraint: NO AUTO-APPROVAL PATH

**The human is the gate. There is no code path in this skill that approves a candidate
without an explicit human decision via AskUserQuestion.**

This is the same doctrine as `mbse-edit` and `mbse-ingest`: no mutation lands without
human approval. The `appendInferredApproval` helper is called ONLY after the human
replies `approve` (or an equivalent affirmative) to the AskUserQuestion prompt. If the
human does not explicitly approve, the candidate is either rejected (on explicit reject)
or left pending (on skip/no answer).

"Approve all" shortcuts, pattern-based auto-approval, and inferred consent are all
forbidden. The rationale: `inferred-approved.json` is the trust boundary between the
inference pipeline and the MBSE model. Any element whose `provenanceSourceId` resolves
through an inferred-approved entry traces back to a human-reviewed judgment call.
Auto-approval would break that chain.

---

## Prerequisites

Before running this skill, confirm these files exist (they are gitignored; run the
inference pipeline to generate them):

- `inference-candidates.json` — pending proposals from the inference pipeline
  (entries with stage `queued`, including those with `uncertain` debate verdict)
- `inferred-approved.json` — approved entries (may be empty `{ "entries": [] }` on first run)
- `inferred-rejections.json` — rejection record (may be empty `{ "rejectedIds": [] }` on first run)

Helper module: `packages/ir/src/inferred-approval-helpers.ts` — exports
`appendInferredApproval`, `recordInferredRejection`, `isInferredApproved`, `isInferredRejected`.

---

## The approval loop

Execute these steps in order. Do not deviate.

### Step 1 — Load candidates and determine pending set

Read `inference-candidates.json`. For each candidate:

1. If stage is not `queued` → skip silently (still in pipeline; not ready for human review).
2. Call `isInferredApproved(candidate.id, inferredApprovedPath)` — if true, skip silently.
3. Call `isInferredRejected(candidate.id, rejectionsPath)` — if true, skip silently.
4. Otherwise: add to the pending set.

If the pending set is empty, report:
```
MBSE-INFER: No pending candidates — all queued candidates are already approved or rejected.
```
and stop.

### Step 2 — Group pending candidates by relation family → target subsystem

Organize the pending set:
```
Group: <relationFamily> / <target subsystem>
  Candidate <id>: <sourceId> → <targetId> [confidence: <conf>] [uncertain]
  ...
```

Multiple candidates in the same relation-family/subsystem are presented together as a
group. Groups are ordered: allocation → modeMembership → flowTyping → controlJoin
(processing order; within each family, order by target subsystem alphabetically).

### Step 3 — Present each group and collect human decisions

For each group, use AskUserQuestion to present the candidates and ask for approve/reject.

**Presentation format for each candidate in the group:**

```
[<relationFamily>] <id>
  Source:     <sourceId> (<sourceName if available>)
  Target:     <targetId> (<targetName if available>)
  Confidence: <confidence>
  Debate:     <verdict> (advocate: <advocate>, challenger: <challenger>)
              (omit if no debate field)
  Premises:
    - <premiseId-1>: "<resolved quote from composed IR if available>"
    - <premiseId-2>: "<resolved quote from composed IR if available>"
    - (if premise cannot be resolved: [unresolvable — NOT in composed IR])
  ⚑ UNCERTAIN verdict — debate was inconclusive. Review premises carefully.
    (only if debate.verdict === "uncertain")
```

If the group has multiple candidates, present them all in the AskUserQuestion body and ask
the human to approve or reject each by id (or approve/reject the group).

**AskUserQuestion format:**

```
PENDING INFERRED CANDIDATES — <relationFamily> / <subsystem>
(<count> candidate(s) pending)

[<relationFamily>] <candidateId-1>
  Source:     <sourceId>
  Target:     <targetId>
  Confidence: <confidence>
  Premises:
    - <premiseId>: "<quote>"

[<relationFamily>] <candidateId-2>
  Source:     <sourceId>
  Target:     <targetId>
  Confidence: <confidence>
  ⚑ UNCERTAIN verdict — review premises carefully.
  Premises:
    - <premiseId>: "<quote>"

Reply with one of:
  approve all           — approve every candidate in this group
  approve <id> [<id>…]  — approve specific candidates
  reject all            — reject every candidate in this group
  reject <id> [<id>…]   — reject specific candidates
  skip                  — leave all pending (decide later)

Or combine: approve <id> reject <id>
```

**The skill MUST NOT call `appendInferredApproval` or `recordInferredRejection` until
the human replies.**

Premises with their resolved quotes are shown so the human can verify the reasoning chain.
**The rationale field is NOT shown** — it is audit-only (DEBAT-04 discipline). Premises
carry the citations; the human verifies whether those premises actually support the link.

### Step 4 — Execute decisions

Parse the human reply:

- `approve all` / `approve <id> [<id>…]` — for each approved candidate:
  - Call `appendInferredApproval(candidate, approvedBy, inferredApprovedPath, rejectionsPath)`
    where `approvedBy` is the current git config `user.name`
    (run `git config user.name` once at skill start; fallback to `"unknown"` on failure).
  - Confirm the returned entry id.

- `reject all` / `reject <id> [<id>…]` — for each rejected candidate:
  - Call `recordInferredRejection(candidate.id, rejectionsPath)`.

- `skip` — leave all candidates in this group pending. Move to the next group.

- Mixed reply (`approve <id1> reject <id2>`) — parse each token pair and apply accordingly.

After processing each group, print a brief summary line:
```
Group <relationFamily> / <subsystem>: +<approved_count> approved, -<rejected_count> rejected, ~<skipped_count> pending
```

### Step 5 — Session summary

After all groups are processed (or the user exits early), print:

```
MBSE-INFER SESSION COMPLETE
─────────────────────────────
Approved:  <total_approved>  (appended to inferred-approved.json)
Rejected:  <total_rejected>  (recorded in inferred-rejections.json)
Pending:   <total_pending>   (not yet decided)

Next steps:
  - Re-run pnpm generate-cc-model to include newly approved inferred links in the model.
  - Run pnpm validate:sysml to confirm model integrity.
  - Pending candidates remain in inference-candidates.json for the next session.
```

---

## Supersede support

If a candidate's `supersedes` field is set (pointing at an already-approved entry id),
surface that in the presentation:

```
  ⚑ Supersedes existing entry: <supersedes-id>
    (approving this will retire the prior entry in composeIR output)
```

The human must explicitly approve it knowing it supersedes the prior entry.
`appendInferredApproval` propagates the `supersedes` field automatically; the prior entry
is RETAINED in `inferred-approved.json` (append-only) but excluded from `composeIR`
inferredEntries output.

---

## Files and paths (gitignored — local-only corpus data)

| File | Description |
|------|-------------|
| `inference-candidates.json` | Input: pending inference proposals from the pipeline |
| `inferred-approved.json` | Output: append-only approved inferred entry store |
| `inferred-rejections.json` | Output: append-only rejection id record |

These three files contain corpus-derived content and are gitignored. They are never
committed to the repo.

---

## Gate integration

Approved entries feed into `composeIR` and are visible in Gate 1 provenance resolution:

1. `composeIR(extractedPath, proseApprovedPath, manifestPath, inferredApprovedPath)` —
   includes approved inferred entries in `approvedInferredIds` set.
2. A model element whose `provenanceSourceId` matches an approved inferred entry id will
   resolve GATE03-unresolvable-provenance (three-layer Gate-1 extension).
3. The mbse-edit skill's Pillar C constraint is satisfied when the `provenanceSourceId`
   is a real approved inferred id returned by `appendInferredApproval`.

---

## Helper API reference

```typescript
// packages/ir/src/inferred-approval-helpers.ts

appendInferredApproval(
  candidate: InferenceCandidate,  // from inference-candidates.json (queued stage)
  approvedBy: string,              // git config user.name
  approvedPath: string,            // path to inferred-approved.json
  rejectionsPath: string           // path to inferred-rejections.json
): Promise<InferredApprovedEntry>
// Validates, stamps status:'approved', approvedAt, approvedBy, appends. Returns the new entry.

recordInferredRejection(
  candidateId: string,        // candidate.id to reject
  rejectionsPath: string      // path to inferred-rejections.json
): Promise<void>
// Appends candidateId to rejectedIds. Idempotent.

isInferredApproved(
  candidateId: string,
  approvedPath: string
): Promise<boolean>
// True if candidateId appears as an entry id in inferred-approved.json.

isInferredRejected(
  candidateId: string,
  rejectionsPath: string
): Promise<boolean>
// True if candidateId appears in rejectedIds.
```

---

## MCP / Tool surface

This skill uses NO MCP store tools directly (no `create_element`, no `create_relationship`).
It calls the TypeScript helpers above (as a script or inline code), reads/writes JSON files,
and uses AskUserQuestion for every approval decision.

The model mutation that follows approval (creating elements with the approved inferred id as
`provenanceSourceId`) is performed by `mbse-edit`, not by this skill.
