# review-ui rendered-browser pass

Closes the gap admitted in the Phase 5 done-criteria record (commit `fcd180a`):

> README: Review UI section; honest note — no browser screenshot was
> fabricated; the page was verified by curl (server-rendered content)
> and the API flow; a human browser pass remains for Joe

## Evidence tier achieved: **rendered + screenshot (headless Chromium, automated)**

This is a genuine rendered-browser pass — a real Chromium engine executed the
page's JS, applied its CSS, and produced pixel screenshots — not curl output.
It is **not** the interactive human pass the Phase 5 note asks for from Joe:
this box (`DISPLAY` unset, no GUI) has no interactive browser session, and
the two Chrome instances connected via the `claude-in-chrome` MCP tool are on
remote hosts (Windows, macOS) that cannot reach this container's
`localhost:4173`/`4174` — the MCP tool itself confirmed this by returning the
two remote `deviceId`s with no local-to-this-host option, and driving it
further would have required an `AskUserQuestion` tool this session doesn't
have. So: no fabricated "I clicked through in a browser as Joe" claim here.
What follows is the strongest evidence tier actually achievable on this box —
real Chromium, headless, driven by Playwright — plus an explicit note on what
still needs a human's eyes.

**Still owed:** an interactive pass by Joe (or a `claude-in-chrome`-connected
browser on the same host as the server) — mouse-driven, human-perceived
layout/contrast/feel, and confirmation the narrow-viewport issue below
actually matters for how the UI gets used.

## Setup

```
pnpm install --frozen-lockfile   # clean, lockfile up to date
pnpm build                       # all 7 buildable packages, green
```

Candidate data already present at `examples/angars/candidates/` (checked
in): `prose-candidates.json` (323 total, `totalCandidates: 323`) and
`inference-candidates.json` (2180 records). No `pnpm demo` regen was needed
— the counts the server derived from this data matched the Phase 5 record
exactly (see below), so the fixture is current.

Server started via the real `pnpm review` script for the read-only pass:

```
pnpm review
# → Candidate review UI running at http://localhost:4173
#   candidates:   .../examples/angars/candidates
#   dispositions: .../examples/angars/out/dispositions
```

For the write-path (Approve/Reject) test, a **second** instance was started
on port 4174 pointing at a scratch dispositions directory under `/tmp`
(`node packages/review-ui/dist/server.js --candidates examples/angars/candidates
--dispositions <scratch dir> --port 4174`), specifically to avoid writing a
junk approval into the real `examples/angars/out/dispositions` path. That
path is gitignored (`examples/*/out/`) and, confirmed after the run, was
**never created** by the read-only pass — proving the GET-only server
instance made zero writes, consistent with the Phase 5 "GET endpoints
proven read-only" claim.

Browser: Playwright 1.61.1, Chromium headless (`chromium-1228`, downloaded
fresh — this box has no display server). Driver scripts were scratch files,
not committed.

## What was exercised

1. **`GET /` — the queue page** (`01-queue.png`). Header reads
   `Prose 319 pending / 0 approved / 0 rejected · Inference 133 pending /
   0 approved / 0 rejected` — an exact match to the Phase 5 record's "319
   prose surfaced of 323 ... 133 reviewable inference proposals normalized
   from 2180 pipeline rows." `.item` rows rendered: 452 (319 + 133,
   confirmed via `page.locator(".item").count()`).
2. **Prose candidate detail** (`02-prose-detail.png`, `02-prose-detail.txt`).
   Clicked the first prose row. Detail pane renders claim text, kind,
   candidate id, and a citation block (quoted source excerpt, doc id,
   section path, chunk id) plus Approve/Reject buttons.
3. **Inference candidate detail** (`03-inference-detail.png`,
   `03-inference-detail.txt`). Clicked into the inference layer. Detail pane
   renders the allocation edge (source → target), confidence score, a
   premises list, and a rationale field explicitly marked
   `(audit-only — redacted; never exported)` — this redaction is a
   documented design choice, not a bug.
4. **Scroll through the full list** (`04-list-scrolled.png`). Scrolled the
   list pane to its end; reached the inference layer's `controlJoin` rows
   with the sticky layer-head label intact. No virtualization cutoff, no
   broken scroll.
5. **Approve flow** (`05-after-approve.png`, scratch dispositions dir).
   Clicked Approve on the first prose candidate. `POST /api/approve` → `200`.
   Badge flipped `PENDING → APPROVED` live (no reload), header counts
   recalculated (`319 pending` → `318 pending / 1 approved`), Approve button
   disabled itself post-click. Confirmed the write actually landed on disk:
   `prose-approved.json` in the scratch dir gained one entry with
   `approvedBy: "verification-pass"` (from `REVIEW_UI_USER` env var),
   `approvedAt` timestamp, full citation payload, and a content-addressed
   `id` distinct from the `candidateId` — matching the Phase 5 claim about
   content-addressed ids and the `/mbse-approve`-compatible writer shape.
6. **Reject flow** (`06-after-reject.png`). Clicked Reject on the second
   prose candidate. `POST /api/reject` → `200`. Badge flipped to `REJECTED`,
   header counts updated (`1 approved / 1 rejected`). On-disk
   `prose-rejections.json` gained the candidate id.
7. **Narrow viewport (480×800)** (`07-narrow-480px.png`) — see Findings.

Console/page-error capture (`console-log.json`) was empty across every pass:
**zero JS console messages, zero uncaught page errors.**

## Findings

- **No console errors, no dead interactions.** Approve/Reject both round-trip
  correctly and update the UI without a page reload; list selection
  highlighting, sticky layer headers, and scroll all work as expected.
- **Real defect — no responsive breakpoint.** At a 480px-wide viewport
  (`07-narrow-480px.png`) the two-pane `grid-template-columns:
  minmax(280px, 40%) 1fr` layout does not collapse to a single column. The
  list column aggressively truncates candidate labels (e.g. "System needs to
  satisfy the…") and the detail pane's claim heading wraps into a very
  narrow column, one or two words per line, pushing the citation block far
  down. The page remains functional (nothing overlaps or clips unreadably)
  but is uncomfortable to use below roughly tablet width. Given this is an
  internal reviewer tool likely used at a desk, this may be acceptable as-is
  — flagging for Joe's call rather than "fixing" a UI whose intended usage
  context is unconfirmed.
- **Data counts match the Phase 5 record exactly**: 319/133 pending on load,
  323 total prose candidates in the source file (4 dropped, consistent with
  "4 dropped with kinds outside the approval schema").
- Rationale redaction on inference candidates (`(audit-only — redacted;
  never exported)`) is visible in the rendered detail pane exactly as
  designed — worth a human confirming this is the intended reviewer-facing
  copy, since it reads a little abruptly out of context.

## Honesty notes

- All screenshots in `docs/verification/screenshots/` are real PNG captures
  written by Playwright from an actual headless Chromium render of the
  running server — not mocked, not hand-edited.
- The approve/reject test wrote to a scratch dispositions directory under
  `/tmp`, not the repo's `examples/angars/out/dispositions` (which stayed
  nonexistent throughout — proof the read-only pass made no writes). No
  disposition data under version control was touched.
- Both server instances (ports 4173 and 4174) were shut down at the end of
  this pass; confirmed via `curl` returning connection-refused on both
  ports afterward.
- This is **still not** the human-eyes browser pass Joe owes himself per the
  Phase 5 note — it is the strongest automated substitute available on this
  box. The remaining gap is narrow and specific: a person looking at the
  page (ideally also at a normal desktop width, where no issue was found)
  and forming a subjective judgment the model can't fully substitute for.
