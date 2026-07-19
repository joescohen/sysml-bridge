/**
 * source-scan-ratchet.ts — a source-scanning ratchet over the LIVE tree.
 *
 * The pattern: some helper (an approval writer, a mutating store call, …) must
 * be called from only a small, shrink-only allowlist of "defining" modules. A
 * ratchet walks the real source tree, greps for call sites, and reports every
 * one that is NOT allowlisted. A new escape fails CI until reviewed.
 *
 * Two doctrine points are baked in:
 *   - Scan scope is DERIVED from the live tree (every `packages/<pkg>/src` that
 *     exists), never a hardcoded path list — so a newly added package is
 *     automatically in scope. A hardcoded list would silently go blind as the
 *     repo grows. Callers assert `scanRoots.length >= minRoots` to prove the
 *     derivation actually found the packages.
 *   - The allowlist is an explicit, auditable set of DEFINING modules; anything
 *     else that calls the guarded token is an offender.
 *
 * Dependency-light: node fs/path only.
 */

import * as fs from "node:fs";
import * as path from "node:path";

export interface SourceScanRatchetOptions {
  /** Absolute repo root; every reported path is relative to it (posix-normalized). */
  repoRoot: string;
  /**
   * Explicit absolute scan roots. If omitted, roots are DERIVED from the live
   * tree via {@link deriveScanRoots} (`<repoRoot>/<packagesDir>/<*>/<srcDir>`).
   */
  roots?: string[];
  /** Directory (relative to repoRoot) holding one subdir per package. Default "packages". */
  packagesDir?: string;
  /** Per-package source subdir that becomes a scan root. Default "src". */
  srcDir?: string;
  /** Call-site tokens to grep for, e.g. ["appendApproval", "appendInferredApproval"]. */
  tokens: readonly string[];
  /** Repo-relative paths of the DEFINING / allowlisted modules exempt from the ratchet. */
  allowlist: Iterable<string>;
  /** Directory names to skip while walking. Default: __tests__, node_modules, dist. */
  skipDirs?: Iterable<string>;
  /** Whether a source file participates in the scan. Default: `.ts && !.d.ts && !.test.ts`. */
  includeFile?: (absPath: string) => boolean;
  /**
   * Whether `line` is a genuine CALL SITE of `token` (vs. a declaration, import,
   * re-export, or comment). Default: {@link defaultIsCallSite}.
   */
  isCallSite?: (line: string, token: string) => boolean;
}

export interface SourceScanRatchetResult {
  /** Absolute scan roots actually used (existing only), sorted. */
  scanRoots: string[];
  /** Absolute source files scanned. */
  files: string[];
  /** "relPath:lineNo: trimmedLine" for every non-allowlisted call site. */
  offenders: string[];
}

const DEFAULT_SKIP_DIRS = ["__tests__", "node_modules", "dist"];

/** Default file filter: TypeScript source, excluding declaration and test files. */
export function defaultIncludeFile(p: string): boolean {
  return p.endsWith(".ts") && !p.endsWith(".d.ts") && !p.endsWith(".test.ts");
}

/**
 * Default call-site predicate. A line is a call site of `token` iff it contains
 * `token(` and is NOT a function declaration, an import/re-export, or a comment.
 * (Mirrors the original no-auto-approve ratchet predicate exactly.)
 */
export function defaultIsCallSite(line: string, token: string): boolean {
  if (!line.includes(`${token}(`)) return false;
  if (/\b(export\s+)?(async\s+)?function\s/.test(line)) return false;
  if (/^\s*(import|export)\b/.test(line)) return false;
  const trimmed = line.trimStart();
  if (trimmed.startsWith("*") || trimmed.startsWith("//")) return false;
  return true;
}

/**
 * Derive one scan root per package that has a source dir:
 * `<repoRoot>/<packagesDir>/<*>/<srcDir>` for every existing match. Sorted.
 */
export function deriveScanRoots(
  repoRoot: string,
  packagesDir = "packages",
  srcDir = "src"
): string[] {
  const base = path.join(repoRoot, packagesDir);
  let entries: string[];
  try {
    entries = fs.readdirSync(base);
  } catch {
    return [];
  }
  const roots: string[] = [];
  for (const name of entries) {
    const candidate = path.join(base, name, srcDir);
    if (fs.existsSync(candidate) && fs.statSync(candidate).isDirectory()) {
      roots.push(candidate);
    }
  }
  return roots.sort();
}

function walk(
  dir: string,
  skip: Set<string>,
  includeFile: (p: string) => boolean
): string[] {
  const out: string[] = [];
  for (const name of fs.readdirSync(dir)) {
    const p = path.join(dir, name);
    const stat = fs.statSync(p);
    if (stat.isDirectory()) {
      if (skip.has(name)) continue;
      out.push(...walk(p, skip, includeFile));
    } else if (includeFile(p)) {
      out.push(p);
    }
  }
  return out;
}

export function sourceScanRatchet(opts: SourceScanRatchetOptions): SourceScanRatchetResult {
  const {
    repoRoot,
    packagesDir = "packages",
    srcDir = "src",
    tokens,
    allowlist,
    includeFile = defaultIncludeFile,
    isCallSite = defaultIsCallSite,
  } = opts;

  const skip = new Set<string>(opts.skipDirs ? [...opts.skipDirs] : DEFAULT_SKIP_DIRS);
  const allow = [...allowlist].map((m) => m.replace(/\\/g, "/"));

  const scanRoots = (opts.roots ?? deriveScanRoots(repoRoot, packagesDir, srcDir))
    .filter((p) => fs.existsSync(p))
    .sort();

  const files: string[] = [];
  const offenders: string[] = [];

  for (const root of scanRoots) {
    for (const file of walk(root, skip, includeFile)) {
      files.push(file);
      const rel = path.relative(repoRoot, file).replace(/\\/g, "/");
      const isAllowlisted = allow.some((m) => rel === m || rel.endsWith(m));
      const lines = fs.readFileSync(file, "utf8").split("\n");
      for (const token of tokens) {
        for (const [i, line] of lines.entries()) {
          if (!isCallSite(line, token)) continue;
          if (isAllowlisted) continue;
          offenders.push(`${rel}:${i + 1}: ${line.trim()}`);
        }
      }
    }
  }

  return { scanRoots, files, offenders };
}
