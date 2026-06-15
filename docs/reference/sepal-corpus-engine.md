# SEPAL corpus engine (prose/PDF ingestion)

## What it is
The corpus ingestion subsystem of the SEPAL project (`~/Engineering/projects/se-process-platform`,
`packages/engine/src/corpus/`). Production-grade document ingestion: parse → banner-check →
hash → section-map → strategy dispatch → manifest.

## What it gives us
- **Parsers** for pdf (unpdf primary, mupdf fallback), docx, xlsx, pptx, csv, md, txt
  (`corpus/parsers/`).
- **Strategies**: `full-context`, `chunked-rag` (embeddings + LanceDB), `hybrid`,
  `structured-parse` (spreadsheets → `Requirement[]`).
- **Citations**: `ChunkSchema.chunk_id` ↔ `CitationSchema` — chunk-level provenance for
  anything extracted from prose.
- **Link discovery / traceability expansion** across documents (`link-discovery.ts`,
  `expand-traceability` tool).
- **Security boundary**: banner detection REFUSES CUI/FOUO/ITAR/PROPRIETARY before any
  chunking/embedding (T-02-25). Critical for defense corpora.

## How we use / interoperate
sysml-bridge's planned prose-ingestion feature should **integrate this engine, not rebuild
parsing**: SEPAL chunks+cites the prose corpus → an LLM extraction pass proposes IR
candidates (requirements / modes / interfaces), each citing chunk IDs → a human
accept/reject queue → approved entries merge into `extracted.json` (which the gates,
skills, export, and renders already consume). A candidate without a resolvable chunk
citation must not be approvable — extends the GATE-03 anti-fabrication contract to prose.
SEPAL RTA is also the designated independent auditor in the original demo narrative —
this is the second seam between the projects.

## Integration verdict (2026-06-10, after source read)
**Port the modules; do NOT take the package dependency.** The engine is workspace-internal
(private, dist-only exports) and drags the full RAG stack (LanceDB, onnxruntime, HF
transformers, VoyageAI) that a one-shot extraction pass doesn't need. Port with attribution:
`parsers/pdf.ts` (134 lines; unpdf+mupdf), `banner.ts` (210 lines, dep-free), the chunk-ID
core from `chunker.ts` (IDs hash position+context, NOT text — citations survive document
edits), `requirement-chunker.ts` (deterministic prose patterns: req-ID / numbered-shall /
"Requirement N:" + Traces-to extraction), `section-map.ts` (stable section IDs).
**Key lessons:** (1) deterministic-first, LLM-second — the LLM validates/structures
regex-found candidates instead of finding them; (2) banner refusal before any LLM call;
(3) text-excluded chunk hashes for citation stability; (4) the debate-pass
(advocate/challenger, audit-only rationale) is the v2 pattern for mid-confidence
candidates and fidelity near-matches.

## Status & maturity
Actively developed sibling project (CEI registry id `sepal`); the corpus subsystem has
dedicated tests (`packages/engine/tests/corpus/`). Verify the package export surface
(`@sepal/...` name, what `corpus/index.ts` re-exports) before importing as a library —
porting individual modules is the fallback if it isn't consumable as a package.

## Source links
- `~/Engineering/projects/se-process-platform/packages/engine/src/corpus/` (ingest.ts is
  the orchestrator; types.ts is the schema source of truth)
- Spec IDs in code comments: CORP-01..CORP-07, D-CORP-01/02

## Verification caveats
- Confirmed by direct source read 2026-06-10 (parsers, strategies, banner boundary,
  citation schema). The *requirement-chunker*'s prose-extraction depth (vs spreadsheet
  parsing) was not exercised — test before relying on it for shall-statement extraction.
