/**
 * telemetry.ts — tiny shared counter for silent-drop / default-fallback sites.
 *
 * Motivation (the §6 fix): LLM parse/schema failures used to return a silent
 * fallback — prose returned `[]`, inference debate returned `{score: 0.5}` — with
 * NO log and NO count. A run could quietly degrade (every debate defaulting to
 * 0.5, every chunk yielding nothing) and look identical to a healthy run.
 *
 * `RunCounters` makes every such fallback observable: each drop site both
 * `console.error`s one context line (provider, method, reason — NEVER the full
 * prompt) AND increments a named counter that is surfaced in the run stats /
 * pipeline result. The fallback behavior itself is unchanged (0.5 still defaults,
 * `[]` still returns) — it is now COUNTED and LOGGED.
 *
 * Providers are classes, so each holds a `RunCounters` instance and exposes it
 * via a `counters` getter; callers read the snapshot after a run. A module-level
 * singleton was deliberately avoided — per-instance counters keep concurrent
 * providers (and parallel tests) from cross-contaminating each other's counts.
 */

/** Immutable snapshot of a RunCounters' fields. */
export interface RunCountersSnapshot {
  /** Response contained no usable text block (missing / non-text content). */
  missingText: number;
  /** Raw response text yielded no parseable JSON. */
  jsonParseError: number;
  /** Parsed JSON failed zod schema validation. */
  schemaError: number;
}

/**
 * Per-run failure counter for LLM parse / default-fallback sites.
 *
 * Not thread-shared state in any dangerous sense: JS is single-threaded, so the
 * increments below are atomic w.r.t. the event loop. One instance per provider.
 */
export class RunCounters {
  private _missingText = 0;
  private _jsonParseError = 0;
  private _schemaError = 0;

  /** Response had no usable text block. Logs one context line + increments. */
  recordMissingText(context: string): void {
    this._missingText++;
    logDrop(context, "missing_text", "response contained no usable text block");
  }

  /** Raw text yielded no parseable JSON. Logs one context line + increments. */
  recordJsonParseError(context: string, detail?: string): void {
    this._jsonParseError++;
    logDrop(context, "json_parse_error", detail ?? "no parseable JSON in response");
  }

  /** Parsed JSON failed schema validation. Logs one context line + increments. */
  recordSchemaError(context: string, detail?: string): void {
    this._schemaError++;
    logDrop(context, "schema_error", detail ?? "response failed schema validation");
  }

  /** Total across all failure classes. */
  get total(): number {
    return this._missingText + this._jsonParseError + this._schemaError;
  }

  /** Immutable snapshot of the current counts. */
  snapshot(): RunCountersSnapshot {
    return {
      missingText: this._missingText,
      jsonParseError: this._jsonParseError,
      schemaError: this._schemaError,
    };
  }

  /** Reset all counts to zero (for reusing an instance across runs). */
  reset(): void {
    this._missingText = 0;
    this._jsonParseError = 0;
    this._schemaError = 0;
  }
}

/**
 * Emit one stderr line for a drop/default-fallback site.
 * NEVER includes the full prompt — only provider/method context and the reason.
 */
function logDrop(context: string, reason: string, detail: string): void {
  console.error(`[candidates] parse_failure ${context} reason=${reason}: ${detail}`);
}
