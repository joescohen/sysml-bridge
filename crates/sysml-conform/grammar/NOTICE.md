# Origin and licence of the files in this directory

`SysML-textual-bnf.kebnf`, `KerML-textual-bnf.kebnf` and `LICENSE` are unchanged
copies from the OMG SysML v2 release:

- Repository: https://github.com/Systems-Modeling/SysML-v2-Release
- Release tag: `2026-08` (commit `fb97b754f29588b8e9c7a35f370880cd15eb29e7`)
- Paths in the release: `bnf/SysML-textual-bnf.kebnf`, `bnf/KerML-textual-bnf.kebnf`, `LICENSE`

They are under the Eclipse Public License 2.0; the full text is in `LICENSE`.
The same holds for the model files under `../tests/official/`, which are
unchanged copies from `sysml.library/` and `sysml/src/` of the same release.

`corrections.kebnf` is ours (MIT). It does not change the official files; the
checker reads it next to them.

To move to a newer release: replace the two `.kebnf` files and `LICENSE`, update
the tag here and `GRAMMAR_RELEASE` in `src/lib.rs`, then run the tests and the
full official run described in `docs/conformance-checker.md`.
