/**
 * server.ts — the candidate review UI server. A zero-dependency node:http
 * server (no framework) that serves ONE inline-styled HTML page and three JSON
 * endpoints for the human approval gate:
 *
 *   GET  /             → the review page (list + detail + approve/reject)
 *   GET  /api/state    → candidates from both layers merged with dispositions
 *   POST /api/approve  → { layer, candidateId } → appendApproval / appendInferredApproval
 *   POST /api/reject   → { layer, candidateId } → recordRejection / recordInferredRejection
 *
 * THE HUMAN GATE. Every write happens on an explicit user click reaching a POST
 * endpoint with a single candidateId — there is no batch or auto-approve path.
 * The POST handlers call the SAME approval-writer helpers the /mbse-approve
 * skill uses, so a UI approval is byte-compatible with a skill approval by
 * construction. This is the ONLY module in packages/review-ui that calls the
 * approval writers; the no-auto-approve ratchet allowlists exactly this file.
 */

import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import { promises as fs } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  appendApproval,
  appendInferredApproval,
  appendEntityMerge,
  recordRejection,
  recordInferredRejection,
  recordEntityRejection,
} from "@sysml-bridge/model";

import {
  buildState,
  findProseCandidate,
  findInferenceCandidate,
  findEntityCandidate,
  DISPOSITION_FILES,
  type Layer,
  type ReviewState,
  type ReviewItem,
} from "./candidates.js";

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export interface ReviewServerOptions {
  /** Directory holding prose-candidates.json + inference-candidates.json. */
  candidatesDir: string;
  /** Directory holding (or to receive) the disposition JSON files. */
  dispositionsDir: string;
  /** Listen port (default 4173). */
  port?: number;
}

const DEFAULT_PORT = 4173;

/** Human identity stamped as approvedBy — env override, else OS username. */
function approverIdentity(): string {
  const env = process.env.REVIEW_UI_USER;
  if (env && env.trim()) return env.trim();
  try {
    return os.userInfo().username;
  } catch {
    return "unknown";
  }
}

// ---------------------------------------------------------------------------
// createReviewServer — the http.Server (exported for tests)
// ---------------------------------------------------------------------------

export function createReviewServer(opts: ReviewServerOptions): http.Server {
  const { candidatesDir, dispositionsDir } = opts;

  return http.createServer((req, res) => {
    handle(req, res, candidatesDir, dispositionsDir).catch((err) => {
      sendJson(res, 500, { error: "internal", detail: String(err) });
    });
  });
}

async function handle(
  req: http.IncomingMessage,
  res: http.ServerResponse,
  candidatesDir: string,
  dispositionsDir: string
): Promise<void> {
  const url = new URL(req.url ?? "/", "http://localhost");
  const method = req.method ?? "GET";

  // --- GET / → the page (server-rendered with the initial state) ---
  if (method === "GET" && url.pathname === "/") {
    const state = await buildState(candidatesDir, dispositionsDir);
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(renderPage(state));
    return;
  }

  // --- GET /api/state → merged candidates + dispositions ---
  if (method === "GET" && url.pathname === "/api/state") {
    const state = await buildState(candidatesDir, dispositionsDir);
    sendJson(res, 200, state);
    return;
  }

  // --- POST /api/approve ---
  if (method === "POST" && url.pathname === "/api/approve") {
    await handleDisposition(req, res, candidatesDir, dispositionsDir, "approve");
    return;
  }

  // --- POST /api/reject ---
  if (method === "POST" && url.pathname === "/api/reject") {
    await handleDisposition(req, res, candidatesDir, dispositionsDir, "reject");
    return;
  }

  // --- unknown ---
  sendJson(res, 404, { error: "not_found", path: url.pathname });
}

// ---------------------------------------------------------------------------
// POST handlers — approve / reject
// ---------------------------------------------------------------------------

async function handleDisposition(
  req: http.IncomingMessage,
  res: http.ServerResponse,
  candidatesDir: string,
  dispositionsDir: string,
  action: "approve" | "reject"
): Promise<void> {
  let body: unknown;
  try {
    body = await readBody(req);
  } catch {
    sendJson(res, 400, { error: "bad_request", detail: "invalid JSON body" });
    return;
  }

  const { layer, candidateId } = (body ?? {}) as {
    layer?: unknown;
    candidateId?: unknown;
  };
  if (layer !== "prose" && layer !== "inference" && layer !== "entity") {
    sendJson(res, 400, {
      error: "bad_request",
      detail: "layer must be 'prose', 'inference', or 'entity'",
    });
    return;
  }
  if (typeof candidateId !== "string" || candidateId.length === 0) {
    sendJson(res, 400, {
      error: "bad_request",
      detail: "candidateId (non-empty string) is required",
    });
    return;
  }

  await fs.mkdir(dispositionsDir, { recursive: true });
  const approvedBy = approverIdentity();

  if (layer === "prose") {
    const candidate = await findProseCandidate(candidatesDir, candidateId);
    if (!candidate) {
      sendJson(res, 404, { error: "not_found", detail: `no prose candidate ${candidateId}` });
      return;
    }
    const approvedPath = path.join(dispositionsDir, DISPOSITION_FILES.proseApproved);
    const rejectionsPath = path.join(dispositionsDir, DISPOSITION_FILES.proseRejections);
    if (action === "approve") {
      const entry = await appendApproval(candidate, approvedBy, approvedPath, rejectionsPath);
      sendJson(res, 200, { ok: true, layer, action, candidateId, entryId: entry.id });
    } else {
      await recordRejection(candidateId, rejectionsPath);
      sendJson(res, 200, { ok: true, layer, action, candidateId });
    }
    return;
  }

  if (layer === "inference") {
    const candidate = await findInferenceCandidate(candidatesDir, candidateId);
    if (!candidate) {
      sendJson(res, 404, { error: "not_found", detail: `no inference candidate ${candidateId}` });
      return;
    }
    const approvedPath = path.join(dispositionsDir, DISPOSITION_FILES.inferredApproved);
    const rejectionsPath = path.join(dispositionsDir, DISPOSITION_FILES.inferredRejections);
    if (action === "approve") {
      const entry = await appendInferredApproval(candidate, approvedBy, approvedPath, rejectionsPath);
      sendJson(res, 200, { ok: true, layer, action, candidateId, entryId: entry.id });
    } else {
      await recordInferredRejection(candidateId, rejectionsPath);
      sendJson(res, 200, { ok: true, layer, action, candidateId });
    }
    return;
  }

  // layer === "entity" — a merge proposal. Reject records the CONTENT-ADDRESSED
  // pair key (== candidateId), so the same suggestion is never re-proposed.
  const candidate = await findEntityCandidate(candidatesDir, candidateId);
  if (!candidate) {
    sendJson(res, 404, { error: "not_found", detail: `no entity-merge candidate ${candidateId}` });
    return;
  }
  const approvedPath = path.join(dispositionsDir, DISPOSITION_FILES.entityApproved);
  const rejectionsPath = path.join(dispositionsDir, DISPOSITION_FILES.entityRejections);
  if (action === "approve") {
    const entry = await appendEntityMerge(candidate, approvedBy, approvedPath, rejectionsPath);
    sendJson(res, 200, { ok: true, layer, action, candidateId, entryId: entry.id });
  } else {
    await recordEntityRejection(candidateId, rejectionsPath);
    sendJson(res, 200, { ok: true, layer, action, candidateId });
  }
}

// ---------------------------------------------------------------------------
// http helpers
// ---------------------------------------------------------------------------

function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json; charset=utf-8" });
  res.end(payload);
}

function readBody(req: http.IncomingMessage): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    req.on("data", (c: Buffer) => {
      size += c.length;
      if (size > 1_000_000) {
        reject(new Error("body too large"));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8").trim();
      if (raw.length === 0) {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(raw));
      } catch (err) {
        reject(err);
      }
    });
    req.on("error", reject);
  });
}

// ---------------------------------------------------------------------------
// The page — one inline HTML/CSS/JS document. No framework, no CDN.
//
// The candidate list is rendered SERVER-SIDE from the current state, and the
// full state is embedded as JSON for the client to hydrate from (no fetch on
// first paint). The page works without JS (the list renders); JS adds the
// detail pane + approve/reject interactions.
// ---------------------------------------------------------------------------

/** Server-side HTML escaper — mirrors the client `esc()`; used for SSR + hydration safety. */
function escapeHtml(value: unknown): string {
  return String(value == null ? "" : value).replace(/[&<>"']/g, (c) => {
    return (
      { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" } as Record<
        string,
        string
      >
    )[c]!;
  });
}

/** Render one layer's list rows (server-side, matches the client's layerSection). */
function renderLayerSection(title: string, layer: Layer, items: ReviewItem[]): string {
  const rows = items
    .map((it) => {
      return (
        `<div class="item" data-layer="${layer}" data-id="${escapeHtml(it.candidateId)}">` +
        `<div class="body"><div class="kind">${escapeHtml(it.kind)}</div>` +
        `<div class="label">${escapeHtml(it.name)}</div></div>` +
        `<span class="badge ${it.status}">${it.status}</span></div>`
      );
    })
    .join("");
  return `<div class="layer-head">${escapeHtml(title)} (${items.length})</div>${rows}`;
}

/** Render the full page for a given state — server-rendered list + embedded state. */
function renderPage(state: ReviewState): string {
  const listHtml =
    renderLayerSection("Prose layer", "prose", state.prose) +
    renderLayerSection("Inference layer", "inference", state.inference) +
    renderLayerSection("Entity merges", "entity", state.entity);
  const p = state.counts.prose;
  const i = state.counts.inference;
  const e = state.counts.entity;
  const countsText =
    `Prose ${p.pending} pending / ${p.approved} approved / ${p.rejected} rejected  ·  ` +
    `Inference ${i.pending} pending / ${i.approved} approved / ${i.rejected} rejected  ·  ` +
    `Entity ${e.pending} pending / ${e.approved} approved / ${e.rejected} rejected`;
  // Embed state safely: escape </script> so the JSON literal can never close the tag.
  const stateJson = JSON.stringify(state).replace(/</g, "\\u003c");
  return PAGE_TEMPLATE.replace("%%LIST%%", listHtml)
    .replace("%%COUNTS%%", escapeHtml(countsText))
    .replace("%%STATE%%", stateJson);
}

const PAGE_TEMPLATE = /* html */ `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>SysML Foundry — Candidate Review</title>
  <style>
    :root {
      --bg: #f7f7f5;
      --panel: #ffffff;
      --ink: #1c1c1a;
      --muted: #6b6b66;
      --line: #e4e4df;
      --accent: #2f5d50;
      --pending: #8a6d00;
      --pending-bg: #fdf5d8;
      --approved: #1f6b3a;
      --approved-bg: #dff3e4;
      --rejected: #9a2c2c;
      --rejected-bg: #f7dede;
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; height: 100%; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      color: var(--ink);
      background: var(--bg);
      font-size: 14px;
      line-height: 1.5;
    }
    header {
      padding: 14px 22px;
      border-bottom: 1px solid var(--line);
      background: var(--panel);
      display: flex;
      align-items: baseline;
      gap: 14px;
    }
    header h1 { font-size: 15px; margin: 0; font-weight: 600; letter-spacing: 0.01em; }
    header .sub { color: var(--muted); font-size: 12px; }
    .layout { display: grid; grid-template-columns: minmax(280px, 40%) 1fr; height: calc(100vh - 51px); }
    .list { border-right: 1px solid var(--line); overflow-y: auto; background: var(--panel); }
    .layer-head {
      position: sticky; top: 0; z-index: 1;
      padding: 8px 16px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em;
      color: var(--muted); background: #efefec; border-bottom: 1px solid var(--line);
    }
    .item {
      padding: 10px 16px; border-bottom: 1px solid var(--line); cursor: pointer;
      display: flex; gap: 10px; align-items: flex-start;
    }
    .item:hover { background: #fafaf8; }
    .item.selected { background: #eef4f1; box-shadow: inset 3px 0 0 var(--accent); }
    .item .body { min-width: 0; flex: 1; }
    .item .kind { font-size: 11px; color: var(--accent); text-transform: uppercase; letter-spacing: 0.05em; }
    .item .label {
      font-size: 13px; margin-top: 1px;
      overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .badge {
      flex: none; font-size: 10px; font-weight: 600; text-transform: uppercase;
      letter-spacing: 0.05em; padding: 2px 7px; border-radius: 10px; align-self: center;
    }
    .badge.pending  { color: var(--pending);  background: var(--pending-bg); }
    .badge.approved { color: var(--approved); background: var(--approved-bg); }
    .badge.rejected { color: var(--rejected); background: var(--rejected-bg); }
    .detail { overflow-y: auto; padding: 24px 28px; }
    .detail .empty { color: var(--muted); margin-top: 40px; text-align: center; }
    .detail h2 { font-size: 17px; margin: 0 0 2px; font-weight: 600; }
    .detail .meta { color: var(--muted); font-size: 12px; margin-bottom: 18px; }
    .field-block { margin: 0 0 18px; }
    .field-block .k { font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); margin-bottom: 4px; }
    .field-block .v { white-space: pre-wrap; }
    blockquote.citation {
      margin: 0; padding: 12px 16px; border-left: 3px solid var(--accent);
      background: #f0f4f2; color: #33413c; border-radius: 0 4px 4px 0; font-style: italic;
    }
    .cite-meta { font-size: 11px; color: var(--muted); margin-top: 6px; font-style: normal; }
    dl.kv { margin: 0; display: grid; grid-template-columns: max-content 1fr; gap: 4px 14px; }
    dl.kv dt { color: var(--muted); }
    dl.kv dd { margin: 0; word-break: break-word; }
    .premises { margin: 4px 0 0; padding-left: 18px; }
    .premises li { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
    .actions { margin-top: 24px; display: flex; gap: 10px; align-items: center; }
    button {
      font: inherit; font-weight: 600; padding: 8px 18px; border-radius: 6px;
      border: 1px solid transparent; cursor: pointer;
    }
    button.approve { color: #fff; background: var(--approved); }
    button.reject  { color: var(--rejected); background: #fff; border-color: var(--rejected); }
    button:disabled { opacity: 0.5; cursor: default; }
    .status-note { color: var(--muted); font-size: 12px; }
    .redacted { color: var(--muted); font-style: italic; }
  </style>
</head>
<body>
  <header>
    <h1>SysML Foundry — Candidate Review</h1>
    <span class="sub" id="counts">%%COUNTS%%</span>
  </header>
  <div class="layout">
    <div class="list" id="list">%%LIST%%</div>
    <div class="detail" id="detail"><div class="empty">Select a candidate to review its citation and disposition.</div></div>
  </div>

  <script id="initial-state" type="application/json">%%STATE%%</script>
  <script>
    // Hydrate from the server-embedded state — no fetch needed on first paint.
    let STATE = JSON.parse(document.getElementById("initial-state").textContent);
    let selected = null; // { layer, candidateId }

    function esc(s) {
      return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
        return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
      });
    }

    async function load() {
      const res = await fetch("/api/state");
      STATE = await res.json();
      renderList();
      renderCounts();
      if (selected) renderDetail(find(selected));
    }

    function renderCounts() {
      const p = STATE.counts.prose, i = STATE.counts.inference, e = STATE.counts.entity;
      document.getElementById("counts").textContent =
        "Prose " + p.pending + " pending / " + p.approved + " approved / " + p.rejected + " rejected  ·  " +
        "Inference " + i.pending + " pending / " + i.approved + " approved / " + i.rejected + " rejected  ·  " +
        "Entity " + e.pending + " pending / " + e.approved + " approved / " + e.rejected + " rejected";
    }

    function renderList() {
      const parts = [];
      parts.push(layerSection("Prose layer", "prose", STATE.prose));
      parts.push(layerSection("Inference layer", "inference", STATE.inference));
      parts.push(layerSection("Entity merges", "entity", STATE.entity));
      document.getElementById("list").innerHTML = parts.join("");
      document.querySelectorAll(".item").forEach(function (el) {
        el.addEventListener("click", function () {
          selected = { layer: el.dataset.layer, candidateId: el.dataset.id };
          renderList();
          renderDetail(find(selected));
        });
      });
    }

    function layerSection(title, layer, items) {
      const rows = items.map(function (it) {
        const sel = selected && selected.layer === layer && selected.candidateId === it.candidateId ? " selected" : "";
        return '<div class="item' + sel + '" data-layer="' + layer + '" data-id="' + esc(it.candidateId) + '">' +
          '<div class="body"><div class="kind">' + esc(it.kind) + '</div>' +
          '<div class="label">' + esc(it.name) + '</div></div>' +
          '<span class="badge ' + it.status + '">' + it.status + '</span></div>';
      }).join("");
      return '<div class="layer-head">' + esc(title) + ' (' + items.length + ')</div>' + rows;
    }

    function find(sel) {
      const list = sel.layer === "prose" ? STATE.prose
        : sel.layer === "inference" ? STATE.inference
        : STATE.entity;
      return list.find(function (it) { return it.candidateId === sel.candidateId; });
    }

    function renderDetail(it) {
      if (!it) { document.getElementById("detail").innerHTML = '<div class="empty">Select a candidate.</div>'; return; }
      const c = it.candidate;
      let inner = "";
      if (it.layer === "prose") {
        const fields = Object.entries(c.fields).map(function (kv) {
          return '<div class="field-block"><div class="k">' + esc(kv[0]) + '</div><div class="v">' + esc(kv[1]) + '</div></div>';
        }).join("");
        const cit = c.citation || {};
        inner =
          '<h2>' + esc(it.name) + '</h2>' +
          '<div class="meta">prose · ' + esc(it.kind) + ' · ' + esc(it.candidateId) + '</div>' +
          fields +
          '<div class="field-block"><div class="k">Citation</div>' +
          '<blockquote class="citation">' + esc(cit.quote) +
          '<div class="cite-meta">' + esc(cit.docId) + ' · ' + esc(cit.sectionPath) + ' · chunk ' + esc(cit.chunkId) + '</div>' +
          '</blockquote></div>';
      } else if (it.layer === "entity") {
        const ev = c.evidence || { aQuotes: [], bQuotes: [] };
        const aq = (ev.aQuotes || []).map(function (q) { return '<li>' + esc(q) + '</li>'; }).join("");
        const bq = (ev.bQuotes || []).map(function (q) { return '<li>' + esc(q) + '</li>'; }).join("");
        inner =
          '<h2>' + esc(it.name) + '</h2>' +
          '<div class="meta">entity-merge · ' + esc(c.reason) + ' · ' + esc(it.candidateId) + '</div>' +
          '<dl class="kv">' +
          '<dt>canonical</dt><dd>' + esc(c.canonicalName) + '</dd>' +
          '<dt>kind</dt><dd>' + esc(c.kind) + '</dd>' +
          '<dt>entity A</dt><dd>' + esc(c.entityIdA) + '</dd>' +
          '<dt>entity B</dt><dd>' + esc(c.entityIdB) + '</dd>' +
          '<dt>aliases</dt><dd>' + esc((c.aliases || []).join(", ")) + '</dd>' +
          '<dt>confidence</dt><dd>' + esc(c.confidence) + '</dd>' +
          '</dl>' +
          '<div class="field-block" style="margin-top:16px"><div class="k">Evidence — entity A</div><ul class="premises">' + aq + '</ul></div>' +
          '<div class="field-block"><div class="k">Evidence — entity B</div><ul class="premises">' + bq + '</ul></div>';
      } else {
        const premises = (c.premises || []).map(function (p) { return '<li>' + esc(p) + '</li>'; }).join("");
        inner =
          '<h2>' + esc(it.name) + '</h2>' +
          '<div class="meta">inference · ' + esc(it.kind) + ' · ' + esc(it.candidateId) + '</div>' +
          '<dl class="kv">' +
          '<dt>source</dt><dd>' + esc(c.sourceId) + '</dd>' +
          '<dt>target</dt><dd>' + esc(c.targetId) + '</dd>' +
          '<dt>confidence</dt><dd>' + esc(c.confidence) + '</dd>' +
          '</dl>' +
          '<div class="field-block" style="margin-top:16px"><div class="k">Premises</div><ul class="premises">' + premises + '</ul></div>' +
          '<div class="field-block"><div class="k">Rationale</div><div class="v redacted">(audit-only — redacted; never exported)</div></div>';
      }
      inner +=
        '<div class="actions">' +
        '<button class="approve" ' + (it.status === "approved" ? "disabled" : "") + ' onclick="dispose(\\'approve\\')">Approve</button>' +
        '<button class="reject" ' + (it.status === "rejected" ? "disabled" : "") + ' onclick="dispose(\\'reject\\')">Reject</button>' +
        '<span class="status-note">current: <strong>' + it.status + '</strong></span>' +
        '</div>';
      document.getElementById("detail").innerHTML = inner;
    }

    async function dispose(action) {
      if (!selected) return;
      const res = await fetch("/api/" + action, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ layer: selected.layer, candidateId: selected.candidateId }),
      });
      if (!res.ok) {
        const err = await res.json().catch(function () { return {}; });
        alert("Failed: " + (err.detail || err.error || res.status));
        return;
      }
      await load();
    }

    // First paint: the list is already server-rendered. Just wire up the click
    // handlers against the hydrated STATE (no initial fetch).
    renderList();
    renderCounts();
  </script>
</body>
</html>`;

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

function parseArgs(argv: string[]): ReviewServerOptions {
  let candidatesDir = process.env.REVIEW_UI_CANDIDATES_DIR ?? "examples/angars/candidates";
  let dispositionsDir =
    process.env.REVIEW_UI_DISPOSITIONS_DIR ?? "examples/angars/out/dispositions";
  let port = Number(process.env.PORT ?? process.env.REVIEW_UI_PORT ?? DEFAULT_PORT);

  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--candidates") candidatesDir = argv[++i];
    else if (a === "--dispositions") dispositionsDir = argv[++i];
    else if (a === "--port") port = Number(argv[++i]);
  }
  return {
    candidatesDir: path.resolve(candidatesDir),
    dispositionsDir: path.resolve(dispositionsDir),
    port,
  };
}

async function main(): Promise<void> {
  const opts = parseArgs(process.argv.slice(2));
  const server = createReviewServer(opts);
  const port = opts.port ?? DEFAULT_PORT;
  server.listen(port, "0.0.0.0", () => {
    // eslint-disable-next-line no-console
    console.log(`Candidate review UI running at http://localhost:${port}`);
    // eslint-disable-next-line no-console
    console.log(`  candidates:   ${opts.candidatesDir}`);
    // eslint-disable-next-line no-console
    console.log(`  dispositions: ${opts.dispositionsDir}`);
  });
}

function isEntryPoint(): boolean {
  const invoked = process.argv[1];
  if (!invoked) return false;
  try {
    return import.meta.url === pathToFileURL(invoked).href;
  } catch {
    return false;
  }
}

// Only start listening when run as the entry point. Importing this module
// (e.g. from a test that wants createReviewServer) must NOT bind a port.
if (isEntryPoint()) {
  main().catch((err) => {
    // eslint-disable-next-line no-console
    console.error(err);
    process.exit(1);
  });
}

// Referenced so `fileURLToPath` import is not flagged unused when the guard is
// tree-shaken in some builds; also useful for tests wanting the module path.
export const MODULE_PATH = fileURLToPath(import.meta.url);
export type { Layer };
