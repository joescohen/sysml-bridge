/**
 * seeded-defects.ts — the seeded-defect eval harness (demo:seeded).
 *
 * A gate you never watch catch a defect is a gate you cannot trust. This
 * harness plants KNOWN defects with FIXED identifiers into a copy of the clean
 * ANGARS C&C build and proves each gate reports its specific defect — by rule id
 * and by the id of the element it flags. It also runs a paired clean control
 * (the same build demo:build proves clean) and asserts zero error-severity
 * findings, so the catches are real and not vacuous.
 *
 * The plant/catch/clean-control machinery is the shared @sysml-bridge/invariants
 * `seededDefectHarness` — this module is a THIN caller: it supplies the clean
 * base model, the audit seam, the error predicate, and the seven planted defects.
 * The harness owns the non-vacuous clean control, the per-defect catch check
 * (including the `soleError` isolation discipline, below), the summary table, and
 * the pass/fail `ok` flag this module maps to the process exit code.
 *
 * It reuses the EXACT build `demo:build` runs:
 *   - loadCorpus()        — same corpus, same path (examples/angars/cc-extracted.json)
 *   - buildModelStore()   — the shared build seam (Steps 1..8) from build-model.ts
 *   - projectForPresentation() — the same def→usage presentation projection
 * so "the model the harness audits" is the model the demo audits and serializes.
 *
 * ─────────────────────────────────────────────────────────────────────────────
 * COVERAGE POLICY — one planted defect per ERROR-severity rule; warnings/info at
 * family level. This is a DELIBERATE, DOCUMENTED cap (repo doctrine: "no silent
 * caps"). The table below is the ground truth — every rule the gates can emit,
 * its severity, and whether this harness plants a dedicated defect for it.
 *
 *   rule id                         | sev     | seeded here? | note
 *   --------------------------------|---------|--------------|-------------------
 *   R4-def-operand                  | error   | YES (c)      | trace operand is a Definition
 *   GATE02-dangling-endpoint        | error   | YES (d)      | rel endpoint id not in id set
 *   GATE02-id-duplicate             | error   | YES (e)      | two elements share an id
 *   GATE03-unresolvable-provenance  | error   | YES (f)      | provenance id resolves to nothing
 *   INFER-unpremised                | error   | YES (g)      | inferred entry with zero premises
 *   PROSE-unverbatim-quote          | error   | YES (h)      | approved prose quote absent from cited chunk
 *   ENT-unapproved-merge            | error   | YES (i)      | entity alias with no merge disposition
 *   ENT-dangling-mention-ref        | error   | YES (j)      | entity references a mentionId not in the store
 *   GATE03-missing-provenance       | warning | YES (a)      | legacy DEF type, no provenance
 *   GATE02-orphan                   | warning | family only  | leaf design elt, no trace edge
 *   GATE02-unsatisfied              | warning | family only  | system req, no satisfy edge
 *   GATE02-unverified               | warning | family only  | system req, no verify edge
 *   GATE02-unbacktraced             | warning | family only  | system req, no derive trace
 *   GATE02-uncovered-need           | warning | family only  | need, no inbound derive
 *   GATE03-corpus-unavailable       | warning | family only  | corpus arg is null
 *   INFER-suspect-premise           | warning | family only  | inferred entry status 'suspect'
 *   PROSE-suspect-source            | warning | family only  | prose entry status 'suspect'
 *   ENT-duplicate-suspect           | warning | family only  | two same-kind entities auto-cluster-match
 *   ENT-mention-store-unavailable   | warning | family only  | entities present, mention store absent
 *   GATE03-model-asserted           | info    | family only  | provenance === 'model-asserted'
 *
 * (Non-audit gate) Gate 2 grammar validator — the local ANTLR validator — is
 * exercised by defect (b); it is a process-exit gate, not an audit() rule id.
 *
 * WARNING/INFO rules stay at family-level coverage on purpose: they are
 * completeness/observability signals, not correctness rejects, and each already
 * has unit coverage in packages/gates/src/__tests__/. The one non-obvious gap:
 * INFER-suspect-premise and PROSE-suspect-source require a *suspect* entry in a
 * ProseComposedIR / InferredComposedIR corpus (premise/citation drift) — a
 * corpus-layer state this build-copy harness does not synthesize. They are
 * covered by audit-provenance/relational unit tests instead. INFER-unpremised is
 * the only error-severity rule on the composed-IR path, so it IS seeded here (g),
 * via a minimal in-memory InferredComposedIR (see below).
 * ─────────────────────────────────────────────────────────────────────────────
 *
 * The planted defects (all fixed ids; each isolated so "rule X flagged element Y"
 * is exact — every error defect is planted with `soleError: true`, so the harness
 * asserts the seeded audit yields EXACTLY ONE error finding and it is the target.
 * A cross-triggering seed fails loudly rather than passing on a coincidental extra
 * catch):
 *   (a) GATE 1 provenance — a PartDefinition named "seeded-uncited-part" with NO
 *       provenanceSourceId → GATE03-missing-provenance (warning). We assert a
 *       finding with that rule id AND elementId === the seeded element's id.
 *       NOTE ON TYPE: the provenance rule scopes its presence check to the three
 *       legacy DEFINITION types (RequirementDefinition | PartDefinition |
 *       ActionDefinition) only — a PartUsage with no provenance produces NO
 *       finding, so the seed is a PartDefinition (the closest type the rule
 *       actually checks). This is a WARNING catch (0 error findings expected), so
 *       it does NOT carry soleError — the sole-error discipline is for the
 *       error-severity defects.
 *   (b) GATE 2 grammar — the clean serialized text with ONE "requirement "
 *       keyword replaced by "requirment " (out/angars-poisoned.sysml). We assert
 *       the validator exits NON-zero on the poisoned file AND exits ZERO on the
 *       clean out/angars.sysml — a paired control in both directions (via the
 *       shared invariants `pairedControl`).
 *   (c) GATE 1 relational (R4) — a SatisfyRequirementUsage "seeded-def-operand-satisfy"
 *       whose SOURCE references a Definition (a seeded PartDefinition "SeededDefPart",
 *       itself validly cited to the corpus "C&C") → R4-def-operand (error) because
 *       trace operands must be Usages, not Definitions. Asserted on
 *       elementId === SeededDefPart, sole error.
 *   (d) GATE 1 relational — a SatisfyRequirementUsage "seeded-dangling-rel" whose
 *       source AND target are ids present in NO element/relationship →
 *       GATE02-dangling-endpoint (error) on elementId === the relationship id.
 *       (R4 skips unresolvable ids — the dangling rule owns them — so this fires
 *       dangling and nothing else.)
 *   (e) GATE 1 relational — two RequirementUsage elements both with id
 *       "seeded-dup-id" → GATE02-id-duplicate (error) on elementId === the shared
 *       id. RequirementUsage is neither a legacy-provenance type nor a design
 *       type, so no other rule fires on it.
 *   (f) GATE 1 provenance — a PartUsage "seeded-unresolvable-prov" whose
 *       provenanceSourceId is a forged value absent from the resolution set →
 *       GATE03-unresolvable-provenance (error) on that element's id. (The presence
 *       check is scoped to legacy DEFINITION types, so a PartUsage yields the
 *       existence/error finding cleanly, not the missing-provenance warning.)
 *   (g) GATE 1 inferred layer — the SAME clean model audited against a minimal
 *       in-memory InferredComposedIR (wrapping the same corpus, empty prose layer)
 *       carrying ONE inferred entry "seeded-unpremised-inferred" with premises: []
 *       → INFER-unpremised (error) on that entry's id. The schema forbids empty
 *       premises on disk (min(1)); this defense-in-depth audit rule is exercised by
 *       constructing the IR object directly, the only way a zero-premise entry can
 *       reach audit(). Approved-status + empty prose ⇒ no suspect warnings, and the
 *       resolution set is identical to the plain-corpus one ⇒ the clean elements
 *       stay clean and INFER-unpremised is the sole error. Expressed here by a
 *       plant that swaps the model's audit corpus to the in-memory IR.
 *   (h) GATE 1 prose layer — the SAME clean model audited against a minimal
 *       in-memory ProseComposedIR carrying ONE approved prose entry
 *       "seeded-unverbatim-prose" whose citation.quote does NOT occur in the text
 *       of its cited chunk (the chunk text is supplied via the IR's chunkStore) →
 *       PROSE-unverbatim-quote (error) on that entry's id. Approved status ⇒ no
 *       suspect warning; the (unreferenced) prose id widens the resolution set
 *       harmlessly ⇒ the clean elements stay clean and PROSE-unverbatim-quote is
 *       the sole error. This is the SEPAL-style verbatim-citation audit re-check:
 *       a hallucinated quote over a real chunk, caught after approval.
 *   (i) GATE 1 entity layer (W1) — the SAME clean model audited with ONE entity
 *       "seeded-unapproved-merge-entity" whose alias set spans TWO distinct
 *       normalized surface groups ("Fuel Control Module" + "FCM") but carries ZERO
 *       merge dispositions → ENT-unapproved-merge (error) on that entity's id. A
 *       fuzzy/LLM merge that appended a divergent alias with no human approval is
 *       the no-auto-approve violation. The plant supplies the mention-id store
 *       (both referenced mentionIds present) so dangling-ref does NOT fire and the
 *       merge rule is the SOLE error; the clean elements/corpus are untouched.
 *   (j) GATE 1 entity layer (W1) — the SAME clean model audited with ONE entity
 *       "seeded-dangling-mention-entity" (single normalized alias group, no
 *       merge → no ENT-unapproved-merge) that references mentionId
 *       "seeded-absent-mention" which is NOT in the supplied mention-id store →
 *       ENT-dangling-mention-ref (error) on that entity's id, the C4
 *       citation-resolution discipline applied to entities. The store IS supplied
 *       (so no degrade warning) and contains the entity's OTHER mentionId, so the
 *       dangling reference is the SOLE error.
 *
 * Clean control (no seeded defects): the SAME presentation model, audited with
 * the gates audit(), yields zero error-severity findings. VerifyRequirementUsage
 * edges — whose source is a VerificationCaseDefinition kept as a def by the
 * presentation projection (the serializer emits `objective { verify <req>; }`, a
 * grammatically valid form) — are excluded from the relational R4 scope here, the
 * same way the demo pipeline treats them: they are a legitimate presentation form,
 * not an R4 defect. Every other trace operand in the presentation model is a Usage,
 * so the clean control is genuinely, non-vacuously clean.
 *
 * Exit 0 iff ALL assertions hold: every planted defect caught AND the clean
 * control is clean. Any missed catch OR any unexpected clean-control finding OR
 * any error defect that cross-triggers a second error → non-zero.
 *
 * Usage:
 *   pnpm demo:seeded
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

import { loadCorpus, buildModelStore } from "./build-model.js";
import { projectForPresentation, type ComponentFlow } from "./cc-presentation.js";
import { serializeToSysml } from "../../../packages/sysml/src/index.js";
import { audit, loadCorpusCached } from "../../../packages/gates/src/index.js";
import type { Finding, EntityRecordLike } from "../../../packages/gates/src/index.js";
import {
  seededDefectHarness,
  pairedControl,
  type PairedControlResult,
} from "../../../packages/invariants/src/index.js";
import type {
  SysmlElement,
  SysmlRelationship,
  Extracted,
  InferredComposedIR,
  InferredApprovedEntry,
  ProseComposedIR,
  ProseApprovedEntry,
} from "../../../packages/model/src/index.js";

// ---------------------------------------------------------------------------
// Paths & fixed defect identifiers
// ---------------------------------------------------------------------------

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../../..");

const SEEDED_STORE_DIR = path.join(REPO_ROOT, "examples/angars/out/.store-seeded");
const CLEAN_SYSML = path.join(REPO_ROOT, "examples/angars/out/angars.sysml");
const POISONED_SYSML = path.join(REPO_ROOT, "examples/angars/out/angars-poisoned.sysml");
const VALIDATOR_SH = path.join(REPO_ROOT, "tools/sysml-validator/run.sh");
// gates audit() corpus (ExtractedSchema shape) — distinct from the build's cc-extracted.json.
const GATES_CORPUS = path.join(REPO_ROOT, "examples/angars/extracted.json");

// Fixed, known identifiers — the whole point is that each gate names THESE.
const UNCITED_PART_ID = "seeded-uncited-part";
const UNCITED_PART_NAME = "seeded-uncited-part";
const DEF_OPERAND_SATISFY_ID = "seeded-def-operand-satisfy";
const SEEDED_DEF_PART_ID = "SeededDefPart";
const SEEDED_DEF_PART_NAME = "SeededDefPart";
// (d) dangling endpoint
const DANGLING_REL_ID = "seeded-dangling-rel";
const DANGLING_ENDPOINT_ID = "seeded-dangling-endpoint";
// (e) duplicate id
const DUP_ELEMENT_ID = "seeded-dup-id";
// (f) unresolvable provenance
const UNRESOLVABLE_PROV_ID = "seeded-unresolvable-prov";
const FORGED_PROVENANCE = "seeded-forged-provenance-not-in-corpus";
// (g) unpremised inferred entry
const UNPREMISED_INFERRED_ID = "seeded-unpremised-inferred";
// (h) unverbatim approved prose entry — quote absent from its cited chunk
const UNVERBATIM_PROSE_ID = "seeded-unverbatim-prose";
const UNVERBATIM_CHUNK_ID = "seeded-unverbatim-chunk-0000000000";
const UNVERBATIM_CHUNK_TEXT =
  "The ANGARS system shall refuel the receiver aircraft autonomously within sixty seconds of contact.";
// A quote that does NOT occur in UNVERBATIM_CHUNK_TEXT (hallucinated span).
const UNVERBATIM_QUOTE = "the system shall self-destruct on command from the ground station";

// (i) unapproved entity merge — alias with no merge disposition
const UNAPPROVED_MERGE_ENTITY_ID = "seeded-unapproved-merge-entity";
const UNAPPROVED_MERGE_MENTION_1 = "seeded-merge-mention-1";
const UNAPPROVED_MERGE_MENTION_2 = "seeded-merge-mention-2";
// (j) dangling mention reference — entity cites a mentionId absent from the store
const DANGLING_MENTION_ENTITY_ID = "seeded-dangling-mention-entity";
const DANGLING_PRESENT_MENTION = "seeded-present-mention";
const DANGLING_ABSENT_MENTION = "seeded-absent-mention";

const PROVENANCE_RULE_ID = "GATE03-missing-provenance";
const R4_RULE_ID = "R4-def-operand";
const DANGLING_RULE_ID = "GATE02-dangling-endpoint";
const DUP_RULE_ID = "GATE02-id-duplicate";
const UNRESOLVABLE_RULE_ID = "GATE03-unresolvable-provenance";
const UNPREMISED_RULE_ID = "INFER-unpremised";
const UNVERBATIM_RULE_ID = "PROSE-unverbatim-quote";
const UNAPPROVED_MERGE_RULE_ID = "ENT-unapproved-merge";
const DANGLING_MENTION_RULE_ID = "ENT-dangling-mention-ref";

// The presentation projection intentionally keeps VerificationCaseDefinition as a
// def (the serializer emits `objective { verify <req>; }`). The gates R4 rule flags
// ANY Definition trace operand, so these legitimate verify edges are excluded from
// the audited relationships — mirroring how the demo pipeline treats them.
const VERIFY_TRACE_TYPE = "VerifyRequirementUsage";

// ---------------------------------------------------------------------------
// Build the clean presentation model (identical to demo:build's serialize input)
// ---------------------------------------------------------------------------

const TRACE_REL_TYPES = new Set([
  "SatisfyRequirementUsage",
  "VerifyRequirementUsage",
  "DeriveRequirementUsage",
  "AllocationUsage",
  "FeatureMembership",
]);

function idsFrom(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    if (typeof item === "string") out.push(item);
    else if (item && typeof item === "object" && "@id" in (item as object)) {
      const id = (item as { "@id"?: unknown })["@id"];
      if (typeof id === "string") out.push(id);
    }
  }
  return out;
}

async function buildCleanPresentation(): Promise<{
  elements: SysmlElement[];
  relationships: SysmlRelationship[];
}> {
  const corpus = loadCorpus();
  const { store } = await buildModelStore(corpus, SEEDED_STORE_DIR);
  const allElements = await store.queryElements();

  const structuralElements = allElements.filter((e) => !TRACE_REL_TYPES.has(e.type));
  const relElements = allElements.filter((e) => TRACE_REL_TYPES.has(e.type));
  const relationships: SysmlRelationship[] = relElements.map((e) => ({
    id: e.id,
    type: e.type,
    sourceIds: idsFrom(e.raw.source),
    targetIds: idsFrom(e.raw.target),
    raw: e.raw,
  }));

  const componentFlows: ComponentFlow[] = (corpus.n2Interfaces ?? [])
    .filter((n) => n.scope === "component")
    .map((n) => ({
      id: n.id,
      sourceLabel: n.sourceLabel,
      targetLabel: n.targetLabel,
      flow: n.flow,
    }));

  const presentation = projectForPresentation(
    structuralElements,
    relationships,
    componentFlows
  );
  return { elements: presentation.elements, relationships: presentation.relationships };
}

/**
 * Relationships audited by the R4 rule: the presentation trace edges MINUS the
 * VerifyRequirementUsage edges (whose VerificationCaseDefinition source is a
 * legitimate presentation form, not an R4 defect).
 */
function auditableRelationships(
  relationships: SysmlRelationship[]
): SysmlRelationship[] {
  return relationships.filter((r) => r.type !== VERIFY_TRACE_TYPE);
}

// ---------------------------------------------------------------------------
// Seed builders (pure — return the extra element / relationship to add)
// ---------------------------------------------------------------------------

function mkElement(
  id: string,
  type: string,
  name: string,
  raw: Record<string, unknown> = {}
): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: { "@id": id, "@type": type, name, ...raw },
  };
}

// ---------------------------------------------------------------------------
// Model audited by the harness. `corpus` is the audit()'s third argument; the
// clean base carries the plain Extracted corpus, and defect (g) swaps it for an
// in-memory InferredComposedIR so the composed-IR rule pack runs.
// ---------------------------------------------------------------------------

type AuditCorpus = Extracted | ProseComposedIR | InferredComposedIR;

type PresentationModel = {
  elements: SysmlElement[];
  relationships: SysmlRelationship[];
  corpus: AuditCorpus;
  // W1 entity-store inputs. The clean base leaves both undefined (no entities to
  // audit); defects (i)/(j) supply an entity set + mention-id store so the ENT-*
  // rule pack runs. Absent ⇒ audit() skips ENT-* entirely ⇒ clean stays clean.
  entities?: EntityRecordLike[];
  mentionIds?: ReadonlySet<string>;
};

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  console.log("=== ANGARS seeded-defect eval harness (demo:seeded) ===\n");

  const clean = await buildCleanPresentation();
  const gatesCorpus = await loadCorpusCached(GATES_CORPUS);
  if (gatesCorpus === null) {
    console.error(
      `FATAL: gates corpus failed to load from ${GATES_CORPUS}. ` +
      `Provenance existence checks require it; refusing to run a degraded audit.`
    );
    process.exit(1);
  }

  // -- Serialize the clean presentation model so out/angars.sysml exists for
  //    the paired grammar control (also what demo:build writes). ---------------
  const cleanText = serializeToSysml(clean.elements, clean.relationships);
  fs.mkdirSync(path.dirname(CLEAN_SYSML), { recursive: true });
  fs.writeFileSync(CLEAN_SYSML, cleanText, "utf8");

  // -- Poison exactly ONE "requirement " keyword → "requirment " (a typo the
  //    grammar rejects), for the DEFECT (b) grammar paired control. ------------
  const marker = "requirement ";
  const idx = cleanText.indexOf(marker);
  if (idx === -1) {
    console.error(`FATAL: no "${marker}" keyword found in clean SysML to poison.`);
    process.exit(1);
  }
  const poisonedText =
    cleanText.slice(0, idx) + "requirment " + cleanText.slice(idx + marker.length);
  fs.writeFileSync(POISONED_SYSML, poisonedText, "utf8");

  // Target for DEFECT (c): an existing requirement USAGE (a valid Usage operand,
  // so the seeded source Definition is the sole R4 violation).
  const someReqUsage = clean.elements.find((e) => e.type === "RequirementUsage");
  if (!someReqUsage) {
    console.error("FATAL: no RequirementUsage in presentation model to target.");
    process.exit(1);
  }

  // In-memory three-layer IR for DEFECT (g): the SAME corpus, empty prose layer,
  // carrying one approved-status entry with premises: []. Approved status + empty
  // prose ⇒ no suspect warnings; the resolution set equals the plain-corpus one
  // ⇒ the clean elements stay clean and INFER-unpremised is the sole error.
  const unpremised: InferredApprovedEntry = {
    id: UNPREMISED_INFERRED_ID,
    relationFamily: "allocation",
    sourceId: "seeded-src",
    targetId: "seeded-tgt",
    premises: [], // ← the defect: zero premises
    rationale: "seeded zero-premise entry (harness defect g)",
    confidence: 1,
    inferenceRunId: "seeded-run",
    approvedBy: "seeded-harness",
    approvedAt: "2026-01-01T00:00:00Z",
    status: "approved",
  };
  const inferredIR: InferredComposedIR = {
    extracted: gatesCorpus,
    proseEntries: [],
    approvedProseIds: new Set<string>(),
    inferredEntries: [unpremised],
    approvedInferredIds: new Set<string>(),
  };

  // In-memory two-layer IR for DEFECT (h): the SAME corpus, one APPROVED prose
  // entry whose citation.quote is absent from the chunk text carried in the
  // chunkStore. Approved status ⇒ no suspect warning; the resolution set equals
  // the plain-corpus one plus the (unreferenced) prose id ⇒ the clean elements
  // stay clean and PROSE-unverbatim-quote is the sole error.
  const unverbatimEntry: ProseApprovedEntry = {
    id: UNVERBATIM_PROSE_ID,
    kind: "requirement",
    fields: { text: "seeded unverbatim requirement (harness defect h)" },
    citation: {
      docId: "seeded-doc",
      docSha256: "s".repeat(64),
      chunkId: UNVERBATIM_CHUNK_ID,
      sectionPath: "root",
      quote: UNVERBATIM_QUOTE, // ← the defect: not present in the cited chunk
    },
    approvedBy: "seeded-harness",
    approvedAt: "2026-01-01T00:00:00Z",
    candidateId: "seeded-unverbatim-candidate",
    status: "approved",
  };
  const proseIR: ProseComposedIR = {
    extracted: gatesCorpus,
    proseEntries: [unverbatimEntry],
    approvedProseIds: new Set<string>([UNVERBATIM_PROSE_ID]),
    chunkStore: new Map<string, string>([[UNVERBATIM_CHUNK_ID, UNVERBATIM_CHUNK_TEXT]]),
  };

  // Grammar paired control result is captured so the summary detail can report
  // both exit codes; the harness runs `check` before rendering `detail`.
  let grammarControl: PairedControlResult<number> | undefined;

  const result = await seededDefectHarness<PresentationModel, Finding>({
    base: {
      elements: clean.elements,
      relationships: clean.relationships,
      corpus: gatesCorpus,
    },
    // audit() over the presentation model MINUS the legitimate VerifyRequirementUsage
    // edges — the same relationships the demo pipeline audits. The corpus travels
    // WITH the model so defect (g) can substitute the in-memory InferredComposedIR.
    audit: (m) =>
      audit(m.elements, auditableRelationships(m.relationships), m.corpus, {
        entities: m.entities,
        mentionIds: m.mentionIds,
      }).findings,
    isError: (f) => f.severity === "error",
    cleanControl: {
      defect: "(clean control — no defects)",
      gate: "Gate 1 audit()",
      ruleOrCheck: "zero error-severity findings",
    },
    defects: [
      // === DEFECT (a): uncited PartDefinition → GATE03-missing-provenance ======
      // Warning-severity: NOT soleError (the seeded audit yields 0 error findings).
      {
        defect: `(a) uncited part "${UNCITED_PART_NAME}" (no provenanceSourceId)`,
        gate: "Gate 1 audit()",
        ruleOrCheck: PROVENANCE_RULE_ID,
        // NO provenanceSourceId set — that is the defect.
        plant: (b) => ({
          ...b,
          elements: [...b.elements, mkElement(UNCITED_PART_ID, "PartDefinition", UNCITED_PART_NAME)],
        }),
        expectRule: PROVENANCE_RULE_ID,
        expectElementId: UNCITED_PART_ID,
      },
      // === DEFECT (c): SatisfyRequirementUsage with a Definition source → R4 ===
      {
        defect: `(c) satisfy "${DEF_OPERAND_SATISFY_ID}" source is Definition "${SEEDED_DEF_PART_NAME}"`,
        gate: "Gate 1 audit()",
        ruleOrCheck: R4_RULE_ID,
        plant: (b) => {
          // Validly-cited Definition (provenance "C&C" resolves) — so the ONLY
          // thing wrong is a trace operand pointing at a Definition (R4).
          const seededDefPart = mkElement(
            SEEDED_DEF_PART_ID,
            "PartDefinition",
            SEEDED_DEF_PART_NAME,
            { provenanceSourceId: "C&C" }
          );
          const satisfy: SysmlRelationship = {
            id: DEF_OPERAND_SATISFY_ID,
            type: "SatisfyRequirementUsage",
            sourceIds: [SEEDED_DEF_PART_ID],
            targetIds: [someReqUsage.id],
            raw: {
              "@id": DEF_OPERAND_SATISFY_ID,
              "@type": "SatisfyRequirementUsage",
              source: [{ "@id": SEEDED_DEF_PART_ID }],
              target: [{ "@id": someReqUsage.id }],
            },
          };
          return {
            ...b,
            elements: [...b.elements, seededDefPart],
            relationships: [...b.relationships, satisfy],
          };
        },
        expectRule: R4_RULE_ID,
        expectElementId: SEEDED_DEF_PART_ID,
        soleError: true,
      },
      // === DEFECT (d): dangling endpoints → GATE02-dangling-endpoint ==========
      {
        defect: `(d) dangling rel "${DANGLING_REL_ID}" (endpoints absent from model)`,
        gate: "Gate 1 audit()",
        ruleOrCheck: DANGLING_RULE_ID,
        plant: (b) => {
          // Source AND target reference ids present in NO element and NO
          // relationship. R4 skips unresolvable ids (the dangling rule owns them),
          // so this fires dangling and nothing else.
          const dangling: SysmlRelationship = {
            id: DANGLING_REL_ID,
            type: "SatisfyRequirementUsage",
            sourceIds: [DANGLING_ENDPOINT_ID],
            targetIds: [`${DANGLING_ENDPOINT_ID}-2`],
            raw: {
              "@id": DANGLING_REL_ID,
              "@type": "SatisfyRequirementUsage",
              source: [{ "@id": DANGLING_ENDPOINT_ID }],
              target: [{ "@id": `${DANGLING_ENDPOINT_ID}-2` }],
            },
          };
          return { ...b, relationships: [...b.relationships, dangling] };
        },
        expectRule: DANGLING_RULE_ID,
        expectElementId: DANGLING_REL_ID,
        soleError: true,
      },
      // === DEFECT (e): two elements share an id → GATE02-id-duplicate =========
      {
        defect: `(e) duplicate id "${DUP_ELEMENT_ID}" (two RequirementUsage elements)`,
        gate: "Gate 1 audit()",
        ruleOrCheck: DUP_RULE_ID,
        plant: (b) => {
          // Two RequirementUsage elements with the SAME id. RequirementUsage is
          // neither a legacy-provenance type nor a design type, so only the
          // duplicate rule fires (the SECOND occurrence is flagged, elementId ===
          // the shared id).
          const first = mkElement(DUP_ELEMENT_ID, "RequirementUsage", "seeded-dup-first");
          const second = mkElement(DUP_ELEMENT_ID, "RequirementUsage", "seeded-dup-second");
          return { ...b, elements: [...b.elements, first, second] };
        },
        expectRule: DUP_RULE_ID,
        expectElementId: DUP_ELEMENT_ID,
        soleError: true,
      },
      // === DEFECT (f): forged provenance → GATE03-unresolvable-provenance =====
      {
        defect: `(f) forged provenance "${FORGED_PROVENANCE}" on "${UNRESOLVABLE_PROV_ID}"`,
        gate: "Gate 1 audit()",
        ruleOrCheck: UNRESOLVABLE_RULE_ID,
        plant: (b) => {
          // A PartUsage whose provenanceSourceId resolves to NOTHING in the corpus
          // resolution set (the T-05-02 fabrication control). PartUsage is not a
          // legacy DEFINITION type, so the presence check is skipped and only the
          // existence/error finding fires.
          const forged = mkElement(UNRESOLVABLE_PROV_ID, "PartUsage", "seeded-unresolvable-prov", {
            provenanceSourceId: FORGED_PROVENANCE,
          });
          return { ...b, elements: [...b.elements, forged] };
        },
        expectRule: UNRESOLVABLE_RULE_ID,
        expectElementId: UNRESOLVABLE_PROV_ID,
        soleError: true,
      },
      // === DEFECT (g): zero-premise inferred entry → INFER-unpremised =========
      {
        defect: `(g) unpremised inferred entry "${UNPREMISED_INFERRED_ID}" (premises: [])`,
        gate: "Gate 1 audit()",
        ruleOrCheck: UNPREMISED_RULE_ID,
        // Same clean elements/relationships; swap the audit corpus to the
        // in-memory InferredComposedIR carrying the zero-premise entry.
        plant: (b) => ({ ...b, corpus: inferredIR }),
        expectRule: UNPREMISED_RULE_ID,
        expectElementId: UNPREMISED_INFERRED_ID,
        soleError: true,
      },
      // === DEFECT (h): unverbatim approved prose quote → PROSE-unverbatim-quote =
      {
        defect: `(h) unverbatim prose entry "${UNVERBATIM_PROSE_ID}" (quote absent from cited chunk)`,
        gate: "Gate 1 audit()",
        ruleOrCheck: UNVERBATIM_RULE_ID,
        // Same clean elements/relationships; swap the audit corpus to the
        // in-memory ProseComposedIR whose one approved entry's quote is not in
        // the chunk text carried by chunkStore.
        plant: (b) => ({ ...b, corpus: proseIR }),
        expectRule: UNVERBATIM_RULE_ID,
        expectElementId: UNVERBATIM_PROSE_ID,
        soleError: true,
      },
      // === DEFECT (i): entity alias with no merge disposition → ENT-unapproved-merge =
      {
        defect: `(i) unapproved merge entity "${UNAPPROVED_MERGE_ENTITY_ID}" (alias with no disposition)`,
        gate: "Gate 1 audit()",
        ruleOrCheck: UNAPPROVED_MERGE_RULE_ID,
        // Same clean elements/relationships/corpus; supply ONE entity whose alias
        // set spans two normalized surface groups with zero merge dispositions,
        // plus a mention-id store covering both referenced mentions (so only the
        // unapproved-merge rule fires).
        plant: (b) => ({
          ...b,
          entities: [
            {
              entityId: UNAPPROVED_MERGE_ENTITY_ID,
              kind: "component",
              canonicalName: "Fuel Control Module",
              aliases: ["Fuel Control Module", "FCM"], // two distinct normSurfaces
              mentionIds: [UNAPPROVED_MERGE_MENTION_1, UNAPPROVED_MERGE_MENTION_2],
              mergeDispositions: [], // ← the defect: no human-approved merge
            },
          ],
          mentionIds: new Set<string>([
            UNAPPROVED_MERGE_MENTION_1,
            UNAPPROVED_MERGE_MENTION_2,
          ]),
        }),
        expectRule: UNAPPROVED_MERGE_RULE_ID,
        expectElementId: UNAPPROVED_MERGE_ENTITY_ID,
        soleError: true,
      },
      // === DEFECT (j): entity references a missing mentionId → ENT-dangling-mention-ref =
      {
        defect: `(j) dangling mention entity "${DANGLING_MENTION_ENTITY_ID}" (mentionId absent from store)`,
        gate: "Gate 1 audit()",
        ruleOrCheck: DANGLING_MENTION_RULE_ID,
        // Same clean model; ONE entity (single normalized alias group → no
        // unapproved-merge) referencing a mentionId that is NOT in the supplied
        // mention-id store. The store IS supplied (no degrade warning) and holds
        // the entity's OTHER mentionId, so the dangling ref is the sole error.
        plant: (b) => ({
          ...b,
          entities: [
            {
              entityId: DANGLING_MENTION_ENTITY_ID,
              kind: "function",
              canonicalName: "Command Boom",
              aliases: ["Command Boom"], // single normSurface → no merge finding
              mentionIds: [DANGLING_PRESENT_MENTION, DANGLING_ABSENT_MENTION],
              mergeDispositions: [],
            },
          ],
          mentionIds: new Set<string>([DANGLING_PRESENT_MENTION]), // absent one dangles
        }),
        expectRule: DANGLING_MENTION_RULE_ID,
        expectElementId: DANGLING_MENTION_ENTITY_ID,
        soleError: true,
      },
      // === DEFECT (b): poisoned grammar → validator NON-zero; clean → ZERO =====
      {
        defect: `(b) poisoned grammar "requirement "→"requirment " (${path.basename(POISONED_SYSML)})`,
        gate: "Gate 2 validator",
        ruleOrCheck: "exit != 0 on poisoned; exit 0 on clean",
        // Caught iff BOTH directions hold: poisoned rejected AND clean accepted.
        check: async () => {
          grammarControl = await pairedControl<string, number>({
            good: CLEAN_SYSML,
            bad: POISONED_SYSML,
            run: (file) => runValidator(file),
            // PASS = validator exit code 0 (this is the default, made explicit).
            passes: (exitCode) => exitCode === 0,
          });
          return grammarControl.ok;
        },
        detail: () =>
          grammarControl
            ? `poisoned exit=${grammarControl.badResult} (want !=0), ` +
              `clean exit=${grammarControl.goodResult} (want 0)`
            : "grammar control did not run",
      },
    ],
  });

  if (!result.ok) process.exit(1);
}

// ---------------------------------------------------------------------------
// Validator invocation — returns the process exit code (0 = PASS).
// ---------------------------------------------------------------------------

function runValidator(sysmlPath: string): number {
  try {
    execFileSync("bash", [VALIDATOR_SH, sysmlPath], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
    return 0;
  } catch (err) {
    const e = err as { status?: number };
    // exit 2 = venv missing (validator could not run) — treat as a hard failure
    // here too: without the venv there is no grammar gate, so "(b) caught" cannot
    // be claimed. A non-zero, non-1 code still means "not a clean PASS".
    return typeof e.status === "number" ? e.status : 1;
  }
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
