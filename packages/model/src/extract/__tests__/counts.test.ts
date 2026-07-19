import { describe, it, expect } from "vitest";
import { assertCount } from "../counts.js";

describe("assertCount", () => {
  it("throws an [ETL-03]-tagged Error when actual does not equal expected", () => {
    expect(() => assertCount("Final", 160, 165)).toThrow(/\[ETL-03\]/);
  });

  it("throw message includes both the expected and actual counts", () => {
    expect(() => assertCount("Final", 160, 165)).toThrow(
      /expected 165.*got 160/
    );
  });

  it("includes the label in the thrown message", () => {
    expect(() => assertCount("Sheet3", 7, 10)).toThrow(/Sheet3/);
  });

  it("returns void without throwing when actual equals expected", () => {
    expect(() => assertCount("Final", 165, 165)).not.toThrow();
  });
});
