/**
 * One-off computation script: runs the REAL weave-mini derivation pipeline
 * (parse -> chunk -> ingest (fixture provider) -> mentions -> auto-cluster ->
 * suggest-merges -> cooccurrence -> chains) and prints every id the answer
 * key needs, PLUS a full dump of entities/links so the output can be
 * eyeballed for correctness before pinning into answer-key.json.
 *
 * Not part of build/test — run manually via `tsx fixtures/wm-compute-answer-key.ts`
 * from packages/candidates/, mirroring gen-fixtures.ts's own docstring.
 */
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { runIngestPipeline } from "../src/prose/ingest-pipeline.js";
import { parseDocument } from "../src/prose/parsers/dispatch.js";
import type { CandidateProposal, LlmProvider } from "../src/prose/llm-provider.js";
import { autoCluster, entityIdFor, suggestMerges } from "../src/entities/index.js";
import { enumerateCooccurrence, enumerateChains, chainStableId } from "../src/inference/index.js";
import type { MentionRecord } from "../src/mentions/index.js";
import type { AcceptedRelation } from "../src/inference/index.js";
import { entityMergePairKey } from "@sysml-bridge/model";

// fixture-responses.json is DATA (not a TS module) — read by path, same as
// the eval test — so this dev script and the test can never drift on how
// the recorded proposals are loaded.
type FixtureProposal = Omit<CandidateProposal, "citedChunkId">;
interface FixtureResponses {
  docs: Array<{ file: string; documentId: string; sectionPath: string }>;
  proposals: Record<string, FixtureProposal[]>;
}
class FixtureProvider implements LlmProvider {
  constructor(
    private readonly documentId: string,
    private readonly proposalsByDoc: Record<string, FixtureProposal[]>,
  ) {}
  async propose(chunkId: string): Promise<CandidateProposal[]> {
    const batch = this.proposalsByDoc[this.documentId] ?? [];
    return batch.map((p) => ({ ...p, citedChunkId: chunkId }));
  }
}

async function main() {
  const WEAVE_MINI_DIR = join(process.cwd(), "../../examples/weave-mini");
  const CORPUS = join(WEAVE_MINI_DIR, "corpus");
  const fixtures = JSON.parse(
    await readFile(join(WEAVE_MINI_DIR, "fixture-responses.json"), "utf8"),
  ) as FixtureResponses;
  const allMentions: MentionRecord[] = [];
  let totalDropped = 0;

  for (const d of fixtures.docs) {
    const filePath = join(CORPUS, d.file);
    const raw = await readFile(filePath);
    const docSha256 = createHash("sha256").update(raw).digest("hex");
    const parsed = await parseDocument(filePath);
    const provider = new FixtureProvider(d.documentId, fixtures.proposals);

    const result = await runIngestPipeline({
      text: parsed.text,
      context: {
        documentHash: docSha256,
        sectionId: "sec-root",
        sectionPath: d.sectionPath,
        pageStart: 0,
        pageEnd: Math.max(parsed.pages.length - 1, 0),
        documentId: d.documentId,
      },
      provider,
      chunkOptions: { chunkSize: 20000, chunkOverlap: 0 },
    });

    console.log(
      `${d.file}: chunks=${result.totalChunks} candidates=${result.candidates.length} mentions=${result.mentions.length} droppedUnverbatimMentions=${result.droppedUnverbatimMentions}`,
    );
    totalDropped += result.droppedUnverbatimMentions;
    allMentions.push(...result.mentions);
  }

  console.log(`\ntotal mentions=${allMentions.length} totalDroppedUnverbatimMentions=${totalDropped}`);

  const entities = autoCluster(allMentions);
  console.log(`\n=== entities (${entities.length}) ===`);
  for (const e of entities) {
    console.log(`${e.entityId}  kind=${e.kind}  canonical=${JSON.stringify(e.canonicalName)}  aliases=${JSON.stringify(e.aliases)}  mentionDocs=${[...new Set(e.mentionIds.map((mid) => allMentions.find((m) => m.mentionId === mid)?.citation.docId))].join(",")}`,
    );
  }

  const merges = suggestMerges(entities);
  console.log(`\n=== suggested merges (${merges.length}) ===`);
  for (const m of merges) {
    console.log(`${m.id}  reason=${m.reason}  A=${m.entityIdA} B=${m.entityIdB}  canonical=${JSON.stringify(m.canonicalName)}`);
  }

  // ── Direct id sanity: entityIdFor is order-independent (kind + normSurface only)
  const wanted: Array<[string, "component" | "function" | "requirement" | "mode" | "interface" | "flow" | "unknown"]> = [
    ["Cargo Handling Controller", "component"],
    ["CHC", "component"],
    ["Position Sensor Array", "component"],
    ["Conveyor Drive Motor", "component"],
    ["Load Cell Assembly", "component"],
    ["Interlock", "component"],
    ["Boom Actuator", "component"],
    ["Fault Logger", "component"],
    ["Diagnostic Interface", "component"],
    ["Detect Cargo Presence", "function"],
    ["Compute Load Distribution", "function"],
    ["Monitor Conveyor Speed", "function"],
    ["Validate Load Capacity", "function"],
    ["Log Fault Event", "function"],
    ["Transmit Fault Summary", "function"],
    ["Interlock", "mode"],
    ["Standby", "mode"],
  ];
  console.log(`\n=== entityIdFor(kind, surface) ===`);
  const idOf = new Map<string, string>();
  for (const [surface, kind] of wanted) {
    const id = entityIdFor(kind, surface);
    idOf.set(`${kind}:${surface}`, id);
    console.log(`${kind.padEnd(11)} ${surface.padEnd(28)} -> ${id}`);
  }

  const chcId = idOf.get("component:CHC")!;
  const chcFullId = idOf.get("component:Cargo Handling Controller")!;
  console.log(`\nentityMergePairKey(CHC, Cargo Handling Controller) = ${entityMergePairKey(chcId, chcFullId)}`);

  // ── Cross-document cooccurrence (section-prefix bridged via "Cargo Handling") ──
  const cooc = enumerateCooccurrence(entities, allMentions, {
    families: ["allocation", "modeMembership"],
  });
  console.log(`\n=== cooccurrence candidates (${cooc.candidates.length}) ===`);
  for (const c of cooc.candidates) {
    console.log(`${c.id}  family=${c.relationFamily} kind=${c.cooccurKind}  ${c.sourceId} -> ${c.targetId}`);
  }

  const monitorId = idOf.get("function:Monitor Conveyor Speed")!;
  const diagId = idOf.get("component:Diagnostic Interface")!;
  const detectId = idOf.get("function:Detect Cargo Presence")!;
  const interlockCompId = idOf.get("component:Interlock")!;
  const computeId = idOf.get("function:Compute Load Distribution")!;
  const standbyId = idOf.get("mode:Standby")!;
  const posSensorId = idOf.get("component:Position Sensor Array")!;

  console.log(`\nL1 expected id (allocation, MonitorConveyorSpeed -> DiagnosticInterface): present=${cooc.candidates.some((c) => c.sourceId === monitorId && c.targetId === diagId && c.relationFamily === "allocation")}`);
  console.log(`L2 expected id (allocation, DetectCargoPresence -> Interlock[component]): present=${cooc.candidates.some((c) => c.sourceId === detectId && c.targetId === interlockCompId && c.relationFamily === "allocation")}`);
  console.log(`L3 expected id (modeMembership, ComputeLoadDistribution -> Standby): present=${cooc.candidates.some((c) => c.sourceId === computeId && c.targetId === standbyId && c.relationFamily === "modeMembership")}`);

  // ── 2-hop chain ──
  const doc1Chunk = "e7bb67061ae5fbe06de53471cae0ff78";
  const doc2Chunk = "490c9f768275e014354012c9012717c2";
  const accepted: AcceptedRelation[] = [
    {
      id: "wm-accepted-r1",
      family: "allocation",
      sourceId: detectId,
      targetId: posSensorId,
      status: "accepted",
      evidenceChunkIds: [doc1Chunk],
    },
    {
      id: "wm-accepted-r2",
      family: "containment",
      sourceId: posSensorId,
      targetId: chcFullId,
      status: "accepted",
      evidenceChunkIds: [doc2Chunk],
    },
  ];
  const chains = enumerateChains(accepted);
  console.log(`\n=== chains (${chains.candidates.length}), pendingSkipped=${chains.pendingSkipped} ===`);
  for (const c of chains.candidates) {
    console.log(`${c.stableId}  ${c.leftFamily}+${c.rightFamily}  ${c.sourceId} -> ${c.middleId} -> ${c.targetId}`);
  }
  const expectedChainId = chainStableId("allocation", "containment", detectId, posSensorId, chcFullId);
  console.log(`\nexpected chain id = ${expectedChainId}  present=${chains.candidates.some((c) => c.stableId === expectedChainId)}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
