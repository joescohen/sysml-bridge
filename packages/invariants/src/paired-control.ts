/**
 * paired-control.ts — the "prove the gate can fail" primitive.
 *
 * A gate you never watch reject a bad input is a gate you cannot trust. A paired
 * control feeds a KNOWN-GOOD and a KNOWN-BAD input through the same check and
 * asserts the good passes AND the bad is rejected. If the bad input also passes,
 * the control has caught a vacuous / broken gate.
 *
 * This is the runtime embodiment of the CI grammar-fixture pattern
 * (smoke-good.sysml passes / smoke-bad.sysml must fail the validator) and of the
 * seeded-defect harness's grammar defect (clean exits 0 / poisoned exits != 0).
 */

export interface PairedControlOptions<Input, Result> {
  /** The known-good input — expected to PASS the check. */
  good: Input;
  /** The known-bad input — expected to be REJECTED by the check. */
  bad: Input;
  /** Run the check against an input, yielding a result. */
  run: (input: Input) => Result | Promise<Result>;
  /**
   * True iff `result` counts as PASS. Default: `result === 0 || result === true`
   * (an exit code of 0, or a boolean true).
   */
  passes?: (result: Result) => boolean;
}

export interface PairedControlResult<Result> {
  goodResult: Result;
  badResult: Result;
  goodPassed: boolean;
  badPassed: boolean;
  /** ok = good passed AND bad was rejected. */
  ok: boolean;
}

function defaultPasses(result: unknown): boolean {
  return result === 0 || result === true;
}

export async function pairedControl<Input, Result>(
  opts: PairedControlOptions<Input, Result>
): Promise<PairedControlResult<Result>> {
  const passes = opts.passes ?? (defaultPasses as (r: Result) => boolean);
  const goodResult = await opts.run(opts.good);
  const badResult = await opts.run(opts.bad);
  const goodPassed = passes(goodResult);
  const badPassed = passes(badResult);
  return {
    goodResult,
    badResult,
    goodPassed,
    badPassed,
    ok: goodPassed && !badPassed,
  };
}
