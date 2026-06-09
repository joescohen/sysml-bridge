# sysml-bridge — Project Instructions

Claude Code skill suite + MCP server for bidirectional natural-language ↔ SysML v2 MBSE
workflows, portable from open-source dev to Cameo Enterprise Architecture. See
`docs/design.md` for the architecture and `.planning/RESUME.md` for current project state.

This file is the **project** instruction set, scoped to this repo's SysML v2 emission
discipline. It does not duplicate the user's global `~/.claude/CLAUDE.md` conventions —
those still apply.

---

## RULES — SysML v2 emission discipline

These rules are authoritative. They exist because the serializer previously emitted SysML v2
from training-memory of the syntax, producing errors that were only caught by manually
importing into Cameo CE — slow, and it burned import attempts. The durable fix is an in-repo
grammar reference plus a local validator in the pipeline. Do not regress to guessing.

### R1 — The grammar is the SOURCE OF TRUTH

`packages/mcp-server/src/utils/sysml-serializer.ts` emits SysML v2 textual notation. It MUST
conform to the grammar vendored in `docs/sysml-v2-reference/`. The grammar is the SOURCE OF
TRUTH — **never guess SysML v2 syntax from memory.** When in doubt about a form, read the
vendored `.g4` and the cheatsheet, not your recollection.

### R2 — Validate locally BEFORE claiming it imports

Run the local validator on any generated or edited `.sysml` file **before** claiming it
imports into Cameo:

```
tools/sysml-validator/run.sh <file>.sysml      # or: pnpm validate:sysml <file>.sysml
```

A non-zero result means the file will **NOT** import — fix the syntax from the grammar in
`docs/sysml-v2-reference/`, never by guessing. "It should import" is not a claim you may make
without a 0-error validator run on the exact file.

### R3 — `verify` placement

`verify` is legal **ONLY** inside a requirement/verification body. The correct form is:

```sysml
verification def V {
    objective {
        verify <reqUsage>;
    }
}
```

Top-level `verify X by Y;` is **INVALID** (Cameo's LSP reports "extraneous input 'verify'").
*Rationale:* the grammar admits `verify` only as a membership inside a body that owns an
`objective`/requirement scope — never as a package-level statement.

### R4 — `def` vs `usage` for trace operands

`satisfy` / `allocate` / `verify` operands MUST be **usages (Features)**, never Definitions.
Emit trace participants as usages.

*Rationale:* the local validator checks **grammar only** and will **NOT** catch a Definition
operand — `satisfy <Def> by <Def>;` parses clean. Cameo rejects it **semantically**
("RequirementDefinition cannot be cast to Feature"). So a Definition operand passes the local
gate but fails the live import. Either emit usage-correct trace participants (preferred), or
confirm against a live Cameo import — the local validator alone is insufficient for this one.

---

## Validation gate (the workflow)

Edit the serializer or model, then run this gate **in order**. Do not skip to Cameo.

1. Edit the serializer: `packages/mcp-server/src/utils/sysml-serializer.ts`
2. Regenerate the model: `pnpm tsx scripts/generate-cc-model.ts`
3. Validate — this MUST report **0 errors**:
   `pnpm validate:sysml examples/angars/model/cc-subsystem.sysml`
   (equivalently `tools/sysml-validator/run.sh examples/angars/model/cc-subsystem.sysml`)
4. **Only then** import to Cameo.

A non-zero result at step 3 STOPS the gate. Fix the syntax from
`docs/sysml-v2-reference/` and rerun from step 2 — never proceed to Cameo on a failing
validator.

Run the unit tests when you touch serializer or generator code:
`pnpm --filter mcp-server test`.

---

## Reference

For the trace / verify / usage patterns (satisfy, allocate, verify, def-vs-usage), see
`docs/sysml-v2-reference/cheatsheet.md`. Its examples are the canonical, validator-passing
forms — prefer copying from there over reconstructing syntax.
