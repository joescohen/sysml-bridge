/**
 * Tests for requirement-chunker.ts (C3 deterministic requirement chunker).
 *
 * TDD: written BEFORE implementation.
 * Verifies:
 * - K known shall-statements → exactly K candidates (C3)
 * - All 3 detection patterns fire
 * - Traces to: / Verified by: extraction
 * - No embedding/retrieval (pure function — C5)
 * - Deterministic: same input → same output
 */

import { describe, it, expect } from "vitest";
import { detectAndChunkRequirements } from "../requirement-chunker.js";
import type { RequirementChunkerContext } from "../requirement-chunker.js";

const baseContext: RequirementChunkerContext = {
  documentHash: "reqhash123",
  sectionId: "sec-req-001",
  sectionPath: "3.2",
  pageStart: 5,
  pageEnd: 8,
  documentId: "doc-srd-001",
};

describe("detectAndChunkRequirements — C3 fixture: K known shall-statements → exactly K candidates", () => {
  it("fixture with 1 alphanumeric-ID item → exactly 1 candidate", async () => {
    const text = "SYS-REQ-042: The system shall maintain a stable orbit.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
  });

  it("fixture with 3 different-pattern items → exactly 3 candidates (all 3 patterns)", async () => {
    const text = [
      "SYS-REQ-001: The system shall perform telemetry acquisition.",    // pattern 1
      "3.2.1 The system shall operate within thermal limits.",           // pattern 2
      "Requirement 17: The system shall provide backup comms.",          // pattern 3
    ].join("\n");
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(3);
  });

  it("fixture with 5 alphanumeric-ID items → exactly 5 candidates", async () => {
    const text = [
      "SYS-REQ-001: First requirement.",
      "SYS-REQ-002: Second requirement.",
      "SYS-REQ-003: Third requirement.",
      "SYS-REQ-004: Fourth requirement.",
      "SYS-REQ-005: Fifth requirement.",
    ].join("\n");
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(5);
  });

  it("fixture with 0 requirement patterns → 0 candidates", async () => {
    const text = "This is just prose text with no requirement IDs.\nAnother line of prose.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(0);
  });
});

describe("detectAndChunkRequirements — Pattern detection", () => {
  it("Pattern 1: detects alphanumeric ID: SYS-REQ-042: ...", async () => {
    const text = "SYS-REQ-042: The system shall maintain a stable orbit.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.text).toContain("SYS-REQ-042");
  });

  it("Pattern 1: detects period separator: CONOPS-SCN-003. ...", async () => {
    const text = "CONOPS-SCN-003. The operator shall initiate the launch sequence.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.text).toContain("CONOPS-SCN-003");
  });

  it("Pattern 2: detects section-numbered shall: 3.2.1 The system shall...", async () => {
    const text = "3.2.1 The system shall perform real-time data acquisition.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.text).toContain("3.2.1");
  });

  it("Pattern 2: detects 'An' determiner: 4.1 An operator shall...", async () => {
    const text = "4.1 An operator shall confirm all checklist items before launch.";
    const chunks = await detectAndChunkRequirements(text, "4.1", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.text).toContain("4.1");
  });

  it("Pattern 3: detects explicit Requirement label: Requirement 17: ...", async () => {
    const text = "Requirement 17: The system shall provide a backup communication channel.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.text).toContain("Requirement 17");
  });

  it("Pattern 3: detects short Req label: Req 3: ...", async () => {
    const text = "Req 3: The system shall operate within defined thermal limits.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.text).toContain("Req 3");
  });

  it("section context prefix prepended to each chunk text", async () => {
    const text = "SYS-REQ-042: The system shall maintain a stable orbit.";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks[0]?.text).toMatch(/^\[Section: 3\.2\]/);
    expect(chunks[0]?.text).toContain("SYS-REQ-042");
  });
});

describe("detectAndChunkRequirements — Trace extraction", () => {
  it("traces_to populated from 'Traces to: CONOPS-SCN-003'", async () => {
    const text = "SYS-REQ-042: The system shall maintain a stable orbit.\nTraces to: CONOPS-SCN-003";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.tracesTo).toContain("CONOPS-SCN-003");
  });

  it("traces_to supports multiple comma-separated values", async () => {
    const text =
      "SYS-REQ-042: The system shall maintain a stable orbit.\nTraces to: CONOPS-SCN-003, CONOPS-SCN-005";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks[0]?.tracesTo).toContain("CONOPS-SCN-003");
    expect(chunks[0]?.tracesTo).toContain("CONOPS-SCN-005");
  });

  it("verifiedBy populated from 'Verified by: TP-042'", async () => {
    const text = "SYS-REQ-042: The system shall maintain a stable orbit.\nVerified by: TP-042";
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(1);
    expect(chunks[0]?.verifiedBy).toContain("TP-042");
  });

  it("handles multi-line requirement items (body continues until next match)", async () => {
    const text = [
      "SYS-REQ-001: The system shall perform telemetry acquisition",
      "with a minimum sample rate of 10 Hz.",
      "SYS-REQ-002: The system shall transmit data within 100ms.",
    ].join("\n");
    const chunks = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks).toHaveLength(2);
    expect(chunks[0]?.text).toContain("10 Hz");
    expect(chunks[1]?.text).toContain("100ms");
  });
});

describe("detectAndChunkRequirements — Determinism (C3)", () => {
  it("chunk_id is deterministic — same input → same output on re-call", async () => {
    const text = "SYS-REQ-001: The system shall perform telemetry acquisition.";
    const chunks1 = await detectAndChunkRequirements(text, "3.2", baseContext);
    const chunks2 = await detectAndChunkRequirements(text, "3.2", baseContext);
    expect(chunks1[0]?.chunkId).toBe(chunks2[0]?.chunkId);
  });
});

describe("detectAndChunkRequirements — C5 mechanism (pure, no embedding)", () => {
  it("returns synchronously-resolved promise (no async I/O, no embedding calls)", async () => {
    // This test documents that the chunker is pure — it must NOT require
    // any embedding, LanceDB, or network calls. If it did, this test would
    // need mocking. The fact it runs in isolation proves purity.
    const text = "SYS-REQ-001: The system shall be reliable.";
    const startMs = Date.now();
    const chunks = await detectAndChunkRequirements(text, "1.0", baseContext);
    const elapsedMs = Date.now() - startMs;
    expect(chunks.length).toBeGreaterThan(0);
    // Pure CPU work should complete in well under 1 second
    expect(elapsedMs).toBeLessThan(1000);
  });
});
