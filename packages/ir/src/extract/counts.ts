/**
 * Assert that `actual` equals `expected`. Throws an [ETL-03]-tagged Error on
 * any mismatch — never console.warn, always throw.
 *
 * This is the single ETL-03 loud-failure primitive every sheet read calls;
 * "parsed without error at the wrong count" is a failure.
 */
export function assertCount(
  label: string,
  actual: number,
  expected: number
): void {
  if (actual !== expected) {
    throw new Error(
      `[ETL-03] ${label}: expected ${expected} data rows, got ${actual}`
    );
  }
}
