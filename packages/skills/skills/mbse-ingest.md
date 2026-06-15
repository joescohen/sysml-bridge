---
name: mbse-ingest
description: Human-approval prose ingestion skill — present pending candidates grouped by doc/section, human approve/reject each, approve calls appendApproval, reject calls recordRejection. NO auto-approval path exists.
---

> **Grounding rules:** read `packages/skills/skills/_shared/knowledge-preamble.md` before this skill.

# MBSE Ingest

Drive prose extraction candidates through the human-approval gate. Every candidate in
`prose-candidates.json` is presented to the human reviewer grouped by document and section.
The human decides approve or reject for each item (or each group). Approvals are appended to
`prose-approved.json` via `appendApproval`; rejections are recorded in `prose-rejections.json`
via `recordRejection`. Already-decided candidates (approved or rejected) are silently skipped.

---

## Hard constraint: NO AUTO-APPROVAL PATH

**The human is the gate. There is no code path in this skill that approves a candidate
without an explicit human decision via AskUserQuestion.**

This is the same doctrine as `mbse-edit`: no mutation lands without human approval. The
`appendApproval` helper is called ONLY after the human replies `approve` (or an equivalent
affirmative) to the AskUserQuestion prompt. If the human does not explicitly approve, the
candidate is either rejected (on explicit reject) or left pending (on skip/no answer).
"Approve all" shortcuts, pattern-based auto-approval, and inferred consent are all forbidden.

The rationale: `prose-approved.json` is the trust boundary between prose extraction and the
MBSE model. Any element whose `provenanceSourceId` resolves through a prose-approved entry
traces back to a human-reviewed judgment call. Auto-approval would break that chain.

---

## Prerequisites

Before running this skill, confirm these files exist (they are gitignored; run the ingestion
pipeline to generate them):

- `prose-candidates.json` — pending candidates from the prose extraction pipeline
- `prose-approved.json` — approved entries (may be empty `{ "entries": [] }` on first run)
- `prose-rejections.json` — rejection record (may be empty `{ "rejectedIds": [] }` on first run)

Helper module: `packages/ir/src/approval-helpers.ts` — exports `appendApproval`,
`recordRejection`, `isApproved`, `isRejected`.

---

## The approval loop

Execute these steps in order. Do not deviate.

### Step 1 — Load candidates and determine pending set

Read `prose-candidates.json`. For each candidate:

1. Call `isApproved(candidate.id, proseApprovedPath)` — if true, skip silently (already approved).
2. Call `isRejected(candidate.id, rejectionsPath)` — if true, skip silently (already rejected).
3. Otherwise: add to the pending set.

If the pending set is empty, report:
```
MBSE-INGEST: No pending candidates — all candidates are already approved or rejected.
```
and stop.

### Step 2 — Group pending candidates by doc → section

Organize the pending set:
```
Group: <docId> / <citation.sectionPath>
  Candidate <id>: "<quote>" [kind: <kind>]
  ...
```

Multiple candidates in the same doc/section are presented together as a group.
Groups are presented in document order (preserving the order they appear in `prose-candidates.json`).

### Step 3 — Present each group and collect human decisions

For each group, use AskUserQuestion to present the candidates and ask for approve/reject decisions.

**Presentation format for each candidate in the group:**

```
[<kind>] <id>
  Quote:    "<quote>"
  Section:  <citation.sectionPath>  (doc: <citation.docId>, page chunk: <citation.chunkId>)
  Fields:   <key>: <value> | <key>: <value> | ...
```

If the group has multiple candidates, present them all in the AskUserQuestion body and ask
the human to approve or reject each by id (or approve/reject the group).

**AskUserQuestion format:**

```
PENDING CANDIDATES — <docId> / <sectionPath>
(<count> candidate(s) pending)

[<kind>] <candidateId-1>
  Quote:   "<quote-1>"
  Fields:  <fields summary>

[<kind>] <candidateId-2>
  Quote:   "<quote-2>"
  Fields:  <fields summary>

Reply with one of:
  approve all           — approve every candidate in this group
  approve <id> [<id>…]  — approve specific candidates
  reject all            — reject every candidate in this group
  reject <id> [<id>…]   — reject specific candidates
  skip                  — leave all pending (decide later)

Or combine: approve <id> reject <id>
```

**The skill MUST NOT call `appendApproval` or `recordRejection` until the human replies.**

### Step 4 — Execute decisions

Parse the human reply:

- `approve all` / `approve <id> [<id>…]` — for each approved candidate:
  - Call `appendApproval(candidate, approvedBy, proseApprovedPath, rejectionsPath)`
    where `approvedBy` is the current git config `user.name`
    (run `git config user.name` once at skill start; fallback to `"unknown"` on failure).
  - Confirm the returned entry id.

- `reject all` / `reject <id> [<id>…]` — for each rejected candidate:
  - Call `recordRejection(candidate.id, rejectionsPath)`.

- `skip` — leave all candidates in this group pending. Move to the next group.

- Mixed reply (`approve <id1> reject <id2>`) — parse each token pair and apply accordingly.

After processing each group, print a brief summary line:
```
Group <docId> / <sectionPath>: +<approved_count> approved, -<rejected_count> rejected, ~<skipped_count> pending
```

### Step 5 — Session summary

After all groups are processed (or the user exits early), print:

```
MBSE-INGEST SESSION COMPLETE
─────────────────────────────
Approved:  <total_approved>  (appended to prose-approved.json)
Rejected:  <total_rejected>  (recorded in prose-rejections.json)
Pending:   <total_pending>   (not yet decided)

Next steps:
  - Re-run pnpm generate-cc-model to include newly approved entries in the model.
  - Run pnpm validate:sysml to confirm model integrity.
  - Pending candidates remain in prose-candidates.json for the next session.
```

---

## Supersede support

If a candidate's `supersedes` field is set (pointing at an already-approved entry id),
surface that in the presentation:

```
  ⚑ Supersedes existing entry: <supersedes-id>
    (approving this will retire the prior entry in composeIR output)
```

The human must explicitly approve it knowing it supersedes the prior entry. `appendApproval`
propagates the `supersedes` field automatically; the prior entry is RETAINED in
`prose-approved.json` (append-only) but excluded from `composeIR` proseEntries output.

---

## Files and paths (gitignored — local-only corpus data)

| File | Description |
|------|-------------|
| `prose-candidates.json` | Input: pending extraction candidates from the pipeline |
| `prose-approved.json` | Output: append-only approved entry store |
| `prose-rejections.json` | Output: append-only rejection id record |

These three files contain corpus-derived content and are gitignored. They are never
committed to the repo. The fixture at
`packages/ir/src/__tests__/fixtures/prose-approved-fixture.json` is the committed
test fixture (synthetic field text, real chunk IDs — safe to commit).

---

## Gate integration

Approved entries feed into `composeIR` and are visible in Gate 1 provenance resolution:

1. `composeIR(extractedPath, proseApprovedPath)` — includes approved prose entries in
   `approvedProseIds` set.
2. A model element whose `provenanceSourceId` matches an approved prose entry id will
   resolve GATE03-unresolvable-provenance.
3. The mbse-edit skill's Pillar C constraint is satisfied when the `provenanceSourceId`
   is a real approved prose id returned by `appendApproval`.

**This is the C6/C8 round-trip**: candidate → human approve → `appendApproval` → id in
`prose-approved.json` → `composeIR` includes it → model element citing that id passes Gate 1.

---

## Helper API reference

```typescript
// packages/ir/src/approval-helpers.ts

appendApproval(
  candidate: CandidateEntry,  // from prose-candidates.json
  approvedBy: string,         // git config user.name
  approvedPath: string,       // path to prose-approved.json
  rejectionsPath: string      // path to prose-rejections.json
): Promise<ProseApprovedEntry>
// Validates, assigns stableId, sets status:'approved', appends. Returns the new entry.

recordRejection(
  candidateId: string,        // candidate.id to reject
  rejectionsPath: string      // path to prose-rejections.json
): Promise<void>
// Appends candidateId to rejectedIds. Idempotent.

isApproved(
  candidateId: string,
  approvedPath: string
): Promise<boolean>
// True if candidateId appears as candidateId in an approved entry. (Re-ingest skip check.)

isRejected(
  candidateId: string,
  rejectionsPath: string
): Promise<boolean>
// True if candidateId appears in rejectedIds. (Re-ingest skip check.)
```

---

## MCP / Tool surface

This skill uses NO MCP store tools directly (no `create_element`, no `create_relationship`).
It calls the TypeScript helpers above (as a script or inline code), reads/writes JSON files,
and uses AskUserQuestion for every approval decision.

The model mutation that follows approval (creating elements with the approved prose id as
`provenanceSourceId`) is performed by `mbse-edit`, not by this skill.
