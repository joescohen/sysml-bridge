/**
 * roundtrip.test.ts — Milestone 1 acceptance test: identity + structure
 * round trip through text (docs/superpowers/specs/2026-07-11-repository-substrate-design.md §3).
 *
 * THE BUG THIS GUARDS: before this milestone, serializeToSysml() never
 * emitted an element's id anywhere in the text, ParsedElement had no id
 * field, and import-sysml.ts threaded nothing into attributes["@id"] — so
 * every store -> serialize -> parse -> import round trip minted a FRESH
 * randomUUID() for every element (FileStore.buildElement). Two further
 * pre-existing gaps made round-tripping the REQUIRED relationship kinds
 * impossible even with an id mechanism in place, and are fixed alongside it
 * (see packages/sysml/src/sysml-import.ts and sysml-parser.ts's bare-`verify`
 * handling for details):
 *   - the parser only recognized the LEGACY, R3-INVALID `verify X by Y;`
 *     form — the only grammar-legal `verify` placement (nested inside
 *     `objective { ... }`, no "by" clause) produced "Unparseable line".
 *   - import-sysml.ts mapped ONLY the top-level `parsed.elements` array,
 *     silently dropping every nested child (parsed.elements is a TREE).
 *
 * SCOPE (Milestone 1, spec §3.4): text carries IDENTITY + STRUCTURE — id,
 * type, name, shortName, qualifiedName, ownership/containment, and
 * relationship endpoints. Arbitrary `raw` properties the textual notation
 * does NOT represent (e.g. provenanceSourceId) are OUT OF SCOPE — that is
 * the API/JSON substrate's job (Milestone 2). qualifiedName has NO textual
 * token in this notation at all, so this fixture keeps it null throughout
 * (trivially equal on both sides) rather than asserting a round-trip this
 * text format cannot represent.
 */

import { describe, it, expect, afterEach } from "vitest";
import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { serializeToSysml } from "../sysml-serializer.js";
import { parseSysml } from "../sysml-parser.js";
import {
  parsedElementsToStoreInputs,
  parsedRelationshipsToStoreInputs,
  buildNameIndex,
} from "../sysml-import.js";
import { FileStore } from "@sysml-bridge/model";
import type { SysmlElement, SysmlRelationship } from "@sysml-bridge/model";

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

const tmpDirs: string[] = [];

async function freshStore(label: string): Promise<FileStore> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), `roundtrip-${label}-`));
  tmpDirs.push(dir);
  const store = new FileStore(dir);
  await store.createProject(`RoundTrip-${label}`);
  return store;
}

afterEach(async () => {
  while (tmpDirs.length > 0) {
    const dir = tmpDirs.pop()!;
    await fs.rm(dir, { recursive: true, force: true });
  }
});

function findRel(rels: SysmlRelationship[], type: string): SysmlRelationship {
  const found = rels.find((r) => r.type === type);
  expect(found, `expected a reconstructed ${type} relationship`).toBeDefined();
  return found!;
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

describe("identity + structure round trip through text (Milestone 1)", () => {
  it("preserves id/elementId/type/name/shortName/qualifiedName/ownerId/ownedElementIds and relationship endpoints across store -> serialize -> parse -> import", async () => {
    // ---- 1. Build a representative model directly in a FileStore (store1) ----
    // Covers: a package, a def+usage pair (PartDefinition/PartUsage), a
    // RequirementDefinition+Usage (with a shortName), an ActionUsage, a
    // VerificationCaseDefinition, and 4 relationship kinds — satisfy, verify,
    // allocate, dependency — every trace operand (the argument to satisfy/
    // allocate/the bare verify, and the dependency endpoints) is a USAGE
    // (Feature), never a Definition, per R4.
    const store1 = await freshStore("source");

    const pkg = await store1.createElement("Package", "RoundTripPkg");
    const controllerDef = await store1.createElement("PartDefinition", "ControllerDef", {
      ownerId: pkg.id,
    });
    const controller = await store1.createElement("PartUsage", "controller", {
      ownerId: pkg.id,
      typeName: "ControllerDef",
    });
    const massReqDef = await store1.createElement("RequirementDefinition", "MassReq", {
      ownerId: pkg.id,
    });
    const massReqUsage = await store1.createElement("RequirementUsage", "massReqUsage", {
      ownerId: pkg.id,
      typeName: "MassReq",
      shortName: "SYS-001",
    });
    const needDef = await store1.createElement("RequirementDefinition", "NeedDef", {
      ownerId: pkg.id,
    });
    const needUsage = await store1.createElement("RequirementUsage", "needUsage", {
      ownerId: pkg.id,
      typeName: "NeedDef",
    });
    const manageDef = await store1.createElement("ActionDefinition", "ManageDef", {
      ownerId: pkg.id,
    });
    const manageAction = await store1.createElement("ActionUsage", "manageAction", {
      ownerId: pkg.id,
      typeName: "ManageDef",
    });
    const verCase = await store1.createElement(
      "VerificationCaseDefinition",
      "MassVerification",
      { ownerId: pkg.id }
    );

    // Relationships — persisted as elements (mirrors create-relationship.ts),
    // ownerId omitted (root), exactly like the real create_relationship path.
    await store1.createElement("SatisfyRequirementUsage", "", {
      source: [{ "@id": controller.id }], // "by" — the satisfier (a usage)
      target: [{ "@id": massReqUsage.id }], // the requirement satisfied (a usage)
    });
    await store1.createElement("VerifyRequirementUsage", "", {
      source: [{ "@id": verCase.id }], // the verification case
      target: [{ "@id": massReqUsage.id }], // the requirement verified (a usage)
    });
    await store1.createElement("AllocationUsage", "", {
      source: [{ "@id": manageAction.id }], // a usage
      target: [{ "@id": controller.id }], // a usage
    });
    await store1.createElement("DeriveRequirementUsage", "", {
      source: [{ "@id": massReqUsage.id }], // a usage
      target: [{ "@id": needUsage.id }], // a usage
    });

    const allElements = await store1.queryElements();
    const originalRelationships = await store1.queryRelationships();
    const relationshipIds = new Set(originalRelationships.map((r) => r.id));
    // Mirrors export_sysml's own filtering — relationship-shaped elements are
    // never part of the structural element tree passed to the serializer.
    const structuralElements = allElements.filter((e) => !relationshipIds.has(e.id));

    expect(originalRelationships.length).toBe(4);

    // ---- 2. Serialize -> parse (the round trip under test) ----
    const text = serializeToSysml(structuralElements, originalRelationships, undefined, {
      emitElementIds: true,
    });

    const parsed = parseSysml(text);
    // A non-empty errors array means the id-carrying/relationship-recovery
    // form is not actually recoverable — that is itself a failure of this
    // milestone, not a detail to route around.
    expect(parsed.errors).toEqual([]);

    // ---- 3. Import into a FRESH store (store2) — mirrors import_sysml.ts ----
    const store2 = await freshStore("reimport");

    const elementInputs = parsedElementsToStoreInputs(parsed.elements);
    const reimportedElements = await store2.createElements(elementInputs);

    const reimportedByName = buildNameIndex(reimportedElements);
    const relationshipInputs = parsedRelationshipsToStoreInputs(
      parsed.relationships,
      reimportedByName
    );
    expect(relationshipInputs.length).toBe(4);
    await store2.createElements(relationshipInputs);
    const reimportedRelationships = await store2.queryRelationships();

    // ---- 4. Assert deep-equality on the text-carried identity+structure set ----
    // {id, elementId, type, name, shortName, qualifiedName, ownerId,
    // ownedElementIds} for EVERY structural element, matched by name (names
    // are unique in this fixture — the metadata-annotation alternative this
    // milestone rejected shares this same name-uniqueness constraint; see
    // sysml-parser.ts's extractIdCommentsByLine doc comment).
    for (const original of structuralElements) {
      expect(original.name, "fixture elements must be named").not.toBeNull();
      const reimported = reimportedByName.get(original.name!);
      expect(reimported, `no reimported element named ${original.name}`).toBeDefined();

      // THE CORE CLAIM: id/elementId survive (this is what regenerates a
      // fresh randomUUID() on `main`, before this milestone).
      expect(reimported!.id).toBe(original.id);
      expect(reimported!.elementId).toBe(original.elementId);

      expect(reimported!.type).toBe(original.type);
      expect(reimported!.name).toBe(original.name);
      expect(reimported!.shortName).toBe(original.shortName);
      // qualifiedName: out of scope for text (see file header) — both sides
      // are null in this fixture, so this is a trivial-but-honest assertion.
      expect(reimported!.qualifiedName).toBe(original.qualifiedName);
      expect(reimported!.ownerId).toBe(original.ownerId);
      expect([...reimported!.ownedElementIds].sort()).toEqual(
        [...original.ownedElementIds].sort()
      );
    }

    // ---- 5. Assert relationship endpoints for all 4 kinds ----
    const origSatisfy = findRel(originalRelationships, "SatisfyRequirementUsage");
    const reSatisfy = findRel(reimportedRelationships, "SatisfyRequirementUsage");
    expect(reSatisfy.sourceIds).toEqual(origSatisfy.sourceIds);
    expect(reSatisfy.targetIds).toEqual(origSatisfy.targetIds);

    const origVerify = findRel(originalRelationships, "VerifyRequirementUsage");
    const reVerify = findRel(reimportedRelationships, "VerifyRequirementUsage");
    expect(reVerify.sourceIds).toEqual(origVerify.sourceIds);
    expect(reVerify.targetIds).toEqual(origVerify.targetIds);

    const origAllocate = findRel(originalRelationships, "AllocationUsage");
    const reAllocate = findRel(reimportedRelationships, "AllocationUsage");
    expect(reAllocate.sourceIds).toEqual(origAllocate.sourceIds);
    expect(reAllocate.targetIds).toEqual(origAllocate.targetIds);

    const origDerive = findRel(originalRelationships, "DeriveRequirementUsage");
    const reDerive = findRel(reimportedRelationships, "DeriveRequirementUsage");
    expect(reDerive.sourceIds).toEqual(origDerive.sourceIds);
    expect(reDerive.targetIds).toEqual(origDerive.targetIds);
  });
});
