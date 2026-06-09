#!/usr/bin/env python3
"""Local SysML v2 grammar validator.

Parses one or more ``.sysml`` files against the committed, pre-generated ANTLR
parser in ``generated/`` (regenerated from the OMG/ANTLR grammar vendored in
``docs/sysml-v2-reference/grammar/``). No Java is needed at validate time --
only the ``antlr4-python3-runtime`` package (see ``requirements.txt``).

It checks GRAMMAR (syntax) only. Cameo additionally enforces semantic
def-vs-usage rules; the live Cameo import remains the semantic backstop.

Usage:
    python validate_sysml.py path/to/model.sysml [more.sysml ...]

Output (per file):
    OK <path>                       (no syntax errors)
    FAIL (<n> errors) <path>        (followed by each "line L:C msg")

Exit code: 0 if every file is OK, 1 if any file FAILed.
"""
import os
import sys

# Make the committed generated parser importable regardless of CWD.
_HERE = os.path.dirname(os.path.abspath(__file__))
_GENERATED = os.path.join(_HERE, "generated")
if _GENERATED not in sys.path:
    sys.path.insert(0, _GENERATED)

from antlr4 import CommonTokenStream, FileStream  # noqa: E402
from antlr4.error.ErrorListener import ErrorListener  # noqa: E402

from SysMLv2Lexer import SysMLv2Lexer  # noqa: E402
from SysMLv2Parser import SysMLv2Parser  # noqa: E402


class Collector(ErrorListener):
    """Collects lexer + parser syntax errors as readable strings."""

    def __init__(self):
        self.errors = []

    def syntaxError(self, recognizer, offendingSymbol, line, column, msg, e):
        self.errors.append(f"line {line}:{column} {msg}")


def check(path):
    """Parse ``path`` via rootNamespace; return a list of syntax-error strings."""
    stream = FileStream(path, encoding="utf-8")
    lexer = SysMLv2Lexer(stream)
    tokens = CommonTokenStream(lexer)
    parser = SysMLv2Parser(tokens)

    collector = Collector()
    lexer.removeErrorListeners()
    lexer.addErrorListener(collector)
    parser.removeErrorListeners()
    parser.addErrorListener(collector)

    parser.rootNamespace()
    return collector.errors


def validate(path):
    """Validate one file; print result. Return True if OK, False if FAIL."""
    errors = check(path)
    if errors:
        print(f"FAIL ({len(errors)} errors) {path}")
        for err in errors:
            print(f"  {err}")
        return False
    print(f"OK {path}")
    return True


def main(argv):
    paths = argv[1:]
    if not paths:
        print("usage: validate_sysml.py <file.sysml> [more.sysml ...]", file=sys.stderr)
        return 2

    ok = True
    for path in paths:
        if not validate(path):
            ok = False
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
