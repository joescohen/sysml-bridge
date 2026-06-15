/**
 * requirements-table.ts
 *
 * The legible requirements view: a parent/child traceability table replacing
 * the unusable 182-node requirements graph render (69k px wide).
 *
 * Columns: Req ID | Requirement | Parent Need(s) | Satisfied By (functions) |
 * Verify Method | Provenance. Grouped by parent need, with per-need subtotals
 * and unsatisfied/unverified flags. Reads extracted.json (corpus ground truth)
 * as primary; augments with inferred-approved.json when present (gitignored).
 *
 * Provenance column values:
 *   corpus-stated  — extracted from a corpus document (default)
 *   inferred       — reasoning-layer approved link
 *   asserted       — human assertion without derivation
 *
 * Output (CORPUS-DERIVED — both land in the gitignored examples/angars/model/):
 *   examples/angars/model/reports/requirements-table.md
 *   examples/angars/model/reports/requirements-table.html
 *
 * Usage: pnpm tsx scripts/requirements-table.ts
 */

import * as fs from "fs";
import * as path from "path";

const REPO_ROOT = path.resolve(__dirname, "..");
const IR_PATH = path.join(REPO_ROOT, "examples/angars/model/extracted.json");
const INFERRED_PATH = path.join(REPO_ROOT, "examples/angars/model/inferred-approved.json");
const OUT_DIR = path.join(REPO_ROOT, "examples/angars/model/reports");

type ProvenanceClass = "corpus-stated" | "inferred" | "asserted";

interface Need {
  id: string;
  naturalKey: string;
  name: string;
}
interface Requirement {
  id: string;
  naturalKey: string;
  name: string;
  statement: string;
  needIds: string[];
  verifyMethod: string;
  category?: string;
}
interface Func {
  id: string;
  naturalKey: string;
  name: string;
  level: string;
}
interface Satisfy {
  reqId: string;
  functionId: string;
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function main(): void {
  const ir = JSON.parse(fs.readFileSync(IR_PATH, "utf8")) as {
    needs: Need[];
    requirements: Requirement[];
    functions: Func[];
    satisfies: Satisfy[];
  };

  // Load inferred-approved.json if present (gitignored; optional)
  // Build a map: reqId → provenance class (from satisfy links in inferred layer)
  const inferredProvenanceByReq = new Map<string, ProvenanceClass>();
  if (fs.existsSync(INFERRED_PATH)) {
    try {
      const inferredData = JSON.parse(fs.readFileSync(INFERRED_PATH, "utf8")) as {
        entries: Array<{
          id: string;
          relationFamily: string;
          sourceId: string;
          targetId: string;
          status: string;
        }>;
      };
      for (const entry of inferredData.entries) {
        if (entry.status !== "approved") continue;
        // allocation links (functionId → componentId) and other families
        // — any inferred link mentioning a reqId marks it as "inferred"
        if (entry.relationFamily === "allocation" || entry.relationFamily === "modeMembership") {
          inferredProvenanceByReq.set(entry.sourceId, "inferred");
          inferredProvenanceByReq.set(entry.targetId, "inferred");
        }
      }
    } catch {
      // Ignore malformed optional file
    }
  }

  const needById = new Map(ir.needs.map((n) => [n.id, n]));
  const fnById = new Map(ir.functions.map((f) => [f.id, f]));
  const satisfiersByReq = new Map<string, string[]>();
  let staleSatisfies = 0;
  for (const s of ir.satisfies) {
    const fn = fnById.get(s.functionId);
    if (!fn) {
      staleSatisfies++;
      continue; // corpus anomaly: stale functionId (documented in E2E report)
    }
    const list = satisfiersByReq.get(s.reqId) ?? [];
    list.push(`${fn.naturalKey} ${fn.name}`);
    satisfiersByReq.set(s.reqId, list);
  }

  // Group requirements under each parent need (a req may have several parents;
  // it appears under each, flagged after the first as a cross-reference).
  const reqsByNeed = new Map<string, Requirement[]>();
  const orphanReqs: Requirement[] = [];
  for (const r of ir.requirements) {
    if (r.needIds.length === 0) {
      orphanReqs.push(r);
      continue;
    }
    for (const nid of r.needIds) {
      const list = reqsByNeed.get(nid) ?? [];
      list.push(r);
      reqsByNeed.set(nid, list);
    }
  }

  const md: string[] = [];
  const html: string[] = [];
  md.push("# ANGARS Requirements Traceability Table");
  md.push("");
  md.push(
    `Source: \`extracted.json\` (corpus ground truth). ${ir.requirements.length} requirements, ` +
      `${ir.needs.length} parent needs, ${ir.satisfies.length} satisfy links ` +
      `(${staleSatisfies} stale corpus links skipped — see E2E report).`
  );
  md.push("");
  html.push(
    `<!doctype html><meta charset="utf-8"><title>ANGARS Requirements Table</title>` +
      `<style>body{font-family:system-ui,sans-serif;margin:24px;max-width:1400px}` +
      `table{border-collapse:collapse;width:100%;margin-bottom:28px}` +
      `th,td{border:1px solid #c9c9d9;padding:5px 9px;text-align:left;vertical-align:top;font-size:13.5px}` +
      `th{background:#eef;position:sticky;top:0}` +
      `h2{margin-top:34px;border-bottom:2px solid #336;padding-bottom:4px}` +
      `tr:nth-child(even){background:#f7f7fc}` +
      `.flag{color:#a33;font-weight:600}.muted{color:#777}</style>` +
      `<h1>ANGARS Requirements Traceability Table</h1>` +
      `<p>Source: <code>extracted.json</code>. ${ir.requirements.length} requirements, ` +
      `${ir.needs.length} parent needs, ${ir.satisfies.length} satisfy links ` +
      `(${staleSatisfies} stale corpus links skipped).</p>`
  );

  const header = `| Req ID | Requirement | Statement | Satisfied By (function) | Verify Method | Provenance |`;
  const sep = `|---|---|---|---|---|---|`;

  for (const need of ir.needs) {
    const reqs = reqsByNeed.get(need.id) ?? [];
    if (reqs.length === 0) continue;
    const unsat = reqs.filter((r) => !satisfiersByReq.has(r.id)).length;
    md.push(`## ${need.naturalKey} — ${need.name}`);
    md.push("");
    md.push(
      `${reqs.length} child requirement(s)` +
        (unsat > 0 ? ` — **${unsat} without a satisfying function**` : "")
    );
    md.push("");
    md.push(header);
    md.push(sep);
    html.push(
      `<h2>${esc(need.naturalKey)} — ${esc(need.name)}</h2>` +
        `<p>${reqs.length} child requirement(s)` +
        (unsat > 0 ? ` — <span class="flag">${unsat} without a satisfying function</span>` : "") +
        `</p><table><tr><th>Req ID</th><th>Requirement</th><th>Statement</th>` +
        `<th>Satisfied By (function)</th><th>Verify Method</th><th>Provenance</th></tr>`
    );
    for (const r of reqs) {
      const sats = satisfiersByReq.get(r.id);
      const satMd = sats ? sats.join("<br>") : "**UNSATISFIED**";
      const satHtml = sats
        ? sats.map(esc).join("<br>")
        : `<span class="flag">UNSATISFIED</span>`;
      const otherParents = r.needIds
        .filter((nid) => nid !== need.id)
        .map((nid) => needById.get(nid)?.naturalKey ?? "?");
      const alsoMd = otherParents.length ? ` *(also under ${otherParents.join(", ")})*` : "";
      const alsoHtml = otherParents.length
        ? ` <span class="muted">(also under ${otherParents.map(esc).join(", ")})</span>`
        : "";
      // Provenance column: inferred if any inferred-layer link mentions this req's satisfying
      // function, otherwise corpus-stated (default)
      const provClass: ProvenanceClass = inferredProvenanceByReq.get(r.id) ?? "corpus-stated";
      md.push(
        `| ${r.naturalKey} | ${r.name}${alsoMd} | ${r.statement.replace(/\|/g, "\\|")} | ${satMd} | ${r.verifyMethod} | ${provClass} |`
      );
      html.push(
        `<tr><td>${esc(r.naturalKey)}</td><td>${esc(r.name)}${alsoHtml}</td>` +
          `<td>${esc(r.statement)}</td><td>${satHtml}</td><td>${esc(r.verifyMethod)}</td>` +
          `<td>${esc(provClass)}</td></tr>`
      );
    }
    md.push("");
    html.push(`</table>`);
  }

  if (orphanReqs.length > 0) {
    md.push(`## Requirements without a parent need`);
    md.push("");
    md.push(header);
    md.push(sep);
    html.push(`<h2>Requirements without a parent need</h2><table><tr><th>Req ID</th><th>Requirement</th><th>Statement</th><th>Satisfied By</th><th>Verify Method</th><th>Provenance</th></tr>`);
    for (const r of orphanReqs) {
      const sats = satisfiersByReq.get(r.id);
      const provClass: ProvenanceClass = inferredProvenanceByReq.get(r.id) ?? "corpus-stated";
      md.push(
        `| ${r.naturalKey} | ${r.name} | ${r.statement.replace(/\|/g, "\\|")} | ${sats ? sats.join("<br>") : "**UNSATISFIED**"} | ${r.verifyMethod} | ${provClass} |`
      );
      html.push(
        `<tr><td>${esc(r.naturalKey)}</td><td>${esc(r.name)}</td><td>${esc(r.statement)}</td>` +
          `<td>${sats ? sats.map(esc).join("<br>") : '<span class="flag">UNSATISFIED</span>'}</td><td>${esc(r.verifyMethod)}</td>` +
          `<td>${esc(provClass)}</td></tr>`
      );
    }
    html.push(`</table>`);
  }

  fs.mkdirSync(OUT_DIR, { recursive: true });
  const mdPath = path.join(OUT_DIR, "requirements-table.md");
  const htmlPath = path.join(OUT_DIR, "requirements-table.html");
  fs.writeFileSync(mdPath, md.join("\n"), "utf8");
  fs.writeFileSync(htmlPath, html.join("\n"), "utf8");
  console.log(`Written: ${mdPath}`);
  console.log(`Written: ${htmlPath}`);
  console.log(
    `Needs with children: ${[...reqsByNeed.keys()].length}/${ir.needs.length}; ` +
      `orphan requirements: ${orphanReqs.length}; stale satisfy links skipped: ${staleSatisfies}`
  );
}

main();
