# Sources — SysML v2 API repository substrate research (2026-07-11)

Graded per GRADE-adapted scale: HIGH = primary spec / official repo / direct code inspection · MODERATE = official docs / practitioner blog with expertise / single conference paper · LOW = single/undated/marketing/contradicted.

## HIGH
- OMG, *Systems Modeling API and Services* v1.0 (formal) — https://www.omg.org/spec/SystemsModelingAPI/1.0/About-SystemsModelingAPI — spec status, Part-3 framing, supersedes Beta3.
- OMG spec index — https://www.omg.org/spec/SystemsModelingAPI
- Systems-Modeling/**SysML-v2-Release** releases — https://github.com/Systems-Modeling/SysML-v2-Release/releases — monthly tags; 2026-04 (May 14 2026) bundles formal KerML 1.0 / SysML 2.0 / API 1.0.
- Systems-Modeling/**SysML-v2-API-Java-Client** `openapi.yaml` — https://github.com/Systems-Modeling/SysML-v2-API-Java-Client/blob/master/api/openapi.yaml — authoritative resource/path/schema definitions (Project/Branch/Tag/Commit/DataVersion/Query).
- Systems-Modeling/**SysML-v2-API-Services** (README/license/activity) — https://github.com/Systems-Modeling/SysML-v2-API-Services — Java/Play, PostgreSQL, "proof-of-concept pilot," LGPL→EPL-2.0 at 2026-04.
- SysML-v2-API-Services **issue #97** — https://github.com/Systems-Modeling/SysML-v2-API-Services/issues/97 — DataVersion immutability, delete-as-new-version, ProjectUsage open question.
- Systems-Modeling/**SysML-v2-API-Cookbook** — https://github.com/Systems-Modeling/SysML-v2-API-Cookbook — Jupyter notebooks exercising commits/branches/tags/CRUD/query (latest 2025-02).
- Systems-Modeling/**SysML-v2-Pilot-Implementation** — https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation — Xtext editors + Jupyter kernel; **issue #509** (Usage re-serialization NPE, closed "not planned" 2023-08).
- eclipse-syson/**syson** — https://github.com/eclipse-syson/syson — Sirius Web based; `import-export-textual.adoc` documents partial textual import/export; v2026.7.0 (Jul 10 2026).
- Obeo Sirius Web API docs — https://docs.obeosoft.com/sirius-web/ — GraphQL primary + REST "subset" of SysML v2 API.
- Direct code inspection — `sysml-foundry/packages/model/src/store/store.ts`, `file-store.ts`, `types.ts`, `stable-id.ts`; `packages/sysml/src/sysml-parser.ts`, `sysml-serializer.ts`; `packages/mcp-server/src/index.ts`, `tools/import-sysml.ts`; and `sysml-bridge/packages/mcp-server/src/smaps-client.ts`, `types/smaps.ts`, `__tests__/integration.test.ts`.

## MODERATE
- DLR, *MBSqlE* performance study (2025) — https://elib.dlr.de/214792/1/DASC_250623_AA_Final.pdf — REST overhead ~10ms/req; deep traversal >10s; std-library scale ~200k elements / 16M relationships. (Single paper, server version unknown — re-derive.)
- Sodius Willert, "A practical guide for SysML v2 adoption" — https://www.sodiuswillert.com/en/blog/a-practical-guide-for-sysml-v2-adoption — pro-JSON/API interchange (commercially interested).
- MBSE4U, "Interoperability: live SysML v2 API in action" (Jan 2025, demo Oct 2024) — https://mbse4u.com/2025/01/14/ — multi-tool sync through one Intercax pilot API repo.
- Sensmetry forum — https://forum.sensmetry.com/t/.../507 — pro-text+Git stance; Syside REST API "on roadmap"; flexo_syside bridge.
- MontiCore/sysmlv2 — https://github.com/MontiCore/sysmlv2 — independent parser built for quality-check against the pilot grammar.
- `sysml-v2-parser` (Rust) — https://lib.rs/crates/sysml-v2-parser — validates against 393 real files/8 sources; pragmatic deviations from spec grammar.
- Cascadia PLM API docs — https://docs.cascadiaplm.com/api/sysml — forced `@id`==`elementId` "as the spec has not decided."
- docs.nomagic.com — CATIA Magic/Cameo SysML v2 Community Edition & Solution (2026x) — https://docs.nomagic.com/SYSML2P/2026x/ — plugin packaging, 500-element cap, Teamwork Cloud API.
- Webel IT Australia — https://www.webel.com.au/node/4448 and /node/4910 — hands-on Cameo SysML v2 gotchas; experimental simulation plugin. (Reseller; version currency uncertain.)
- GoEngineer blog (2026) — https://www.goengineer.com/blog/advantages-of-sysml-v2-now-available-in-no-magic-cameo-and-catia-magic-2026 — tier/edition licensing detail.
- Intercax Syndeia blog — https://intercax.com/blog/ — commercial digital-thread interop with pilot + SysON.
- Tom Sawyer SysML v2 Viewer — https://www.tomsawyer.com/sysml-v2-viewer — auto-computes layout from semantics (no stored diagram positions).
- OMG press release, final adoption (2025-07-21) — https://www.omg.org/news/releases/pr2025/07-21-25.htm

## LOW / disregarded
- 3ds.com / discover.3ds.com marketing pages — https://www.3ds.com/products/catia/catia-magic/sysmlv2 — "Teamwork Cloud fully conforms to OMG SysML v2 API" (uncertified vendor claim; certification program only launched Jun 2026).
- DeepWiki AI summary — https://deepwiki.com/Systems-Modeling/SysML-v2-Release/ — claimed Branch/Tag "absent" from the API; **contradicted** by primary openapi.yaml. Disregarded.
- Assorted single snippet claiming SysON "full compliance" — superseded by Obeo docs' "subset." Disregarded.

## Excluded
- Marketing superlatives ("only solution that…") — excluded as non-evidential.
- Undated forum opinions with no corroboration — excluded unless a maintainer/primary source echoed the claim.
