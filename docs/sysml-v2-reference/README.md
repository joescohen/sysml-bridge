# SysML v2 Reference

This directory vendors the authoritative **SysML v2 textual-notation grammar**
and a distilled cheatsheet of the trace / verification / usage patterns the
project relies on. It is the **source of truth** for the SysML v2 serializer:
when the serializer emits text, that text must parse against the grammar
checked in here. When a human needs to confirm what is and is not legal SysML
v2 syntax, this directory is the place to look.

## Contents

| Path | What it is |
| --- | --- |
| `grammar/SysMLv2Parser.g4` | ANTLR4 parser grammar for SysML v2 textual notation. |
| `grammar/SysMLv2Lexer.g4` | ANTLR4 lexer grammar for SysML v2 textual notation. |
| `grammar/LICENSE` | MIT license + attribution for the vendored grammar. |
| `cheatsheet.md` | Distilled, validated trace/verify/usage patterns (every example parses). |

## Provenance

The grammar is a third-party artifact, vendored unmodified (byte-identical to
the upstream `.g4` files). Two upstream sources combine to produce it:

1. **ANTLR4 grammar** — `antlr/grammars-v4`, path `sysml-v2`.
   - URL: https://github.com/antlr/grammars-v4 (path: `sysml-v2`)
   - License: **MIT**
   - This is the actual `.g4` text we vendor.

2. **OMG SysML v2 specification / release** — `Systems-Modeling/SysML-v2-Release`,
   tag **2026-01**. The grammar above is derived from the KEBNF textual-notation
   definition published in this release.
   - URL: https://github.com/Systems-Modeling/SysML-v2-Release (tag `2026-01`)

The file headers inside the `.g4` files record the same lineage
("Derived from the OMG SysML v2 specification (KEBNF format)").

## License

The vendored grammar is redistributed under the **MIT License**. See
[`grammar/LICENSE`](grammar/LICENSE) for the full text and attribution. The
rest of this repository is MIT (see the repository root `LICENSE`).

## What it is for

- **The local validator parses against this grammar.** A small ANTLR-generated
  Python parser (built from these `.g4` files) is run over emitted SysML v2 to
  catch syntax errors deterministically — no live Cameo round-trip required for
  the syntactic gate.
- **The serializer treats this grammar as the source of truth.** Any construct
  the serializer emits must be derivable from these productions. When in doubt
  about legal syntax (e.g. where `verify` may appear), the grammar — not
  intuition — decides.
- **Humans consult it.** The `cheatsheet.md` distills the handful of
  trace/verification patterns that matter for traceability work, and every
  example in it has been validated against this exact grammar.

## Regenerating the parser

The committed grammar can be turned into a parser with ANTLR4 (the project's
validation tooling uses `antlr4-python3-runtime==4.13.2`):

```
antlr4 -Dlanguage=Python3 grammar/SysMLv2Lexer.g4 grammar/SysMLv2Parser.g4
```

Parse from the `rootNamespace` rule to validate a full `.sysml` file.
