# Validation & QA

How the core claim — *the tool emits grammar-valid SysML v2 that imports into Cameo* — is
mechanically verified, what an exhaustive smoke pass has confirmed, and the known limitations.

## How to prove it yourself (one command each)

```bash
pnpm install && pnpm build          # build all 5 packages
pnpm -r test                        # full unit suite (no backend, no API key)
pnpm stress:sysml                   # synthetic stress: dozens of models → grammar gate
pnpm validate:sysml <file.sysml>    # the grammar gate on any .sysml (auto-bootstraps .venv)
```

`pnpm stress:sysml` is the headline check: it builds a wide, deliberately hostile battery of
models, serializes each, and asserts **every one passes the local ANTLR grammar validator**. It
exits non-zero if any model would fail to import — a CI-ready guard on the core claim.

## What the smoke pass covers

The grammar gate ([`scripts/synthetic-stress.ts`](../scripts/synthetic-stress.ts)) exercises the
serializer → validator → importer path across:

- **Every SysML aspect** — BDD hierarchy + multiplicity, IBD ports/connectors, activity
  (succession + flow), state machines, requirements + traceability (satisfy/allocate/derive/verify),
  constraints + enums + typed attribute values, interfaces (ends + typed connect), typed item flows,
  use cases (actor + include), crosscutting (specialization/subsetting/redefinition), analysis cases,
  views/viewpoints/concerns, metadata/occurrence/calc.
- **Adversarial names** that must be quoted or escaped: spaces, `&`, `/`, unicode/CJK, leading
  digits, embedded apostrophes/backslashes, and the 173 SysML v2 **reserved keywords** used as names
  (e.g. a part named `state`, a port named `out`).
- **Scale** — deep ownership nesting and wide fan-out.
- **Named nested statements** — `connection`/`interface`/`flow` statements with multi-word names and
  literal endpoints.

Plus the standing suite: serializer/parser unit tests, the GATE-02 relational audit pack, the binary
traceability gate, and the human-gate discipline proofs (`scripts/gd-*-proof.ts`).

## Fixed during the 2026-06-20 hardening pass

A deep smoke pass (synthetic stress + a multi-agent adversarial bug-hunt with executed repros and
independent verification) found and fixed seven real bugs, each now covered by a regression test:

| # | Severity | Bug | Fix |
|---|---|---|---|
| 1 | high | Element names equal to SysML reserved keywords (`state`, `out`, …) emitted bare → grammar FAIL | quote reserved keywords in `quoteName` (sourced from the vendored grammar's 173 lexer keywords) |
| 2 | high | Embedded `'` / `\` in names not escaped inside `'…'` → grammar FAIL | `escapeQuotedName` per the grammar `STRING` token |
| 3 | high | Multi-word `ConnectionUsage` name (e.g. "Power Link") emitted unquoted in `connect` stmt | `quoteName` the connection name |
| 4 | high | Multi-word `InterfaceUsage` name emitted unquoted in `interface` stmt | `quoteName` the interface name |
| 5 | medium | Literal multi-word flow endpoints emitted unquoted | quote each dotted segment (`quoteRef`) |
| 6 | **critical** | `validate_model` fabricated a `GATE02-id-duplicate` error for **every** relationship (FileStore stores relationships as elements, so the duplicate scan double-counted them) → a perfectly-traced model FAILed the binary gate | skip relationship ids already counted as element ids in the duplicate scan |
| 7 | high | Importer corrupted names containing an escaped `\'` on re-parse (round-trip of fix #2) | escape-aware quoted-name reader in the parser |

Bug #6 was the most important: the binary traceability gate is a headline feature, and it failed on
exactly the kind of complete, fully-traced model the demo shows.

## Known limitations (intentionally deferred)

These are real but low-likelihood for the demo path; tracked for follow-up rather than rushed:

- **Line-based importer is lenient.** `import_sysml` (the regex/line parser, distinct from the
  authoritative ANTLR validator) does not recover bare enumeration literals (`LiIon;`) or nested
  `objective { verify …; }` statements; a name containing `{`/`}` can confuse its brace tracking. The
  ANTLR validator and Cameo are the authoritative gates; serializer output is always grammar-valid.
- **`createProject` slug collision.** Two project names that slugify to the same id (e.g. "My Model"
  vs "my-model") write to the same on-disk file; the second overwrites the first. Use distinct names,
  or namespace the model dir, until a disambiguation/guard lands.
- **Caller-supplied duplicate `@id`.** `create_element` forwards an explicit `@id`; passing one that
  collides with an existing element can shadow it. Normal authoring lets the store generate UUIDs and
  is unaffected.
- **Attribute `value` is a literal expression.** The serializer emits `raw.value` verbatim, so a
  string-valued attribute must include its quotes (`raw.value = '"text"'`); numbers/expressions are
  bare. Requirement statement text flows through `doc`, not `value`.
- **`pnpm check:skills`** reports `examples/angars/model/extracted.json` and `cc-subsystem.sysml` as
  missing on a clean checkout — these are the gitignored, local-only ANGARS corpus artifacts and are
  expected to be absent in the public repo.
