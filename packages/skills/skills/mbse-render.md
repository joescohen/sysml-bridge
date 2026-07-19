---
name: mbse-render
description: Regenerate and render the model — build the validated .sysml, drive the viewer with a view spec, refresh the gallery
---

# /mbse-render — regenerate and render

Produce the graphical views (BDD / IBD / activity / traceability) from the model. Renders are
generated from the **validated** `.sysml` artifact — never from an unvalidated draft and never
from the raw store JSON. The rule is: no render without a clean gate first.

## The render sequence

1. **Serialize the current model.** Call `export_sysml` to emit SysML v2 textual notation from
   the live store. (In the demo pipeline this is done by `pnpm demo:build`, which builds the
   model, runs the Gate-1 audit, and writes `examples/angars/out/angars.sysml`.)

2. **Gate it — R2.** The `.sysml` MUST pass the grammar validator at exit 0 before it is
   rendered or claimed importable:

   ```
   pnpm validate:sysml examples/angars/out/angars.sysml
   ```

   A non-zero result STOPS the render — the file will not import into Cameo. Fix the syntax
   against `docs/sysml-v2-reference/` (never by guessing), rebuild, and re-run the validator.
   Do not tell the user the model imports without this exit-0 run on the exact file. The
   Gate-1 side of this (semantic findings) comes from `validate_model`; run it too and clear
   any error-severity findings before rendering.

3. **Render the views** from the validated `.sysml` with an explicit view spec:

   ```
   tools/viewer/render.sh examples/angars/out/angars.sysml examples/angars/out/renders \
     --spec examples/angars/views.json --png
   ```

   The `--spec` file selects which views to render (general / interconnection / activity, per
   the entries in `examples/angars/views.json`); `--png` rasterizes each exported PDF to PNG.
   Without `--spec`, the viewer renders its full default view set.

4. **Refresh the gallery.** Update the README gallery so the new renders are the ones shown:

   ```
   pnpm demo:gallery
   ```

## Renders come from the validated artifact

State this to the user explicitly: the images they see are rendered from the `.sysml` that
passed the grammar validator (exit 0) and the Gate-1 audit — not from an intermediate draft.
If the validator did not pass, there is no render to show; report the failure and the fixes
needed instead of producing views from a file that will not import.

## MCP tools

This skill drives the viewer through shell commands, but it depends on two MCP tools for the
model artifact and its gate:

- `export_sysml` — serialize the live store to the `.sysml` that gets validated and rendered.
- `validate_model` — the Gate-1 semantic audit; clear error-severity findings before rendering.

For building or fixing model content before a render, use `/mbse-edit`. For inspecting the
model, use `/mbse-query`. For lifecycle position, use `/mbse` (rendering advances the session
to `render`).
