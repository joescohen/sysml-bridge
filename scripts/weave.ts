/**
 * weave.ts — the gap-driven pass loop CLI (W3).
 *
 * One pass = audit → queue → propose → (human reviews) → recompose → re-audit →
 * record (spec §5). This script is the I/O orchestration for the PURE building
 * blocks in `packages/candidates/src/weave/`. It lives OUTSIDE any package src
 * so `packages/candidates` keeps no runtime dependency on `packages/gates`
 * (which `audit()` lives in) and so the no-auto-approve source-scan ratchet
 * (which walks only `packages/<pkg>/src`) never sees this orchestration.
 *
 *   pnpm weave --project <dir>              # open a pass: audit → propose → STOP
 *   pnpm weave --project <dir> --close-pass # recompose → re-audit → record + gate
 *
 * `weave` NEVER writes a disposition. An open pass writes proposals to the
 * normal review queue (`<project>/candidates/inference-candidates.json`) and a
 * pending-pass marker, then STOPS for the human. `--close-pass` recomposes,
 * re-audits, writes `<project>/passes/pass-NNN.json`, and enforces the HARD
 * convergence gate (non-zero exit if error findings increased or remain).
 *
 * Project layout (files are optional unless noted):
 *   <project>/model/<id>.json         FileModel (REQUIRED — the model store)
 *   <project>/extracted.json          corpus (optional; absent → corpus=null)
 *   <project>/entities.json           canonical entities (optional)
 *   <project>/mentions.json           mentions / co-occurrence signal (optional)
 *   <project>/accepted-relations.json AcceptedRelation[] for chains (optional)
 *   <project>/candidates/             review queue output (written by open pass)
 *   <project>/dispositions/           human dispositions input (read at close)
 *   <project>/passes/                 pass records + the pending-pass marker
 *
 * Provider (when --mock is not passed): OpenRouterInferenceProvider when
 * OPENROUTER_API_KEY is set (model via OPENROUTER_MODEL, default z-ai/glm-5.2),
 * else AnthropicInferenceProvider when ANTHROPIC_API_KEY is set; otherwise a
 * deterministic mock provider so the loop runs with zero API key (as the W3
 * done-criteria require).
 */

import { readFile, readdir, writeFile, mkdir, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

import {
  audit,
  type Finding,
} from "../packages/gates/src/index.js";
import {
  FileStore,
  composeIR,
  SCHEMA_VERSION,
  InferredApprovedEntrySchema,
  type InferredComposedIR,
  type SysmlElement,
} from "../packages/model/src/index.js";
import {
  loadEntityStoreFile,
  loadMentionStoreFile,
  planQueries,
  queryFamiliesToRelationFamilies,
  runTargetedInference,
  summarizeAudit,
  computeWarningsDelta,
  evaluateConvergence,
  passFileName,
  writePassRecordFile,
  parsePassRecord,
  projectApprovedInferredToRelations,
  type WeaveFinding,
  type GapContext,
  type PassRecord,
  type DispositionSummary,
  type EntityRecord,
  type MentionRecord,
  type AcceptedRelation,
} from "../packages/candidates/src/index.js";
import type { InferenceProvider } from "../packages/candidates/src/inference/inference-provider.js";
import type {
  ContextBundle,
  ProposeResult,
  ProposalOutput,
  RelationFamily,
} from "../packages/candidates/src/inference/types.js";

// ── CLI args ──────────────────────────────────────────────────────────────────

interface Args {
  project: string;
  closePass: boolean;
  dryRun: boolean;
  mock: boolean;
  budgetUsd?: number;
}

function parseArgs(argv: string[]): Args {
  let project = "";
  let closePass = false;
  let dryRun = false;
  let mock = false;
  let budgetUsd: number | undefined;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--project" || a === "-p") project = argv[++i] ?? "";
    else if (a === "--close-pass") closePass = true;
    else if (a === "--dry-run") dryRun = true;
    else if (a === "--mock") mock = true;
    else if (a === "--budget") budgetUsd = Number(argv[++i]);
    else if (!a.startsWith("-") && project === "") project = a;
  }
  if (project === "") {
    throw new Error(
      "usage: tsx scripts/weave.ts --project <dir> [--close-pass] [--budget <usd>] [--dry-run] [--mock]",
    );
  }
  return { project, closePass, dryRun, mock, budgetUsd };
}

// ── Deterministic mock provider (zero API key) ──────────────────────────────────

/** Cites every offered fact id → all premises resolve → queued (no debate at 0.95). */
class MockCiteProvider implements InferenceProvider {
  async propose(
    family: RelationFamily,
    sourceId: string,
    targetId: string,
    context: ContextBundle,
  ): Promise<ProposeResult> {
    return {
      kind: "proposal",
      proposal: {
        sourceId,
        targetId,
        relationFamily: family,
        premises: context.offeredFacts.map((f) => f.id),
        rationale: "audit-only (mock)",
        confidence: 0.95,
      },
    };
  }
  async advocate(): Promise<{ score: number; summary: string }> {
    return { score: 0.9, summary: "n/a" };
  }
  async challenge(
    _family: RelationFamily,
    _proposal: ProposalOutput,
    _advocateSummary: string,
    _context: ContextBundle,
  ): Promise<{ score: number; summary: string }> {
    return { score: 0.1, summary: "n/a" };
  }
}

async function selectProvider(mock: boolean, log: (m: string) => void): Promise<InferenceProvider> {
  const orKey = process.env["OPENROUTER_API_KEY"];
  const anthKey = process.env["ANTHROPIC_API_KEY"];
  if (mock || (!orKey && !anthKey)) {
    log(
      `[weave] using deterministic mock provider${
        mock ? " (--mock)" : " (no OPENROUTER_API_KEY / ANTHROPIC_API_KEY)"
      } — proposals are structural, not model-authored`,
    );
    return new MockCiteProvider();
  }
  const providerMod = await import(
    "../packages/candidates/src/inference/inference-provider.js"
  );
  if (orKey) {
    const { resolveOpenRouterModel } = await import(
      "../packages/candidates/src/openrouter-client.js"
    );
    const model = resolveOpenRouterModel();
    log(`[weave] using OpenRouterInferenceProvider (model=${model})`);
    return new providerMod.OpenRouterInferenceProvider(orKey);
  }
  log("[weave] using AnthropicInferenceProvider (ANTHROPIC_API_KEY present)");
  return new providerMod.AnthropicInferenceProvider(anthKey!);
}

// ── Project I/O ─────────────────────────────────────────────────────────────────

interface LoadedState {
  elements: SysmlElement[];
  relationships: Awaited<ReturnType<FileStore["queryRelationships"]>>;
  corpus: InferredComposedIR | null;
  ir: InferredComposedIR;
  entities: EntityRecord[];
  mentions: MentionRecord[];
  mentionIds: Set<string>;
  acceptedRelations: AcceptedRelation[];
  findings: Finding[];
}

function emptyIR(): InferredComposedIR {
  const extracted = {
    schema_version: SCHEMA_VERSION,
    subsystem: "weave",
    needs: [],
    requirements: [],
    functions: [],
    components: [],
    satisfies: [],
    allocations: [],
    subsystems: [],
    n2Interfaces: [],
    kpps: [],
    behaviorDecomp: [],
  };
  return {
    extracted: extracted as unknown as InferredComposedIR["extracted"],
    proseEntries: [],
    approvedProseIds: new Set<string>(),
    inferredEntries: [],
    approvedInferredIds: new Set<string>(),
    chunkStore: undefined,
  } as InferredComposedIR;
}

async function findModelId(modelDir: string): Promise<string> {
  const files = await readdir(modelDir);
  for (const f of files) {
    if (!f.endsWith(".json")) continue;
    try {
      const doc = JSON.parse(await readFile(join(modelDir, f), "utf8")) as { "@type"?: string; id?: string };
      if (doc["@type"] === "FileModel" && typeof doc.id === "string") return doc.id;
    } catch {
      /* skip */
    }
  }
  throw new Error(`weave: no FileModel JSON found in ${modelDir}`);
}

async function loadState(project: string, log: (m: string) => void): Promise<LoadedState> {
  const modelDir = join(project, "model");
  const store = new FileStore(modelDir);
  const projectId = await findModelId(modelDir);
  await store.loadProject(projectId);
  const elements = await store.queryElements();
  const relationships = await store.queryRelationships();

  // Corpus (optional). Absent → null → GATE03-corpus-unavailable warning (not an error).
  const extractedPath = join(project, "extracted.json");
  let corpus: InferredComposedIR | null = null;
  let ir: InferredComposedIR = emptyIR();
  if (existsSync(extractedPath)) {
    corpus = await composeIR(extractedPath);
    ir = corpus;
    log(`[weave] corpus loaded from ${extractedPath}`);
  } else {
    log("[weave] no extracted.json — corpus=null (GATE03 warning), engine runs over an empty IR");
  }

  // Entity + mention stores (optional).
  const entitiesPath = join(project, "entities.json");
  const mentionsPath = join(project, "mentions.json");
  const entities: EntityRecord[] = existsSync(entitiesPath)
    ? await loadEntityStoreFile(entitiesPath)
    : [];
  const mentions: MentionRecord[] = existsSync(mentionsPath)
    ? await loadMentionStoreFile(mentionsPath)
    : [];
  const mentionIds = new Set(mentions.map((m) => m.mentionId));
  log(`[weave] ${entities.length} entities, ${mentions.length} mentions loaded`);

  // Accepted relations for chains (optional). NEVER pending — filter defensively.
  const acceptedPath = join(project, "accepted-relations.json");
  let acceptedRelations: AcceptedRelation[] = [];
  if (existsSync(acceptedPath)) {
    const raw = JSON.parse(await readFile(acceptedPath, "utf8")) as AcceptedRelation[];
    acceptedRelations = raw.filter((r) => r.status === "accepted");
    if (acceptedRelations.length !== raw.length) {
      log(
        `[weave] accepted-relations: dropped ${raw.length - acceptedRelations.length} non-accepted relation(s) (pending never composes)`,
      );
    }
  }

  // §B — HUMAN-APPROVED inferred entries ALSO become AcceptedRelations, so an
  // approved containment (or allocation, …) proposed in a prior pass composes in
  // THIS pass (`allocation ∘ containment → allocation`). Uses the RAW on-disk
  // status (the human's decision); the compose-time premise-propagation "suspect"
  // downgrade for unresolved chunk-id premises is a serialization-provenance
  // concern, not a composition one. Only status:"approved" (non-superseded)
  // entries project → the "never compose pending" invariant is preserved (pending
  // proposals never live in inferred-approved.json).
  const inferredApprovedPath = join(project, "dispositions", "inferred-approved.json");
  if (existsSync(inferredApprovedPath)) {
    try {
      const parsed = JSON.parse(await readFile(inferredApprovedPath, "utf8")) as { entries?: unknown };
      const entries = InferredApprovedEntrySchema.array().parse(parsed.entries ?? []);
      const projected = projectApprovedInferredToRelations(entries);
      if (projected.length > 0) {
        log(
          `[weave] accepted-relations: +${projected.length} from approved inferred entries (approved containment/allocation → chain substrate)`,
        );
        acceptedRelations = [...acceptedRelations, ...projected];
      }
    } catch (err) {
      log(
        `[weave] WARN: could not load approved inferred entries from ${inferredApprovedPath} as chain substrate: ${(err as Error).message}`,
      );
    }
  }

  const { findings } = audit(elements, relationships, corpus, {
    entities,
    mentionIds: mentions.length > 0 ? mentionIds : undefined,
  });

  return {
    elements,
    relationships,
    corpus,
    ir,
    entities,
    mentions,
    mentionIds,
    acceptedRelations,
    findings,
  };
}

/** Resolve a gap element's name + text (statement/description/text) from the store. */
function makeResolver(elements: SysmlElement[]): (elementId: string) => GapContext {
  const byId = new Map(elements.map((e) => [e.id, e]));
  return (elementId: string): GapContext => {
    const el = byId.get(elementId);
    if (!el) return { name: null, text: "" };
    const raw = el.raw as Record<string, unknown>;
    const text = [raw["statement"], raw["description"], raw["text"]]
      .filter((v): v is string => typeof v === "string")
      .join(" ");
    return { name: el.name, text };
  };
}

function asWeaveFindings(findings: readonly Finding[]): WeaveFinding[] {
  return findings.map((f) => ({
    elementId: f.elementId,
    ruleId: f.ruleId,
    message: f.message,
    severity: f.severity,
    suggestedFix: f.suggestedFix,
  }));
}

// ── Pending-pass marker (bridges open → close) ──────────────────────────────────

const PENDING_FILE = "pending-pass.json";

interface PendingPass {
  passNumber: number;
  auditBefore: PassRecord["auditBefore"];
  queries: PassRecord["queries"];
  candidatesProposed: PassRecord["candidatesProposed"];
}

async function nextPassNumber(passesDir: string): Promise<number> {
  if (!existsSync(passesDir)) return 1;
  const files = await readdir(passesDir);
  let max = 0;
  for (const f of files) {
    const m = /^pass-(\d+)\.json$/.exec(f);
    if (m) max = Math.max(max, Number(m[1]));
  }
  return max + 1;
}

/** Count human dispositions present in the dispositions dir (for the record). */
async function readDispositions(dispositionsDir: string): Promise<DispositionSummary[]> {
  const out: DispositionSummary[] = [];
  if (!existsSync(dispositionsDir)) return out;
  const map: Array<[string, "approved" | "rejected", string]> = [
    ["inferred-approved.json", "approved", "inference"],
    ["inferred-rejections.json", "rejected", "inference"],
    ["prose-approved.json", "approved", "prose"],
    ["prose-rejections.json", "rejected", "prose"],
    ["entity-approved.json", "approved", "entity"],
    ["entity-rejections.json", "rejected", "entity"],
  ];
  for (const [file, disposition, layer] of map) {
    const p = join(dispositionsDir, file);
    if (!existsSync(p)) continue;
    try {
      const raw = JSON.parse(await readFile(p, "utf8")) as unknown;
      const arr: unknown[] = Array.isArray(raw)
        ? raw
        : Array.isArray((raw as { entries?: unknown }).entries)
          ? (raw as { entries: unknown[] }).entries
          : Array.isArray((raw as { rejectedIds?: unknown }).rejectedIds)
            ? (raw as { rejectedIds: unknown[] }).rejectedIds
            : [];
      for (const item of arr) {
        const candidateId =
          typeof item === "string"
            ? item
            : typeof (item as { id?: unknown }).id === "string"
              ? (item as { id: string }).id
              : typeof (item as { candidateId?: unknown }).candidateId === "string"
                ? (item as { candidateId: string }).candidateId
                : "unknown";
        out.push({ candidateId, disposition, layer });
      }
    } catch {
      /* skip malformed disposition file — never invents dispositions */
    }
  }
  return out;
}

// ── Open pass ───────────────────────────────────────────────────────────────────

async function openPass(args: Args, log: (m: string) => void): Promise<number> {
  const state = await loadState(args.project, log);
  const auditBeforeSummary = summarizeAudit(asWeaveFindings(state.findings));
  log(
    `[weave] audit before: ${auditBeforeSummary.errorCount} error(s), ${auditBeforeSummary.warningCount} warning(s)`,
  );

  // GATE02 completeness family → queries. Unmapped ids are REPORTED, not skipped.
  const gate02 = asWeaveFindings(state.findings).filter((f) => f.ruleId.startsWith("GATE02-"));
  const plan = planQueries(gate02, makeResolver(state.elements));
  log(`[weave] planned ${plan.queries.length} quer${plan.queries.length === 1 ? "y" : "ies"}`);
  for (const u of plan.unmappedFindings) {
    log(`[weave] REPORTED (no query strategy): ${u.ruleId} on ${u.elementId}`);
  }

  // Scope enumeration to exactly the trace families the gaps reported missing
  // (satisfy/allocation/derive), so a satisfy gap yields satisfy candidates.
  // Empty → the engine's default family set (all enumerable families).
  const passFamilies = queryFamiliesToRelationFamilies(plan.queries);
  if (passFamilies.length > 0) {
    log(`[weave] targeted families: ${passFamilies.join(", ")}`);
  }

  const provider = await selectProvider(args.mock, log);
  const targeted = await runTargetedInference({
    ir: state.ir,
    provider,
    queries: plan.queries,
    entities: state.entities,
    mentions: state.mentions,
    acceptedRelations: state.acceptedRelations,
    ...(passFamilies.length > 0 ? { families: passFamilies } : {}),
    budgetUsd: args.budgetUsd,
    dryRun: args.dryRun,
    log,
  });

  // Proposals → normal review queue (a CANDIDATE write, not a disposition).
  const candidatesDir = join(args.project, "candidates");
  await mkdir(candidatesDir, { recursive: true });
  await writeFile(
    join(candidatesDir, "inference-candidates.json"),
    `${JSON.stringify(
      {
        generatedAt: new Date().toISOString(),
        irHash: targeted.irHash,
        stats: targeted.engineResult.stats,
        records: targeted.engineResult.records,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  // Pending-pass marker bridges to --close-pass.
  const passesDir = join(args.project, "passes");
  await mkdir(passesDir, { recursive: true });
  const passNumber = await nextPassNumber(passesDir);
  const pending: PendingPass = {
    passNumber,
    auditBefore: auditBeforeSummary,
    queries: plan.queries,
    candidatesProposed: targeted.proposedCandidates,
  };
  await writeFile(join(passesDir, PENDING_FILE), `${JSON.stringify(pending, null, 2)}\n`, "utf8");

  log("");
  log(`[weave] pass ${passNumber} OPEN`);
  log(`[weave]   ${targeted.proposedCandidates.length} candidate(s) proposed to the review queue:`);
  log(`[weave]     ${join(candidatesDir, "inference-candidates.json")}`);
  log("[weave]   NO disposition written — the pass STOPS for the human.");
  log("[weave]   Review + approve in the review UI, then run:  pnpm weave --project <dir> --close-pass");
  return 0;
}

// ── Close pass ────────────────────────────────────────────────────────────────

async function closePass(args: Args, log: (m: string) => void): Promise<number> {
  const passesDir = join(args.project, "passes");
  const pendingPath = join(passesDir, PENDING_FILE);
  if (!existsSync(pendingPath)) {
    throw new Error(
      `weave --close-pass: no pending pass at ${pendingPath}. Run an open pass first: pnpm weave --project ${args.project}`,
    );
  }
  const pending = JSON.parse(await readFile(pendingPath, "utf8")) as PendingPass;

  // Recompose + re-audit against the CURRENT model + dispositions.
  const state = await loadState(args.project, log);
  const auditAfterSummary = summarizeAudit(asWeaveFindings(state.findings));
  const dispositionsApplied = await readDispositions(join(args.project, "dispositions"));

  const record: PassRecord = {
    auditBefore: pending.auditBefore,
    queries: pending.queries,
    candidatesProposed: pending.candidatesProposed,
    dispositionsApplied,
    auditAfter: auditAfterSummary,
    warningsDelta: computeWarningsDelta(pending.auditBefore, auditAfterSummary),
  };

  const outPath = join(passesDir, passFileName(pending.passNumber));
  await writePassRecordFile(outPath, pending.passNumber, record);
  await rm(pendingPath, { force: true });

  const verdict = evaluateConvergence(pending.auditBefore, auditAfterSummary);

  log("");
  log(`[weave] pass ${pending.passNumber} CLOSED → ${outPath}`);
  log(`[weave]   dispositions applied: ${dispositionsApplied.length}`);
  log(
    `[weave]   audit: ${verdict.errorsBefore} → ${verdict.errorsAfter} error(s); ` +
      `${pending.auditBefore.warningCount} → ${auditAfterSummary.warningCount} warning(s)`,
  );
  log("[weave]   warnings delta per rule (SOFT — reported, not gated):");
  if (record.warningsDelta.length === 0) {
    log("[weave]     (none)");
  } else {
    for (const d of record.warningsDelta) {
      const sign = d.delta > 0 ? `+${d.delta}` : `${d.delta}`;
      log(`[weave]     ${d.ruleId}: ${d.before} → ${d.after} (${sign})`);
    }
  }

  if (verdict.ok) {
    log("[weave]   convergence: OK (zero errors, none introduced)");
    return 0;
  }
  if (verdict.errorsIncreased) {
    log(
      `[weave]   convergence: FAIL — error findings INCREASED (${verdict.errorsBefore} → ${verdict.errorsAfter})`,
    );
  }
  if (verdict.endsWithErrors) {
    log(`[weave]   convergence: FAIL — pass ended with ${verdict.errorsAfter} error finding(s) (must be zero)`);
  }
  return 1;
}

// ── Entry point ─────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  const log = (m: string): void => {
    process.stderr.write(m + "\n");
  };
  const code = args.closePass ? await closePass(args, log) : await openPass(args, log);
  process.exit(code);
}

main().catch((err) => {
  process.stderr.write(`[weave] ERROR: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exit(2);
});
