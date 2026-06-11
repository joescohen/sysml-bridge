#!/usr/bin/env tsx
/**
 * w3-inspect-queued.ts — Wave-3 T3 helper: list QUEUED allocation candidates from
 * inference-candidates.json with their premise IDS and resolved premise LABELS so the
 * integrator can read the premises and decide which to fixture-approve.
 *
 * Resolves premise ids against the composed IR (corpus entities + prose + N2 flows)
 * to a short human label for review ONLY — committed output uses ids, never corpus quotes.
 */

import { readFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { composeIR } from "@sysml-bridge/ir";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const CANDIDATES = join(REPO_ROOT, "examples/angars/model/inference-candidates.json");
const EXTRACTED = join(REPO_ROOT, "examples/angars/model/extracted.json");
const PROSE = join(REPO_ROOT, "examples/angars/model/prose-approved-modes.json");

interface QueuedRec {
  id: string;
  relationFamily: string;
  sourceId: string;
  targetId: string;
  stage: string;
  confidence: number;
  premises: string[];
  debateVerdict?: string;
  debateAdvocate?: number;
  debateChallenger?: number;
}

async function main(): Promise<void> {
  const file = JSON.parse(await readFile(CANDIDATES, "utf8"));
  const ir = await composeIR(EXTRACTED, PROSE);
  const corpus = ir.extracted;

  // Build id -> label map for premise resolution
  const label = new Map<string, string>();
  for (const f of corpus.functions ?? []) label.set(f.id, `FN ${f.naturalKey} ${f.name}`);
  for (const c of corpus.components ?? []) label.set(c.id, `COMP ${c.name}`);
  for (const s of corpus.subsystems ?? []) label.set(s.id, `SUBSYS ${s.naturalKey ?? s.name}`);
  for (const n of corpus.n2Interfaces ?? []) label.set(n.id, `N2 ${n.sourceId}->${n.targetId} [${(n as any).scope}]`);
  for (const e of ir.proseEntries) label.set(e.id, `PROSE ${e.kind}`);

  const fnById = new Map((corpus.functions ?? []).map((f: any) => [f.id, f]));
  const compById = new Map((corpus.components ?? []).map((c: any) => [c.id, c]));

  const records: QueuedRec[] = (file.records ?? []).filter(
    (r: QueuedRec) => r.stage === "queued" && r.relationFamily === "allocation"
  );

  console.log(`IR hash: ${file.irHash}`);
  console.log(`Queued allocation candidates: ${records.length}\n`);

  // Sort by confidence desc for review
  records.sort((a, b) => b.confidence - a.confidence);

  for (const r of records) {
    const src = fnById.get(r.sourceId) as any;
    const tgt = compById.get(r.targetId) as any;
    console.log(`── id: ${r.id} | conf: ${r.confidence.toFixed(2)} | verdict: ${r.debateVerdict ?? "(no debate)"} ──`);
    console.log(`   source: ${src ? `FN ${src.naturalKey} "${src.name}"` : r.sourceId}`);
    console.log(`   target: ${tgt ? `COMP "${tgt.name}"` : r.targetId}`);
    if (r.debateAdvocate !== undefined) {
      console.log(`   debate: advocate=${r.debateAdvocate} challenger=${r.debateChallenger}`);
    }
    console.log(`   premises (${r.premises.length}):`);
    for (const p of r.premises) {
      console.log(`     - ${p}  =>  ${label.get(p) ?? "(unresolved id)"}`);
    }
    console.log("");
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
