/**
 * sysml-validator.test.ts
 *
 * G5 gate: drives the local SysML v2 grammar validator
 * (tools/sysml-validator/run.sh -> .venv python -> validate_sysml.py) through
 * the SAME mechanism the generator's hard gate uses (execFileSync on run.sh),
 * with explicit NEGATIVE and POSITIVE controls.
 *
 * Fail-loud policy: if the repo .venv / validator is ABSENT, these tests FAIL
 * (they do NOT silently skip). A silent skip would hide regressions in the
 * grammar gate, which is exactly the control we are protecting. The existing
 * integration.test.ts uses describe.skipIf; we deliberately diverge to
 * fail-loud here per the gate requirement.
 */
import { describe, it, expect, beforeAll } from "vitest";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// __tests__ -> src -> mcp-server -> packages -> <repo root>
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..", "..", "..", "..");

const VENV_PY = path.join(REPO_ROOT, ".venv", "bin", "python");
const VALIDATE_PY = path.join(REPO_ROOT, "tools", "sysml-validator", "validate_sysml.py");
const RUN_SH = path.join(REPO_ROOT, "tools", "sysml-validator", "run.sh");
const REQUIREMENTS = path.join(REPO_ROOT, "tools", "sysml-validator", "requirements.txt");
const MODEL_SYSML = path.join(REPO_ROOT, "examples", "angars", "model", "cc-subsystem.sysml");

interface ValidatorResult {
  exitCode: number;
  stdout: string;
}

/** Run the validator on a single file via run.sh; capture exit code + stdout. */
function runValidator(sysmlPath: string): ValidatorResult {
  try {
    const stdout = execFileSync("bash", [RUN_SH, sysmlPath], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
    return { exitCode: 0, stdout };
  } catch (err) {
    const e = err as { status?: number; stdout?: string; stderr?: string };
    return {
      exitCode: typeof e.status === "number" ? e.status : 1,
      stdout: (e.stdout ?? "") + (e.stderr ?? ""),
    };
  }
}

describe("SysML v2 grammar validator gate (G5)", () => {
  // Fail-loud env guard: the venv and validator MUST exist. We assert this in a
  // beforeAll so a missing toolchain surfaces as a single, prominent failure
  // rather than a silent skip across every control.
  beforeAll(() => {
    if (!fs.existsSync(VENV_PY) || !fs.existsSync(VALIDATE_PY)) {
      throw new Error(
        "SysML v2 validator toolchain is ABSENT — refusing to silently skip.\n" +
          `  expected venv python: ${VENV_PY} (exists: ${fs.existsSync(VENV_PY)})\n` +
          `  expected validator  : ${VALIDATE_PY} (exists: ${fs.existsSync(VALIDATE_PY)})\n` +
          "  Set it up once with:\n" +
          `    python -m venv "${path.join(REPO_ROOT, ".venv")}"\n` +
          `    "${path.join(REPO_ROOT, ".venv", "bin", "pip")}" install -r "${REQUIREMENTS}"\n` +
          "  A silent skip here would hide grammar-gate regressions.",
      );
    }
  });

  it("toolchain (venv python + validate_sysml.py + run.sh) is present", () => {
    expect(fs.existsSync(VENV_PY), `missing venv python at ${VENV_PY}`).toBe(true);
    expect(fs.existsSync(VALIDATE_PY), `missing validator at ${VALIDATE_PY}`).toBe(true);
    expect(fs.existsSync(RUN_SH), `missing run.sh at ${RUN_SH}`).toBe(true);
  });

  it("NEGATIVE control: legacy top-level `verify <req> by <vcase>;` form FAILs non-zero", () => {
    // Inline fixture using the legacy/invalid top-level verify form (the exact
    // shape the generator must never emit). Written to a temp file so the test
    // is self-contained and does not depend on any vendored fixture.
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "sysml-neg-"));
    const badPath = path.join(tmpDir, "bad.sysml");
    fs.writeFileSync(
      badPath,
      ["package P {", "    requirement def R1;", "    verification def V1;", "}", "verify R1 by V1;", ""].join("\n"),
      "utf8",
    );

    try {
      const { exitCode, stdout } = runValidator(badPath);
      expect(exitCode, `expected non-zero exit, got ${exitCode}. stdout:\n${stdout}`).not.toBe(0);
      expect(stdout).toContain("FAIL");
      // The grammar error must point at the illegal `verify` token.
      expect(stdout).toContain("verify");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  // cc-subsystem.sysml is a generated, gitignored artifact (its corpus input is
  // local-only), so it is absent in CI and cannot be regenerated there. This
  // real-model control runs only where the generated model exists (local dev);
  // the validator's positive path stays covered in CI by the minimal control below.
  it.skipIf(!fs.existsSync(MODEL_SYSML))(
    "POSITIVE control: regenerated cc-subsystem.sysml validates clean (exit 0, OK)",
    () => {
      const { exitCode, stdout } = runValidator(MODEL_SYSML);
      expect(exitCode, `expected clean exit 0, got ${exitCode}. stdout:\n${stdout}`).toBe(0);
      expect(stdout).toContain("OK");
    },
  );

  it("POSITIVE control (minimal): a tiny valid requirement snippet validates clean", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "sysml-pos-"));
    const goodPath = path.join(tmpDir, "good.sysml");
    fs.writeFileSync(
      goodPath,
      ["package P {", "    requirement <'R'> r;", "    part def C;", "}", ""].join("\n"),
      "utf8",
    );

    try {
      const { exitCode, stdout } = runValidator(goodPath);
      expect(exitCode, `expected clean exit 0, got ${exitCode}. stdout:\n${stdout}`).toBe(0);
      expect(stdout).toContain("OK");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});
