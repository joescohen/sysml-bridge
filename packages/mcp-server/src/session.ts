import { promises as fs, readFileSync } from "node:fs";
import * as path from "node:path";

// ---------------------------------------------------------------------------
// SessionTracker
//
// Records lifecycle progress as a side effect of MCP tool calls. The stages
// run strictly forward — init → ingest → build → trace → enrich → validate →
// render. (`enrich` is the gap-driven weave phase: weave_pass / close_pass
// propose links to close audit gaps, after trace and before validate.)
// A tool call can only ever push the session further along (or hold it in
// place); it can never regress. That invariant is what makes the recorded
// state a trustworthy "where are we in the MBSE workflow" signal for the
// /mbse orchestrator skill, which reads <dir>/.mbse/session.json.
//
// Forward moves may SKIP stages (e.g. init → build when the user creates an
// element before importing anything) — the lifecycle is an ordering, not a
// mandatory sequence. Same-state advances are idempotent no-ops. Backward
// advances throw; the caller (index.ts) swallows that so a rejected regression
// never fails the tool call itself.
// ---------------------------------------------------------------------------

export const LIFECYCLE = [
  "init",
  "ingest",
  "build",
  "trace",
  "enrich",
  "validate",
  "render",
] as const;

export type LifecycleState = (typeof LIFECYCLE)[number];

interface SessionDoc {
  state: LifecycleState;
  updatedAt: string;
}

export class SessionTracker {
  private readonly dir: string;

  /**
   * @param dir directory that owns the `.mbse/` folder (session.json lives at
   *            `<dir>/.mbse/session.json`).
   */
  constructor(dir: string) {
    this.dir = dir;
  }

  private get mbseDir(): string {
    return path.join(this.dir, ".mbse");
  }

  private get file(): string {
    return path.join(this.mbseDir, "session.json");
  }

  /**
   * The current lifecycle state, or null when no session has started yet
   * (the file is absent) or the file is unreadable/corrupt.
   */
  state(): LifecycleState | null {
    let raw: string;
    try {
      // Synchronous is deliberate: state() is a cheap read used by callers
      // that expect an immediate answer, and the file is tiny.
      raw = readFileSync(this.file, "utf8");
    } catch {
      return null;
    }
    try {
      const doc = JSON.parse(raw) as Partial<SessionDoc>;
      if (typeof doc.state === "string" && isLifecycleState(doc.state)) {
        return doc.state;
      }
      return null;
    } catch {
      return null;
    }
  }

  /**
   * Advance the session toward `to`.
   *
   * - Forward moves (including skips over intermediate stages) persist `to`.
   * - Same-state advances are idempotent no-ops (no write).
   * - Backward moves throw `invalid lifecycle transition <from> → <to>`.
   * - An unknown `to` throws.
   * - When no prior state exists, any valid `to` is a forward move from the
   *   start of the lifecycle.
   */
  async advance(to: LifecycleState): Promise<void> {
    const toIndex = LIFECYCLE.indexOf(to);
    if (toIndex === -1) {
      throw new Error(`unknown lifecycle state ${to}`);
    }

    const from = this.state();
    if (from !== null) {
      const fromIndex = LIFECYCLE.indexOf(from);
      if (fromIndex === -1) {
        // Persisted state is not in the lifecycle — treat as corrupt.
        throw new Error(`unknown lifecycle state ${from}`);
      }
      if (toIndex === fromIndex) {
        // Idempotent: already here, nothing to persist.
        return;
      }
      if (toIndex < fromIndex) {
        throw new Error(`invalid lifecycle transition ${from} → ${to}`);
      }
    }

    await this.persist(to);
  }

  private async persist(state: LifecycleState): Promise<void> {
    await fs.mkdir(this.mbseDir, { recursive: true });
    const doc: SessionDoc = { state, updatedAt: new Date().toISOString() };
    // Atomic write: write to a temp file in the same directory, then rename
    // over the target (rename is atomic on POSIX filesystems within one
    // mount). Mirrors FileStore.persist so a crash mid-write can never leave
    // a half-written session.json. Human-readable JSON (2-space indent).
    const target = this.file;
    const tmp = `${target}.tmp-${process.pid}`;
    await fs.writeFile(tmp, JSON.stringify(doc, null, 2), "utf8");
    await fs.rename(tmp, target);
  }
}

function isLifecycleState(value: string): value is LifecycleState {
  return (LIFECYCLE as readonly string[]).includes(value);
}
