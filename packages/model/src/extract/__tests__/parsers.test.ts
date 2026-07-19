import { describe, it, expect } from "vitest";
import { parseNeeds, parseActivityId, stripIdPrefix } from "../parsers.js";

describe("parseNeeds", () => {
  it("returns an array of N-tokens from a comma-separated string", () => {
    expect(parseNeeds("N1, N12, N3")).toEqual(["N1", "N12", "N3"]);
  });

  it("handles whitespace-separated tokens (no commas)", () => {
    expect(parseNeeds("N1 N12 N3")).toEqual(["N1", "N12", "N3"]);
  });

  it("returns empty array for null input", () => {
    expect(parseNeeds(null)).toEqual([]);
  });

  it("returns empty array for undefined input", () => {
    expect(parseNeeds(undefined)).toEqual([]);
  });

  it("returns empty array for empty string", () => {
    expect(parseNeeds("")).toEqual([]);
  });

  it("filters out non-N tokens such as 'TBD' and 'see N1a'", () => {
    expect(parseNeeds("TBD, N1, see N1a, N12")).toEqual(["N1", "N12"]);
  });

  it("handles mixed comma-and-whitespace delimiters", () => {
    expect(parseNeeds("N1, N12 N3")).toEqual(["N1", "N12", "N3"]);
  });
});

describe("parseActivityId", () => {
  it("returns the token before the first colon", () => {
    expect(parseActivityId("F3.3: Initiate Fuel Transfer")).toBe("F3.3");
  });

  it("returns the trimmed whole string when there is no colon", () => {
    expect(parseActivityId("F3.3")).toBe("F3.3");
  });

  it("trims whitespace from the result", () => {
    expect(parseActivityId("  F1.1 : Receive & Authenticate Request  ")).toBe(
      "F1.1"
    );
  });
});

describe("stripIdPrefix", () => {
  it("returns the substring after the first colon, trimmed", () => {
    expect(stripIdPrefix("F1.1: Receive & Authenticate Request")).toBe(
      "Receive & Authenticate Request"
    );
  });

  it("returns the trimmed input when there is no colon", () => {
    expect(stripIdPrefix("Receive & Authenticate Request")).toBe(
      "Receive & Authenticate Request"
    );
  });

  it("trims whitespace from the result", () => {
    expect(stripIdPrefix("F1.1:   Manage Refueling Requests   ")).toBe(
      "Manage Refueling Requests"
    );
  });
});
