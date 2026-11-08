# The SysML v2 engine (Rust)

`crates/` holds a native SysML v2 engine. In it, the model text is the source of truth: every write
generates canonical SysML v2 text, applies it through one edit engine, and parses the result again
before saving. A write that would add errors is refused.

| Crate | Content |
| --- | --- |
| `sysml-core` | Lexer, parser, model, name resolution, edit engine, intent-level writes, traceability, impact, budgets, ReqIF/CSV import plans, rule gates |
| `sysml-conform` | Checks text against the official OMG textual grammar (`grammar/*.kebnf`, EPL-2.0), with no code shared with `sysml-core` |
| `sysml-check` | Command-line checker: reads a folder of `.sysml`/`.kerml` files, runs every gate, prints one report, never writes |
| `sysml-server` | Binary `sysml-bridge-engine`: file store with revisions, history and undo, and an MCP endpoint over stdio or `POST /mcp` |

## Build and test

```bash
cargo test --workspace
cargo build -p sysml-server -p sysml-check
```

## Use it from an agent

`.mcp.json` registers the engine as `sysml-engine` over stdio, next to the TypeScript `sysml-bridge` server:

```bash
target/debug/sysml-bridge-engine --workspace <model-folder> --stdio
```

The server has 56 tools. Writes take `dry_run` and an expected revision, and `undo` reverts the last write.

| Group | Tools |
| --- | --- |
| Read | `list_files`, `read_file`, `get_outline`, `find_elements`, `get_element`, `query_elements`, `query_relationships`, `find`, `get_usages`, `export_facts`, `get_project_state` |
| Write | `create_element`, `update_element`, `delete_element`, `create_relationship`, `add_interface`, `edit_model`, `apply_edits`, `write_file`, `delete_file`, `reorder_members`, `fold_copy`, `mint_ids`, `set_source_id` |
| Check | `validate`, `validate_model`, `check_all`, `list_rules`, `check_connections`, `find_unconnected_ports`, `check_merge`, `get_validation_results` |
| Trace and impact | `get_traceability`, `coverage_report`, `get_coverage_history`, `list_traceability_properties`, `get_impact`, `list_suspect_links`, `get_relation_map`, `get_connections`, `get_icd`, `get_budget` |
| Change | `create_baseline`, `list_baselines`, `compare_revisions`, `get_change_package`, `get_history`, `undo` |
| Exchange | `import_requirements`, `plan_from_csv`, `plan_from_reqif`, `export_reqif`, `plan_renumber`, `get_next_id`, `resolve_source_id`, `export_sysml` |

## Check a folder

```bash
target/debug/sysml-check examples/angars/model
```

It ends with `CHECK PASS` or `CHECK FAIL` and exits 0 only on a pass. CI also runs it on
`crates/sysml-conform/tests/bad`, where it must fail.

## Relation to the TypeScript packages

The corpus pipeline (`packages/model`, `packages/sysml`, `packages/gates`) builds a JSON model with
provenance and human approvals, then serializes it to text. The engine works on text directly. The
next step moves the pipeline onto the engine and retires the TypeScript serializer, parser and MCP
server; until then the two live side by side.
