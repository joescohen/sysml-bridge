/**
 * bm25.ts — dependency-free, in-memory BM25 lexical index over chunk texts.
 *
 * Scope guardrail (deliberate): this is LEXICAL retrieval only — standard BM25,
 * no embeddings, no vector DB, no external dependency. It lives OUTSIDE the
 * prose/ ingest path: the C5 ingest invariant ("No vector/embedding/retrieval
 * calls" in packages/candidates/src/prose) and its enforcing grep-test are
 * unaffected because nothing here is imported from prose/ or scripts/.
 *
 * Determinism: given the same chunk set + same query, results are byte-for-byte
 * reproducible. Scoring is a pure function of term statistics; ties are broken by
 * chunkId (ascending) so ordering never depends on Map insertion or Array sort
 * stability.
 *
 * Standard Okapi BM25:
 *   score(D,Q) = Σ_t IDF(t) · ( f(t,D)·(k1+1) ) / ( f(t,D) + k1·(1 − b + b·|D|/avgdl) )
 *   IDF(t)     = ln( 1 + (N − n(t) + 0.5) / (n(t) + 0.5) )   (always ≥ 0)
 * with k1 = 1.2, b = 0.75.
 */

// ── Types ─────────────────────────────────────────────────────────────────────

/** A retrievable unit of corpus text. `chunkId` + `sectionPath` keep every hit
 *  resolvable so a premise citing it can be validated downstream. */
export interface RetrievalChunk {
  /** Content-addressed chunk id (from the prose chunker); premise-resolvable. */
  chunkId: string;
  /** Human-readable section path for citation display. */
  sectionPath: string;
  /** The chunk's full text (the retrieval body). */
  text: string;
}

/** A retrieval hit: the chunk plus its BM25 score for the issued query. */
export interface ScoredChunk extends RetrievalChunk {
  /** BM25 relevance score (higher = more relevant). */
  score: number;
}

/** BM25 tuning knobs. Defaults are the standard k1=1.2, b=0.75. */
export interface Bm25Options {
  k1?: number;
  b?: number;
}

const DEFAULT_K1 = 1.2;
const DEFAULT_B = 0.75;

// ── Tokenizer ─────────────────────────────────────────────────────────────────

/**
 * Simple deterministic tokenizer: lowercase, split on alphanumeric runs.
 * "Fuel-Pump v2!" → ["fuel", "pump", "v2"].
 */
export function tokenize(text: string): string[] {
  const matches = text.toLowerCase().match(/[a-z0-9]+/g);
  return matches ?? [];
}

// ── Index ─────────────────────────────────────────────────────────────────────

interface IndexedDoc {
  chunk: RetrievalChunk;
  /** term → frequency in this doc */
  tf: Map<string, number>;
  /** total token count of this doc */
  length: number;
}

/**
 * In-memory BM25 index over a fixed set of chunks. Build once, query many times.
 *
 * Duplicate chunkIds are collapsed deterministically (first occurrence wins) so a
 * caller passing an over-broad chunk list still gets a stable index.
 */
export class Bm25Index {
  private readonly docs: IndexedDoc[];
  /** term → number of docs containing it */
  private readonly df: Map<string, number>;
  private readonly avgdl: number;
  private readonly k1: number;
  private readonly b: number;
  private readonly ids: Set<string>;

  constructor(chunks: readonly RetrievalChunk[], opts?: Bm25Options) {
    this.k1 = opts?.k1 ?? DEFAULT_K1;
    this.b = opts?.b ?? DEFAULT_B;

    this.docs = [];
    this.df = new Map();
    this.ids = new Set();

    for (const chunk of chunks) {
      // Deterministic dedup: first occurrence of a chunkId wins.
      if (this.ids.has(chunk.chunkId)) continue;
      this.ids.add(chunk.chunkId);

      const tokens = tokenize(chunk.text);
      const tf = new Map<string, number>();
      for (const tok of tokens) tf.set(tok, (tf.get(tok) ?? 0) + 1);
      for (const term of tf.keys()) this.df.set(term, (this.df.get(term) ?? 0) + 1);

      this.docs.push({ chunk, tf, length: tokens.length });
    }

    const totalLen = this.docs.reduce((sum, d) => sum + d.length, 0);
    this.avgdl = this.docs.length > 0 ? totalLen / this.docs.length : 0;
  }

  /** Number of (deduped) chunks in the index. */
  get size(): number {
    return this.docs.length;
  }

  /** The set of chunkIds in this index — used for premise resolution. */
  chunkIds(): Set<string> {
    return new Set(this.ids);
  }

  private idf(term: string): number {
    const n = this.df.get(term) ?? 0;
    const N = this.docs.length;
    // Non-negative BM25 IDF variant: ln(1 + (N − n + 0.5)/(n + 0.5)).
    return Math.log(1 + (N - n + 0.5) / (n + 0.5));
  }

  /**
   * Query the index. Returns up to `topK` hits with score > 0, sorted by score
   * (descending) then chunkId (ascending) for deterministic tie-breaking.
   */
  query(queryText: string, topK: number): ScoredChunk[] {
    if (this.docs.length === 0 || topK <= 0) return [];

    // Dedup query terms — repeating a term must not inflate its weight.
    const terms = [...new Set(tokenize(queryText))];
    if (terms.length === 0) return [];

    const scored: ScoredChunk[] = [];
    for (const doc of this.docs) {
      let score = 0;
      for (const term of terms) {
        const f = doc.tf.get(term);
        if (!f) continue;
        const idf = this.idf(term);
        const denom = f + this.k1 * (1 - this.b + this.b * (doc.length / (this.avgdl || 1)));
        score += idf * ((f * (this.k1 + 1)) / denom);
      }
      if (score > 0) {
        scored.push({ ...doc.chunk, score });
      }
    }

    scored.sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return a.chunkId < b.chunkId ? -1 : a.chunkId > b.chunkId ? 1 : 0;
    });

    return scored.slice(0, topK);
  }
}
