---
name: mbse-edit
description: Translate natural-language changes into store mutations via create/update/delete/create_relationship — never a serialize/import round-trip
---

# /mbse-edit — mutations through the store

Translate a natural-language change request ("add a cooling requirement", "link REQ-3 to the
pump", "rename the controller") into MCP store mutations. Every edit goes through the four
mutation tools directly against the live model store.

## The four mutation tools — and ONLY these

- `create_element` — add a new element (Definition or Usage). Args: type, name, owning
  package/parent.
- `update_element` — change an existing element's name or attributes, by id.
- `delete_element` — remove an element by id.
- `create_relationship` — link two elements (satisfy, allocate, verify, connection, etc.), by
  endpoint ids.

## FORBIDDEN: the import_sysml round-trip

Never implement an edit by exporting the model to text, editing the text, and re-importing it
with `import_sysml`. This is a data-loss trap (TF-3): **the parser recognizes only a subset of
what the serializer emits.** A serialize → edit → `import_sysml` round-trip silently drops the
structure the parser does not understand — ports, nested usages, trace metadata — and the user
loses model content with no error. `import_sysml` is for INGESTING a fresh external source
document at the start of a project (the `ingest` lifecycle stage), never for round-tripping an
edit to a model that already exists in the store. If you catch yourself reaching for
`import_sysml` to make a change, stop and use the mutation tools instead.

## R4 — trace operands are Usages

When creating a `satisfy` / `allocate` / `verify` relationship, the participants MUST be
usages (Features), never Definitions. `create_relationship` with a Definition operand parses
clean through the grammar but Cameo rejects it semantically, and the Gate-1 relational audit
flags it. Always pass usage-level endpoint ids to `create_relationship` for trace links.

## Every mutation is followed by validate + diff

After each mutation (or a small batch of related mutations):

1. Call `validate_model` and read the result. Surface any new `issues` or error-severity
   `findings` (`ruleId` / `severity` / `suggestedFix`) that the edit introduced. If the edit
   created a Gate-1 error, offer to fix or roll it back before moving on.
2. Present a **diff summary** to the user in plain language: what element or relationship was
   added, changed, or removed, with its id. State the before and after for an `update_element`;
   name the endpoints for a `create_relationship`. Never report an edit as done without showing
   what changed.

Do not claim the model imports into Cameo after an edit without a clean validator run — route
rendering and the exit-0 `pnpm validate:sysml` gate through `/mbse-render`. For answering
questions rather than making changes, use `/mbse-query`.
