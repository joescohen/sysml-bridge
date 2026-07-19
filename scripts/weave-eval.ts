/**
 * weave-eval.ts — LIVE proof-of-recall eval for examples/weave-mini/ (W4,
 * spec §6, §8 Phase W4).
 *
 *   pnpm weave:eval
 *
 * Runs a REAL prose-ingestion provider — OpenRouter/GLM (OpenRouterLlmProvider)
 * when OPENROUTER_API_KEY is set, else Anthropic (AnthropicLlmProvider) —
 * over the weave-mini corpus — NOT the recorded fixture-responses.json the
 * CI test uses — then the SAME deterministic downstream pipeline (mention
 * derivation -> auto-cluster -> merge suggestion -> cross-document
 * co-occurrence -> chain enumeration) and scores the result against
 * examples/weave-mini/answer-key.json.
 *
 * This is a REPORT, not a gate: exits 0 regardless of score (the CI
 * deterministic-layer test — packages/candidates/src/__tests__/weave-mini-eval.test.ts —
 * is the gate; this script only exists to show how a live provider performs
 * against the same fixed corpus). Scores are inherently PROVIDER-DEPENDENT —
 * model version, prompt drift, and sampling all move the numbers. Because a
 * live LLM will not reproduce the fixture's exact wording, matching below is
 * fuzzy (normSurface equality, then token-Jaccard) rather than the CI test's
 * exact-id pinning.
 *
 * Requires OPENROUTER_API_KEY (preferred) or ANTHROPIC_API_KEY. Absent both -> a
 * clean guard message and a non-zero exit (no stack trace, no fabricated output) — mirrors the
 * KEY-REQUIRED convention used elsewhere in this repo (see
 * packages/candidates/src/prose/__tests__/gc-real-run.test.ts).
 */

import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { runIngestPipeline } from "../packages/candidates/src/prose/ingest-pipeline.js";
import { parseDocument } from "../packages/candidates/src/prose/parsers/dispatch.js";
import {
  AnthropicLlmProvider,
  OpenRouterLlmProvider,
  type LlmProvider,
} from "../packages/candidates/src/prose/llm-provider.js";
import { resolveOpenRouterModel } from "../packages/candidates/src/openrouter-client.js";
import { autoCluster, suggestMerges } from "../packages/candidates/src/entities/index.js";
import type { EntityRecord } from "../packages/candidates/src/entities/index.js";
import {
  enumerateCooccurrence,
  enumerateChains,
  type AcceptedRelation,
} from "../packages/candidates/src/inference/index.js";
import type { MentionRecord } from "../packages/candidates/src/mentions/index.js";
import { normSurface } from "@sysml-bridge/model";

const WEAVE_MINI_DIR = join(import.meta.dirname, "../examples/weave-mini");

interface AnswerKey {
  entities: {
    crossDocumentExactAlias: Array<{ entityId: string; kind: string; canonicalName: string; mentionDocIds: string[] }>;
    acronymPair: {
      full: { entityId: string; kind: string; canonicalName: string };
      acronym: { entityId: string; kind: string; canonicalName: string };
      expectedMergeReason: string;
    };
  };
  trap: {
    surfaceForm: string;
    entityA: { entityId: string; kind: string };
    entityB: { entityId: string; kind: string };
  };
  crossDocumentLinks: {
    links: Array<{ family: string; sourceId: string; sourceName: string; targetId: string; targetName: string }>;
  };
  chain: {
    sourceName: string;
    middleName: string;
    targetName: string;
    leftFamily: string;
    rightFamily: string;
  };
}

interface WeaveMiniDoc {
  file: string;
  documentId: string;
  sectionPath: string;
}

// ── guard: zero API key -> clean report-mode skip, no crash ────────────────

function guardMissingKey(): never {
  process.stderr.write(
    [
      "[weave:eval] No live provider key set (OPENROUTER_API_KEY or ANTHROPIC_API_KEY).",
      "[weave:eval] weave:eval exercises a REAL prose-ingestion provider against",
      "[weave:eval] examples/weave-mini/ — it cannot run without a live key.",
      "[weave:eval] This is a REPORT, not a gate: the deterministic-layer eval that DOES",
      "[weave:eval] run in CI with zero API key is",
      "[weave:eval]   pnpm --filter @sysml-bridge/candidates test -- weave-mini-eval",
      "[weave:eval] Set OPENROUTER_API_KEY (model via OPENROUTER_MODEL, default z-ai/glm-5.2)",
      "[weave:eval] or ANTHROPIC_API_KEY and re-run `pnpm weave:eval` for a live score.",
      "",
    ].join("\n"),
  );
  process.exit(1);
}

/**
 * Select the live prose provider: OpenRouter (GLM) when OPENROUTER_API_KEY is set,
 * else Anthropic when ANTHROPIC_API_KEY is set. Guards (exit 1) when neither.
 */
function selectEvalProvider(): LlmProvider {
  const orKey = process.env["OPENROUTER_API_KEY"];
  if (orKey) {
    const model = resolveOpenRouterModel();
    process.stderr.write(`[weave:eval] using OpenRouterLlmProvider (model=${model})\n`);
    return new OpenRouterLlmProvider(orKey);
  }
  const anthKey = process.env["ANTHROPIC_API_KEY"];
  if (anthKey) {
    process.stderr.write("[weave:eval] using AnthropicLlmProvider (ANTHROPIC_API_KEY present)\n");
    return new AnthropicLlmProvider(anthKey);
  }
  guardMissingKey();
}

// ── fuzzy matching helpers (live LLM wording will not match fixtures exactly) ──

function fuzzyMatches(candidate: string, target: string): boolean {
  if (normSurface(candidate) === normSurface(target)) return true;
  const a = new Set(normSurface(candidate).split(" ").filter((t) => t.length >= 3));
  const b = new Set(normSurface(target).split(" ").filter((t) => t.length >= 3));
  if (a.size === 0 || b.size === 0) return false;
  let inter = 0;
  for (const t of a) if (b.has(t)) inter++;
  const union = a.size + b.size - inter;
  return union > 0 && inter / union >= 0.5;
}

function findEntity(entities: readonly EntityRecord[], kind: string, name: string): EntityRecord | undefined {
  return entities.find(
    (e) => e.kind === kind && (fuzzyMatches(e.canonicalName, name) || e.aliases.some((a) => fuzzyMatches(a, name))),
  );
}

// ── main ─────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  // Guards (exit 1) when neither OPENROUTER_API_KEY nor ANTHROPIC_API_KEY is set.
  const provider = selectEvalProvider();

  const answerKey = JSON.parse(
    await readFile(join(WEAVE_MINI_DIR, "answer-key.json"), "utf8"),
  ) as AnswerKey;
  const fixtures = JSON.parse(
    await readFile(join(WEAVE_MINI_DIR, "fixture-responses.json"), "utf8"),
  ) as { docs: WeaveMiniDoc[] };

  const allMentions: MentionRecord[] = [];
  for (const d of fixtures.docs) {
    const filePath = join(WEAVE_MINI_DIR, "corpus", d.file);
    const raw = await readFile(filePath);
    const docSha256 = createHash("sha256").update(raw).digest("hex");
    const parsed = await parseDocument(filePath);

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
      chunkOptions: { chunkSize: 20_000, chunkOverlap: 0 },
    });
    process.stderr.write(
      `[weave:eval] ${d.file}: ${result.candidates.length} candidate(s), ${result.mentions.length} mention(s), ` +
        `droppedUnverbatimMentions=${result.droppedUnverbatimMentions}\n`,
    );
    allMentions.push(...result.mentions);
  }

  const entities = autoCluster(allMentions);
  const merges = suggestMerges(entities);
  const cooccurrence = enumerateCooccurrence(entities, allMentions, {
    families: ["allocation", "modeMembership"],
  });

  // ── entity resolution scoring ──────────────────────────────────────────────
  const entityRows: Array<{ label: string; found: boolean }> = [];
  for (const e of answerKey.entities.crossDocumentExactAlias) {
    entityRows.push({ label: `entity: ${e.canonicalName} (${e.kind})`, found: findEntity(entities, e.kind, e.canonicalName) !== undefined });
  }
  const { full, acronym } = answerKey.entities.acronymPair;
  const fullEntity = findEntity(entities, full.kind, full.canonicalName);
  const acronymEntity = findEntity(entities, acronym.kind, acronym.canonicalName);
  entityRows.push({ label: `entity: ${full.canonicalName} (${full.kind})`, found: fullEntity !== undefined });
  entityRows.push({ label: `entity: ${acronym.canonicalName} (${acronym.kind}, acronym)`, found: acronymEntity !== undefined });

  const acronymMergeFound =
    fullEntity !== undefined &&
    acronymEntity !== undefined &&
    merges.some(
      (m) =>
        (m.entityIdA === fullEntity.entityId && m.entityIdB === acronymEntity.entityId) ||
        (m.entityIdA === acronymEntity.entityId && m.entityIdB === fullEntity.entityId),
    );
  entityRows.push({ label: "merge-proposal: CHC <-> Cargo Handling Controller (acronym)", found: acronymMergeFound });

  const trapA = findEntity(entities, answerKey.trap.entityA.kind, answerKey.trap.surfaceForm);
  const trapB = findEntity(entities, answerKey.trap.entityB.kind, answerKey.trap.surfaceForm);
  const trapSeparate = trapA !== undefined && trapB !== undefined && trapA.entityId !== trapB.entityId;
  const trapNotMerged =
    trapA === undefined ||
    trapB === undefined ||
    !merges.some(
      (m) =>
        (m.entityIdA === trapA.entityId && m.entityIdB === trapB.entityId) ||
        (m.entityIdA === trapB.entityId && m.entityIdB === trapA.entityId),
    );
  entityRows.push({ label: "trap: 'Interlock' mode/component stay SEPARATE", found: trapSeparate });
  entityRows.push({ label: "trap: 'Interlock' mode/component NEVER merge-proposed", found: trapNotMerged });

  // ── link discovery scoring ─────────────────────────────────────────────────
  const linkRows: Array<{ label: string; found: boolean }> = [];
  for (const link of answerKey.crossDocumentLinks.links) {
    const src = entities.find((e) => fuzzyMatches(e.canonicalName, link.sourceName));
    const tgt = entities.find((e) => fuzzyMatches(e.canonicalName, link.targetName));
    const found =
      src !== undefined &&
      tgt !== undefined &&
      cooccurrence.candidates.some(
        (c) => c.relationFamily === link.family && c.sourceId === src.entityId && c.targetId === tgt.entityId,
      );
    linkRows.push({ label: `link (${link.family}): ${link.sourceName} -> ${link.targetName}`, found });
  }

  // ── chain scoring (endpoints resolved by fuzzy match against live entities) ─
  const chainSrc = entities.find((e) => fuzzyMatches(e.canonicalName, answerKey.chain.sourceName));
  const chainMid = entities.find((e) => fuzzyMatches(e.canonicalName, answerKey.chain.middleName));
  const chainTgt = entities.find((e) => fuzzyMatches(e.canonicalName, answerKey.chain.targetName));
  let chainFound = false;
  if (chainSrc && chainMid && chainTgt) {
    const accepted: AcceptedRelation[] = [
      { id: "eval-r1", family: answerKey.chain.leftFamily, sourceId: chainSrc.entityId, targetId: chainMid.entityId, status: "accepted" },
      { id: "eval-r2", family: answerKey.chain.rightFamily, sourceId: chainMid.entityId, targetId: chainTgt.entityId, status: "accepted" },
    ];
    const chains = enumerateChains(accepted);
    chainFound = chains.candidates.some(
      (c) => c.sourceId === chainSrc!.entityId && c.middleId === chainMid!.entityId && c.targetId === chainTgt!.entityId,
    );
  }
  linkRows.push({
    label: `chain: ${answerKey.chain.sourceName} -> ${answerKey.chain.middleName} -> ${answerKey.chain.targetName}`,
    found: chainFound,
  });

  // ── print scored table ──────────────────────────────────────────────────────
  const print = (title: string, rows: Array<{ label: string; found: boolean }>): void => {
    console.log(`\n${title}`);
    console.log("-".repeat(title.length));
    for (const r of rows) console.log(`  [${r.found ? "PASS" : "MISS"}] ${r.label}`);
    const recall = rows.length === 0 ? 0 : rows.filter((r) => r.found).length / rows.length;
    console.log(`  recall: ${(recall * 100).toFixed(0)}% (${rows.filter((r) => r.found).length}/${rows.length})`);
  };

  console.log("\n=== weave-mini live eval (provider-dependent — see examples/weave-mini/README.md) ===");
  print("Entity resolution", entityRows);
  print("Link discovery (co-occurrence + chain)", linkRows);

  // Suggested-merge precision: of every merge suggestMerges proposed, how
  // many are the intended acronym pair (vs. spurious over-merging)?
  const wantedMergeCount = acronymMergeFound ? 1 : 0;
  const mergePrecision = merges.length === 0 ? 1 : wantedMergeCount / merges.length;
  console.log(
    `\nMerge-suggestion precision: ${(mergePrecision * 100).toFixed(0)}% (${wantedMergeCount}/${merges.length} proposed merges were the intended acronym pair)`,
  );
  console.log(
    "\nNOTE: this is a REPORT, not a gate — scores depend on the live provider/model and are expected to vary run to run.",
  );
}

main().catch((err) => {
  process.stderr.write(`[weave:eval] ERROR: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exit(2);
});
