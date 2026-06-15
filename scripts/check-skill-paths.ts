/**
 * check-skill-paths.ts
 *
 * Path-existence verifier for the sysml-bridge skill corpus.
 *
 * Scans packages/skills/skills (recursively, *.md) — which includes the 11
 * /mbse-* skills AND _shared/knowledge-preamble.md — for backtick-quoted
 * repo-relative path citations and verifies that every cited path exists on
 * disk relative to the repo root.
 *
 * Extraction strategy (conservative — prefer false positives over misses):
 *   A backtick token is treated as a repo path if it matches EITHER of:
 *     1. Contains a forward slash AND ends with a recognized file extension
 *        (.md .ts .json .sysml .py .sh .g4 .yml .yaml)
 *     2. Begins with a known top-level directory prefix
 *        (packages/ docs/ examples/ scripts/ tools/)
 *
 * NOTE: examples/angars/model/extracted.json is gitignored (local-only corpus
 * data). It does not exist on a fresh clone until `pnpm extract:angars` is run.
 * The check will flag it as MISSING on fresh checkouts — that is correct and
 * intentional.
 *
 * Usage: pnpm check:skills
 *        (or: pnpm tsx scripts/check-skill-paths.ts)
 */

import * as fs from "node:fs";
import * as path from "node:path";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const REPO_ROOT = path.resolve(__dirname, "..");
const SKILLS_DIR = path.join(REPO_ROOT, "packages", "skills", "skills");

const KNOWN_EXTENSIONS = new Set([
  ".md",
  ".ts",
  ".json",
  ".sysml",
  ".py",
  ".sh",
  ".g4",
  ".yml",
  ".yaml",
]);

const TOP_LEVEL_PREFIXES = [
  "packages/",
  "docs/",
  "examples/",
  "scripts/",
  "tools/",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Walk a directory recursively and return all *.md file paths. */
function findMarkdownFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...findMarkdownFiles(fullPath));
    } else if (entry.isFile() && entry.name.endsWith(".md")) {
      results.push(fullPath);
    }
  }
  return results;
}

/** Return true if a backtick token looks like a repo-relative path citation. */
function isRepoPath(token: string): boolean {
  // Criterion 1: contains a slash AND ends with a known file extension
  if (token.includes("/")) {
    const ext = path.extname(token).toLowerCase();
    if (KNOWN_EXTENSIONS.has(ext)) return true;
  }

  // Criterion 2: begins with a known top-level directory prefix
  for (const prefix of TOP_LEVEL_PREFIXES) {
    if (token.startsWith(prefix)) return true;
  }

  return false;
}

/** Extract all backtick-quoted tokens from a markdown string. */
function extractBacktickTokens(content: string): string[] {
  const tokens: string[] = [];
  // Match inline code spans: `...` (non-greedy, single line)
  const re = /`([^`\n]+)`/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(content)) !== null) {
    tokens.push(m[1]);
  }
  return tokens;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const mdFiles = findMarkdownFiles(SKILLS_DIR).sort();

if (mdFiles.length === 0) {
  console.error(`ERROR: no .md files found under ${SKILLS_DIR}`);
  process.exit(1);
}

// Map: repoRelativePath -> Set<citingFile (repo-relative)>
const pathToCiters = new Map<string, Set<string>>();

for (const absFilePath of mdFiles) {
  const content = fs.readFileSync(absFilePath, "utf8");
  const tokens = extractBacktickTokens(content);
  const citingFile = path.relative(REPO_ROOT, absFilePath);

  for (const token of tokens) {
    if (isRepoPath(token)) {
      if (!pathToCiters.has(token)) {
        pathToCiters.set(token, new Set());
      }
      pathToCiters.get(token)!.add(citingFile);
    }
  }
}

// De-duplicate and check existence
let missingCount = 0;
const checkedPaths = [...pathToCiters.keys()].sort();

for (const repoRelPath of checkedPaths) {
  const absPath = path.join(REPO_ROOT, repoRelPath);
  if (!fs.existsSync(absPath)) {
    missingCount++;
    const citers = [...pathToCiters.get(repoRelPath)!].sort();
    for (const citingFile of citers) {
      console.error(`MISSING: ${repoRelPath}  (cited in: ${citingFile})`);
    }
  }
}

const checkedCount = checkedPaths.length;
console.log(
  `check:skills — ${checkedCount} path(s) checked, ${missingCount} missing`
);

if (missingCount > 0) {
  process.exit(1);
}
