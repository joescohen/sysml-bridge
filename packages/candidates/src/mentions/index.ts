/**
 * mentions — the naming-event substrate for cross-document entity resolution.
 *
 * A "mention" is one naming event of a model-relevant thing in one chunk (spec
 * §3, `docs/superpowers/specs/2026-07-14-corpus-weaver-design.md`). Mentions
 * are the hub W1 (entity resolution) clusters on: without them, cross-document
 * links are structurally impossible because two documents never share a
 * candidate id, only a surface form.
 *
 * Two derivation sources, harvested from the SAME provider call the prose
 * pipeline already makes per chunk (C5 — no extra call):
 *   1. IMPLICIT — every `CandidateProposal`'s own name-like field (component
 *      name, function name, mode name, …) derives a mention deterministically,
 *      so mentions ⊇ candidates even when a provider returns no explicit
 *      `mentions[]`.
 *   2. EXPLICIT — the optional `CandidateProposal.mentions[]` the provider may
 *      additionally emit for entities named in the chunk that don't rise to a
 *      full candidate (see `packages/candidates/src/prose/llm-provider.ts`).
 *
 * Same C4/C6 discipline as candidates: a mention whose citation cites an
 * unresolvable chunkId, OR whose quote does not verbatim-resolve into that
 * chunk's text, is DROPPED and COUNTED (`droppedUnverbatimMentions`) — never
 * silently emitted, never silently capped.
 *
 * PURE derivation — no I/O in this module. Persistence (`mentions.json`)
 * mirrors `../chunk-store/index.ts` exactly: self-describing envelope
 * (`sysml-foundry/mention-store@1`), throw-on-malformed load. This module
 * lives OUTSIDE `prose/` and `scripts/` (like chunk-store) so the C5
 * no-retrieval grep-test (`prose/__tests__/gc-sweep.test.ts`) stays green.
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";

import { stableId, normSurface, quoteOccursInChunk } from "@sysml-bridge/model";
import type { CandidateProposal } from "../prose/llm-provider.js";

// normSurface is the single canonical mention/entity normalizer. It now lives in
// @sysml-bridge/model (beside `normalizeForVerbatim`, whose rules it reuses) so
// the ENT-* gate in packages/gates can share it without a candidates→gates
// dependency. Re-exported here so W0's `normSurface` public API is unchanged.
export { normSurface } from "@sysml-bridge/model";

// ── MentionRecord ──────────────────────────────────────────────────────────

/** Coarse type hint for a mention — mirrors the model-relevant thing named. */
export type MentionKind =
  | "component"
  | "function"
  | "requirement"
  // Stakeholder needs are their OWN kind (distinct from requirements) so the
  // derive trace family (requirement → need) can type-gate its endpoints. A
  // `need` proposal derives a `need` mention (see KIND_TO_MENTION_KIND).
  | "need"
  | "mode"
  | "interface"
  | "flow"
  // `verification` has no prose-proposal source (verification cases are not
  // extracted as mentions) — it exists only so the verify trace family's source
  // endpoint is typeable if ever supplied structurally. No mention derivation
  // maps TO it, so it is inert in the mention pipeline.
  | "verification"
  | "unknown";

/** Citation for a mention — same shape as a prose candidate's citation. */
export interface MentionCitation {
  docId: string;
  docSha256: string;
  chunkId: string;
  sectionPath: string;
  /** Verbatim quote from the source chunk — verbatim-checked (C6) at derivation time. */
  quote: string;
}

/** One naming event of a model-relevant thing in one chunk. */
export interface MentionRecord {
  /** stableId("mention", `${docSha256}:${chunkId}:${normSurface}:${kindHint}`) */
  mentionId: string;
  /** As written in the document — case/whitespace preserved. */
  surfaceForm: string;
  kindHint: MentionKind;
  citation: MentionCitation;
  /** 0.0–1.0 confidence, inherited from the proposal/mention that produced this record. */
  confidence: number;
}

// ── Derivation ────────────────────────────────────────────────────────────

/** Maps a candidate's `kind` to the coarser `MentionKind` taxonomy. */
const KIND_TO_MENTION_KIND: Record<CandidateProposal["kind"], MentionKind> = {
  requirement: "requirement",
  need: "need",
  mode: "mode",
  modeTransition: "mode",
  interface: "interface",
  component: "component",
  function: "function",
  succession: "function",
  decision: "function",
  parallel: "function",
};

interface RawSurface {
  surfaceForm: string;
  kindHint: MentionKind;
}

/**
 * Extract the name-like surface form(s) implicit in a proposal's own fields.
 * Most kinds carry a single name/text field; `modeTransition` names two modes
 * (fromMode, toMode); the control-flow kinds (succession/decision/parallel)
 * name their owning function. Returns zero forms if nothing name-like is
 * present (never invents a surface form).
 */
function surfaceFormsFromProposal(proposal: CandidateProposal): RawSurface[] {
  const f = proposal.fields;
  switch (proposal.kind) {
    case "modeTransition": {
      const out: RawSurface[] = [];
      const from = f["fromMode"];
      const to = f["toMode"];
      if (typeof from === "string" && from.trim().length > 0) {
        out.push({ surfaceForm: from.trim(), kindHint: "mode" });
      }
      if (typeof to === "string" && to.trim().length > 0) {
        out.push({ surfaceForm: to.trim(), kindHint: "mode" });
      }
      return out;
    }
    case "succession":
    case "decision":
    case "parallel": {
      const owning = f["owningFunction"];
      return typeof owning === "string" && owning.trim().length > 0
        ? [{ surfaceForm: owning.trim(), kindHint: "function" }]
        : [];
    }
    default: {
      const mentionKind = KIND_TO_MENTION_KIND[proposal.kind] ?? "unknown";
      const name = f["name"] ?? f["text"] ?? f["title"];
      return typeof name === "string" && name.trim().length > 0
        ? [{ surfaceForm: name.trim(), kindHint: mentionKind }]
        : [];
    }
  }
}

/** A single unverified candidate mention prior to the citation gate. */
interface CandidateMention {
  surfaceForm: string;
  kindHint: MentionKind;
  citedChunkId: string;
  quote: string;
  confidence: number;
}

/** Context a document's proposals were extracted under. */
export interface MentionDerivationContext {
  documentId: string;
  /** docSha256 — same hash `ChunkContext.documentHash` carries. */
  documentHash: string;
  sectionPath: string;
}

export interface MentionDerivationResult {
  /** Mentions that passed the citation gate (deterministic order — see below). */
  mentions: MentionRecord[];
  /**
   * Candidate mentions dropped because their citation's chunkId did not
   * resolve in `chunkStore`, OR their quote did not verbatim-resolve into
   * that chunk's text (C4/C6 discipline extended to mentions). Never
   * silently emitted, never silently capped.
   */
  droppedUnverbatimMentions: number;
}

/**
 * Derive mentions from a batch of raw provider proposals — PURE, no I/O.
 *
 * Every proposal contributes:
 *   - its own implicit name-like surface form(s) (`surfaceFormsFromProposal`),
 *     citing the proposal's own `citedChunkId`/`quote`/`confidence`; and
 *   - any explicit `proposal.mentions[]` the provider additionally returned,
 *     each citing its own `citedChunkId`/`quote`.
 *
 * Each candidate mention is independently gated: unresolvable chunkId or a
 * non-verbatim quote drops it and increments `droppedUnverbatimMentions`
 * (never emitted). Surviving mentions are deduplicated by `mentionId`
 * (first-seen wins) — same normSurface + kindHint + chunk + doc is one
 * naming event, not N. Order is deterministic: proposals are processed in
 * input order, so re-running derivation on the identical input twice
 * produces byte-identical output (asserted in tests).
 */
export function deriveMentions(
  proposals: readonly CandidateProposal[],
  chunkStore: ReadonlyMap<string, string>,
  context: MentionDerivationContext,
): MentionDerivationResult {
  const candidates: CandidateMention[] = [];

  for (const proposal of proposals) {
    for (const { surfaceForm, kindHint } of surfaceFormsFromProposal(proposal)) {
      candidates.push({
        surfaceForm,
        kindHint,
        citedChunkId: proposal.citedChunkId,
        quote: proposal.quote,
        confidence: proposal.confidence,
      });
    }
    for (const explicit of proposal.mentions ?? []) {
      candidates.push({
        surfaceForm: explicit.surfaceForm,
        kindHint: explicit.kindHint,
        citedChunkId: explicit.citedChunkId,
        quote: explicit.quote,
        confidence: explicit.confidence,
      });
    }
  }

  const mentions: MentionRecord[] = [];
  const seen = new Set<string>();
  let droppedUnverbatimMentions = 0;

  for (const cand of candidates) {
    const chunkText = chunkStore.get(cand.citedChunkId);
    if (chunkText === undefined || !quoteOccursInChunk(cand.quote, chunkText)) {
      droppedUnverbatimMentions++;
      continue; // DROP: unresolvable chunkId or non-verbatim quote
    }

    const mentionId = stableId(
      "mention",
      `${context.documentHash}:${cand.citedChunkId}:${normSurface(cand.surfaceForm)}:${cand.kindHint}`,
    );
    if (seen.has(mentionId)) continue; // dedup: same naming event, first-seen wins
    seen.add(mentionId);

    mentions.push({
      mentionId,
      surfaceForm: cand.surfaceForm,
      kindHint: cand.kindHint,
      citation: {
        docId: context.documentId,
        docSha256: context.documentHash,
        chunkId: cand.citedChunkId,
        sectionPath: context.sectionPath,
        quote: cand.quote.slice(0, 300),
      },
      confidence: cand.confidence,
    });
  }

  return { mentions, droppedUnverbatimMentions };
}

// ── Persistence — mirrors ../chunk-store/index.ts exactly ─────────────────

/** Schema tag stamped into every mentions.json envelope. */
export const MENTION_STORE_SCHEMA = "sysml-foundry/mention-store@1";

/** The on-disk envelope wrapping the mention records. */
export interface MentionStoreFile {
  schema: typeof MENTION_STORE_SCHEMA;
  generatedAt: string;
  mentions: MentionRecord[];
}

const MENTION_KINDS: readonly MentionKind[] = [
  "component",
  "function",
  "requirement",
  "mode",
  "interface",
  "flow",
  "unknown",
];

/** Validate a single decoded citation, throwing on any missing/mistyped field. */
function assertCitation(value: unknown, index: number): MentionCitation {
  if (typeof value !== "object" || value === null) {
    throw new Error(`mention-store: mentions[${index}].citation is not an object`);
  }
  const rec = value as Record<string, unknown>;
  for (const key of ["docId", "docSha256", "chunkId", "sectionPath", "quote"] as const) {
    if (typeof rec[key] !== "string") {
      throw new Error(
        `mention-store: mentions[${index}].citation.${key} must be a string (got ${typeof rec[key]})`,
      );
    }
  }
  return {
    docId: rec["docId"] as string,
    docSha256: rec["docSha256"] as string,
    chunkId: rec["chunkId"] as string,
    sectionPath: rec["sectionPath"] as string,
    quote: rec["quote"] as string,
  };
}

/** Validate a single decoded record, throwing on any missing/mistyped field. */
function assertRecord(value: unknown, index: number): MentionRecord {
  if (typeof value !== "object" || value === null) {
    throw new Error(`mention-store: mentions[${index}] is not an object`);
  }
  const rec = value as Record<string, unknown>;
  if (typeof rec["mentionId"] !== "string") {
    throw new Error(
      `mention-store: mentions[${index}].mentionId must be a string (got ${typeof rec["mentionId"]})`,
    );
  }
  if (typeof rec["surfaceForm"] !== "string") {
    throw new Error(
      `mention-store: mentions[${index}].surfaceForm must be a string (got ${typeof rec["surfaceForm"]})`,
    );
  }
  if (
    typeof rec["kindHint"] !== "string" ||
    !MENTION_KINDS.includes(rec["kindHint"] as MentionKind)
  ) {
    throw new Error(
      `mention-store: mentions[${index}].kindHint must be one of ${MENTION_KINDS.join("|")} (got ${String(rec["kindHint"])})`,
    );
  }
  if (typeof rec["confidence"] !== "number") {
    throw new Error(
      `mention-store: mentions[${index}].confidence must be a number (got ${typeof rec["confidence"]})`,
    );
  }
  return {
    mentionId: rec["mentionId"] as string,
    surfaceForm: rec["surfaceForm"] as string,
    kindHint: rec["kindHint"] as MentionKind,
    citation: assertCitation(rec["citation"], index),
    confidence: rec["confidence"] as number,
  };
}

/**
 * Serialize mention records to the on-disk envelope JSON string.
 *
 * @param generatedAt Override the timestamp (tests pass a fixed value for
 *                    byte-stable fixtures). Defaults to `new Date()`.
 */
export function serializeMentionStore(
  records: readonly MentionRecord[],
  generatedAt: Date = new Date(),
): string {
  const file: MentionStoreFile = {
    schema: MENTION_STORE_SCHEMA,
    generatedAt: generatedAt.toISOString(),
    mentions: records.map((r) => ({
      mentionId: r.mentionId,
      surfaceForm: r.surfaceForm,
      kindHint: r.kindHint,
      citation: { ...r.citation },
      confidence: r.confidence,
    })),
  };
  return `${JSON.stringify(file, null, 2)}\n`;
}

/**
 * Parse a mentions.json string into validated records. Throws on a malformed
 * envelope or record — a corrupt mention store must fail loudly, never
 * silently yield an empty store that would let downstream entity resolution
 * degrade to a vacuous pass.
 */
export function parseMentionStore(json: string): MentionRecord[] {
  let decoded: unknown;
  try {
    decoded = JSON.parse(json);
  } catch (err) {
    throw new Error(
      `mention-store: file is not valid JSON: ${err instanceof Error ? err.message : String(err)}`,
    );
  }
  if (typeof decoded !== "object" || decoded === null) {
    throw new Error("mention-store: top-level value is not an object");
  }
  const env = decoded as Record<string, unknown>;
  if (env["schema"] !== MENTION_STORE_SCHEMA) {
    throw new Error(
      `mention-store: unexpected schema '${String(env["schema"])}' (want '${MENTION_STORE_SCHEMA}')`,
    );
  }
  if (!Array.isArray(env["mentions"])) {
    throw new Error("mention-store: 'mentions' must be an array");
  }
  return env["mentions"].map((m, i) => assertRecord(m, i));
}

/**
 * Write mention records to `filePath` (creating parent dirs). Byte-stable
 * content for a fixed `generatedAt`.
 */
export async function writeMentionStoreFile(
  filePath: string,
  records: readonly MentionRecord[],
  generatedAt?: Date,
): Promise<void> {
  await mkdir(dirname(filePath), { recursive: true });
  await writeFile(filePath, serializeMentionStore(records, generatedAt), "utf8");
}

/**
 * Load and validate a mentions.json file into `MentionRecord[]` — the input
 * W1 entity auto-clustering consumes.
 */
export async function loadMentionStoreFile(filePath: string): Promise<MentionRecord[]> {
  const json = await readFile(filePath, "utf8");
  return parseMentionStore(json);
}
