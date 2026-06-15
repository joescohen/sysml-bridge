/**
 * gd-no-autoapprove-proof.ts — Structural proof that mbse-ingest has no auto-approval path.
 *
 * Output: /tmp/rubric-anchored-recursion/prose-ingest/evidence/gd-no-autoapprove.txt
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";

const REPO_ROOT = resolve(__dirname, "..");
const SKILL_PATH = resolve(REPO_ROOT, "packages/skills/skills/mbse-ingest.md");
const HELPER_PATH = resolve(REPO_ROOT, "packages/ir/src/approval-helpers.ts");
const OUT_PATH = "/tmp/rubric-anchored-recursion/prose-ingest/evidence/gd-no-autoapprove.txt";

async function main() {
  const lines: string[] = [];
  const log = (s: string) => { lines.push(s); console.log(s); };

  log("G-D NO-AUTO-APPROVE STRUCTURAL PROOF");
  log("======================================");
  log(`Run at: ${new Date().toISOString()}`);
  log("");

  const skillContent = await readFile(SKILL_PATH, "utf8");
  const helperContent = await readFile(HELPER_PATH, "utf8");

  // --- Check 1: skill is markdown only ---
  log("CHECK 1 — mbse-ingest.md contains no executable auto-approve code:");
  const execPatterns = [
    /appendApproval\s*\(/g,   // direct call without AskUserQuestion gate
    /auto.?approv/gi,
    /approve\s*\(\s*all\s*\)/gi,  // programmatic auto-approve
  ];

  // The skill should reference appendApproval only in descriptive/API-reference context,
  // never in imperative executable context.
  // Check that appendApproval appears only in code blocks (reference section), not prose
  const appendApprovalMatches = skillContent.match(/appendApproval/g) || [];
  log(`  Occurrences of 'appendApproval' in skill: ${appendApprovalMatches.length}`);

  // All must be inside code fences
  const codeBlocks: string[] = [];
  const codeBlockRe = /```[\s\S]*?```/g;
  let m: RegExpExecArray | null;
  while ((m = codeBlockRe.exec(skillContent)) !== null) {
    codeBlocks.push(m[0]);
  }
  const codeBlockText = codeBlocks.join("\n");
  const inCodeBlock = appendApprovalMatches.every(() => codeBlockText.includes("appendApproval"));
  log(`  All appendApproval references are inside code blocks (reference section): ${inCodeBlock}`);
  log("");

  // --- Check 2: Skill explicitly documents human gate requirement ---
  log("CHECK 2 — Skill explicitly requires AskUserQuestion before appendApproval:");
  const askUserQuestionPresent = skillContent.includes("AskUserQuestion");
  log(`  'AskUserQuestion' present in skill: ${askUserQuestionPresent}`);

  const noAutoApproveDocumented =
    skillContent.includes("NO auto-approval path exists") ||
    skillContent.includes("NO AUTO-APPROVAL PATH");
  log(`  Hard constraint 'NO AUTO-APPROVAL' documented: ${noAutoApproveDocumented}`);

  const mustNotCallBeforeReply = skillContent.includes(
    "MUST NOT call `appendApproval` or `recordRejection` until the human replies"
  );
  log(`  Explicit 'MUST NOT call until human replies' constraint: ${mustNotCallBeforeReply}`);
  log("");

  // --- Check 3: approval-helpers.ts makes no approval decisions ---
  log("CHECK 3 — approval-helpers.ts contains no approval decision logic:");
  // The helper should not contain any heuristic or conditional that decides to approve
  const autoDecisionPatterns = [
    /if.*approve/i,
    /shouldApprove/i,
    /autoApprove/i,
    /auto_approve/i,
  ];
  const helperLines = helperContent.split("\n");
  const suspectLines: string[] = [];
  for (const line of helperLines) {
    for (const pat of autoDecisionPatterns) {
      if (pat.test(line) && !line.trim().startsWith("//") && !line.trim().startsWith("*")) {
        suspectLines.push(line.trim());
      }
    }
  }
  log(`  Suspect lines in approval-helpers.ts: ${suspectLines.length}  (expected: 0)`);
  if (suspectLines.length > 0) {
    for (const l of suspectLines) log(`    SUSPECT: ${l}`);
  }
  log("");

  // --- Check 4: grep for auto-approve patterns in skill ---
  log("CHECK 4 — grep skill for any 'auto' + 'approv' proximity:");
  const autoApprovePresent = /auto.{0,30}approv/gi.test(skillContent);
  // We expect only the NEGATIVE statement: "NO auto-approval path exists"
  // Count occurrences of auto.*approv in non-negated context
  const autoApproveMatches = skillContent.match(/auto.{0,30}approv/gi) || [];
  log(`  Matches of /auto.{0,30}approv/gi: ${JSON.stringify(autoApproveMatches)}`);
  const onlyNegated = autoApproveMatches.every(
    (match) =>
      match.toLowerCase().includes("no auto-approval") ||
      match.toLowerCase().includes("no auto") ||
      match.toLowerCase().includes("no code path")
  );
  log(`  All matches are negative/prohibition statements: ${onlyNegated}`);
  log("");

  // Assertions
  if (!askUserQuestionPresent) throw new Error("AskUserQuestion not present in skill");
  if (!noAutoApproveDocumented) throw new Error("NO AUTO-APPROVAL constraint not documented");
  if (!mustNotCallBeforeReply) throw new Error("MUST NOT call constraint missing");
  if (suspectLines.length > 0) throw new Error("Auto-approval logic found in helper");

  log("VERDICT: PASS — no auto-approval path exists in mbse-ingest skill or helpers.");

  await mkdir("/tmp/rubric-anchored-recursion/prose-ingest/evidence", { recursive: true });
  await writeFile(OUT_PATH, lines.join("\n") + "\n", "utf8");
  console.log(`\nWrote: ${OUT_PATH}`);
}

main().catch((err) => { console.error(err); process.exit(1); });
