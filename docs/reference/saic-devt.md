# SAIC Digital Engineering Validation Tool (DEVT)

## What it is
A commercial validation tool from **SAIC** (Science Applications International Corp.)
that ships **~251 validation rules** — both **language** (semantic well-formedness) and
**style** (style-guide conformance) — for **MagicDraw / Cameo Enterprise Architecture**
(native to Cameo 2022x / 2024x). It runs as an in-tool validation suite that flags
rule violations across a model.

## What it gives us
- Automated enforcement of a large model style guide + language semantics, reducing
  large-model review from **weeks to minutes** (vendor claim).
- A concrete reference point for what **"SAIC-style relational consistency"** means in
  practice: a broad, enforced rule set covering naming/identification, relationship
  well-formedness, traceability completeness, and stereotype/profile conformance.

## How we use / interoperate
- We do **not** have the 251 proprietary rules. When a task says "follow SAIC guidelines,"
  we interpret it as the **relational-consistency principles** the rule set embodies and
  enforce them with our own gates:
  - **Bidirectional traceability completeness** (IEEE 15288 §6.3.3) — every requirement
    derived ↔ satisfied ↔ verified; no orphans.
  - **def/usage discipline** — trace operands (`satisfy`/`allocate`/`verify`) are **usages**,
    never definitions (repo rule R4).
  - **No dangling relationships** — every relationship endpoint resolves to a declared element.
  - **Identification** — every requirement carries a stable id (short name).
- These map onto the repo's own checks: the grammar validator (`tools/sysml-validator/`),
  the `validate_model` MCP tool, and the fidelity comparator. If a model must clear the
  *actual* SAIC DEVT, that requires the licensed tool inside Cameo.

## Status & maturity
Commercial, actively sold; native Cameo integration. Authoritative for defense/DE MBSE
programs that standardize on it.

## Source links
- Product page: https://www.saic.com/digital-engineering-validation-tool

## Verification caveats
- The "251 rules" figure and the language/style split came from a research-agent digest
  (`.planning/research/raw/best-practices.md`), not from first-party SAIC documentation —
  **verify the exact rule count and categories at the source** before citing in any
  deliverable. The §-level standard mappings above (IEEE 15288 §6.3.3 etc.) are likewise
  research-derived; confirm at the standard before presentation.
- If the user supplies the real SAIC rule set, supersede this interpretation with the
  actual rules.
