/**
 * check-skills.ts
 *
 * Skill → tool drift check for sysml-bridge.
 *
 * The /mbse skills (packages/skills/skills/*.md) reference MCP tools by name in
 * backticks. If a skill names a tool that the server does not actually register
 * — a typo, a renamed tool, a copied-from-the-old-repo tool that no longer
 * exists — the skill silently lies to the reviewer. This script asserts that
 * every backticked tool-shaped token in every skill IS a registered tool, and
 * that every skill references at least one tool (drift the other way: a skill
 * that has drifted into referencing no tools at all).
 *
 * What counts as a "tool-shaped token": a backticked snake_case identifier —
 * /`([a-z][a-z0-9]*(?:_[a-z0-9]+)+)`/g. The skills are written so the ONLY
 * such tokens are tool names; if an unavoidable non-tool token ever appears
 * (e.g. a JSON key), add it to KNOWN_NONTOOLS below WITH a comment. Prefer
 * rewording the skill over growing that list.
 *
 * Registered names come from the actual server.tool("<name>", ...) literals in
 * packages/mcp-server/src/index.ts and packages/mcp-server/src/tools/*.ts.
 *
 * Exit non-zero (listing the offenders) if any referenced token is not a
 * registered tool, or if any skill references zero tools.
 *
 * Usage: pnpm check:skills   (or: tsx scripts/check-skills.ts)
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SKILLS_DIR = path.join(REPO_ROOT, "packages", "skills", "skills");
const MCP_SRC_DIR = path.join(REPO_ROOT, "packages", "mcp-server", "src");

/**
 * Backticked snake_case tokens that are deliberately NOT MCP tool names and
 * must be exempted from the drift check. Keep this EMPTY if at all possible —
 * reword the skill instead. Every entry needs a comment justifying it.
 */
const KNOWN_NONTOOLS = new Set<string>([
  // (intentionally empty — skills are written so every backticked snake_case
  //  token is a registered tool name; add here only with a justification.)
]);

/** Grammar for a tool-shaped token inside backticks: snake_case, ≥1 underscore. */
const TOOL_TOKEN_RE = /`([a-z][a-z0-9]*(?:_[a-z0-9]+)+)`/g;

/** Grammar for a registered tool name: the first string literal of server.tool(...). */
const REGISTER_RE = /server\.tool\(\s*"([a-z][a-z0-9_]*)"/gs;

function findFiles(dir: string, ext: string): string[] {
  const out: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...findFiles(full, ext));
    else if (entry.isFile() && entry.name.endsWith(ext)) out.push(full);
  }
  return out;
}

/** All tool names registered via server.tool("<name>", ...) across the mcp-server source. */
function collectRegisteredTools(): Set<string> {
  const names = new Set<string>();
  for (const file of findFiles(MCP_SRC_DIR, ".ts")) {
    const src = fs.readFileSync(file, "utf8");
    for (const m of src.matchAll(REGISTER_RE)) names.add(m[1]);
  }
  return names;
}

/** Distinct backticked tool-shaped tokens in one skill markdown file. */
function collectReferencedTokens(content: string): Set<string> {
  const tokens = new Set<string>();
  for (const m of content.matchAll(TOOL_TOKEN_RE)) {
    if (!KNOWN_NONTOOLS.has(m[1])) tokens.add(m[1]);
  }
  return tokens;
}

// ── main ────────────────────────────────────────────────────────────────────

const registered = collectRegisteredTools();
if (registered.size === 0) {
  console.error(`ERROR: no registered tools found under ${MCP_SRC_DIR}`);
  process.exit(1);
}

const skillFiles = findFiles(SKILLS_DIR, ".md").sort();
if (skillFiles.length === 0) {
  console.error(`ERROR: no skill .md files found under ${SKILLS_DIR}`);
  process.exit(1);
}

let failures = 0;

for (const file of skillFiles) {
  const rel = path.relative(REPO_ROOT, file);
  const referenced = collectReferencedTokens(fs.readFileSync(file, "utf8"));

  // Drift #1: a skill that references no tools at all.
  if (referenced.size === 0) {
    console.error(`DRIFT: ${rel} references ZERO tools (expected at least one)`);
    failures++;
    continue;
  }

  // Drift #2: a referenced tool that is not registered.
  for (const token of [...referenced].sort()) {
    if (!registered.has(token)) {
      console.error(`UNREGISTERED: \`${token}\` referenced in ${rel} is not a registered tool`);
      failures++;
    }
  }
}

console.log(
  `check:skills — ${skillFiles.length} skill(s), ${registered.size} registered tool(s), ${failures} problem(s)`
);

if (failures > 0) process.exit(1);
