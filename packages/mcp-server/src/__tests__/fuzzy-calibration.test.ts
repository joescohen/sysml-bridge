/**
 * Fuzzy calibration fixture — pins the normalize/similarity/band functions
 * against a labeled corpus of real ANGARS name pairs.
 *
 * Pinned band constants: confident >= 0.90, review >= 0.78, else unmatched
 * (supersedes provisional 0.92/0.75 per 05-RESEARCH.md Fuzzy Calibration)
 *
 * The regression guard for token-set normalization lives in the "reorder" and
 * "synonym/reorder" rows — raw Levenshtein scores these 0.25/0.11 (unmatched),
 * token-sort normalization must rescue them to confident.
 */

import { describe, it, expect } from "vitest";
import { similarity, normalize, band, type Band } from "../audit/fuzzy.js";

describe("normalize", () => {
  it("lowercases, trims, and sorts tokens", () => {
    expect(normalize("Aircraft ID Verification")).toBe("aircraft id verification");
    expect(normalize("ID Verification Aircraft")).toBe("aircraft id verification");
  });

  it("replaces & with and", () => {
    expect(normalize("Receive & Authenticate Request")).toBe(
      normalize("Receive and Authenticate Request")
    );
  });

  it("strips non-alphanumeric to space and collapses whitespace", () => {
    expect(normalize("Probe-Alignment Accuracy")).toBe("accuracy alignment probe");
    expect(normalize("Probe Alignment Accuracy")).toBe("accuracy alignment probe");
  });

  it("handles trailing spaces", () => {
    expect(normalize("Generate Schedule ")).toBe(normalize("Generate Schedule"));
  });
});

describe("fuzzy similarity — calibration fixture", () => {
  // Pinned band constants (source of truth for the implementation):
  // confident >= 0.90 | review >= 0.78 | else unmatched
  type FixtureRow = { label: string; a: string; b: string; expectedBand: Band };

  const EXACT_CONFIDENT: FixtureRow[] = [
    {
      label: "exact",
      a: "Control Autonomous Docking",
      b: "Control Autonomous Docking",
      expectedBand: "confident",
    },
    {
      label: "case-only",
      a: "Prioritize Requests",
      b: "prioritize requests",
      expectedBand: "confident",
    },
    {
      label: "trailing-space",
      a: "Generate Schedule",
      b: "Generate Schedule ",
      expectedBand: "confident",
    },
    {
      label: "typo",
      a: "Receive Aircraft Telemetry",
      b: "Receive Aircaft Telemetry",
      expectedBand: "confident",
    },
    {
      label: "punct",
      a: "Probe Alignment Accuracy",
      b: "Probe-Alignment Accuracy",
      expectedBand: "confident",
    },
    {
      label: "amp-and-spelled",
      a: "Receive & Authenticate Request",
      b: "Receive and Authenticate Request",
      expectedBand: "confident",
    },
    // The two critical reorder regression guards:
    {
      label: "reorder",
      a: "Aircraft ID Verification",
      b: "ID Verification Aircraft",
      expectedBand: "confident",
    },
    {
      label: "synonym/reorder",
      a: "Update Schedule Dynamically",
      b: "Dynamically Update Schedule",
      expectedBand: "confident",
    },
  ];

  const CLEAR_UNMATCHED: FixtureRow[] = [
    {
      label: "word-drop",
      a: "Manage Refueling Requests",
      b: "Manage Refueling",
      expectedBand: "unmatched",
    },
    {
      label: "verb-prefix",
      a: "Fuel Capacity Check",
      b: "Validate Fuel Capacity",
      expectedBand: "unmatched",
    },
    {
      label: "unrelated",
      a: "Autonomous Refueling",
      b: "Collision Avoidance",
      expectedBand: "unmatched",
    },
    {
      label: "unrelated2",
      a: "Refueling Time",
      b: "Secure Communication",
      expectedBand: "unmatched",
    },
    {
      label: "near-unrelated",
      a: "Turbulence Stability",
      b: "Proximity Safety Abort",
      expectedBand: "unmatched",
    },
  ];

  for (const { label, a, b, expectedBand } of [...EXACT_CONFIDENT, ...CLEAR_UNMATCHED]) {
    it(`${label}: band(similarity("${a}", "${b}")) === "${expectedBand}"`, () => {
      expect(band(similarity(a, b))).toBe(expectedBand);
    });
  }

  it("borderline: Validate Fuel Capacity vs Validate Fuel Capacity Margin is review or unmatched", () => {
    const result = band(similarity("Validate Fuel Capacity", "Validate Fuel Capacity Margin"));
    expect(["review", "unmatched"]).toContain(result);
  });

  it("borderline: Multi-Platform Compatibility vs Multiple Platform Compatibility is confident or review", () => {
    const result = band(similarity("Multi-Platform Compatibility", "Multiple Platform Compatibility"));
    expect(["confident", "review"]).toContain(result);
  });
});

describe("band thresholds", () => {
  it("sim >= 0.90 → confident", () => {
    expect(band(0.90)).toBe("confident");
    expect(band(1.00)).toBe("confident");
  });

  it("sim >= 0.78 and < 0.90 → review", () => {
    expect(band(0.78)).toBe("review");
    expect(band(0.89)).toBe("review");
  });

  it("sim < 0.78 → unmatched", () => {
    expect(band(0.77)).toBe("unmatched");
    expect(band(0.00)).toBe("unmatched");
  });
});
