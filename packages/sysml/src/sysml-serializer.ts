import type { SysmlElement, SysmlRelationship } from "@sysml-bridge/model";
import type { InferredApprovedEntry } from "@sysml-bridge/model";

// ---------------------------------------------------------------------------
// SysML v2 type → textual keyword mapping
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SysML v2 trace relationship → textual statement emitters
//
// Trace operands MUST be USAGES (Features), never definitions — this follows
// from the grammar (satisfyRequirementUsage / allocationUsageDeclaration /
// dependency connect Features). The presentation projection (cc-presentation.ts)
// guarantees the operands passed here are package-level usages.
//
// `verify` is NOT emitted here. There is no top-level `verify` production: the
// only legal placement is as a requirementVerificationMember inside the
// `objective { ... }` of a `verification def`. VerifyRequirementUsage /
// RequirementVerificationMembership are therefore handled structurally (a
// nested objective body on the VerificationCaseDefinition), not in this flat
// trace path.
// ---------------------------------------------------------------------------

const TRACE_EMIT: Record<string, (src: string, tgt: string) => string> = {
  SatisfyRequirementUsage: (src, tgt) => `satisfy ${tgt} by ${src};`,
  AllocationUsage: (src, tgt) => `allocate ${src} to ${tgt};`,
  DeriveRequirementUsage: (src, tgt) => `dependency from ${src} to ${tgt};`,
  TraceRequirementUsage: (src, tgt) => `dependency from ${src} to ${tgt};`,
};

// Verify relationships are emitted as nested objective bodies, not flat lines.
const VERIFY_REL_TYPES = new Set([
  "VerifyRequirementUsage",
  "RequirementVerificationMembership",
]);

// ---------------------------------------------------------------------------
// Header-suffix relationships
//
// These modify an element's declaration (appended before the `;` / `{`):
//   Specialization / Subclassification  →  ` :> <baseName>`  (DEFINITION only)
//   Subsetting                           →  ` :> <baseName>`  (USAGE→USAGE)
//   Redefinition                         →  ` :>> <baseName>` (USAGE→USAGE)
//
// The grammar uses `:>` for both subclassification (on definitions) and
// subsetting (on usages); `:>>` is redefinition. BUT the grammar is laxer than
// Cameo's *semantics*: Cameo rejects `:>` on a usage whose target is a
// Definition (a usage cannot specialize a Definition — `:>` on a usage is
// SUBSETTING and the target must itself be a Feature/usage). So the emitter is
// def-vs-usage aware:
//   - Specialization rel: emit `:> X` ONLY when the SOURCE is a Definition.
//   - On a USAGE source: emit `:> X` only for a Subsetting rel whose TARGET is
//     a usage (feature); emit `:>> X` for Redefinition whose target is a usage.
//   - A Specialization rel whose source is a usage is SKIPPED (invalid form).
// ---------------------------------------------------------------------------

function isDefinitionType(type: string): boolean {
  return type.endsWith("Definition");
}

function isUsageType(type: string): boolean {
  return type.endsWith("Usage");
}

/**
 * Decide the header-suffix operator (`:>` / `:>>`) for a relationship given the
 * resolved source and target elements, applying Cameo's def-vs-usage semantics.
 * Returns null when the form is invalid and must be suppressed.
 */
function headerSuffixOp(
  relType: string,
  source: SysmlElement,
  target: SysmlElement | undefined
): ":>" | ":>>" | null {
  switch (relType) {
    case "Specialization":
    case "Subclassification":
      // `:>` specialization is valid ONLY on a Definition source.
      return isDefinitionType(source.type) ? ":>" : null;
    case "Subsetting":
      // `:>` subsetting is valid on a Usage source AND requires a Usage
      // (feature) target — never a Definition.
      if (isUsageType(source.type) && target && isUsageType(target.type)) {
        return ":>";
      }
      // A subsetting on a definition is just subclassification → `:>`.
      if (isDefinitionType(source.type)) return ":>";
      return null;
    case "Redefinition":
      // `:>>` redefinition is valid on a Usage source against a Usage target.
      if (isUsageType(source.type) && target && isUsageType(target.type)) {
        return ":>>";
      }
      return null;
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Nested-statement relationships
//
// These are emitted INSIDE the body of the common owner of their two
// endpoints, after the owner's child elements. Each maps an ordered
// (src, tgt) reference pair to a grammar-valid statement.
// ---------------------------------------------------------------------------

type NestedKind =
  | "connect"
  | "bind"
  | "succession"
  | "flow"
  | "transition"
  | "interface";

const NESTED_REL_KIND: Record<string, NestedKind> = {
  Connector: "connect",
  ConnectionUsage: "connect",
  BindingConnector: "bind",
  Succession: "succession",
  SuccessionAsUsage: "succession",
  Flow: "flow",
  FlowConnectionUsage: "flow",
  Transition: "transition",
  TransitionUsage: "transition",
  InterfaceUsage: "interface",
};

function nestedStatement(
  kind: NestedKind,
  src: string,
  tgt: string,
  name: string | null,
  payloadType?: string | null,
  typeName?: string | null,
  guard?: string | null
): string {
  switch (kind) {
    case "connect":
      return name !== null
        ? `connection ${name} connect ${src} to ${tgt};`
        : `connect ${src} to ${tgt};`;
    case "bind":
      return `bind ${src} = ${tgt};`;
    case "succession":
      // Guarded succession: `first X if <guard> then Y;` — validated in activity-control-flow.sysml
      if (guard && guard.length > 0) {
        return `first ${src} if ${guard} then ${tgt};`;
      }
      return `first ${src} then ${tgt};`;
    case "transition":
      return `transition first ${src} then ${tgt};`;
    case "flow":
      // Typed item flow: `flow of <Type> from <src> to <tgt>;`.
      if (payloadType) {
        return `flow of ${quoteName(payloadType)} from ${src} to ${tgt};`;
      }
      return `flow from ${src} to ${tgt};`;
    case "interface":
      // `interface <name> connect a to b;` (optionally typed:
      // `interface <name> : <Type> connect a to b;`).
      if (name !== null) {
        const typed =
          typeof typeName === "string" && typeName.length > 0
            ? ` : ${quoteName(typeName)}`
            : "";
        return `interface ${name}${typed} connect ${src} to ${tgt};`;
      }
      return `interface connect ${src} to ${tgt};`;
  }
}

const TYPE_TO_KEYWORD: Record<string, string> = {
  Package: "package",
  PartDefinition: "part def",
  PartUsage: "part",
  PortDefinition: "port def",
  PortUsage: "port",
  ConnectionDefinition: "connection def",
  ConnectionUsage: "connection",
  InterfaceDefinition: "interface def",
  InterfaceUsage: "interface",
  ItemDefinition: "item def",
  ItemUsage: "item",
  AttributeDefinition: "attribute def",
  AttributeUsage: "attribute",
  RequirementDefinition: "requirement def",
  RequirementUsage: "requirement",
  ConstraintDefinition: "constraint def",
  ConstraintUsage: "constraint",
  ActionDefinition: "action def",
  ActionUsage: "action",
  // Activity control nodes — validated against activity-control-flow.sysml fixture
  DecisionNode: "decide",
  ForkNode: "fork",
  JoinNode: "join",
  MergeNode: "merge",
  StateDefinition: "state def",
  StateUsage: "state",
  UseCaseDefinition: "use case def",
  UseCaseUsage: "use case",
  AllocationDefinition: "allocation def",
  AllocationUsage: "allocation",
  ViewDefinition: "view def",
  ViewUsage: "view",
  ViewpointDefinition: "viewpoint def",
  ViewpointUsage: "viewpoint",
  ConcernDefinition: "concern def",
  ConcernUsage: "concern",
  // CRITICAL: VerificationCaseDefinition → "verification def" (NOT "verification case def")
  VerificationCaseDefinition: "verification def",
  VerificationCaseUsage: "verification",
  // CRITICAL: AnalysisCaseDefinition → "analysis def" (NOT "analysis case def")
  AnalysisCaseDefinition: "analysis def",
  AnalysisCaseUsage: "analysis",
  EnumerationDefinition: "enum def",
  EnumerationUsage: "enum",
  CalcDefinition: "calc def",
  CalcUsage: "calc",
  RenderingDefinition: "rendering def",
  RenderingUsage: "rendering",
  OccurrenceDefinition: "occurrence def",
  OccurrenceUsage: "occurrence",
  MetadataDefinition: "metadata def",
};

// ---------------------------------------------------------------------------
// Internal: a resolved nested statement keyed by its owner.
// ---------------------------------------------------------------------------

interface NestedStmt {
  ownerId: string;
  text: string;
}

// ---------------------------------------------------------------------------
// Invariant context threaded through the recursive element serializer. Only
// `element`/`parent`/`depth` change between recursion levels; everything here
// is built once in serializeToSysml and shared by reference. `childrenByOwner`
// is pre-filtered (suppressed connection-like elements removed), so the
// recursion needs no separate suppressed-id set.
// ---------------------------------------------------------------------------

interface SerializeCtx {
  elementById: Map<string, SysmlElement>;
  childrenByOwner: Map<string, SysmlElement[]>;
  lines: string[];
  verifyByCase: Map<string, string[]>;
  headerSuffixById: Map<string, string[]>;
  nestedByOwner: Map<string, string[]>;
  emitElementIds: boolean;
}

// ---------------------------------------------------------------------------
// SysML standard-library scalar value types (TF-10).
//
// When any referenced typeName is one of these, the model needs the
// ScalarValues library in scope. We emit a single `import ScalarValues::*;` at
// the very top of the output so Cameo (and the grammar) resolve `Real`,
// `Integer`, etc. The import is emitted ONLY when at least one such type is
// referenced, so models that use no scalar types (ANGARS, round-2 demos) are
// byte-identical to before.
// ---------------------------------------------------------------------------

const SCALAR_VALUE_TYPES = new Set([
  "Real",
  "Integer",
  "Boolean",
  "String",
  "Natural",
  "Rational",
  "Complex",
  "ScalarValue",
]);

/**
 * True if any element in the model references a standard scalar value type via
 * its `raw.typeName`. Scans all elements (typing is the only place a scalar
 * type name appears in serialized output).
 */
function referencesScalarType(elements: SysmlElement[]): boolean {
  for (const e of elements) {
    const t = e.raw?.typeName;
    if (typeof t === "string" && SCALAR_VALUE_TYPES.has(t)) return true;
  }
  return false;
}

// ---------------------------------------------------------------------------
// InferenceProvenance metatag emission (F8 / §5)
//
// emitInferenceProvenanceTags builds the `metadata def InferenceProvenance { ... }`
// block (emitted once) and per-element `metadata InferenceProvenance about <name> { ... }`
// blocks (emitted once per tagged element).
//
// RATIONALE IS NEVER EXPORTED (DEBAT-04 discipline). premiseRefs carries ids only,
// never corpus quotes. The def uses ScalarValues::* types (import already emitted by
// referencesScalarType path; we add it here when needed for the metatag even if no
// element references a scalar type).
// ---------------------------------------------------------------------------

/** Options controlling optional metatag emission */
export interface SerializeOptions {
  /**
   * When true: model-asserted elements (provenanceSourceId === "model-asserted")
   * also receive an InferenceProvenance tag with provenanceClass = "asserted".
   * Default: false (no tags for model-asserted elements).
   */
  emitAssertedTags?: boolean;
  /**
   * When true: every emitted element carries its stable `id` via a trailing
   * `// @id: <uuid>` comment on its header line (Milestone 1 — identity
   * round-trip; see sysml-parser.ts extractIdCommentsByLine for the recovery
   * side and its doc comment for why a comment convention was chosen over a
   * metadata annotation). Default: false — opt in per call site so existing
   * exact-output assertions in unit tests are unaffected; the real interop
   * paths (export_sysml, the ANGARS demo pipeline) enable it.
   */
  emitElementIds?: boolean;
}

/**
 * Build the InferenceProvenance metadata def block (§5 validated form).
 * Emitted once per model.
 */
function inferenceProvenanceDefBlock(): string {
  return [
    "metadata def InferenceProvenance {",
    "    attribute provenanceClass : ScalarValues::String;",
    "    attribute confidenceScore : ScalarValues::Real;",
    "    attribute premiseRefs : ScalarValues::String;",
    "    attribute inferenceRunId : ScalarValues::String;",
    "    attribute approvedBy : ScalarValues::String;",
    "}",
  ].join("\n");
}

/**
 * Build a per-element `metadata InferenceProvenance about <name> { ... }` block.
 * Rationale is NEVER included. premiseRefs carries ids only.
 */
function inferenceProvenanceAboutBlock(
  elementName: string,
  provenanceClass: "inferred" | "asserted",
  entry: {
    confidence?: number;
    premises?: string[];
    inferenceRunId?: string;
    approvedBy?: string;
  }
): string {
  const confidence = entry.confidence ?? 0;
  const premiseRefs = (entry.premises ?? []).join(", ");
  const runId = entry.inferenceRunId ?? "";
  const approvedBy = entry.approvedBy ?? "";

  return [
    `metadata InferenceProvenance about ${quoteName(elementName)} {`,
    `    provenanceClass = "${provenanceClass}";`,
    `    confidenceScore = ${confidence};`,
    `    premiseRefs = "${premiseRefs}";`,
    `    inferenceRunId = "${runId}";`,
    `    approvedBy = "${approvedBy}";`,
    `}`,
  ].join("\n");
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function serializeToSysml(
  elements: SysmlElement[],
  relationships: SysmlRelationship[],
  approvedInferredEntries?: InferredApprovedEntry[],
  options?: SerializeOptions
): string {
  // Build a set of all element IDs for fast lookup
  const elementIds = new Set(elements.map((e) => e.id));

  // Build map of elements by ID
  const elementById = new Map<string, SysmlElement>(
    elements.map((e) => [e.id, e])
  );

  // Name -> first element id with that name, for O(1) endpoint resolution by
  // name (first-wins mirrors the previous linear scan over insertion order).
  const idByName = new Map<string, string>();
  for (const e of elements) {
    if (e.name !== null && !idByName.has(e.name)) idByName.set(e.name, e.id);
  }

  // Find root elements: those whose ownerId is null or not present in the element set
  const roots = elements.filter(
    (e) => e.ownerId === null || !elementIds.has(e.ownerId)
  );

  // Group verify relationships by their verification-case source id so each
  // VerificationCaseDefinition can emit a nested `objective { verify ...; }`
  // body. Map: verificationCaseId -> [requirement reference names].
  const verifyByCase = new Map<string, string[]>();
  for (const rel of relationships) {
    if (!VERIFY_REL_TYPES.has(rel.type)) continue;
    const caseId = rel.sourceIds[0];
    const reqName = refName(rel.targetIds[0], elementById);
    if (caseId === undefined || reqName === null) continue;
    if (!elementIds.has(caseId)) continue;
    if (!verifyByCase.has(caseId)) verifyByCase.set(caseId, []);
    verifyByCase.get(caseId)!.push(reqName);
  }

  // ----- Header-suffix relationships (Specialization/Subsetting/Redefinition)
  // Map: sourceElementId -> array of suffix fragments (" :> Base", " :>> Base").
  // Cameo's def-vs-usage semantics are enforced by headerSuffixOp(): an invalid
  // form (e.g. a usage `:>` a Definition) is suppressed rather than emitted.
  const headerSuffixById = new Map<string, string[]>();
  for (const rel of relationships) {
    const srcId = rel.sourceIds[0];
    if (srcId === undefined || !elementIds.has(srcId)) continue;
    const source = elementById.get(srcId);
    if (!source) continue;
    const tgtId = rel.targetIds[0];
    const target = tgtId !== undefined ? elementById.get(tgtId) : undefined;
    const op = headerSuffixOp(rel.type, source, target);
    if (op === null) continue;
    const baseName = refName(tgtId, elementById);
    if (baseName === null) continue;
    if (!headerSuffixById.has(srcId)) headerSuffixById.set(srcId, []);
    headerSuffixById.get(srcId)!.push(` ${op} ${baseName}`);
  }

  // ----- Nested-statement relationships (connect / bind / first..then /
  // flow / transition). Resolve each to a (ownerId, text) pair, grouped later
  // by owner so we can emit them inside the right body.
  const nestedByOwner = new Map<string, string[]>();

  // Track element ids that are themselves connection-like elements
  // (ConnectionUsage / FlowConnectionUsage / TransitionUsage) so we do NOT
  // also emit them as ordinary element declarations.
  const suppressedElementIds = new Set<string>();

  // (a) Relationship-shaped nested rels (Connector, Succession, Transition,
  // Flow...). A "Flow" rel may carry raw.payloadType for a typed item flow.
  // A "Succession" rel may carry raw.guard for a guarded succession (`first X if <g> then Y;`).
  for (const rel of relationships) {
    const kind = NESTED_REL_KIND[rel.type];
    if (!kind) continue;
    const payloadType =
      kind === "flow" && typeof rel.raw?.payloadType === "string"
        ? (rel.raw.payloadType as string)
        : null;
    const guard =
      kind === "succession" && typeof rel.raw?.guard === "string" && (rel.raw.guard as string).length > 0
        ? (rel.raw.guard as string)
        : null;
    const stmt = resolveNestedFromEndpoints(
      kind,
      rel.sourceIds[0],
      rel.targetIds[0],
      typeof rel.raw?.name === "string" ? (rel.raw.name as string) : null,
      typeof rel.raw?.ownerId === "string" ? (rel.raw.ownerId as string) : undefined,
      elementById,
      elementIds,
      payloadType,
      null,
      guard
    );
    if (stmt) addNested(nestedByOwner, stmt);
  }

  // (b) Element-shaped nested rels (ConnectionUsage / FlowConnectionUsage /
  // TransitionUsage / InterfaceUsage elements carrying raw.sourceEnd +
  // raw.targetEnd). These live in the element tree but must be emitted as
  // statements, not blocks.
  for (const e of elements) {
    const kind = NESTED_REL_KIND[e.type];
    if (!kind) continue;
    const srcEnd = e.raw?.sourceEnd;
    const tgtEnd = e.raw?.targetEnd;
    if (srcEnd === undefined || tgtEnd === undefined) continue;
    // Resolve endpoint refs (by id if it matches an element, else literal name).
    const srcId = resolveEndpointId(srcEnd, elementIds, idByName);
    const tgtId = resolveEndpointId(tgtEnd, elementIds, idByName);
    // ConnectionUsage and InterfaceUsage carry a name in the statement
    // (`connection L ...` / `interface L ...`).
    const stmtName = kind === "connect" || kind === "interface" ? e.name : null;
    // FlowConnectionUsage may carry a typed payload (`flow of <Type> ...`).
    const payloadType =
      kind === "flow" && typeof e.raw?.payloadType === "string"
        ? (e.raw.payloadType as string)
        : null;
    // InterfaceUsage may be typed (`interface L : <Type> connect ...`).
    const ifaceType =
      kind === "interface" && typeof e.raw?.typeName === "string"
        ? (e.raw.typeName as string)
        : null;
    const stmt = resolveNestedFromEndpoints(
      kind,
      srcId,
      tgtId,
      stmtName,
      e.ownerId ?? undefined,
      elementById,
      elementIds,
      payloadType,
      ifaceType
    );
    if (stmt) {
      addNested(nestedByOwner, stmt);
      suppressedElementIds.add(e.id);
    }
  }

  // ----- Include use-case relationships. An `IncludeUseCase` / `Include` rel
  // whose SOURCE is a UseCaseDefinition emits `include use case <targetName>;`
  // inside that def's body, after its children. These are merged into
  // nestedByOwner so they flow through the same body-emission path.
  for (const rel of relationships) {
    if (rel.type !== "IncludeUseCase" && rel.type !== "Include") continue;
    const srcId = rel.sourceIds[0];
    if (srcId === undefined || !elementIds.has(srcId)) continue;
    const tgtName = refName(rel.targetIds[0], elementById);
    if (tgtName === null) continue;
    addNested(nestedByOwner, {
      ownerId: srcId,
      text: `include use case ${tgtName};`,
    });
  }

  // ----- State action-membership relationships. A `StateActionMembership` rel
  // whose SOURCE is a StateUsage/StateDefinition emits `do <targetName>;` inside
  // that state's body — the state-behavior `do` compartment member (validated:
  // `do <ref>;` and `do '<quoted name>';` both parse inside a state body; rendered
  // by the decisym viewer's entry/do/exit compartment parse, which strips quotes).
  // The reference is quoteName'd so function names with spaces/`&` (e.g. "Monitor
  // Flow & Stability") become `do 'Monitor Flow & Stability';` rather than an
  // invalid bare token. Merged into nestedByOwner so it flows through the same
  // body-emission path.
  for (const rel of relationships) {
    if (rel.type !== "StateActionMembership") continue;
    const srcId = rel.sourceIds[0];
    if (srcId === undefined || !elementIds.has(srcId)) continue;
    const rawDoRef =
      typeof rel.raw?.doRef === "string" && rel.raw.doRef.length > 0
        ? (rel.raw.doRef as string)
        : refNameRaw(rel.targetIds[0], elementById);
    if (rawDoRef === null) continue;
    addNested(nestedByOwner, {
      ownerId: srcId,
      text: `do ${quoteName(rawDoRef)};`,
    });
  }

  // Group non-suppressed elements by owner id ONCE, so each element's children
  // are an O(1) lookup instead of a full O(n) re-scan per recursion level.
  // Built after suppressedElementIds is finalized; insertion order mirrors the
  // previous per-element filter over elementById.values().
  const childrenByOwner = new Map<string, SysmlElement[]>();
  for (const e of elementById.values()) {
    if (e.ownerId === null || suppressedElementIds.has(e.id)) continue;
    if (!childrenByOwner.has(e.ownerId)) childrenByOwner.set(e.ownerId, []);
    childrenByOwner.get(e.ownerId)!.push(e);
  }

  const lines: string[] = [];

  // Build the inferred-entry lookup: id → entry.
  // Also determine which element ids are tagged (by provenanceSourceId).
  const inferredById = new Map<string, InferredApprovedEntry>(
    (approvedInferredEntries ?? []).map((e) => [e.id, e])
  );

  // Determine if any element needs a metatag:
  //   - provenanceSourceId matches an approved inferred entry id → "inferred"
  //   - provenanceSourceId === "model-asserted" and emitAssertedTags → "asserted"
  const emitAssertedTags = options?.emitAssertedTags ?? false;
  const taggedElements: Array<{
    element: SysmlElement;
    provenanceClass: "inferred" | "asserted";
    entry: InferredApprovedEntry | null;
  }> = [];

  for (const e of elements) {
    const prov = e.raw?.provenanceSourceId;
    if (typeof prov !== "string" || prov.length === 0) continue;
    if (inferredById.has(prov)) {
      taggedElements.push({ element: e, provenanceClass: "inferred", entry: inferredById.get(prov)! });
    } else if (emitAssertedTags && prov === "model-asserted") {
      taggedElements.push({ element: e, provenanceClass: "asserted", entry: null });
    }
  }

  const needsMetatags = taggedElements.length > 0;

  // TF-10: emit `import ScalarValues::*;` at the very top when the model
  // references any standard scalar value type (Real, Integer, ...) OR when
  // metatag emission is needed (the def uses ScalarValues:: types). Only when
  // needed — so scalar-free models without metatags stay byte-identical.
  if (referencesScalarType(elements) || needsMetatags) {
    lines.push("import ScalarValues::*;");
    lines.push("");
  }

  const ctx: SerializeCtx = {
    elementById,
    childrenByOwner,
    lines,
    verifyByCase,
    headerSuffixById,
    nestedByOwner,
    emitElementIds: options?.emitElementIds ?? false,
  };

  for (const root of roots) {
    serializeElement(root, null, 0, ctx);
  }

  // Emit trace relationship statements wrapped in a package body.
  // Wrapping in a package (rather than top-level emission) is required so the
  // decisym-viewer parser can resolve the relationships: both `satisfy` and
  // `dependency` handlers require a non-None parent element — top-level
  // statements with parent=None are silently dropped by parse_top_level().
  // A `package 'C&C Trace' { ... }` block gives each statement a parent
  // context while remaining valid per the grammar (satisfyRequirementUsage is
  // legal inside packageMember → usageElement → occurrenceUsageElement →
  // behaviorUsageElement; dependency is a definitionElement). The name matches
  // the traceability-demo.sysml probe and the viewer's Traceability view spec,
  // so the whole-model cross-pillar web resolves 'C&C Trace' as its context.
  const traceLines: string[] = [];
  for (const rel of relationships) {
    const emitter = TRACE_EMIT[rel.type];
    if (!emitter) continue;

    const srcName = refName(rel.sourceIds[0], elementById);
    const tgtName = refName(rel.targetIds[0], elementById);
    if (srcName === null || tgtName === null) continue;

    traceLines.push(emitter(srcName, tgtName));
  }

  if (traceLines.length > 0) {
    lines.push("");
    lines.push("package 'C&C Trace' {");
    lines.push("  // traceability");
    for (const tl of traceLines) {
      lines.push(`  ${tl}`);
    }
    lines.push("}");
  }

  // Any nested statements whose owner was NOT emitted at all (e.g. the owner
  // id is not in the element set / unresolved) are appended at the end so they
  // are not silently dropped. An owner that IS in the element set always opens
  // a body (the serializer opens one when it has children OR nested
  // statements), so this backstop only fires for orphan owners.
  for (const [ownerId, stmts] of nestedByOwner) {
    if (elementIds.has(ownerId)) continue;
    for (const s of stmts) lines.push(s);
  }

  // ── InferenceProvenance metatag emission (F8 §5) ──────────────────────────
  // Emitted AFTER all element/trace content so it does not interfere with the
  // structural model parse. Def is emitted once; about blocks per tagged element.
  // RATIONALE IS NEVER EXPORTED (DEBAT-04). premiseRefs carries ids only.
  if (needsMetatags) {
    lines.push("");
    lines.push(inferenceProvenanceDefBlock());
    for (const { element, provenanceClass, entry } of taggedElements) {
      if (element.name === null) continue;
      lines.push("");
      lines.push(
        inferenceProvenanceAboutBlock(element.name, provenanceClass, {
          confidence: entry?.confidence,
          premises: entry?.premises,
          inferenceRunId: entry?.inferenceRunId,
          approvedBy: entry?.approvedBy,
        })
      );
    }
  }

  // Ensure output ends with a single newline
  let result = lines.join("\n");
  if (!result.endsWith("\n")) {
    result += "\n";
  }

  return result;
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

function serializeElement(
  element: SysmlElement,
  parent: SysmlElement | null,
  depth: number,
  ctx: SerializeCtx
): void {
  const { lines } = ctx;
  const prefix = "  ".repeat(depth);

  // ----- Enumeration literal: a child of an EnumerationDefinition is emitted
  // as a BARE `<name>;` (no `enum` keyword), e.g. `enum def Color { red; ... }`.
  if (
    parent !== null &&
    parent.type === "EnumerationDefinition" &&
    element.name !== null
  ) {
    lines.push(`${prefix}${quoteName(element.name)};`);
    return;
  }

  // ----- Keyword selection. Two context-dependent special cases:
  //   - an `end`-tagged PortUsage inside an interface def → keyword `end`.
  //   - an `actor`-tagged usage inside a use case def     → keyword `actor`.
  let keyword = TYPE_TO_KEYWORD[element.type] ?? element.type;
  if (element.raw.end === true && parent && parent.type === "InterfaceDefinition") {
    keyword = "end";
  } else if (
    element.raw.actor === true &&
    parent &&
    parent.type === "UseCaseDefinition"
  ) {
    keyword = "actor";
  }

  // ----- Constraint usage assertion: `assert constraint <name> : <Type>;`.
  if (element.type === "ConstraintUsage" && element.raw.asserted === true) {
    keyword = `assert ${keyword}`;
  }

  // Build the header: keyword + optional shortName + name
  let header = `${prefix}${keyword}`;

  if (element.shortName !== null) {
    header += ` <'${element.shortName}'>`;
  }

  if (element.name !== null) {
    header += ` ${quoteName(element.name)}`;
  }

  // Typed usage: append `: 'TypeName'` (feature typing). Driven by
  // raw.typeName, set by the presentation projection for usage elements.
  const typeName = element.raw.typeName;
  if (typeof typeName === "string" && typeName.length > 0) {
    header += ` : ${quoteName(typeName)}`;
  }

  // Multiplicity: append `[mult]` AFTER the type (or after the name if no
  // type). Grammar order is `<name> : <Type>[mult] :> <Super>`.
  const multiplicity = element.raw.multiplicity;
  if (typeof multiplicity === "string" && multiplicity.length > 0) {
    header += `[${multiplicity}]`;
  }

  // Header-suffix relationships: specialization/subsetting (`:>`) and
  // redefinition (`:>>`). Appended after the multiplicity. Subclassification /
  // specialization (`:>`) come before redefinition (`:>>`) for readability;
  // both orders parse, but a stable order keeps output deterministic.
  const suffixes = ctx.headerSuffixById.get(element.id);
  if (suffixes && suffixes.length > 0) {
    const ordered = [...suffixes].sort((a, b) => {
      const aRedef = a.startsWith(" :>>") ? 1 : 0;
      const bRedef = b.startsWith(" :>>") ? 1 : 0;
      return aRedef - bRedef;
    });
    header += ordered.join("");
  }

  // Attribute (or any usage) value: append ` = <value>`, e.g.
  // `attribute capacity = 100;` / `attribute voltage : Real = 48.0;`.
  const value = element.raw.value;
  if (value !== undefined && value !== null) {
    header += ` = ${String(value)}`;
  }

  // ----- Constraint definition with an expression body:
  // `constraint def C { <expression> }`. Emitted as a single-line body.
  if (
    element.type === "ConstraintDefinition" &&
    typeof element.raw.expression === "string" &&
    element.raw.expression.length > 0
  ) {
    const provSuffix = buildAnnotationSuffix(element, ctx.emitElementIds);
    lines.push(`${header} {${provSuffix}`);
    lines.push(`${"  ".repeat(depth + 1)}${element.raw.expression}`);
    lines.push(`${prefix}}`);
    return;
  }

  // Children (elements whose ownerId matches this element's id), with
  // connection-like elements already excluded — precomputed in childrenByOwner.
  const children = ctx.childrenByOwner.get(element.id) ?? [];

  // A VerificationCaseDefinition with verify edges emits a nested objective
  // body: `verification def V { objective { verify <reqUsage>; } }`.
  const verifyTargets =
    element.type === "VerificationCaseDefinition"
      ? ctx.verifyByCase.get(element.id) ?? []
      : [];

  // Nested statements (connect/first-then/flow/transition/bind) whose common
  // owner is THIS element, emitted after the child elements inside the body.
  const nestedStmts = ctx.nestedByOwner.get(element.id) ?? [];

  // Append provenance/id annotation comment if either applies.
  const provenanceSuffix = buildAnnotationSuffix(element, ctx.emitElementIds);

  if (
    children.length > 0 ||
    verifyTargets.length > 0 ||
    nestedStmts.length > 0
  ) {
    lines.push(`${header} {${provenanceSuffix}`);
    for (const child of children) {
      serializeElement(child, element, depth + 1, ctx);
    }
    if (verifyTargets.length > 0) {
      const inner = "  ".repeat(depth + 1);
      const inner2 = "  ".repeat(depth + 2);
      lines.push(`${inner}objective {`);
      for (const reqName of verifyTargets) {
        lines.push(`${inner2}verify ${reqName};`);
      }
      lines.push(`${inner}}`);
    }
    if (nestedStmts.length > 0) {
      const inner = "  ".repeat(depth + 1);
      for (const stmt of nestedStmts) {
        lines.push(`${inner}${stmt}`);
      }
    }
    lines.push(`${prefix}}`);
  } else {
    lines.push(`${header};${provenanceSuffix}`);
  }
}

/**
 * Build the trailing `// @source: ... @id: ...` annotation comment for an
 * element's header line. Both annotations share ONE comment (a second `//`
 * on the same line would just become part of the first comment's text, per
 * the grammar's `REGULAR_COMMENT : '//' ~[\r\n]* -> skip` — see
 * docs/sysml-v2-reference/grammar/SysMLv2Lexer.g4:916), so they are joined
 * with a space when both are present. Returns "" when neither applies.
 */
function buildAnnotationSuffix(element: SysmlElement, emitElementIds: boolean): string {
  const parts: string[] = [];
  const provenanceSourceId = element.raw.provenanceSourceId;
  if (typeof provenanceSourceId === "string" && provenanceSourceId.length > 0) {
    parts.push(`@source: ${provenanceSourceId}`);
  }
  if (emitElementIds) {
    parts.push(`@id: ${element.id}`);
  }
  return parts.length > 0 ? `  // ${parts.join(" ")}` : "";
}

function isValidIdentifier(name: string): boolean {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name);
}

/** Render a name as a SysML reference token: bare when it is a valid
 *  identifier, otherwise single-quoted with grammar-legal escapes
 *  (SysMLv2Lexer.g4:893 — a raw ' or \ inside a quoted name is illegal). */
function quoteName(name: string): string {
  if (isValidIdentifier(name)) return name;
  const escaped = name
    .replace(/\\/g, "\\\\")
    .replace(/'/g, "\\'")
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "\\r")
    .replace(/\t/g, "\\t");
  return `'${escaped}'`;
}

function refName(
  id: string | undefined,
  elementById: Map<string, SysmlElement>
): string | null {
  if (id === undefined) return null;
  const element = elementById.get(id);
  if (!element) return null;
  if (element.name === null) return null;
  return quoteName(element.name);
}

/** Like refName but returns the RAW (unquoted) name, for callers that apply
 *  their own quoting (e.g. the `do <ref>;` state-member emitter). */
function refNameRaw(
  id: string | undefined,
  elementById: Map<string, SysmlElement>
): string | null {
  if (id === undefined) return null;
  const element = elementById.get(id);
  if (!element) return null;
  return element.name;
}

// ---------------------------------------------------------------------------
// Nested-statement endpoint resolution
// ---------------------------------------------------------------------------

/**
 * Given a raw endpoint reference (an element id, an element name, or a
 * one-level-qualified `parent.child` string), return the element id if it can
 * be matched, otherwise the raw value as a literal name.
 */
function resolveEndpointId(
  endpoint: unknown,
  elementIds: Set<string>,
  idByName: Map<string, string>
): string | undefined {
  if (typeof endpoint !== "string" || endpoint.length === 0) return undefined;
  if (elementIds.has(endpoint)) return endpoint;
  // Resolve by name (O(1) lookup); otherwise keep the literal (may already be a
  // qualified ref like `battery.dcOut`).
  return idByName.get(endpoint) ?? endpoint;
}

/**
 * Resolve a reference name for an endpoint id:
 *   - direct child of the common owner → simple name
 *   - a port/feature owned by a child of the owner → `<childName>.<portName>`
 * If the id is not a known element, the id/string is returned verbatim (it may
 * already be a literal name or a qualified ref).
 */
function endpointRefName(
  endpointId: string | undefined,
  ownerId: string | undefined,
  elementById: Map<string, SysmlElement>
): string | null {
  if (endpointId === undefined) return null;
  const e = elementById.get(endpointId);
  if (!e) {
    // Not a known element id — treat as a literal reference string.
    return endpointId;
  }
  if (e.name === null) return null;
  const simple = quoteName(e.name);

  // Direct child of the common owner → simple name.
  if (e.ownerId === ownerId || ownerId === undefined) return simple;

  // Port/feature owned by a child of the owner → one-level qualified name.
  const parent = e.ownerId ? elementById.get(e.ownerId) : undefined;
  if (parent && parent.ownerId === ownerId && parent.name !== null) {
    return `${quoteName(parent.name)}.${simple}`;
  }

  // Fallback: simple name.
  return simple;
}

/**
 * Determine the common owner id for two endpoints, then build the nested
 * statement. Returns null if the statement cannot be resolved safely.
 */
function resolveNestedFromEndpoints(
  kind: NestedKind,
  srcId: string | undefined,
  tgtId: string | undefined,
  name: string | null,
  explicitOwnerId: string | undefined,
  elementById: Map<string, SysmlElement>,
  elementIds: Set<string>,
  payloadType?: string | null,
  typeName?: string | null,
  guard?: string | null
): NestedStmt | null {
  if (srcId === undefined || tgtId === undefined) return null;

  const ownerId =
    explicitOwnerId ?? commonOwner(srcId, tgtId, elementById, elementIds);
  if (ownerId === undefined) return null;

  const srcRef = endpointRefName(srcId, ownerId, elementById);
  const tgtRef = endpointRefName(tgtId, ownerId, elementById);
  if (srcRef === null || tgtRef === null) return null;

  return {
    ownerId,
    text: nestedStatement(kind, srcRef, tgtRef, name, payloadType, typeName, guard),
  };
}

/**
 * Compute the common owner of two endpoints. The common owner is:
 *   - the element that owns BOTH endpoints directly, OR
 *   - (for ports/features on sub-parts) the element that owns both endpoints'
 *     owners.
 * Returns undefined if no common owner can be found within the element set.
 */
function commonOwner(
  srcId: string,
  tgtId: string,
  elementById: Map<string, SysmlElement>,
  elementIds: Set<string>
): string | undefined {
  const src = elementById.get(srcId);
  const tgt = elementById.get(tgtId);
  if (!src || !tgt) return undefined;

  // Ancestor chains (owner ids), nearest-first.
  const srcChain = ownerChain(src, elementById);
  const tgtChain = ownerChain(tgt, elementById);
  const tgtSet = new Set(tgtChain);
  for (const a of srcChain) {
    if (tgtSet.has(a) && elementIds.has(a)) return a;
  }
  return undefined;
}

/** Return the chain of owner ids for an element, nearest-first. */
function ownerChain(
  element: SysmlElement,
  elementById: Map<string, SysmlElement>
): string[] {
  const chain: string[] = [];
  let cur: SysmlElement | undefined = element;
  // Walk up via ownerId.
  while (cur && cur.ownerId) {
    chain.push(cur.ownerId);
    cur = elementById.get(cur.ownerId);
  }
  return chain;
}

function addNested(map: Map<string, string[]>, stmt: NestedStmt): void {
  if (!map.has(stmt.ownerId)) map.set(stmt.ownerId, []);
  map.get(stmt.ownerId)!.push(stmt.text);
}
