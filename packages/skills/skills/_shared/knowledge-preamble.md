# Knowledge Preamble — MBSE Skill Grounding Rules

Every `/mbse-*` skill references this file. The three grounding pillars and
data-file discipline apply to ALL skills in this package without exception.

---

## Pillar A: SAIC Relational Discipline

Before relating any model elements, read `docs/reference/saic-devt.md`.
Enforce bidirectional traceability completeness, def/usage discipline (trace
operands must be usages, never definitions), and no dangling relationships.

## Pillar B: Emission Syntax

All SysML v2 textual notation MUST conform to `docs/sysml-v2-reference/cheatsheet.md`.
These are the canonical, validator-passing forms. Never guess SysML v2 syntax
from memory — copy from the cheatsheet, then run the local validator.

## Pillar C: Corpus-Grounding Rule

No model element may be created without a `provenanceSourceId` attribute that
resolves into `examples/angars/model/extracted.json`. Hand-invented elements
(not traceable to the corpus) are forbidden.

## Pillar D: Data-File Discipline

`examples/angars/model/extracted.json` is the local-only canonical IR.
It is READ AS A DATA FILE BY PATH — never imported as a code module.
It is gitignored corpus; do not `import`/`require` it from source files.
