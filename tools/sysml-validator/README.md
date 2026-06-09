# Local SysML v2 grammar validator

A self-contained, in-repo validator that checks `.sysml` files for **grammar
(syntax) conformance** to the SysML v2 textual notation, using a pre-generated
ANTLR parser. No Java is needed to validate — the parser is committed.

## What it is

`generated/` holds the committed Python ANTLR parser (lexer + parser +
listener). It is generated from the OMG/ANTLR SysML v2 grammar (MIT-licensed)
vendored in [`docs/sysml-v2-reference/grammar/`](../../docs/sysml-v2-reference/grammar/):
`SysMLv2Lexer.g4` and `SysMLv2Parser.g4`. The validator drives that parser via
the `rootNamespace` start rule and reports any lexer/parser syntax errors.

This reproduces Cameo's **grammar** acceptance. It does **not** enforce
semantics. Cameo additionally enforces semantic rules (e.g. def-vs-usage
distinctions, reference resolution). A model that passes this validator may
still be rejected by Cameo on semantic grounds — **the live Cameo import is the
semantic backstop.**

## Runtime dependency

Validating requires only:

```
pip install antlr4-python3-runtime==4.13.2
```

(see [`requirements.txt`](./requirements.txt)). No Java is required at validate
time.

## How to run

The wrapper uses the repo virtualenv at `<repo>/.venv`:

```bash
# one-time setup (if .venv does not already exist)
python -m venv .venv
.venv/bin/pip install -r tools/sysml-validator/requirements.txt

# validate one or more files
tools/sysml-validator/run.sh path/to/model.sysml
tools/sysml-validator/run.sh a.sysml b.sysml
```

Or via the root npm script:

```bash
pnpm validate:sysml path/to/model.sysml
```

If the repo `.venv` is missing, `run.sh` prints a setup hint and exits `2`.

### Output

Per file:

- `OK <path>` — no syntax errors (exit 0 if every file is OK)
- `FAIL (<n> errors) <path>` followed by each `line L:C msg` (exit 1 if any
  file FAILs)

## Regenerating the parser (rare)

You only need this after editing the `.g4` grammar — the committed `generated/`
artifact is canonical and is what validation uses.

```bash
tools/sysml-validator/generate-parser.sh
```

This is a build-time step that needs **Java** and the **ANTLR 4.13.2** complete
jar. The private JRE auto-install path is `~/.jre` (the script prepends
`~/.jre/bin` to `PATH` if present). Point `ANTLR_JAR=...` at the ANTLR jar if it
is not in a probed location. The script is best-effort: if Java or the jar are
missing it prints a clear hint and exits non-zero without touching `generated/`.

## Files

| Path | Purpose |
| --- | --- |
| `generated/` | Committed ANTLR Python parser (the canonical artifact) |
| `validate_sysml.py` | The validator (parses via `rootNamespace`, reports syntax errors) |
| `run.sh` | Wrapper invoking the repo `.venv` python |
| `generate-parser.sh` | One-time parser regen from the vendored grammar (needs Java) |
| `requirements.txt` | `antlr4-python3-runtime==4.13.2` |

## License

The vendored grammar and the generated parser derive from the OMG/ANTLR SysML v2
grammar, which is MIT-licensed, consistent with this repository's MIT license.
