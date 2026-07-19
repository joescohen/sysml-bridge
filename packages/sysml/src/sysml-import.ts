import { randomUUID } from "node:crypto";

import type { ParsedElement, ParsedRelationship } from "./sysml-parser.js";

// ---------------------------------------------------------------------------
// sysml-import.ts — parsed-text -> ModelStore input mapping
//
// Milestone 1 (docs/superpowers/specs/2026-07-11-repository-substrate-design.md
// §3): identity + structure round trip through text. This module is the
// SHARED reimport logic used by BOTH packages/mcp-server's import_sysml tool
// and the roundtrip acceptance test (packages/sysml/src/__tests__/roundtrip.test.ts)
// — a single source of truth for the two things a naive `parsed.elements.map(...)`
// gets wrong:
//
//   1. `parsed.elements` is a TREE (ParsedElement.children), not a flat list.
//      Mapping only the top-level array (the pre-Milestone-1 behavior in
//      import-sysml.ts) silently drops every nested element — e.g. a
//      `package P { part def Widget; }` would only ever create the Package.
//      parsedElementsToStoreInputs() recursively flattens the tree, assigning
//      each child's `ownerId` to its parent's PRE-COMPUTED id (ids must be
//      known up front, before any ModelStore.createElements() call, since a
//      child's attributes.ownerId needs a real id string — not something the
//      store can back-fill after the fact for a whole batch).
//
//   2. The recovered `elementId` (from the `// @id: <uuid>` comment — see
//      sysml-parser.ts) must be threaded into `attributes["@id"]` so
//      FileStore.buildElement() REUSES it instead of minting a fresh
//      randomUUID() (packages/model/src/store/file-store.ts ~line 252-254).
//      When no id was recovered (hand-authored text, or emitElementIds was
//      off when the source was serialized), a fresh id is minted here —
//      functionally identical to the store minting it itself, just decided
//      one layer up so nested ownerId wiring can reference it immediately.
//
// parsedRelationshipsToStoreInputs() reconstructs satisfy/verify/allocate/
// dependency relationships (the kinds required by Milestone 1's round-trip
// acceptance test, including R4-compliant usage operands) by resolving the
// parser's NAME-based endpoint references against the just-created elements.
// ---------------------------------------------------------------------------

export interface StoreElementInput {
  type: string;
  name: string;
  attributes: Record<string, unknown>;
}

/**
 * Recursively flatten a parsed element tree into ModelStore.createElements()
 * inputs, preserving containment (ownerId) and threading the recovered/minted
 * id into `attributes["@id"]`. Returned in pre-order (parent before its
 * children), which is REQUIRED: FileStore.insert() attaches a child to its
 * owner by looking the owner up in already-inserted elements, so a child
 * must never precede its parent in the batch.
 */
export function parsedElementsToStoreInputs(
  elements: ParsedElement[],
  ownerId: string | null = null
): StoreElementInput[] {
  const out: StoreElementInput[] = [];
  for (const e of elements) {
    const id = e.elementId ?? randomUUID();
    const attributes: Record<string, unknown> = {
      ...e.attributes,
      "@id": id,
    };
    if (ownerId !== null) attributes.ownerId = ownerId;
    // shortName is part of Milestone 1's required text-carried set
    // ({id, elementId, type, name, shortName, qualifiedName, ownerId,
    // ownedElementIds}) — thread it through so FileStore.buildElement picks
    // it up (it reads attributes.shortName / attributes.declaredShortName).
    if (e.shortName !== undefined) attributes.shortName = e.shortName;
    out.push({ type: e.type, name: e.name, attributes });
    if (e.children.length > 0) {
      out.push(...parsedElementsToStoreInputs(e.children, id));
    }
  }
  return out;
}

/** Minimal shape needed to resolve relationship endpoints by name. */
interface NamedElementRef {
  id: string;
  name: string | null;
}

/**
 * Build a first-name-wins name -> element index (mirrors the serializer's
 * own `idByName` convention in sysml-serializer.ts), used to resolve a
 * ParsedRelationship's textual endpoint names back to element ids.
 */
export function buildNameIndex<T extends NamedElementRef>(elements: T[]): Map<string, T> {
  const byName = new Map<string, T>();
  for (const el of elements) {
    if (el.name !== null && !byName.has(el.name)) byName.set(el.name, el);
  }
  return byName;
}

const RELATIONSHIP_TYPE_BY_PARSED_KIND: Partial<Record<ParsedRelationship["type"], string>> = {
  satisfy: "SatisfyRequirementUsage",
  verify: "VerifyRequirementUsage",
  allocate: "AllocationUsage",
  dependency: "DeriveRequirementUsage",
};

/**
 * Resolve each ParsedRelationship's textual endpoint NAMES to element ids and
 * produce ModelStore.createElements() inputs for the reconstructed
 * relationship elements (raw.source / raw.target arrays, matching how
 * create-relationship.ts already persists relationships as elements).
 *
 * SCOPE (Milestone 1): only satisfy / verify / allocate / dependency are
 * reconstructed — the kinds required by the identity round-trip acceptance
 * test. The nested-statement kinds (connect/bind/succession/flow/transition)
 * are left for a future milestone. A relationship whose endpoint name cannot
 * be resolved against `elementsByName` is silently skipped (best-effort
 * recovery; import_sysml's Gate 1 check operates on the element batch, not
 * on this reconstruction, so a skip here is not a hard import failure).
 *
 * SOURCE/TARGET DIRECTION — inverts exactly what sysml-serializer.ts's
 * TRACE_EMIT / verifyByCase paths emit, given (rel.sourceIds[0], rel.targetIds[0]):
 *   satisfy:    `satisfy <tgt> by <src>;`      -> src="by", tgt="requirement"
 *   verify:     `verify <tgt>;` (nested,       -> src="from" (case name,
 *               tgt = rel.sourceIds[0] is the      resolved by the parser's
 *               case, NOT this text's target)      elementStack lookup),
 *                                                   tgt="requirement"
 *   allocate:   `allocate <src> to <tgt>;`      -> src="from", tgt="to"
 *   dependency: `dependency from <src> to <tgt>;` -> src="from", tgt="to"
 *
 * AMBIGUOUS SYNONYMS (documented, inherent lossy aspect of the text form):
 * `dependency from X to Y;` is emitted for BOTH DeriveRequirementUsage and
 * TraceRequirementUsage — the text cannot distinguish them, so reconstruction
 * always picks DeriveRequirementUsage. Likewise a bare `verify X;` always
 * reconstructs as VerifyRequirementUsage, never RequirementVerificationMembership.
 * Milestone 1 scopes round-trip to identity + structure, not full type
 * fidelity for relationship kinds that collapse to identical text.
 */
export function parsedRelationshipsToStoreInputs(
  relationships: ParsedRelationship[],
  elementsByName: Map<string, NamedElementRef>
): StoreElementInput[] {
  const inputs: StoreElementInput[] = [];
  for (const rel of relationships) {
    let srcName: string | undefined;
    let tgtName: string | undefined;
    let type: string | undefined;

    switch (rel.type) {
      case "satisfy":
        type = RELATIONSHIP_TYPE_BY_PARSED_KIND.satisfy;
        srcName = rel.by;
        tgtName = rel.requirement;
        break;
      case "verify":
        type = RELATIONSHIP_TYPE_BY_PARSED_KIND.verify;
        srcName = rel.from;
        tgtName = rel.requirement;
        break;
      case "allocate":
        type = RELATIONSHIP_TYPE_BY_PARSED_KIND.allocate;
        srcName = rel.from;
        tgtName = rel.to;
        break;
      case "dependency":
        type = RELATIONSHIP_TYPE_BY_PARSED_KIND.dependency;
        srcName = rel.from;
        tgtName = rel.to;
        break;
      default:
        continue;
    }
    if (!type || !srcName || !tgtName) continue;
    const srcEl = elementsByName.get(srcName);
    const tgtEl = elementsByName.get(tgtName);
    if (!srcEl || !tgtEl) continue;

    inputs.push({
      type,
      name: "",
      attributes: {
        source: [{ "@id": srcEl.id }],
        target: [{ "@id": tgtEl.id }],
      },
    });
  }
  return inputs;
}
